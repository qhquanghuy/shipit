# ShipIt

A domain-free framework for agent work: roadmap, slices, receipts, independent review,
test drive, ship.

ShipIt is an interface over skill packs (such as Superpowers), not a how-to. It fixes the
flow, the roles and the rules; packs decide how each phase is carried out.

```
goal → roadmap → build plan → make → check → informed review + second opinion + test drive → recheck → ship
```

Status: design. Nothing to install yet.

## Check and verify

`shipit check` runs the commands in `.config/shipit.json` on a clean commit, counts the changed files against the size cap, and writes a receipt.
The receipt is a JSON file in the git common directory, keyed by the commit's tree, so every working copy sees it.
`shipit verify` exits 0 if the current commit (or the one given) has a valid receipt, 1 if not.
A check that edits the work, or a change over the cap, fails and writes no receipt.
Exit codes: 0 ok, 1 no, 2 usage or setup error, 3 uncommitted changes.

## Develop

```
nix develop      # bash, jq, git, bats, shellcheck, actionlint
bats tests       # run the tests
nix flake check  # shellcheck, bats and actionlint, as CI runs them
```

`nix flake check` sees only files git tracks: `git add` new files first.
