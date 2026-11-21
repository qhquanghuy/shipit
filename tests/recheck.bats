#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

PLAIN_WARNING="::warning title=ShipIt::not a slice/* branch: size cap and setup comparison were not applied"

# start_repo [<check>]: a repo as the current directory, the setup on main with these check
# commands (a TOML array, default none).
start_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo"
  write_setup "$(fixture_with "gate.check=${1:-[]}")"
  SUMMARY="$BATS_TEST_TMPDIR/summary.md"
  export SUMMARY
}

# files <n>: n new files, committed.
files() {
  local i
  for i in $(seq "$1"); do echo "$i" >"f$i"; done
  git add -A
  git commit -q -m "add $1 files"
}

# run_recheck <branch>: stdout in $output, stderr in $stderr, the report in $SUMMARY.
run_recheck() {
  run --separate-stderr "$SHIPIT_BIN" recheck --base main --branch "$1" --summary "$SUMMARY"
}

last_line() {
  tail -n 1 "$1"
}

@test "61 files on slice/x with a receipt for HEAD: exit 1, check: failed (exit 1)" {
  start_repo
  git checkout -q -b slice/x
  files 61
  local tree store
  tree=$(git rev-parse 'HEAD^{tree}')
  store="$(git rev-parse --path-format=absolute --git-common-dir)/shipit/receipts/check"
  mkdir -p "$store"
  printf '{"kind":"check","version":1,"key":"%s"}\n' "$tree" >"$store/$tree.json"
  "$SHIPIT_BIN" verify >/dev/null
  run_recheck slice/x
  [ "$status" -eq 1 ]
  [[ $output == *"size: 61 files (cap 60)"* ]]
  [[ $stderr == *"too big: 61 files (cap 60)"* ]]
  [ "$(last_line "$SUMMARY")" = "check: failed (exit 1)" ]
  [ "$(wc -l <"$SUMMARY")" -eq 6 ]
}

@test "a slice branch that passes: summary says mode slice and check: passed" {
  start_repo
  git checkout -q -b slice/x
  files 2
  run_recheck slice/x
  [ "$status" -eq 0 ]
  [ "$(sed -n 2p "$SUMMARY")" = 'mode: slice (branch "slice/x"): size cap, setup comparison and coverage map apply' ]
  [ "$(sed -n 3p "$SUMMARY")" = 'gate (max_files and check): from the base branch "main"' ]
  [ "$(last_line "$SUMMARY")" = "check: passed" ]
  [[ $output != *"not a slice/* branch"* ]]
}

@test "61 files on fix/x: plain mode runs the checks and passes" {
  start_repo '["echo ran-the-check"]'
  git checkout -q -b fix/x
  files 61
  run_recheck fix/x
  [ "$status" -eq 0 ]
  [[ $output == *"check: echo ran-the-check"* ]]
  [[ $output == *"$PLAIN_WARNING"* ]]
  [[ $output != *"size:"* ]]
  [ "$(sed -n 2p "$SUMMARY")" = 'mode: plain (branch "fix/x" is not slice/*): only the base'"'"'s check commands ran' ]
  [ "$(sed -n 3p "$SUMMARY")" = 'checks: from the base branch "main"' ]
  [ "$(last_line "$SUMMARY")" = "check: passed" ]
}

@test "plain mode: a failing setup check fails the run" {
  start_repo '["exit 3"]'
  git checkout -q -b fix/x
  files 1
  run_recheck fix/x
  [ "$status" -eq 1 ]
  [[ $stderr == *"check failed (exit 3): exit 3"* ]]
  [ "$(last_line "$SUMMARY")" = "check: failed (exit 1)" ]
}

@test "plain mode runs the base's checks: a pull request that sets gate.check = [] still runs main's failing check and fails" {
  start_repo '["exit 4"]'
  git checkout -q -b fix/x
  write_setup "$(fixture_with 'gate.check=[]')"
  run_recheck fix/x
  [ "$status" -eq 1 ]
  [[ $stderr == *"check failed (exit 4): exit 4"* ]]
}

