#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# ready <slice> <id>: a counting plan review, work that is checked and confirms <id> through the
# suite's coverage run, and both reviews; the repo needs suite_repo and is left on the slice's branch.
ready() {
  plan_ready "$1"
  mapped_work "$1" "$2"
  check_here
  "$SHIPIT_BIN" receipt informed-review "$1" --author sonnet:high --by opus:high --result approve >/dev/null
  "$SHIPIT_BIN" receipt second-opinion "$1" --author sonnet:high --by opus:high --result approve >/dev/null
}

READY="owner: ship it, then shipit log alpha shipped --merge <merge commit> --goal demo"

@test "status: a slice walks from planned to shipped" {
  suite_repo
  st
  [ "$status" -eq 0 ]
  [ "$(sed -n 1p <<<"$output")" = "open findings: must fix 0, must answer 0, nice to fix 0 · started: 0 of 3" ]
  [ "$(sed -n 2p <<<"$output")" = "goal demo: roadmap needs a review (shipit receipt plan-review roadmap --goal demo). roadmap v1: stored by hand, not recorded." ]
  [ "$(the_line alpha)" = "demo/alpha: planned. Next: planner: write the build plan at $(store)/goals/demo/slices/alpha/build-plan.md" ]
  [ "$(the_line proof)" = "demo/proof: planned. Next: wait for beta, gamma to ship" ]
  write_build_plan alpha "the plan"
  st
  [ "$(the_line alpha)" = "demo/alpha: planned. Next: reviewer above the planner: review the build plan, then shipit receipt plan-review alpha --goal demo" ]
  plan_ready alpha
  plan_ready beta
  "$SHIPIT_BIN" receipt plan-review roadmap --author opus:high --by fable:high --result approve
  st
  [ "$(sed -n 2p <<<"$output")" = "goal demo: roadmap reviewed. roadmap v1: stored by hand, not recorded." ]
  [ "$(the_line alpha)" = "demo/alpha: build plan ready. Next: lead: start it with shipit start alpha --goal demo" ]
  [ "$(the_line beta)" = "demo/beta: build plan ready. Next: wait for alpha to ship" ]
  mapped_work alpha DW1
  st
  [ "$(the_line alpha)" = "demo/alpha: made. Next: run shipit check on slice/demo/alpha. Started without shipit start." ]
  [[ $output == *"started: 1 of 3"* ]]
  check_here
  st
  [ "$(the_line alpha)" = "demo/alpha: checked. Next: missing: informed review, second opinion. Started without shipit start." ]
  "$SHIPIT_BIN" receipt informed-review alpha --author sonnet:high --by opus:high --result approve
  "$SHIPIT_BIN" receipt second-opinion alpha --author sonnet:high --by opus:high --result approve
  st
  [ "$(the_line alpha)" = "demo/alpha: ready to ship. Next: $READY. Started without shipit start." ]
  git checkout -q main
  git merge -q --no-ff slice/demo/alpha -m "merge alpha"
  lg alpha shipped --merge HEAD
  [ "$status" -eq 0 ]
  st
  [[ $output == *"started: 0 of 3"* ]]
  [[ $output == *"shipped (demo): alpha"* ]]
  [ "$(the_line beta)" = "demo/beta: build plan ready. Next: lead: start it with shipit start beta --goal demo" ]
}

@test "status: a light slice is ready to ship with the informed review alone" {
  suite_repo
  plan_ready alpha
  mapped_work alpha DW1
  check_here
  "$SHIPIT_BIN" receipt informed-review alpha --author sonnet:high --by opus:high --result approve
  st
  [ "$(the_line alpha)" = "demo/alpha: ready to ship. Next: $READY. Started without shipit start." ]
}

