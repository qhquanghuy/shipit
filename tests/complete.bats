#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# a_receipt <kind> <n>: a review receipt in the store with F1 (must fix) and F2 (nice to fix);
# the key is <n> as 40 digits.
a_receipt() {
  local key dir
  key=$(printf '%040d' "$2")
  dir="$(store)/receipts/review/$key"
  mkdir -p "$dir"
  printf '{"kind":"%s","version":1,"key":"%s","findings":[{"id":"F1","label":"issue","class":"must fix","perspectives":[],"text":"a problem"},{"id":"F2","label":"nitpick","class":"nice to fix","perspectives":[],"text":"a detail"}]}\n' \
    "$1" "$key" >"$dir/$1.json"
}

# log_issue <slice> <kind> <n> <finding>: a known issue of the receipt made by a_receipt.
log_issue() {
  printf '{"slice":"%s","event":"known-issue","kind":"%s","key":"%040d","finding":"%s","reason":"later","by":"opus:high","time":"2026-11-21T10:00:00Z","tool":"shipit 0.1.0"}\n' \
    "$1" "$2" "$3" "$4" >>"$(store)/goals/demo/logbook.jsonl"
}

# a_metric <slice> <name> <value> [goal]: a metric record of a slice.
a_metric() {
  local dir
  dir="$(store)/receipts/metric/$(printf '%040d' "$RANDOM")"
  mkdir -p "$dir"
  printf '{"kind":"metric","version":1,"key":"x","goal":"%s","slice":"%s","name":"%s","value":%s,"time":"2026-11-21T10:00:00Z","tool":"shipit 0.1.0"}\n' \
    "${4:-demo}" "$1" "$2" "$3" >"$dir/$2.json"
}

# a_finished_goal: goal demo with everything shipped, two known issues, a nice-to-fix finding that
# was not logged, two metrics, and a second goal w2 that is not finished.
a_finished_goal() {
  make_slice_repo
  fake_gh
  add_goal w2
  local slice
  for slice in alpha beta gamma proof; do
    ship_entry "$slice"
  done
  a_receipt second-opinion 1
  a_receipt informed-review 2
  log_issue alpha second-opinion 1 F1
  log_issue beta informed-review 2 F1
  a_metric alpha clone-ratio 3
  a_metric beta clone-ratio 5
  a_metric alpha clone-ratio 99 w2
}

fingerprint() {
  find "$(store)" | sort | xargs stat -c '%n %s %Y' | md5sum
}

complete_goal() {
  run --separate-stderr "$SHIPIT_BIN" complete "$@"
}

@test "complete: without --confirm it prints the plan and changes nothing" {
  a_finished_goal
  local before
  before=$(fingerprint)
  complete_goal demo
  [ "$status" -eq 0 ]
  [ "$output" = "goal demo is complete. Keeps: metrics of 2 slices (metrics/demo.toml). GitHub issues to create (2): demo/alpha: second-opinion F1: a problem; demo/beta: informed-review F1: a problem. Deletes: goals/demo/. Owner: confirm, then run it again with --confirm." ]
  [ "$(fingerprint)" = "$before" ]
  [ ! -e "$GH_LOG" ]
}

@test "complete: a goal with an unshipped slice exits 1 and changes nothing" {
  a_finished_goal
  local before
  before=$(fingerprint)
  complete_goal w2 --confirm
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: goal w2 is not complete: alpha, beta, gamma, proof not shipped" ]
  [ "$(fingerprint)" = "$before" ]
  [ ! -e "$GH_LOG" ]
}

