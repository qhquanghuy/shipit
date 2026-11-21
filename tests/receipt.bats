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


@test "receipt: a second opinion keys the patch id of the checked work" {
  make_slice_repo
  make_work slice/demo/beta 2
  check_here
  local pid file
  pid=$(patch_id_of slice/demo/beta)
  rec second-opinion beta --author sonnet:high --by opus:high --result approve-after-fixes \
    --finding "issue (blocking, security): a problem" --finding "nitpick: a detail"
  [ "$status" -eq 0 ]
  [[ $output == "receipt written: second-opinion for beta (${pid:0:12}), opus:high above sonnet:high by tier" ]]
  file="$(store)/receipts/review/$pid/second-opinion.json"
  [ "$(jq -c '[.kind, .key, .slice, .rank, .result, .author_is_maker, .author.tier, .by.tier]' "$file")" = \
    "[\"second-opinion\",\"$pid\",\"beta\",\"tier\",\"approve-after-fixes\",true,2,3]" ]
  [ "$(jq -c '.subject' "$file")" = \
    "{\"branch\":\"slice/demo/beta\",\"commit\":\"$(git rev-parse HEAD)\",\"tree\":\"$(git rev-parse 'HEAD^{tree}')\",\"merge_base\":\"$(git rev-parse main)\"}" ]
  [ "$(jq -c '[.findings[] | [.id, .class]]' "$file")" = '[["F1","must fix"],["F2","nice to fix"]]' ]
  [ "$(wc -l <"$(store)/goals/demo/logbook.jsonl")" -eq 1 ]
  [ "$(jq -r '[.event, .kind, .key, .tree, .commit, .findings] | join(" ")' "$(store)/goals/demo/logbook.jsonl")" = \
    "receipt second-opinion $pid $(git rev-parse 'HEAD^{tree}') $(git rev-parse HEAD) 2" ]
}

@test "receipt: the patch id ignores the user's diff config" {
  make_slice_repo
  printf 'one\ntwo\nthree\n' >old.txt
  git add old.txt
  git commit -q -m "an old file"
  git checkout -q -b slice/demo/beta
  git mv old.txt new.txt
  printf 'four\n' >>new.txt
  mkdir sub
  printf 'x\n' >sub/é.txt
  git add -A
  git commit -q -m "work"
  check_here
  rec second-opinion beta --author sonnet:high --by opus:high --result approve
  [ "$status" -eq 0 ]
  git config diff.noprefix true
  git config diff.algorithm histogram
  git config diff.renames copies
  git config diff.external /bin/false
  git config color.ui always
  git config diff.relative true
  git config core.quotePath false
  git config diff.submodule diff
  cd sub
  rec informed-review beta --author sonnet:high --by opus:high --result approve
  [ "$status" -eq 0 ]
  [ "$(ls "$(store)/receipts/review" | wc -l)" -eq 1 ]
  [ "$(ls "$(store)"/receipts/review/*)" = $'informed-review.json\nsecond-opinion.json' ]
}

@test "receipt: a binary file, then a text change: the key changes" {
  make_slice_repo
  make_work slice/demo/beta 1
  printf '\000\001\002' >bin.dat
  git add bin.dat
  git commit -q -m "a binary file"
  check_here
  rec second-opinion beta --author sonnet:high --by opus:high --result approve
  [ "$status" -eq 0 ]
  printf '\000\001\003' >bin.dat
  git commit -q -am "another binary content"
  check_here
  rec second-opinion beta --author sonnet:high --by opus:high --result approve
  [ "$status" -eq 0 ]
  printf 'text\n' >>work1
  git commit -q -am "a text change"
  check_here
  rec second-opinion beta --author sonnet:high --by opus:high --result approve
  [ "$status" -eq 0 ]
  [ "$(ls "$(store)/receipts/review" | wc -l)" -eq 3 ]
}

@test "receipt: no check receipt or no change is refused" {
  make_slice_repo
  make_work slice/demo/beta 1
  rec second-opinion beta --author sonnet:high --by opus:high --result approve
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: no check receipt for this content: run shipit check on slice/demo/beta" ]
  git checkout -q main
  git checkout -q -b slice/demo/gamma
  git commit -q --allow-empty -m "nothing"
  check_here
  rec second-opinion gamma --author sonnet:high --by opus:high --result approve
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: the slice has no changes against main" ]
  [ ! -e "$(store)/receipts/review" ]
}

@test "receipt: a test drive keys the tree and needs a tester above the maker" {
  make_slice_repo
  make_work slice/demo/beta 1
  check_here
  rec test-drive beta --author sonnet:high --by sonnet:high --result pass
  [ "$status" -eq 1 ]
  rec test-drive beta --author sonnet:high --by opus:medium --result pass
  [ "$status" -eq 0 ]
  local tree
  tree=$(git rev-parse 'HEAD^{tree}')
  [ "$(jq -c '[.kind, .key, .subject.tree, .result]' "$(store)/receipts/test-drive/$tree.json")" = \
    "[\"test-drive\",\"$tree\",\"$tree\",\"pass\"]" ]
  rec test-drive alpha --author sonnet:high --by opus:medium --result pass
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: slice "alpha" has no test drive in the roadmap' ]
  rec test-drive beta --author sonnet:high --by opus:medium --result approve
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: approve is a review result" ]
}

@test "receipt: --commit keys that commit" {
  make_slice_repo
  make_work slice/demo/beta 1
  local first
  first=$(git rev-parse HEAD)
  check_here
  printf 'more\n' >>work1
  git commit -q -am "more work"
  check_here
  rec second-opinion beta --author sonnet:high --by opus:high --result approve --commit "$first"
  [ "$status" -eq 0 ]
  local pid
  pid=$(patch_id_of "$first")
  [ "$(jq -r .subject.commit "$(store)/receipts/review/$pid/second-opinion.json")" = "$first" ]
  rec second-opinion beta --author sonnet:high --by opus:high --result approve --commit nonsense
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: 'nonsense' is not a commit" ]
}
