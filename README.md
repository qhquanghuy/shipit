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

From a clone, run `bin/shipit` (for example `bin/shipit check` from inside your project), or
with Nix, without cloning:

```
nix run github:qhquanghuy/shipit -- check
```

Needs git 2.31, bash 4.4 and jq 1.6 or newer, on Linux or macOS 12.3 or newer. The Nix
package brings its own copies.

## Check and verify

`shipit check` runs the commands in `.config/shipit.json` on a clean commit, counts the changed files against the size cap, and writes a receipt.
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
and the one whose setup the slice is compared with. It comes from `base` in the setup, or from `shipit check --base <name>`,
which wins; CI passes the pull request's base branch there, because the setup is the branch's own word. The ref used for
`<base>`: the local branch if it has everything `origin/<base>` has (so an owner's unpushed setup change counts), else
`origin/<base>`, else the local branch; if none exists, exit 2.

A base named by the setup may not be the branch being checked, nor point at the commit being checked: it could not vouch
for the checks. Exit 2, and pass `--base <name>`. With `--base`, the base may be HEAD itself (CI on the base branch):
size is 0 and the checks run.

### Changing the checks

`base`, `cap`, `check` and `environment` in the setup must be identical to the base branch's. Any difference
(a check added, removed or reordered, a new cap, any change to `environment`, another base) is refused:
the owner merges setup changes on the base first (or merge or rebase the base if it moved on). If the base has no setup
file yet (adopting ShipIt), a notice is printed and the check goes on.

A receipt covers the content git tracks. Ignored files are not part of it.

Exit codes:

| Command | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| `shipit check [--base <name>]` | receipt written | a check failed, a check changed the work, the change is over the cap, or the slice changes the checks | usage or setup error (bad arguments, missing or invalid setup file, base branch not found, no shared history with it, a base that is the branch itself, sparse checkout) | uncommitted changes, or a tracked file hidden from git |
| `shipit verify [commit]` | valid receipt | no valid receipt, or (no argument only) uncommitted changes or a tracked file hidden from git | usage error (bad arguments, not a commit) | not used |

## Develop

```
nix develop      # bash, jq, git, bats, shellcheck, actionlint
bats tests       # run the tests
nix flake check  # shellcheck, bats and actionlint, as CI runs them
```

`nix flake check` sees only files git tracks: `git add` new files first.
