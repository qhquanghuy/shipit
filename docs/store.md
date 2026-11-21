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
  logbooks/<slice>.jsonl                        append-only
  roadmap/roadmap.md                            the roadmap
  slices/<slice>/build-plan.md                  one build plan per slice
```

Slice 1 writes only `receipts/check/`. The other paths are reserved for later slices, which
add files and never change this layout.

## Rules

- `<tree>` is the git tree hash (`git rev-parse HEAD^{tree}`): a receipt belongs to the
  content of a commit, not to the commit.
- Every file is written under a temporary name in its own folder, then moved into place.
- Reading never creates folders.
- Every receipt is one JSON object with `kind`, `version`, `key`, `time` (UTC, ISO 8601) and
  `tool` (`shipit <version>`).

## Check receipt

`receipts/check/<tree>.json`, written by `shipit check`:

```json
{"kind": "check", "version": 1, "key": "<tree hash>", "commit": "<commit hash>",
 "size": {"files": 24, "cap": 60}, "commands": ["…"],
 "time": "2026-11-21T10:00:00Z", "tool": "shipit 0.1.0"}
```

## In code

`lib/store.sh` has `store_path <relative path>`, which prints the absolute path and creates
nothing, and `write_json_atomic <path>`, which reads JSON on stdin and writes it as above.
