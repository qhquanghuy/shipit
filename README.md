# ShipIt

A domain-free framework for agent work: roadmap, slices, receipts, independent review,
test drive, ship.

ShipIt is an interface over skill packs (such as Superpowers), not a how-to. It fixes the
flow, the roles and the rules; packs decide how each phase is carried out.

```
goal → roadmap → build plan → make → check → informed review + second opinion + test drive → recheck → ship
```

Status: early. Two commands exist, `shipit check` and `shipit verify`.

## Run it

With Nix, without cloning:

```
nix run github:qhquanghuy/shipit -- check
```

From a clone, `cargo build --release` and run `target/release/shipit` (for example `shipit check` from
inside your project). On x86_64 Linux, `nix build .#static` builds a static binary that runs anywhere.

Needs git 2.31 or newer, and bash (only to run the check commands) on the `PATH`.

## Check and verify

`shipit check` runs the commands in `.config/shipit.toml` on a clean commit, counts the changed files against the size cap, and writes a receipt.
The receipt is a JSON file in the git common directory, keyed by the commit's tree, so every working copy sees it.
After each command the work must be as it was (commit, tree, `git status`, no hidden-edit flags); a command that changes it fails the check, naming the command, and no receipt is written.
A change over the cap also fails and writes no receipt.

`shipit verify` exits 0 if the current commit (or the one given) has a valid receipt, 1 if not. It needs no setup file:
a receipt is valid for a tree and nothing else. With no argument it also needs a clean working tree and no hidden-edit flags.

The setup is read from the commit at `HEAD`, never from the working tree. `check` refuses a tracked file marked
skip-worktree or assume-unchanged (git then hides edits to it), and a sparse checkout (checks would not see every
file: `core.sparseCheckout` must be off).

### The base

The base is the branch the size cap is counted against (the files the current commit changes since it left the base)
and the one whose setup the slice is compared with. It comes from `gate.base` in the setup, or from `shipit check --base <name>`,
which wins; CI passes the pull request's base branch there, because the setup is the branch's own word. A base is a branch
name, as `git check-ref-format --branch` accepts it: never a revision such as `HEAD~1`, and never a tag. ShipIt asks git for
the full refs `refs/heads/<base>` and `refs/remotes/origin/<base>` only, so a tag with the same name cannot stand in for the
branch. The ref used for `<base>`: `origin/<base>` when it exists, else the local branch; if neither exists, exit 2. A
setup change is approved by merging it, so it is on `origin` before it judges anything. The receipt records the full ref
(`base_ref`) and the commit it was at (`base_commit`).

The slice's own `gate.base` never judges the slice: if the branch it names has a setup whose `gate.base` is another branch,
ShipIt follows that (at most 5 steps) until a branch names itself. A slice stacked on a feature branch is therefore
counted, and its setup judged, against the root base that branch's setup names, so its size includes the files of the
branch under it. `--base` is taken as given.

A slice can still name a branch whose own setup names itself (a branch that holds its own code): that branch ends the
steps, and the slice is judged by it. `base_ref` in the receipt shows which branch it was. A local check cannot stop
this; CI's `--base` is the real gate.

A base named by the setup may not be the branch being checked, nor point at the commit being checked: it could not vouch
for the checks. Exit 2, and pass `--base <name>`. With `--base`, the base may be HEAD itself (CI on the base branch):
size is 0 and the checks run.

**What a local check cannot do.** `--base <your own branch>` is trusted, because CI must be able to name the real base: a
local check cannot stop you from passing it, and then the slice is compared with itself (size 0, no setup difference). The
recheck in CI, which passes the pull request's real base and trusts no receipt, is the gate. A receipt written on a
developer's machine can be gamed by design: it says that this machine ran the checks, nothing more. The same holds
for a slice that names a base branch whose setup names itself: it is compared with that branch, and `base_ref` in the
receipt shows it. CI's `--base` is the real gate.

### Changing the checks

The whole setup, field by field (`gate.max_files`, `agents.tiers`, `packs.model`, …), is compared with the base branch's setup as it was where the slice
forked from it. If the base branch's setup has changed since the fork, the check is refused with "merge or rebase" (a
stale base is never mistaken for the slice's own change; a slice whose fields already equal the base tip's, as after a squash merge of its parent, is not refused). The slice's own change is refused, with one exception. If the
slice's diff against the base touches only `.config/shipit.toml`, `check` runs the slice's new checks with its new `gate.max_files` and
environment, prints `shipit: this slice changes the checks (<fields>); setup change: the owner approves it by merging` on
stderr, and writes the receipt with `"setup_change": true` and `"setup_fields"` (the changed fields). `shipit verify` then
prints `valid receipt for <tree> (setup change)`. The owner's merge is the approval, and CI rechecks.

Any other file in the diff together with a setup change is refused (a check added, removed or reordered, a new cap, any
change to `gate.environment`, another `gate.base`): the owner merges setup changes on the base first, or sends them as a slice of
their own (or merge or rebase the base if it moved on). If the base has no setup file yet (adopting ShipIt), a notice is
printed and nothing is compared, but the slice's diff must be the setup file alone: with any other file in it the check
is refused as for any setup change, because a base with no setup vouches for nothing.

A receipt covers the content git tracks, as `git status` reports it with submodules never ignored (`--ignore-submodules=none`)
and no file system monitor (`core.fsmonitor=false`); the counts and lists of changed files use `--ignore-submodules=none` too
(a `.gitmodules` entry with `ignore = all` hides nothing). What git itself does not see is outside the check: ignored files,
`.git/info/exclude`, clean filters and `core.autocrlf` (git compares the converted content, not the bytes on disk).

Exit codes:

| Command | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| `shipit check [--base <name>]` | receipt written (marked `setup_change` when the slice only changes the setup file) | a check failed, a check changed the work, the change is over the cap, or the slice changes the checks together with other files | usage or setup error (bad arguments, a base that is not a branch name, missing or invalid setup file, base branch not found, no shared history with it, a base that is the branch itself, sparse checkout, git failed, the receipt could not be written) | uncommitted changes, or a tracked file hidden from git |
| `shipit verify [commit]` | valid receipt | no valid receipt, or (no argument only) uncommitted changes or a tracked file hidden from git | usage error (bad arguments, not a commit) | not used |

## Develop

```
nix develop                          # rust, bats, jq, git, shellcheck, actionlint
cargo build && bats tests            # build, then run the tests against target/debug/shipit
cargo clippy --all-targets           # lint
cargo test                           # unit tests
rustfmt $(git ls-files '*.rs')       # format
nix flake check                      # what CI runs
```

`SHIPIT_BIN=<path> bats tests` runs the tests against another binary.
`nix flake check` sees only files git tracks: `git add` new files first.
