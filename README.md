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
`shipit verify` exits 0 if the current commit (or the one given) has a valid receipt, 1 if not.
A check that edits the work, or a change over the cap, fails and writes no receipt.

The setup is read from the commit (`HEAD` for `check`, the given commit for `verify`), never from
the working tree. `check` refuses a slice that raises `cap` or drops a check command compared with
the base branch's setup: the owner merges that change on the base first. `check` also refuses when
a tracked file is marked skip-worktree or assume-unchanged, because git then hides edits to it.

The base is the branch in `base` of the setup, the one the size cap is counted against: the files
the current commit changes since it left the base. If `origin/<base>` exists it wins over a
local `<base>`, so a CI clone and a laptop count the same.

A receipt covers the content git tracks. Ignored files are not part of it.

Exit codes:

| Command | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| `shipit check` | receipt written | a check failed, a check changed the work, the change is over the cap, or the slice weakens the checks | usage or setup error (bad arguments, missing or invalid setup file, base branch not found, no shared history with it) | uncommitted changes, or a tracked file hidden from git |
| `shipit verify [commit]` | valid receipt | no valid receipt, or uncommitted changes (no argument only) | usage or setup error (bad arguments, not a commit, missing or invalid setup file) | not used |

## Develop

```
nix develop      # bash, jq, git, bats, shellcheck, actionlint
bats tests       # run the tests
nix flake check  # shellcheck, bats and actionlint, as CI runs them
```

`nix flake check` sees only files git tracks: `git add` new files first.
