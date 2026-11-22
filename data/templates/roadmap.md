## Form

```toml
kind = "roadmap"
outcome = "ready-for-review"
input = "the approved goal, with its numbered done-when items"
```

## Goal
The goal's name, and in a line what it is for. Its done-when items are numbered DW1, DW2 and so on in the goal file; `shipit validate roadmap` is given that file with `--goal` or `--goal-file`.

## Slices
The slices and proof steps in one toml block, in the shape of docs/formats.md. It starts with the goal's name. A slice's branch is derived from the goal and the slice name, never written. Every done-when item is met by a slice, or by a proof step when no automated test can show it. Dependencies are written only as `after`; the waves are computed. Parallel slices must own different paths.

```toml
goal = "goal-name"

[[slice]]
name = "first-slice"
after = []
meets = ["DW1"]
owns = ["src/first.rs"]
test_drive = false

[[proof]]
name = "owner-proof"
after = ["first-slice"]
meets = ["DW2"]
how = "what the owner does by hand, and where"
```

## Shared contracts
One item per interface that two slices share, written `the interface lands first in slice name`. Write the single item `none` when there is none.

## Owner decisions
One item per decision the owner must make, written `the decision, before slice name`: the slice that cannot start without it. Write the single item `none` when there is none.

## Risks
One list for the whole roadmap. Each item names the slice or spike that settles it: `the risk, settled by slice name` or `the risk, settled by spike name`. Write the single item `none` when there is none.
