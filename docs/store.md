# The store

ShipIt keeps its receipts and records in one folder inside the git common directory, so every
working copy of a project (including `git worktree` copies) sees the same files, and git never
tracks them:

```
<git common dir>/shipit/
  receipts/check/<tree>.json                    shipit check
  receipts/review/<patch id>/<kind>.json        kind: informed-review | second-opinion
  receipts/test-drive/<tree>.json
  receipts/plan-review/<file hash>.json         review of a roadmap or a build plan
  receipts/coverage/<tree>.json                 shipit coverage, run by shipit check
  receipts/metric/<tree>/<name>.json            shipit metric, one number per name
  goals/<goal>/goal.vN.md                      one folder per named goal; a changed goal is a new version
  goals/<goal>/roadmap.vN.md                   the goal's roadmap, versioned the same way
  goals/<goal>/logbook.jsonl                   append-only; one per goal, each entry names its slice
  goals/<goal>/slices/<slice>/build-plan.md    one build plan per slice
  metrics/<goal>.toml                          kept metrics: one table per slice, numbers only
```

`shipit check` writes `receipts/check/`; `shipit receipt` writes the review, test-drive and
plan-review receipts; `shipit receipt`, `shipit log` and `shipit start` append to the logbook of a
goal; `shipit coverage` writes `receipts/coverage/` and `shipit metric` writes `receipts/metric/`; `shipit complete` writes `metrics/<goal>.toml` and then
deletes the folder `goals/<goal>/`. The goal files, the roadmaps and the build plans
are put there by hand. Later commands add files and never change this layout. Receipts are keyed by the work and not by a goal, so they
stay at the root. The coverage map of a slice is not here: it is committed in the project, at
`.shipit/coverage/<goal>/<slice>.toml`.

## Rules

- `<tree>` is the git tree hash (`git rev-parse HEAD^{tree}`): a receipt belongs to the
  content of a commit, not to the commit.
- Every file is written under a temporary name in its own folder, then moved into place with an atomic
  rename: a reader sees the old file or the whole new one. The data is not synced to disk first, so after a crash or power
  loss the receipt may be missing or empty: verify then says there is no receipt, and the check is run again.
- Reading never creates folders.
- Review, test-drive and plan-review receipts are never overwritten: a new one is made under a temporary name and
  hard-linked to its final name, which fails if the name is taken.
- Logbooks are appended, never rewritten.
- Check receipts, coverage records, metric records and the metrics file are replaced atomically (the rename above).
- Every receipt is one JSON object with `kind`, `version`, `key`, `time` (UTC, ISO 8601) and
  `tool` (`shipit <version>`).

## Trust

A receipt proves only that this machine ran the checks. Receipts are trusted locally and nowhere
else: the store is not shared, and a CI run rechecks the work and trusts none. A local receipt can be gamed by
design (`shipit check --base <own branch>` compares the slice with itself, and a slice can name a base branch whose setup names itself; `base_ref` shows which base was used): CI's recheck, with its own `--base`, is the gate.

## Check receipt

`receipts/check/<tree>.json`, written by `shipit check`:

```json
{"kind": "check", "version": 1, "key": "<tree hash>", "commit": "<commit hash>",
 "base_ref": "refs/heads/main", "base_commit": "<commit hash of the base>", "base_given": false,
 "size": {"files": 24, "cap": 60}, "commands": ["…"],
 "time": "2026-11-21T10:00:00Z", "tool": "shipit 0.1.0"}
```

`base_ref` is the full ref the slice was compared with (`refs/heads/<base>` or `refs/remotes/origin/<base>`) and
`base_commit` the commit it was at. `base_given` is true when `--base` named the base (CI's receipts say so); a receipt
without it reads as not given, and `shipit verify` names the ref of a given base. When the slice changed only the setup file (docs/setup.md), the receipt also has
`"setup_change": true` and `"setup_fields": ["gate.check"]` (the changed field paths, in the setup's key order, then other keys by name),
placed after `commands`; `shipit verify` says so. Receipts without a setup change have neither key.

## Coverage record

`receipts/coverage/<tree>.json`, written by `shipit coverage` when it runs as a check command of
`shipit check` on a `slice/*` branch, and replaced by a rerun:

```json
{"kind": "coverage", "version": 1, "key": "<tree hash>", "check": "<the check command>",
 "maps": {"demo/beta": {"DW2": ["a test name", "another test name"]}},
 "time": "2026-11-21T10:00:00Z", "tool": "shipit 0.1.0"}
```

`maps` holds every map committed at `.shipit/coverage/<goal>/<slice>.toml`, read from git at the
checked commit, with the tests of each done-when item; every one of them ran.

## Metric record

`receipts/metric/<tree>/<name>.json`, written by `shipit metric <name> <value>` and replaced by a rerun:

```json
{"kind": "metric", "version": 1, "key": "<tree hash>", "goal": "demo", "slice": "beta",
 "name": "clone-ratio", "value": 2.1,
 "time": "2026-11-21T10:00:00Z", "tool": "shipit 0.1.0"}
```

`goal` and `slice` come from the current branch `slice/<goal>/<slice>` and are `null` on any other
branch. `value` is a finite number, not negative. The history an outlier is judged against is the newest
record of each other slice that has one, and, for a goal that has completed, the numbers in
`metrics/<goal>.toml`: one `[<slice>]` table of `<name> = <number>` lines for each slice.

## Kept metrics

`metrics/<goal>.toml`, written by `shipit complete <goal> --confirm` when every slice and proof step of the
goal has shipped. It is all that stays of the goal in the store: the goal's own folder, with its roadmap,
build plans and logbook, is deleted after it is written. Receipts and records are keyed by the work, so they
stay.

```toml
[slices.alpha]
clone-ratio = 3

[complete]
issues = ["https://example.invalid/issues/1"]
```

Each `[slices.<slice>]` table holds numbers only, the newest record of each metric of that slice. `[complete]`
holds the addresses of the GitHub issues made for the goal's open known issues, in order; it is written
before anything is deleted, so a rerun after a failure creates only the missing ones. A goal name is
never used again: `shipit start` refuses a goal whose `metrics/<goal>.toml` exists.

## In code

`src/edge/store.rs` finds the store folder (it creates nothing) and writes each file as above.
