#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# draft_plan <file> [text]: a valid build plan for slice alpha of goal demo; [text] is its Goal part.
draft_plan() {
  cat >"$1" <<'PLAN'
## Form

```toml
kind = "build-plan"
outcome = "ready-for-review"
input = "slice alpha of goal demo"
```

## Goal
@TEXT@

## Architecture
One file.

## Global constraints
none

## Done-when coverage
Alpha meets DW1.

```toml
DW1 = ["alpha works"]
```

## Steps
1. Make it.

## Review focus
none

## Needs from other slices
none

## Decisions for the owner
none
PLAN
  sed -i "s/^@TEXT@\$/${2-Alpha works.}/" "$1"
}

# draft_explanation <file>: a valid explanation.
draft_explanation() {
  cat >"$1" <<'EXPLANATION'
## Form

```toml
kind = "explanation"
outcome = "understood"
input = "the diff of slice alpha"
```

## Parts
- a.txt:1: adds a line

## Verified
- none

## Inferred
- none

## Unverifiable
- none
EXPLANATION
}

# put <args>: run shipit store put, stdout in $output, stderr in $stderr.
put() {
  run --separate-stderr "$SHIPIT_BIN" store put "$@"
}

@test "store put: a build plan lands where receipt and status read it, and its path is printed" {
  make_slice_repo
  S="$(store)/goals/demo/slices/alpha"
  draft_plan plan.md
  put build-plan plan.md --goal demo --slice alpha
  [ "$status" -eq 0 ]
  [ "$output" = "stored $S/build-plan.md" ]
  cmp plan.md "$S/build-plan.md"
  run "$SHIPIT_BIN" receipt plan-review alpha --author opus:high --by fable:high --result approve
  [ "$status" -eq 0 ]
  st
  [[ $(the_line alpha) == "demo/alpha: build plan ready."* ]]
}

@test "store put: a build plan is replaced while the slice is not started" {
  make_slice_repo
  S="$(store)/goals/demo/slices/alpha"
  draft_plan one.md One.
  draft_plan two.md Two.
  put build-plan one.md --goal demo --slice alpha
  [ "$status" -eq 0 ]
  put build-plan two.md --goal demo --slice alpha
  [ "$status" -eq 0 ]
  [ "$output" = "replaced $S/build-plan.md" ]
  grep -qx 'Two.' "$S/build-plan.md"
  ! grep -qx 'One.' "$S/build-plan.md"
}

@test "store put: after the start a build plan is never replaced" {
  make_slice_repo
  S="$(store)/goals/demo/slices/alpha"
  plan_ready alpha
  run "$SHIPIT_BIN" start alpha
  [ "$status" -eq 0 ]
  cp "$S/build-plan.md" before.md
  draft_plan plan.md
  put build-plan plan.md --goal demo --slice alpha
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $S/build-plan.md exists and the slice alpha is started: a changed build plan or plan review is then the lead's copy by hand (docs/store.md)" ]
  cmp before.md "$S/build-plan.md"
}

@test "store put: a review of a round lands at its place and is never replaced" {
  make_slice_repo
  S="$(store)/goals/demo/slices/alpha"
  draft_explanation ex.md
  put explanation ex.md --goal demo --slice alpha --round 1
  [ "$status" -eq 0 ]
  [ "$output" = "stored $S/reviews/round-1/explanation.md" ]
  cmp ex.md "$S/reviews/round-1/explanation.md"
  put explanation ex.md --goal demo --slice alpha --round 1
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $S/reviews/round-1/explanation.md exists; a store file is never replaced" ]
  put explanation ex.md --goal demo --slice alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: explanation needs --round <n>" ]
  put explanation ex.md --goal demo --slice alpha --round 0
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: --round 0: a number from 1 up, with no sign and no leading zero" ]
}

@test "store put: a draft that is not valid is refused with its problems, and nothing is written" {
  make_slice_repo
  S="$(store)/goals/demo/slices/alpha"
  draft_plan bad.md ""
  put build-plan bad.md --goal demo --slice alpha
  [ "$status" -eq 1 ]
  [[ $stderr == "shipit: bad.md is not a valid build-plan:"* ]]
  [ ! -e "$S/build-plan.md" ]
}

@test "store put: a goal or a roadmap version is the owner's" {
  make_slice_repo
  printf 'x\n' >g.md
  put goal g.md --goal demo --slice alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: a goal version is the owner's: copy it into the store by hand (docs/store.md)" ]
  put roadmap g.md --goal demo --slice alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: a roadmap version is the owner's decision: the lead copies it into the store by hand once the owner approves it (docs/store.md)" ]
}

@test "store put: a slice the roadmap does not name, or a proof step, is refused" {
  make_slice_repo
  draft_plan plan.md
  put build-plan plan.md --goal demo --slice nonesuch
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no slice named "nonesuch" in the roadmap of goal "demo"' ]
  put build-plan plan.md --goal demo --slice proof
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: proof is a proof step: it has no build plan or reviews" ]
}

@test "store put: a link on the way into the store is refused, and nothing is written through it" {
  make_slice_repo
  mkdir "$BATS_TEST_TMPDIR/elsewhere"
  ln -s "$BATS_TEST_TMPDIR/elsewhere" "$(store)/goals/demo/slices"
  draft_plan plan.md
  put build-plan plan.md --goal demo --slice alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: $(store)/goals/demo/slices is a symbolic link; the store holds no links" ]
  [ -z "$(ls -A "$BATS_TEST_TMPDIR/elsewhere")" ]
}
