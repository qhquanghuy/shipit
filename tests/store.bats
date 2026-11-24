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

# draft_roadmap <file> [goal]: the validate fixture's roadmap, wrapped as the roadmap kind's output,
# naming <goal> (default demo).
draft_roadmap() {
  {
    printf '## Form\n\n```toml\nkind = "roadmap"\noutcome = "ready-for-review"\ninput = "a goal"\n```\n\n'
    sed "s/^goal = \"demo\"/goal = \"${2-demo}\"/" "$SHIPIT_SRC/tests/fixtures/roadmap.md"
    printf '\n## Goal\nThe demo goal.\n\n## Shared contracts\n- none\n\n## Owner decisions\n- none\n\n## Risks\n- none\n'
  } >"$1"
}

@test "store put: anyone may put the next goal or roadmap version, and each put is recorded in the logbook" {
  make_slice_repo
  cp "$SHIPIT_SRC/tests/fixtures/goal.md" g.md
  put goal g.md --goal demo
  [ "$status" -eq 0 ]
  [ "$output" = "stored $(store)/goals/demo/goal.v1.md" ]
  cmp g.md "$(store)/goals/demo/goal.v1.md"
  put goal g.md --goal demo
  [ "$status" -eq 0 ]
  [ "$output" = "stored $(store)/goals/demo/goal.v2.md" ]
  cmp g.md "$(store)/goals/demo/goal.v2.md"
  draft_roadmap r.md
  put roadmap r.md --goal demo
  [ "$status" -eq 0 ]
  [ "$output" = "stored $(store)/goals/demo/roadmap.v2.md" ]
  cmp r.md "$(store)/goals/demo/roadmap.v2.md"
  run jq -c 'select(.event == "stored") | [.kind, .version, .hash, (.time | test("Z$")), (.tool | startswith("shipit "))]' "$(store)/goals/demo/logbook.jsonl"
  [ "$status" -eq 0 ]
  h() { git hash-object --no-filters "$(store)/goals/demo/$1"; }
  [ "$(sed -n 1p <<<"$output")" = "[\"goal\",1,\"$(h goal.v1.md)\",true,true]" ]
  [ "$(sed -n 2p <<<"$output")" = "[\"goal\",2,\"$(h goal.v2.md)\",true,true]" ]
  [ "$(sed -n 3p <<<"$output")" = "[\"roadmap\",2,\"$(h roadmap.v2.md)\",true,true]" ]
  [ "$(wc -l <<<"$output")" -eq 3 ]
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

# draft_plan_review <file>: a valid plan review of the build plan of slice alpha.
draft_plan_review() {
  cat >"$1" <<'REVIEW'
## Form

```toml
kind = "plan-review"
outcome = "approve"
input = "the build plan of slice alpha of goal demo"
```

## Summary
It holds.

## Done-when
- [covered] DW1: slice alpha

## Findings
- nitpick: a name

## Verified
- none

## Inferred
- none

## Unverifiable
- none
REVIEW
}

# draft_informed_review <file>: a valid informed review of slice alpha.
draft_informed_review() {
  cat >"$1" <<'REVIEW'
## Form

```toml
kind = "informed-review"
outcome = "approve"
input = "the change, the goal, the build plan and the reader's explanation"
```

## Summary
It holds.

## Teach-back
- [matches] a.txt: adds a line

## Reading path
- none

## Other files
- none

## Look here
- a.txt:1: the only line

## Simple code
- Easy to review.

## Security
- none found: no input

## Performance
- none found: no loop

## Findings
- nitpick: a name

## Verified
- none

## Inferred
- none

## Unverifiable
- none
REVIEW
}

# show <args>: run shipit store show, stdout in $output, stderr in $stderr.
show() {
  run --separate-stderr "$SHIPIT_BIN" store show "$@"
}

# shown_equals <folder> <file> <args>: shipit store show <args> run in <folder> prints <file> byte
# for byte, and nothing on stderr.
shown_equals() {
  local folder=$1 file=$2
  shift 2
  (cd "$folder" && "$SHIPIT_BIN" store show "$@" >"$BATS_TEST_TMPDIR/got" 2>"$BATS_TEST_TMPDIR/err")
  cmp "$file" "$BATS_TEST_TMPDIR/got"
  [ ! -s "$BATS_TEST_TMPDIR/err" ]
}

# store_with_files: the goal demo, slice alpha: a build plan, a plan review and a round 1 informed
# review put with store put, two versions of the goal and of the roadmap, and a worktree of
# slice/demo/alpha at .shipit/wt/alpha.
store_with_files() {
  local dir
  make_slice_repo
  dir="$(store)/goals/demo"
  draft_plan plan.md
  draft_plan_review pr.md
  draft_informed_review ir.md
  put build-plan plan.md --goal demo --slice alpha
  [ "$status" -eq 0 ]
  put plan-review pr.md --goal demo --slice alpha
  [ "$status" -eq 0 ]
  put informed-review ir.md --goal demo --slice alpha --round 1
  [ "$status" -eq 0 ]
  printf 'goal one\n' >"$dir/goal.v1.md"
  printf 'goal two\n' >"$dir/goal.v2.md"
  cp "$dir/roadmap.v1.md" "$dir/roadmap.v2.md"
  printf '\nversion two\n' >>"$dir/roadmap.v2.md"
  git branch slice/demo/alpha
  git worktree add -q .shipit/wt/alpha slice/demo/alpha
}

@test "store show: prints each kind from any working copy, byte for byte" {
  store_with_files
  local dir
  dir="$(store)/goals/demo"
  for folder in . .shipit/wt/alpha; do
    shown_equals "$folder" plan.md build-plan --goal demo --slice alpha
    shown_equals "$folder" pr.md plan-review --goal demo --slice alpha
    shown_equals "$folder" ir.md informed-review --goal demo --slice alpha --round 1
    shown_equals "$folder" "$dir/goal.v2.md" goal --goal demo
    shown_equals "$folder" "$dir/roadmap.v2.md" roadmap --goal demo
  done
}

@test "store show: a missing file, a link or a wrong flag is refused" {
  store_with_files
  local before
  before=$(find "$(store)" | sort)
  show build-plan --goal demo --slice beta
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $(store)/goals/demo/slices/beta/build-plan.md is not in the store" ]
  show informed-review --goal demo --slice alpha --round 2
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $(store)/goals/demo/slices/alpha/reviews/round-2/informed-review.md is not in the store" ]
  show goal --goal demo --slice alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: goal takes no --slice or --round" ]
  show roadmap --goal demo --round 1
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: roadmap takes no --slice or --round" ]
  show nope --goal demo
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: shipit store show takes goal, roadmap, build-plan, plan-review, explanation, informed-review, second-opinion or review, not nope" ]
  show informed-review --goal demo --slice alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: informed-review needs --round <n>" ]
  [ "$(find "$(store)" | sort)" = "$before" ]
  mkdir -p "$BATS_TEST_TMPDIR/elsewhere/alpha"
  cp plan.md "$BATS_TEST_TMPDIR/elsewhere/alpha/build-plan.md"
  mv "$(store)/goals/demo/slices" "$BATS_TEST_TMPDIR/kept"
  ln -s "$BATS_TEST_TMPDIR/elsewhere" "$(store)/goals/demo/slices"
  show build-plan --goal demo --slice alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: $(store)/goals/demo/slices is a symbolic link; the store holds no links" ]
  [ -z "$output" ]
  rm "$(store)/goals/demo/slices"
  mv "$BATS_TEST_TMPDIR/kept" "$(store)/goals/demo/slices"
  mv "$(store)/goals/demo/slices/alpha/build-plan.md" "$BATS_TEST_TMPDIR/plan-kept.md"
  ln -s "$BATS_TEST_TMPDIR/plan-kept.md" "$(store)/goals/demo/slices/alpha/build-plan.md"
  show build-plan --goal demo --slice alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: $(store)/goals/demo/slices/alpha/build-plan.md is a symbolic link; the store holds no links" ]
  [ -z "$output" ]
  rm "$(store)/goals/demo/slices/alpha/build-plan.md"
  mv "$BATS_TEST_TMPDIR/plan-kept.md" "$(store)/goals/demo/slices/alpha/build-plan.md"
  mkdir -p .shipit
  ln -s "$BATS_TEST_TMPDIR/elsewhere" .shipit/td
  show build-plan --goal demo --slice alpha --copy
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: $PWD/.shipit/td is a symbolic link; the drafts folder holds no links" ]
  [ ! -e "$BATS_TEST_TMPDIR/elsewhere/alpha/store" ]
}

@test "store show: a copy lands in the drafts folder, ignored by git" {
  store_with_files
  local repo wt dir
  repo=$PWD
  dir="$(store)/goals/demo"
  wt=$(git -C .shipit/wt/alpha rev-parse --show-toplevel)
  cd "$wt"
  mkdir -p .shipit/td/alpha
  printf '*\n' >.shipit/td/alpha/.gitignore
  printf 'my edit\n' >.shipit/td/alpha/build-plan.md
  show build-plan --goal demo --slice alpha --copy
  [ "$status" -eq 0 ]
  [ "$output" = "copied $wt/.shipit/td/alpha/store/build-plan.md" ]
  cmp "$repo/plan.md" .shipit/td/alpha/store/build-plan.md
  [ "$(cat .shipit/td/alpha/store/.gitignore)" = '*' ]
  [ "$(LC_ALL=C ls -A .shipit/td/alpha/store)" = $'.gitignore\nbuild-plan.md' ]
  [ "$(cat .shipit/td/alpha/build-plan.md)" = "my edit" ]
  [ -z "$(git status --porcelain)" ]
  draft_plan "$repo/two.md" Two.
  cp "$repo/two.md" "$dir/slices/alpha/build-plan.md"
  show build-plan --goal demo --slice alpha --copy
  [ "$status" -eq 0 ]
  cmp "$repo/two.md" .shipit/td/alpha/store/build-plan.md
  [ "$(LC_ALL=C ls -A .shipit/td/alpha/store)" = $'.gitignore\nbuild-plan.md' ]
  show roadmap --goal demo --copy
  [ "$status" -eq 0 ]
  [ "$output" = "copied $wt/.shipit/td/demo/store/roadmap.v2.md" ]
  cmp "$dir/roadmap.v2.md" .shipit/td/demo/store/roadmap.v2.md
  [ "$(cat .shipit/td/demo/.gitignore)" = '*' ]
  show goal --goal demo --copy
  [ "$status" -eq 0 ]
  [ "$output" = "copied $wt/.shipit/td/demo/store/goal.v2.md" ]
  cmp "$dir/goal.v2.md" .shipit/td/demo/store/goal.v2.md
  show informed-review --goal demo --slice alpha --round 1 --copy
  [ "$status" -eq 0 ]
  [ "$output" = "copied $wt/.shipit/td/alpha/store/round-1-informed-review.md" ]
  cmp "$repo/ir.md" .shipit/td/alpha/store/round-1-informed-review.md
  [ "$(cat .shipit/td/alpha/build-plan.md)" = "my edit" ]
  [ -z "$(git status --porcelain)" ]
}