@test "status: a reviewed slice stays checked until each done-when item is confirmed" {
  make_slice_repo
  plan_ready alpha
  make_work slice/demo/alpha 1
  check_here
  "$SHIPIT_BIN" receipt informed-review alpha --author sonnet:high --by opus:high --result approve \
    --finding "nitpick: DW1 has no test yet" >/dev/null
  "$SHIPIT_BIN" receipt second-opinion alpha --author sonnet:high --by opus:high --result approve >/dev/null
  st
  [ "$status" -eq 0 ]
  [ "$(the_line alpha)" = "demo/alpha: checked. Next: missing: proof of demo:DW1. Open: nice to fix 1. Started without shipit start." ]
  st --json
  [ "$(jq -c '.goals[0].slices[] | select(.name == "alpha") | .unconfirmed' <<<"$output")" = '["demo:DW1"]' ]
  lg alpha known-issue --kind informed-review --finding F1 --reason "DW1: waits for the owner's run" --by sonnet:high
  [ "$status" -eq 0 ]
  st
  [ "$(the_line alpha)" = "demo/alpha: ready to ship. Next: $READY. Not proven: demo:DW1. Started without shipit start." ]
}

@test "status: deleting each receipt lowers the state" {
  suite_repo
  ready alpha DW1
  st
  [[ $(the_line alpha) == "demo/alpha: ready to ship."* ]]
  local file
  for file in "$(store)"/receipts/review/*/informed-review.json "$(store)"/receipts/review/*/second-opinion.json; do
    mv "$file" "$file.away"
    st
    [[ $(the_line alpha) == "demo/alpha: checked. Next: missing: "* ]]
    mv "$file.away" "$file"
  done
  file=$(ls "$(store)"/receipts/check/*.json)
  mv "$file" "$file.away"
  st
  [[ $(the_line alpha) == "demo/alpha: made."* ]]
  mv "$file.away" "$file"
  file=$(ls "$(store)"/receipts/plan-review/*.json)
  mv "$file" "$file.away"
  st
  [[ $(the_line alpha) == "demo/alpha: planned. Next: reviewer above the planner"* ]]
}

@test "status: a new commit after the reviews drops the state" {
  suite_repo
  ready alpha DW1
  printf 'more\n' >>alpha-1
  git commit -q -am "more work"
  st
  [[ $(the_line alpha) == "demo/alpha: made. Next: run shipit check on slice/demo/alpha"* ]]
  check_here
  st
  [[ $(the_line alpha) == "demo/alpha: checked. Next: missing: informed review, second opinion"* ]]
}

@test "status: a rebase with the same change keeps the reviews, not the check" {
  suite_repo
  ready alpha DW1
  git checkout -q main
  printf 'unrelated\n' >unrelated.txt
  git add unrelated.txt
  git commit -q -m "unrelated"
  git rebase -q main slice/demo/alpha
  st
  [[ $(the_line alpha) == "demo/alpha: made."* ]]
  check_here
  st
  [[ $(the_line alpha) == "demo/alpha: ready to ship."* ]]
}

@test "status: a changed build plan loses its review" {
  make_slice_repo
  plan_ready alpha
  write_build_plan alpha "a different plan"
  st
  [[ $(the_line alpha) == "demo/alpha: planned. Next: reviewer above the planner"* ]]
}

@test "status: a branch made without shipit start shows over the cap" {
  make_slice_repo
  write_setup "$(fixture_with 'gate.check=[]'; printf '\n[agents.parallel]\nslices = 1\n')"
  git branch slice/demo/alpha main
  git branch slice/demo/beta main
  st
  [ "$status" -eq 0 ]
  [ "$(sed -n 1p <<<"$output")" = "open findings: must fix 0, must answer 0, nice to fix 0 · started: 2 of 1, over the cap" ]
  [[ $(the_line alpha) == *"Started without shipit start." ]]
  [[ $(the_line beta) == *"Started without shipit start." ]]
}

@test "status: the fix-round limit comes from the base branch's setup" {
  make_slice_repo
  plan_ready alpha
  make_work slice/demo/alpha 1
  check_here
  "$SHIPIT_BIN" receipt informed-review alpha --author sonnet:high --by opus:high --result rework \
    --finding "issue (blocking): first"
  st
  [[ $(the_line alpha) == "demo/alpha: checked. Next: maker: fix round 1 of 2: fix informed-review F1 or log them as known issues"* ]]
  [[ $(the_line alpha) == *"Open: must fix 1."* ]]
  printf 'fixed\n' >>alpha-1
  git commit -q -am "a fix"
  check_here
  "$SHIPIT_BIN" receipt informed-review alpha --author sonnet:high --by opus:high --result rework \
    --finding "issue (blocking): second"
  st
  [[ $(the_line alpha) == "demo/alpha: checked. Next: maker: fix round 2 of 2: fix informed-review F1 or log them as known issues"* ]]
  git checkout -q main
  write_setup "$(fixture_with 'gate.check=[]' 'agents.max_fix_rounds=1')"
  git checkout -q slice/demo/alpha
  st
  [[ $(the_line alpha) == "demo/alpha: checked. Next: lead: fix-round limit (1) reached: decide, then record it with shipit log alpha note --text <decision> --goal demo"* ]]
}

@test "status: stays within the line limit" {
  make_slice_repo 'gate.check=[]' 'owner.status_update_max_lines=2'
  st
  [ "$status" -eq 0 ]
  [ "$(wc -l <<<"$output")" -eq 2 ]
  [ "$(sed -n 2p <<<"$output")" = "… 5 more: shipit status --all" ]
  st --all
  [ "$(wc -l <<<"$output")" -eq 6 ]
  st --json
  [ "$status" -eq 0 ]
  [ "$(jq -c '[.started.cap, .started.over, (.goals[0].slices | length), .goals[0].roadmap.reviewed]' <<<"$output")" = "[3,false,4,false]" ]
}

@test "status: a damaged logbook line exits 2 naming it" {
  make_slice_repo
  mkdir -p "$(store)/goals/demo"
  printf '{\n' >"$(store)/goals/demo/logbook.jsonl"
  st
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: logbook $(store)/goals/demo/logbook.jsonl line 1 is not a logbook entry; fix that line by hand" ]
}

@test "status: writes nothing and creates no folder" {
  suite_repo
  ready alpha DW1
  git checkout -q main
  local before after
  before=$(find "$(store)" | sort | xargs stat -c '%n %s %Y' | md5sum)
  st
  [ "$status" -eq 0 ]
  st --json
  after=$(find "$(store)" | sort | xargs stat -c '%n %s %Y' | md5sum)
  [ "$before" = "$after" ]
  rm -rf "$(store)/goals" "$(store)/receipts"
  mkdir -p "$(store)/goals/demo"
  cp "$SHIPIT_SRC/tests/fixtures/roadmap.md" "$(store)/goals/demo/roadmap.v1.md"
  st
  [ "$status" -eq 0 ]
  [ ! -e "$(store)/receipts" ]
  [ ! -e "$(store)/goals/demo/logbook.jsonl" ]
  [ -z "$(git status --porcelain)" ]
}

@test "status: says when a goal has shipped and what completes it" {
  make_slice_repo
  local slice
  for slice in alpha beta gamma proof; do
    ship_entry "$slice"
  done
  st
  [ "$status" -eq 0 ]
  [[ $output == *"shipped (demo): alpha, beta, gamma, proof"* ]]
  [[ $output == *"goal demo is shipped. Next: owner: shipit complete demo (a dry run), then shipit complete demo --confirm" ]]
}

@test "status: shows every goal; status --goal shows one and still counts the cap over all goals" {
  make_slice_repo
  add_goal w2
  git branch slice/demo/alpha main
  git branch slice/demo/beta main
  git branch slice/w2/alpha main
  st
  [ "$status" -eq 0 ]
  [[ $output == *"started: 3 of 3"* ]]
  [[ $output == *"demo/alpha:"* && $output == *"w2/alpha:"* ]]
  st --goal w2
  [ "$status" -eq 0 ]
  [[ $output == *"started: 3 of 3"* ]]
  [[ $output == *"w2/alpha:"* && $output != *"demo/alpha:"* ]]
}

@test "status: a roadmap whose goal differs from its folder exits 2" {
  make_slice_repo
  sed -i 's/^goal = "demo"/goal = "other"/' "$(store)/goals/demo/roadmap.v1.md"
  st
  [ "$status" -eq 2 ]
  [[ $stderr == *'roadmap.v1.md: goal "other" does not match its folder "demo"' ]]
}

@test "status: an unknown goal exits 2" {
  make_slice_repo
  st --goal nope
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no goal named "nope" in the store' ]
}

@test "status: a branch of a goal with no folder shows as unknown goal and counts toward the cap" {
  make_slice_repo
  git branch slice/ghost/one main
  git branch slice/old-name main
  st
  [ "$status" -eq 0 ]
  [[ $output == *"started: 1 of 3"* ]]
  [[ $output == *"unknown goal ghost: slice/ghost/one. Next: put its roadmap in the store, or delete the branch"* ]]
  [[ $output != *"old-name"* ]]
  git branch slice/demo/nowhere main
  st
  [[ $output == *"started: 2 of 3"* ]]
  [[ $output == *"unknown slice nowhere in goal demo: slice/demo/nowhere. Next: add it to the roadmap, or delete the branch"* ]]
}

@test "status: ids are qualified as <goal>:DWn" {
  make_slice_repo
  st --json
  [ "$(jq -c '.goals[0].slices[] | select(.name == "beta") | .meets' <<<"$output")" = '["demo:DW2"]' ]
}

@test "status: a claimed author or a fresh pass shows in the line" {
  make_slice_repo
  plan_ready alpha
  make_work slice/demo/alpha 1
  check_here
  # receipts as the tool wrote them before it refused an author that is not the maker
  "$SHIPIT_BIN" receipt informed-review alpha --author sonnet:high --by fable:high --result approve
  "$SHIPIT_BIN" receipt second-opinion alpha --author sonnet:high --by fable:high --result approve
  old_author informed-review opus
  old_author second-opinion fable fresh
  st
  [[ $(the_line alpha) == *"Claims: informed-review author is not the setup's maker; second-opinion author is not the setup's maker; second-opinion used --fresh."* ]]
  st --json
  [ "$(jq -c '.goals[0].slices[] | select(.name == "alpha") | .claims' <<<"$output")" = '["informed-review author is not the setup'"'"'s maker","second-opinion author is not the setup'"'"'s maker","second-opinion used --fresh"]' ]
}

@test "status: a goal reworded since the slice started names the item, and a version stored or changed by hand is called out" {
  make_slice_repo
  cp "$SHIPIT_SRC/tests/fixtures/goal.md" g.md
  run "$SHIPIT_BIN" store put goal g.md --goal demo
  [ "$status" -eq 0 ]
  logged_start alpha
  sed -e 's/^2\. The second item\./2. The second item, reworded./' -e '/^3\. The third item\./d' g.md >g2.md
  run "$SHIPIT_BIN" store put goal g2.md --goal demo
  [ "$status" -eq 0 ]
  cp g2.md "$(store)/goals/demo/goal.v3.md"
  printf 'edited\n' >>"$(store)/goals/demo/goal.v2.md"
  st
  [ "$status" -eq 0 ]
  [[ $output == *"goal v2: changed by hand after it was stored; goal v3: stored by hand, not recorded; roadmap v1: stored by hand, not recorded."* ]]
  [[ $(the_line alpha) == *"Changed: goal v1 -> v3 since start: DW2 reworded, DW3 removed."* ]]
  run --separate-stderr "$SHIPIT_BIN" status --summary alpha --goal demo
  [ "$status" -eq 0 ]
  [[ $output == *$'Goal and roadmap versions:\n- goal v1 -> v3 since start: DW2 reworded, DW3 removed\n- goal v2: changed by hand after it was stored\n- goal v3: stored by hand, not recorded\n- roadmap v1: stored by hand, not recorded'* ]]
  st --json
  [ "$(jq -c '.goals[0].version_flags' <<<"$output")" = '["goal v2: changed by hand after it was stored","goal v3: stored by hand, not recorded","roadmap v1: stored by hand, not recorded"]' ]
  [ "$(jq -c '.goals[0].slices[] | select(.name == "alpha") | .changes' <<<"$output")" = '["goal v1 -> v3 since start: DW2 reworded, DW3 removed"]' ]
}
