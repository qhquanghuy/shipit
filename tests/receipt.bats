#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

PLAN=(--author opus:high --by fable:high --result approve)

@test "receipt: a plan review keys the build plan's file hash and logs it" {
  make_slice_repo
  write_build_plan beta "the plan"
  local key
  key=$(git hash-object --no-filters "$(store)/goals/demo/slices/beta/build-plan.md")
  rec plan-review beta "${PLAN[@]}"
  [ "$status" -eq 0 ]
  [[ $output == "receipt written: plan-review for beta (${key:0:12}), fable:high above opus:high by tier" ]]
  local file
  file="$(store)/receipts/plan-review/$key.json"
  [ "$(jq -r '[.kind, .version, .key, .slice, .subject.file, .rank, .result, .author_is_planner] | join(" ")' "$file")" = \
    "plan-review 1 $key beta goals/demo/slices/beta/build-plan.md tier approve true" ]
  [ "$(jq -c '[.author.tier, .by.tier, .by.stand_in]' "$file")" = "[3,4,false]" ]
  [ "$(wc -l <"$(store)/goals/demo/logbook.jsonl")" -eq 1 ]
  [ "$(jq -r '[.slice, .event, .kind, .key, .result, .by] | join(" ")' "$(store)/goals/demo/logbook.jsonl")" = \
    "beta receipt plan-review $key approve fable:high" ]
}

@test "receipt: a roadmap review has no slice and no logbook line" {
  make_slice_repo
  rec plan-review roadmap "${PLAN[@]}"
  [ "$status" -eq 0 ]
  local key
  key=$(git hash-object --no-filters "$(store)/goals/demo/roadmap.v1.md")
  [ "$(jq -c '[.slice, .subject.file]' "$(store)/receipts/plan-review/$key.json")" = '[null,"goals/demo/roadmap.v1.md"]' ]
  [ ! -e "$(store)/goals/demo/logbook.jsonl" ]
}

@test "receipt: a reviewer not above the planner is refused and nothing is written" {
  make_slice_repo
  write_build_plan beta "the plan"
  rec plan-review beta --author opus:high --by sonnet:high --result approve
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: sonnet:high does not rank above opus:high (tier 3, effort high): use a higher tier or effort" ]
  [ ! -e "$(store)/receipts" ]
  [ ! -e "$(store)/goals/demo/logbook.jsonl" ]
}

@test "receipt: the same receipt twice exits 1 and keeps the first file" {
  make_slice_repo
  write_build_plan beta "the plan"
  rec plan-review beta "${PLAN[@]}"
  [ "$status" -eq 0 ]
  local file
  file=$(ls "$(store)"/receipts/plan-review/*.json)
  cp "$file" "$BATS_TEST_TMPDIR/first.json"
  rec plan-review beta --author opus:high --by fable:high --result rework \
    --finding "issue (blocking): a problem"
  [ "$status" -eq 1 ]
  [[ $stderr == "shipit: a plan-review receipt for this version already exists (20"* ]]
  cmp "$file" "$BATS_TEST_TMPDIR/first.json"
  [ "$(wc -l <"$(store)/goals/demo/logbook.jsonl")" -eq 1 ]
}

@test "receipt: a roadmap whose goal differs from its folder exits 2 and writes nothing" {
  make_slice_repo
  sed -i 's/^goal = "demo"/goal = "other"/' "$(store)/goals/demo/roadmap.v1.md"
  write_build_plan beta "the plan"
  rec plan-review beta "${PLAN[@]}"
  [ "$status" -eq 2 ]
  [[ $stderr == *'roadmap.v1.md: goal "other" does not match its folder "demo"' ]]
  [ ! -e "$(store)/receipts" ]
}

@test "receipt: a goal with no folder exits 2" {
  make_slice_repo
  rec plan-review roadmap --goal nope "${PLAN[@]}"
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no goal named "nope" in the store' ]
}

@test "receipt: an unknown slice and a proof step exit 2" {
  make_slice_repo
  rec plan-review nope "${PLAN[@]}"
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no slice named "nope" in the roadmap of goal "demo"' ]
  rec plan-review proof "${PLAN[@]}"
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: "proof" is a proof step: it has no build plan' ]
}

@test "receipt: a plan that is not in the store exits 2 naming the path" {
  make_slice_repo
  rec plan-review beta "${PLAN[@]}"
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no file at $(store)/goals/demo/slices/beta/build-plan.md" ]
}

@test "receipt: the goal comes from --goal, else the branch, else the only goal, else exit 2 naming the goals" {
  make_slice_repo
  write_build_plan beta "the plan"
  rec plan-review beta "${PLAN[@]}"
  [ "$status" -eq 0 ]
  add_goal w2
  write_build_plan beta "the other plan" w2
  rec plan-review beta --author opus:high --by fable:high --result approve
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: more than one goal in the store (demo, w2): pass --goal <name>" ]
  rec plan-review beta --goal w2 "${PLAN[@]}"
  [ "$status" -eq 0 ]
  [ "$(wc -l <"$(store)/goals/w2/logbook.jsonl")" -eq 1 ]
  git checkout -q -b slice/demo/beta
  rec plan-review roadmap --author opus:high --by fable:low --result approve
  [ "$status" -eq 0 ]
  [ "$(jq -r .subject.file "$(store)/receipts/plan-review/$(git hash-object --no-filters "$(store)/goals/demo/roadmap.v1.md").json")" = "goals/demo/roadmap.v1.md" ]
}

@test "receipt: the newest roadmap version is the one reviewed" {
  make_slice_repo
  printf '\n' >>"$(store)/goals/demo/roadmap.v1.md"
  { cat "$(store)/goals/demo/roadmap.v1.md"; printf 'more\n'; } >"$(store)/goals/demo/roadmap.v2.md"
  rec plan-review roadmap "${PLAN[@]}"
  [ "$status" -eq 0 ]
  local key
  key=$(git hash-object --no-filters "$(store)/goals/demo/roadmap.v2.md")
  [ "$(jq -r .subject.file "$(store)/receipts/plan-review/$key.json")" = "goals/demo/roadmap.v2.md" ]
}

@test "receipt: the tiers come from the base branch's setup" {
  make_slice_repo
  write_build_plan beta "the plan"
  git checkout -q -b slice/demo/beta
  write_setup "$(fixture_with 'gate.check=[]' 'agents.effort_cap="max"')"
  rec plan-review beta --author opus:high --by fable:xhigh --result approve
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: effort xhigh is above the setup's effort cap (high)" ]
  [ ! -e "$(store)/receipts" ]
  git checkout -q main
  git rm -q .config/shipit.toml
  git commit -q -m "no setup on the base"
  git checkout -q slice/demo/beta
  rec plan-review beta --author opus:high --by fable:xhigh --result approve
  [ "$status" -eq 0 ]
  [[ $stderr == *"shipit: no setup file on main; nothing to compare"* ]]
  [[ $stderr == *"shipit: the policy is this branch's own setup (adoption)"* ]]
}

@test "receipt: a finding or a result that the formats refuse exits 2 and writes nothing" {
  make_slice_repo
  write_build_plan beta "the plan"
  rec plan-review beta --author opus:high --by fable:high --result rework
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: rework needs a must-fix finding" ]
  rec plan-review beta --author opus:high --by fable:high --result pass
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: pass is a test-drive result" ]
  rec plan-review beta --author opus:high --by fable:high --result approve --finding "oops"
  [ "$status" -eq 2 ]
  [[ $stderr == 'shipit: finding "oops": must start with issue'* ]]
  [ ! -e "$(store)/receipts" ]
}

