# ShipIt

A domain-free framework for agent work: roadmap, slices, receipts, independent review,
test drive, ship.

ShipIt is an interface over skill packs (such as Superpowers), not a how-to. It fixes the
flow, the roles and the rules; packs decide how each phase is carried out.

```
goal → roadmap → build plan → make → check → informed review + second opinion + test drive → recheck → ship
```

Status: early. `shipit help` lists the commands, and `shipit help <command>` gives a command's usage and exit codes.

## Start here

1. Install the ShipIt plugin and the skill pack's plugin (`packs.skill`, Superpowers today): the commands are in `docs/claude-code.md`.
2. Adopt ShipIt in your project with `shipit init` (see "Adopt ShipIt in a project" below).
3. Run the `shipit` skill. It shows where the project stands and names the skill to run next.

The owner's guide is `docs/guide.md`: the three jobs of the owner, the skills, the roles and the decision points.

### Permissions the skills need

The skills run commands and write files. In a headless run (`claude -p`) every question becomes a refusal, so give the permissions first. Put this in `.claude/settings.json` at the root of the project (commit it in its own pull request after adoption, not with the setup), or give the same rules with `--allowedTools`. Replace `./test.sh` with your project's own test command. The skills read and search with the Read and Grep tools, so the list has no `cat`, `grep` or `ls` rule. An installed plugin is read from its cache folder, as the last rule shows; a plugin loaded from a folder of your own needs `Read(//<plugin folder>/**)` instead (`//` starts an absolute path).

```json
{
  "permissions": {
    "allow": [
      "Bash(shipit *)",
      "Bash(git status *)",
      "Bash(git log *)",
      "Bash(git diff *)",
      "Bash(git show *)",
      "Bash(git rev-parse *)",
      "Bash(git fetch *)",
      "Bash(git grep *)",
      "Bash(git worktree add *)",
      "Bash(git add *)",
      "Bash(git commit *)",
      "Bash(git push -u origin slice/*)",
      "Bash(gh pr create *)",
      "Bash(gh pr view *)",
      "Bash(gh pr diff *)",
      "Bash(mktemp -d /tmp/shipit.XXXXXX)",
      "Bash(sha256sum *)",
      "Bash(./test.sh *)",
      "Edit(//tmp/shipit.*/**)",
      "Read(~/.claude/plugins/cache/shipit/**)"
    ]
  }
}
```

A skill makes its own drafts folder with `mktemp -d /tmp/shipit.XXXXXX`, so `Edit(//tmp/shipit.*/**)` lets it write drafts there and nowhere else under `/tmp`.

**The worktree.** `shipit start` prints `git worktree add .shipit/wt/<slice> <branch>`: the worktree sits inside the project, where the session may read, write and run commands, so no `--add-dir` is needed (it needs a folder that exists, and the worktree is made later). `shipit start` adds `/.shipit/wt/` to `<git common dir>/info/exclude`, so git, and the search tools that read git's ignore rules, skip it in every working copy; nothing is committed. A tool of the project that walks the folder without git's rules (a test runner, a linter) may still see it: add `.shipit/wt` to its own ignore list.

**The store.** The harness asks for a write under the git folder and a headless run refuses it, whatever the allow rules say; `shipit store put` writes build plans, plan reviews and reviews instead, and `Bash(shipit *)` allows it. A goal or roadmap version is the owner's decision: its copy asks.

These commands are not in the list; they ask:

- Asks: the store copy (`mkdir -p` and `cp`): only for a goal or roadmap version, the owner's decision; the lead copies it, or hands the owner the exact commands.

- Asks: the review's link hash, once per link: its `printf`, piped to the hash.

## Run it

With Nix, without cloning:

```
nix run github:qhquanghuy/shipit -- check
```

From a clone, `cargo build --release` and run `target/release/shipit` (for example `shipit check` from
inside your project). On x86_64 Linux, `nix build .#static` builds a static binary that runs anywhere.

Needs git 2.39 or newer, and bash (only to run the check commands) on the `PATH`.

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
setup change is approved by merging it, so it is on `origin` before it judges anything. When `origin/<base>` is used while the local `<base>` is at another commit, `check` says so on stderr: `shipit: comparing with origin/<base>; the local <base> is at another commit (fetch or push if that is not wanted)`. The receipt records the full ref
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
slice's diff against the base touches only `.config/shipit.toml` and the files the base's `gate.setup_files` lists (the configs the check tools read, such as `deny.toml`), `check` runs the slice's new checks with its new `gate.max_files` and
environment, prints `shipit: this slice changes the checks (<fields>); setup change: the owner approves it by merging` on
stderr, and writes the receipt with `"setup_change": true` and `"setup_fields"` (the changed fields, then the changed listed files by path). `shipit verify` then
prints `valid receipt for <tree> (setup change)`, with `; --base <ref>` before the closing parenthesis when `--base` named the base. The owner's merge is the approval, and CI rechecks.

