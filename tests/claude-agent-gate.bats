#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load claude-helpers

# A repo on main with the slice branch slice/v1/x, checked out, one commit ahead of main. Its
# setup has no check commands, so that shipit check is instant.
setup() {
  REPO=$(shipit_repo)
  cd "$REPO" || return 1
  git checkout -q -b slice/v1/x
  printf 'work\n' >work.txt
  git add work.txt
  git commit -q -m "work"
}

# agent <subagent_type> <prompt>: the review gate on an Agent call in the repo.
agent() {
  run_hook claude-agent-gate "$(agent_input "$REPO" "$1" "$2")" "${3:-}"
}

receipt() {
  "$SHIPIT_BIN" check >/dev/null 2>&1
}

@test "work-reviewer with a receipt passes" {
  receipt
  agent shipit:work-reviewer $'shipit-branch: slice/v1/x\nReview it.'
  [ -z "$output" ]
  agent shipit:tester $'shipit-branch: slice/v1/x\nDrive it.'
  [ -z "$output" ]
}

@test "work-reviewer without a receipt is denied and names shipit check" {
  agent shipit:work-reviewer $'shipit-branch: slice/v1/x\nReview it.'
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt: shipit:work-reviewer needs a check receipt: "*'Run "shipit check" on slice/v1/x, then start it again.' ]]
  [[ $(reason) == *"run shipit check"* ]]
}

@test "plan-reviewer passes with no receipt" {
  agent shipit:plan-reviewer "Review the build plan."
  [ -z "$output" ]
  agent general-purpose "Anything."
  [ -z "$output" ]
}

@test "no branch line: a clean slice branch with a receipt passes; an uncommitted change is denied" {
  receipt
  agent shipit:work-reviewer "Review it."
  [ -z "$output" ]
  printf 'more\n' >>work.txt
  agent shipit:work-reviewer "Review it."
  [ "$(decision)" = deny ]
  [[ $(reason) == *'Run "shipit check" on this working copy, then start it again.' ]]
}

@test "no branch line on main is denied naming shipit-branch" {
  git checkout -q main
  agent shipit:work-reviewer "Review it."
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt: name the slice: add a line \"shipit-branch: <branch>\" to the prompt of shipit:work-reviewer." ]]
}

@test "with the branch line an uncommitted change in cwd does not matter" {
  receipt
  printf 'more\n' >>work.txt
  agent shipit:work-reviewer $'shipit-branch: slice/v1/x\nReview it.'
  [ -z "$output" ]
  git checkout -q -- work.txt
  git checkout -q main
  printf 'untracked\n' >untracked.txt
  agent shipit:work-reviewer $'shipit-branch: slice/v1/x\nReview it.'
  [ -z "$output" ]
}

@test "shipit-branch must name a local slice branch" {
  receipt
  git tag slice/v1/tagged
  local branch
  for branch in HEAD main slice/v1/tagged slice/v1/missing origin/main "HEAD~1" "--all"; do
    agent shipit:work-reviewer "shipit-branch: $branch"
    [ "$(decision)" = deny ]
    [[ $(reason) == "ShipIt: \"shipit-branch: $branch\" must name a local slice branch, not a base branch or another revision." ]]
  done
  agent shipit:work-reviewer $'shipit-branch: slice/v1/x\nshipit-branch: main'
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt: the agent cannot start: the prompt names two branches." ]]
}

@test "bad stdin in a ShipIt project denies, and outside one allows" {
  run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" exec "$3" claude-agent-gate <<<"not json"' \
    _ "$BATS_TEST_TMPDIR" "$REPO" "$SHIPIT_BIN"
  [ "$status" -eq 0 ]
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt: the agent cannot start: "* ]]
  run_hook claude-agent-gate "not json"
  [ -z "$output" ]
}

@test "outside a ShipIt project every agent starts" {
  local plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  run_hook claude-agent-gate "$(agent_input "$plain" shipit:work-reviewer "Review it.")" "$plain"
  [ -z "$output" ]
}

@test "the review gate answers in under one second" {
  local start end input
  input=$(agent_input "$REPO" shipit:work-reviewer $'shipit-branch: slice/v1/x\nReview it.')
  start=$(date +%s%N)
  run_hook claude-agent-gate "$input"
  end=$(date +%s%N)
  [ "$(decision)" = deny ]
  [ $(((end - start) / 1000000)) -lt 1000 ]
  receipt
  start=$(date +%s%N)
  run_hook claude-agent-gate "$input"
  end=$(date +%s%N)
  [ -z "$output" ]
  [ $(((end - start) / 1000000)) -lt 1000 ]
}
