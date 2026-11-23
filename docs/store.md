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
  goals/<goal>/slices/<slice>/plan-review.md   the plan review that approved the build plan
  goals/<goal>/slices/<slice>/reviews/round-<k>/<kind>.md   one file per review of a fix round
  metrics/<goal>.toml                          kept metrics: one table per slice, numbers only
```

`shipit check` writes `receipts/check/`; `shipit receipt` writes the review, test-drive and
plan-review receipts; `shipit receipt`, `shipit log` and `shipit start` append to the logbook of a
goal; `shipit coverage` writes `receipts/coverage/` and `shipit metric` writes `receipts/metric/`; `shipit complete` writes `metrics/<goal>.toml` and then
deletes the folder `goals/<goal>/`. A goal version and a roadmap version are put there by hand: each is the owner's decision, and the lead copies a roadmap version once the owner approves it. `shipit store put` puts the build plans, plan reviews and reviews (Store put). Later commands add files and never change this layout. Receipts are keyed by the work and not by a goal, so they
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

## Store put

`shipit store put <kind> <draft> --goal <name> --slice <name> [--round <n>]` puts a checked draft into the
store, because Claude Code refuses a write under the git folder before any allow rule, and ShipIt writes
the file itself. Each place is below `goals/<goal>/slices/<slice>/`:

| kind | flags | place | a taken place |
|---|---|---|---|
| `build-plan` | `--slice` | `build-plan.md` | replaced while the slice is not started, refused after |
| `plan-review` | `--slice` | `plan-review.md` (the plan review that approved the build plan) | as `build-plan` |
| `explanation`, `informed-review`, `second-opinion`, `review` | `--slice`, `--round` | `reviews/round-<n>/<kind>.md` | refused |

Rules:

- The draft is checked as `shipit validate <kind> <draft>` checks it, and nothing is written when it is not valid.
- The slice must be a slice of the goal's newest roadmap, not a proof step.
- A goal version and a roadmap version are never put: each is the owner's decision, and is copied by hand.
- Names are parsed, so no place leaves `goals/<goal>/`: a round is a number from 1 up.
- Every level from `<git common dir>/shipit` down to the file's folder is a real folder, never a link; a missing level is made.
- A new file is written under a temporary name and hard-linked to its place, so a taken name is never replaced, a link there included.
- A build plan or a plan review is replaced with an atomic rename only while the slice is not started (shown by `shipit status` as planned or build plan ready). A replaced build plan needs a new plan review, because the receipt is keyed by the file's hash. A replaced plan review is not tracked: no receipt or status reads the file.
- After the start, a changed build plan is the lead's copy by hand. The old plan-review receipt no longer matches the new version, so the slice needs a new plan review (`shipit status` shows it).

It prints `stored <path>` or `replaced <path>`, with the absolute path, on stdout. Exit 0 stored, 1 refused (the draft is not valid, or the place is taken), 2 usage, setup, goal, roadmap or slice error, a link on the way, or a write that failed.

The refusals, each after `shipit: `:

- `a goal version is the owner's: copy it into the store by hand (docs/store.md)`
- `a roadmap version is the owner's decision: the lead copies it into the store by hand once the owner approves it (docs/store.md)`
- `shipit store put takes build-plan, plan-review, explanation, informed-review, second-opinion or review, not <kind>`
- `<kind> needs --slice <name>`
- `--round is only for explanation, informed-review, second-opinion and review`
- `<kind> needs --round <n>`
- `--slice <text>: <rule>`
- `--round <text>: a number from 1 up, with no sign and no leading zero`
- `no slice named "<slice>" in the roadmap of goal "<goal>"`
- `<slice> is a proof step: it has no build plan or reviews`
- `<path> exists and the slice <slice> is started: a changed build plan or plan review is then the lead's copy by hand (docs/store.md)`
- `<level> is a symbolic link; the store holds no links`
- `<path> exists; a store file is never replaced`

## Store show

`shipit store show <kind> --goal <name> [--slice <name>] [--round <n>] [--copy]` prints a store file on stdout,
byte for byte, from any working copy of the project, because Claude Code refuses to read a file under the git
folder, and ShipIt reads it itself. The kinds are the six of Store put at the same places and with the same
flags, and two more:

| kind | flags | file |
|---|---|---|
| `goal` | none | the newest `goals/<goal>/goal.vN.md` |
| `roadmap` | none | the newest `goals/<goal>/roadmap.vN.md` |

Rules:

- It reads the one file and judges nothing: no roadmap is read, and the slice need not be one of the goal's.
- Every level from `<git common dir>/shipit` down to the file is a real folder, and the file is a regular file, never a link; nothing is made.
- `--copy` prints nothing of the file. It writes a copy into the drafts folder of the current working copy, `<top of the working copy>/.shipit/td/<folder>/store/<file>`, and prints `copied <path>`. The folder is the slice, or the goal for `goal` and `roadmap`; the folder `store/` below it, so that a draft in `<folder>` is never touched, is made with a `.gitignore` of the one line `*` (and so is `<folder>` when it has none), so that git ignores both. The file is `build-plan.md`, `plan-review.md`, `round-<n>-<kind>.md`, `goal.vN.md` or `roadmap.vN.md`, and an earlier copy is replaced with an atomic rename. The copy is a draft: ShipIt never reads it back. Claude Code cuts a long file printed in a Bash result to a preview, and reads a file inside the project whole with its Read tool.

Exit 0 shown or copied, 1 the file is not in the store, 2 usage, goal or roadmap error, a link on the way, or a write that failed.

The refusals, each after `shipit: `:

- `shipit store show takes goal, roadmap, build-plan, plan-review, explanation, informed-review, second-opinion or review, not <kind>`
- `<kind> takes no --slice or --round` (`goal` and `roadmap`)
- `<kind> needs --slice <name>`, `--round is only for explanation, informed-review, second-opinion and review`, `<kind> needs --round <n>`, `--slice <text>: <rule>`, `--round <text>: a number from 1 up, with no sign and no leading zero`, as for Store put
- `<path> is not in the store`
- `<level> is a symbolic link; the store holds no links`
- `<level> is a symbolic link; the drafts folder holds no links` (`--copy`, a link below the top of the working copy)
- `no goal named "<goal>" in the store` (the goal has no roadmap folder), `no roadmap in <path>`, `no goal "<goal>" in the store` (`goal` finds no `goal.vN.md`); each exit 2
- `--copy given twice; usage: …`, `--goal given twice; usage: …`, and the other usage errors of Store put (`unknown argument`, `<flag> needs a value`); exit 2

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
without it reads as not given, and `shipit verify` names the ref of a given base. When the slice changed only the setup file and the files the base lists in `gate.setup_files` (docs/setup.md), the receipt also has
`"setup_change": true` and `"setup_fields": ["gate.check", "deny.toml"]` (the changed field paths, in the setup's key order, then other keys by name, then the changed listed files by path, in git's order; a file path and a field name share the list),
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