A change to a listed file (or to a folder above it, as a symlink or a submodule does) counts as a setup change, with the changed path among the `<fields>`; a listed file that equals the base tip's is not a change, as for the fields. Any other file in the diff together with a setup change is refused (a check added, removed or reordered, a new cap, any
change to `gate.environment`, another `gate.base`): the owner merges setup changes on the base first, or sends them as a slice of
their own (or merge or rebase the base if it moved on). If the base has no setup file yet (adopting ShipIt), a notice is
printed and nothing is compared, but the slice's diff must be the setup file alone: with any other file in it the check
is refused as for any setup change, because a base with no setup vouches for nothing.

A receipt covers the content git tracks, as `git status` reports it with submodules never ignored (`--ignore-submodules=none`)
and no file system monitor (`core.fsmonitor=false`); the counts and lists of changed files use `--ignore-submodules=none` too
(a `.gitmodules` entry with `ignore = all` hides nothing). What git itself does not see is outside the check: ignored files,
`.git/info/exclude`, clean filters and `core.autocrlf` (git compares the converted content, not the bytes on disk).

`shipit help <command>` lists the exit codes of a command.

## Adopt ShipIt in a project

Needs git 2.39 or newer. Adoption is one command and two small pull requests, then a third for the permission list.

1. Run `shipit init` in the project. It writes the starting setup and adds the pre-push hook; it never overwrites a file.
   Fill in `gate.check` with your check commands. A new repository needs a first commit on the base branch and a push of it.
   A branch with no commits beyond the base exits 2 in `shipit check`: commit first, or pass `--base <base>`.
2. Pull request 1 is the setup file alone: commit it on a new branch, run `shipit check`, push and open the pull request. Adoption branches are not named `slice/...` (use `adopt/setup` and `adopt/ci`): a `slice/...` branch counts toward the cap of started slices.
3. After it merges, on a new branch run `shipit init --ci-job`, set SHIPIT_COMMIT in the file it wrote to a ShipIt commit hash,
   and open pull request 2: the CI job alone, as a normal pull request (not the setup file, not a slice).
4. Make the job a required check in the host's branch rules; otherwise a pull request that deletes the job is rechecked by nothing.
   In a new clone, `shipit init --hook-only` adds the hook again.
5. Last, commit `.claude/settings.json` (the permission list under Start here) in its own pull request, never with the setup.

**The hook.** `shipit pre-push` asks for a check receipt only on a `slice/*` branch, that is `slice/<goal>/<slice>` (any name
starting `slice/` counts). Every other branch passes with a notice such as
`shipit: <branch> is not a slice/* branch; not checked`, and a commit with no setup file passes with a notice too.
A tag or a delete passes silently: it prints nothing.

**Mixed teams.** ShipIt's rules apply only to `slice/*` branches: the pre-push hook asks for a receipt there, and CI holds the size cap, the setup comparison and the coverage map there. Every other branch runs only the base's checks. The hook line `if command -v shipit >/dev/null 2>&1; then exec shipit pre-push "$@"; fi` does nothing
where ShipIt is not installed, so a teammate without it pushes as before. `shipit init` adds it to a plain `.git/hooks/pre-push`,
to a husky or `core.hooksPath` hook inside the project, or to lefthook (`lefthook-local.yml` or `.lefthook-local.yml`, then
`lefthook install`), and only when that is safe. Otherwise it prints the line with the reason, and for pre-commit it always prints.
The v1 limit, plainly: coverage proves that a mapped test exists and ran, not what it asserts. A teammate's pull request on a branch that is not `slice/*` is not checked against the coverage maps, so renaming, removing or weakening a mapped test there is caught only later, by the next slice whose map names it (a CI notice for this comes after v1).

**CI.** `shipit recheck --base <base> --branch <branch>` is what the action runs, and it trusts no receipt. On a `slice/*` branch it
runs the full `shipit check`: the size cap, the comparison of the setup with the base's, and the checks. On any other branch it runs only
the base branch's check commands, never the pull request's own. The job summary reports changes to the setup and to the CI job.

**Machine facts.** `shipit doctor` shows the system, the common tools, the setup's health and the machine notes
(`docs/machine-notes.md`). Hooks and CI are guardrails, not security.

**Claude Code.** ShipIt installs as a Claude Code plugin whose hooks gate reviews, shipping, global installs and parallel agents: see `docs/claude-code.md`.

Known gaps:

- A slice that sets `gate.base` to its own name passes the hook; CI's `--base` is the gate.
- A hook that reads git's input before ShipIt's line (such as `git lfs pre-push`) leaves it no input, so `init` prints the line
  instead of adding it, and the line must go before that reader.
- A branch not named `slice/*` skips the size cap and the setup comparison in CI. That is intended, for teams where not everyone
  uses ShipIt; the summary says `mode: plain` and a warning shows it, and the owner's review is the guard.
- A pull request can edit `.github/workflows/*` or `action.yml` itself (drop the recheck, change the pin). Recheck prints
  `CI job: CHANGED` and a warning, but it cannot judge the change: the owner reads workflow changes in the diff.

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
