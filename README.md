# ShipIt

A domain-free framework for agent work: roadmap, slices, receipts, independent review,
test drive, ship.

ShipIt is an interface over skill packs (such as Superpowers), not a how-to. It fixes the
flow, the roles and the rules; packs decide how each phase is carried out.

```
goal → roadmap → build plan → make → check → informed review + second opinion + test drive → recheck → ship
```

Status: design. Nothing to install yet.

## Develop

```
nix develop      # bash, jq, git, bats, shellcheck, actionlint
bats tests       # run the tests
nix flake check  # shellcheck, bats and actionlint, as CI runs them
```

`nix flake check` sees only files git tracks: `git add` new files first.