@test "a push to the base: plain mode, no plain warning" {
  start_repo
  run --separate-stderr "$SHIPIT_BIN" recheck --base main --branch main --summary "$SUMMARY"
  [ "$status" -eq 0 ]
  [[ $output != *"$PLAIN_WARNING"* ]]
  [ "$(sed -n 2p "$SUMMARY" | cut -c1-24)" = 'mode: plain (branch "mai' ]
}

@test "a changed setup key and a new workflow are reported, with one warning each" {
  start_repo
  git checkout -q -b fix/x
  write_setup "$(fixture_with 'gate.check=[]' 'agents.max_fix_rounds=3')"
  mkdir -p .github/workflows
  echo "on: push" >.github/workflows/x.yml
  git add -A
  git commit -q -m "add a workflow"
  run_recheck fix/x
  [ "$status" -eq 0 ]
  grep -qxF 'setup file: changed: "agents.max_fix_rounds"' "$SUMMARY"
  grep -q '^CI job: CHANGED: ".github/workflows/x.yml" ' "$SUMMARY"
  [ "$(grep -c '^::warning title=ShipIt::this pull request changes the setup file: changed: "agents.max_fix_rounds"$' <<<"$output")" -eq 1 ]
  [ "$(grep -c '^::warning title=ShipIt::this pull request changes the CI job: ".github/workflows/x.yml"$' <<<"$output")" -eq 1 ]
}

@test "a later change on main is not reported as the pull request's" {
  start_repo
  git checkout -q -b fix/x
  files 1
  git checkout -q main
  write_setup "$(fixture_with 'gate.check=[]' 'agents.max_fix_rounds=4')"
  git checkout -q fix/x
  run_recheck fix/x
  [ "$status" -eq 0 ]
  grep -qxF 'setup file: unchanged' "$SUMMARY"
}

@test "a base without a setup file: the adopting line and its warning" {
  local repo
  repo=$(make_repo)
  cd "$repo"
  SUMMARY="$BATS_TEST_TMPDIR/summary.md"
  git rm -q .config/shipit.toml
  git commit -q -m "no setup yet"
  git checkout -q -b slice/adopt
  mkdir -p .config
  fixture_with 'gate.check=[]' >.config/shipit.toml
  git add -A
  git commit -q -m "adopt ShipIt"
  run_recheck slice/adopt
  [ "$status" -eq 0 ]
  [ "$(sed -n 3p "$SUMMARY")" = 'adopting: "main" has no setup file, so this pull request may change the setup file only' ]
  grep -qxF 'setup file: added' "$SUMMARY"
  [[ $output == *'::warning title=ShipIt::this pull request adopts ShipIt: "main" has no setup file'* ]]
}

@test "a check that empties the summary file: the report is still there" {
  start_repo '[": > \"$SUMMARY\""]'
  git checkout -q -b slice/x
  files 1
  echo "earlier" >"$SUMMARY"
  run_recheck slice/x
  [ "$status" -eq 0 ]
  [ "$(head -n 1 "$SUMMARY")" = "## ShipIt recheck" ]
  [ "$(last_line "$SUMMARY")" = "check: passed" ]
}

@test "--summary keeps earlier text" {
  start_repo
  git checkout -q -b slice/x
  files 1
  echo "earlier" >"$SUMMARY"
  run_recheck slice/x
  [ "$status" -eq 0 ]
  [ "$(head -n 1 "$SUMMARY")" = "earlier" ]
  [ "$(sed -n 2p "$SUMMARY")" = "## ShipIt recheck" ]
}

@test "no setup file at HEAD: exit 2 and the summary ends check: failed (exit 2)" {
  start_repo
  git checkout -q -b slice/x
  git rm -q .config/shipit.toml
  git commit -q -m "drop the setup"
  run_recheck slice/x
  [ "$status" -eq 2 ]
  [ "$(last_line "$SUMMARY")" = "check: failed (exit 2)" ]
}

@test "--base naming no branch: exit 2 and recheck stopped:" {
  start_repo
  git checkout -q -b slice/x
  run --separate-stderr "$SHIPIT_BIN" recheck --base nope --branch slice/x --summary "$SUMMARY"
  [ "$status" -eq 2 ]
  [[ $(cat "$SUMMARY") == "recheck stopped: \"base branch \\\"nope\\\" not found"* ]]
  [ "$(wc -l <"$SUMMARY")" -eq 1 ]
}