@test "complete: confirmed, it keeps only the metrics, makes the issues and deletes the rest" {
  a_finished_goal
  git branch slice/demo/alpha main
  local receipts before_files
  receipts=$(find "$(store)/receipts" -type f | sort | xargs md5sum | md5sum)
  before_files=$(find "$(store)" -type f | sort)
  complete_goal demo --confirm
  [ "$status" -eq 0 ]
  [ "$output" = "goal demo completed: metrics kept, 2 issues created (https://example.invalid/issues/1, https://example.invalid/issues/2), goals/demo/ deleted" ]
  [ ! -e "$(store)/goals/demo" ]
  [ -f "$(store)/goals/w2/roadmap.v1.md" ]
  [ "$(find "$(store)/receipts" -type f | sort | xargs md5sum | md5sum)" = "$receipts" ]
  [ "$(cat "$(store)/metrics/demo.toml")" = $'[slices.alpha]\nclone-ratio = 3\n\n[slices.beta]\nclone-ratio = 5\n\n[complete]\nissues = ["https://example.invalid/issues/1", "https://example.invalid/issues/2"]' ]
  [ "$(cat "$GH_LOG")" = $'issue create demo/alpha: second-opinion F1: a problem\nissue create demo/beta: informed-review F1: a problem' ]
  diff <({ grep -v '/goals/demo/' <<<"$before_files"; printf '%s\n' "$(store)/metrics/demo.toml"; } | sort) <(find "$(store)" -type f | sort)
  [ -n "$(git branch --list slice/demo/alpha)" ]
  [ -z "$(git status --porcelain)" ]
}

@test "complete: a failing gh deletes nothing and names the issues created" {
  a_finished_goal
  GH_FAIL_AT=2 complete_goal demo --confirm
  [ "$status" -eq 1 ]
  [ "$stderr" = 'shipit: cannot create the issue "demo/beta: informed-review F1: a problem": gh issue create failed (exit 1): gh: something went wrong; issues created so far: https://example.invalid/issues/1; nothing was deleted' ]
  [ -f "$(store)/goals/demo/roadmap.v1.md" ]
  [ -f "$(store)/goals/demo/logbook.jsonl" ]
  grep -q 'issues = \["https://example.invalid/issues/1"\]' "$(store)/metrics/demo.toml"
}

@test "complete: a rerun after a failure creates only the missing issues" {
  a_finished_goal
  GH_FAIL_AT=2 complete_goal demo --confirm
  [ "$status" -eq 1 ]
  complete_goal demo --confirm
  [ "$status" -eq 0 ]
  [ "$output" = "goal demo completed: metrics kept, 2 issues created (https://example.invalid/issues/1, https://example.invalid/issues/3), goals/demo/ deleted" ]
  [ "$(wc -l <"$GH_LOG")" -eq 3 ]
  [ ! -e "$(store)/goals/demo" ]
}

@test "complete: a goal name with a metrics file is refused by start" {
  a_finished_goal
  complete_goal demo --confirm
  [ "$status" -eq 0 ]
  add_goal demo
  plan_ready alpha
  run --separate-stderr "$SHIPIT_BIN" start alpha --goal demo
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: goal "demo" is already used (metrics/demo.toml): pick another goal name; if the remote branches are new, fetch first' ]
}

@test "complete: an unknown goal exits 2, and a kept file that is not a half-done run is refused" {
  a_finished_goal
  complete_goal nope
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no goal named "nope" in the store' ]
  mkdir -p "$(store)/metrics"
  printf '[alpha]\nclone-ratio = 1\n' >"$(store)/metrics/demo.toml"
  complete_goal demo --confirm
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: metrics/demo.toml exists and is not a half-done completion of this goal" ]
  [ -f "$(store)/goals/demo/roadmap.v1.md" ]
  [ ! -e "$GH_LOG" ]
}

@test "complete: a goal with no known issue makes no issue and still deletes" {
  make_slice_repo
  fake_gh
  local slice
  for slice in alpha beta gamma proof; do
    ship_entry "$slice"
  done
  complete_goal demo --confirm
  [ "$status" -eq 0 ]
  [ "$output" = "goal demo completed: metrics kept, 0 issues created (), goals/demo/ deleted" ]
  [ ! -e "$GH_LOG" ]
  [ "$(cat "$(store)/metrics/demo.toml")" = "[complete]
issues = []" ]
}

@test "complete: a slice named complete keeps its metrics beside the issues" {
  a_finished_goal
  sed -i 's/gamma/complete/g' "$(store)/goals/demo/roadmap.v1.md"
  ship_entry complete
  a_metric complete clone-ratio 7
  complete_goal demo --confirm
  [ "$status" -eq 0 ]
  [ "$(grep -c '^\[complete\]' "$(store)/metrics/demo.toml")" -eq 1 ]
  grep -q '^\[slices.complete\]' "$(store)/metrics/demo.toml"
  run --separate-stderr "$SHIPIT_BIN" metric clone-ratio 1
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
}
