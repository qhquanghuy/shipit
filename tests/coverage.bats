#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# bare_repo <program>: a repo with no store whose only check is the coverage line over <program>.
bare_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo" || return 1
  write_setup "$(fixture_with "gate.check=[\"$SHIPIT_BIN coverage -- $1\"]")"
}


run_check() {
  run --separate-stderr "$SHIPIT_BIN" check "$@"
}

@test "coverage: every named test ran: writes the record and passes" {
  suite_repo
  git checkout -q -b slice/demo/beta
  commit_map demo beta 'DW2 = ["one", "receipt: first"]'
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"coverage demo/beta: demo:DW2 ✓ (2 tests)"* ]]
  local tree
  tree=$(git rev-parse 'HEAD^{tree}')
  [ "$(jq -c '[.kind, .key, .check, .maps["demo/beta"].DW2]' "$(store)/receipts/coverage/$tree.json")" = \
    "[\"coverage\",\"$tree\",\"$SHIPIT_BIN coverage -- bash fake-suite\",[\"one\",\"receipt: first\"]]" ]
}

@test "coverage: a named test that did not run fails check with its done-when item" {
  suite_repo
  git checkout -q -b slice/demo/beta
  commit_map demo beta 'DW2 = ["ghost"]'
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"DW2: test not found: ghost (.shipit/coverage/demo/beta.toml)"* ]]
  [[ $stderr == *"(one coverage line must wrap every test run)"* ]]
  [ ! -e "$(store)/receipts/check" ]
  commit_map demo beta 'DW2 = ["receipt: skipped"]'
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"DW2: test not found: receipt: skipped"* ]]
}

@test "coverage: on a slice/* branch with no store, a change with code and no new map fails" {
  bare_repo true
  git checkout -q -b work
  printf 'code\n' >code.txt
  git add code.txt
  git commit -q -m "code"
  git checkout -q --detach
  SHIPIT_BRANCH=slice/v1/x run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"this change adds no done-when map: commit .shipit/coverage/v1/x.toml (see shipit help coverage)"* ]]
  [ ! -e "$(store)/receipts/check" ]
}

@test "coverage: on another branch it runs the tests and passes with a notice" {
  bare_repo true
  git checkout -q -b work
  printf 'code\n' >code.txt
  git add code.txt
  git commit -q -m "code"
  SHIPIT_BRANCH=feature/x run_check
  [ "$status" -eq 0 ]
  [[ $stderr == *"shipit: feature/x is not a slice/* branch; done-when coverage is not checked"* ]]
  [ ! -e "$(store)/receipts/coverage" ]
  git checkout -q main
  write_setup "$(fixture_with "gate.check=[\"$SHIPIT_BIN coverage -- false\"]")"
  git checkout -q work
  git rebase -q main
  SHIPIT_BRANCH=feature/x run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"the test command failed (exit 1)"* ]]
}

@test "coverage: an empty map passes for a change that meets nothing" {
  bare_repo true
  git checkout -q -b slice/v1/x
  commit_map v1 x ''
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"coverage v1/x: no done-when item"* ]]
}

@test "coverage: a setup-only change needs no map" {
  bare_repo true
  git checkout -q -b slice/v1/x
  write_setup "$(fixture_with "gate.check=[\"$SHIPIT_BIN coverage -- true\"]" 'gate.max_files=61')"
  run_check
  [ "$status" -eq 0 ]
  [[ $stderr != *"adds no done-when map"* ]]
}

@test "coverage: reads the map from HEAD, not the working tree or the store" {
  suite_repo
  git checkout -q -b slice/demo/beta
  commit_map demo beta 'DW2 = ["one"]'
  mkdir -p .shipit/coverage/demo
  printf 'DW3 = ["ghost"]\n' >.shipit/coverage/demo/gamma.toml
  printf '.shipit/coverage/demo/gamma.toml\n' >>"$(git rev-parse --git-path info/exclude)"
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"coverage demo/beta: demo:DW2 ✓ (1 test)"* ]]
  [[ $output != *"gamma"* ]]
}

@test "coverage: the local map must match meets and the build plan" {
  suite_repo
  git checkout -q -b slice/demo/beta
  commit_map demo beta 'DW9 = ["one"]'
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"DW2: in meets but not in the map"* ]]
  [[ $stderr == *"DW9: in the map but not in meets"* ]]
  mkdir -p "$(store)/goals/demo/slices/beta"
  cp "$SHIPIT_SRC/tests/fixtures/build-plan.md" "$(store)/goals/demo/slices/beta/build-plan.md"
  commit_map demo beta 'DW2 = ["one"]'
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"the map differs from the build plan's \"## Done-when coverage\" block"* ]]
  commit_map demo beta 'DW2 = ["receipt: first", "one"]'
  run_check
  [ "$status" -eq 0 ]
}

@test "coverage: outside shipit check it exits 2" {
  suite_repo
  run --separate-stderr "$SHIPIT_BIN" coverage -- true
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: shipit coverage runs as a check command of shipit check (SHIPIT_MERGE_BASE is not set)" ]
}

@test "coverage: a failing test command fails it" {
  suite_repo false
  git checkout -q -b slice/demo/beta
  commit_map demo beta 'DW2 = ["one"]'
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"the test command failed (exit 1)"* ]]
  [ ! -e "$(store)/receipts/check" ]
}

@test "coverage: a command that cannot start gives its exit code" {
  suite_repo /nonexistent/program
  git checkout -q -b slice/demo/beta
  commit_map demo beta 'DW2 = ["one"]'
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"cannot run /nonexistent/program"* ]]
  [[ $stderr == *"the test command failed (exit 127)"* ]]
}

@test "coverage: in CI, only this branch's own map counts, not another map added or an older one removed" {
  bare_repo true
  commit_map v1 old ''
  git checkout -q -b work
  git rm -q .shipit/coverage/v1/old.toml
  commit_map zz y ''
  git checkout -q --detach
  SHIPIT_BRANCH=slice/v1/x run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"this change adds no done-when map: commit .shipit/coverage/v1/x.toml (see shipit help coverage)"* ]]
  [ ! -e "$(store)/receipts/check" ]
  commit_map v1 x ''
  SHIPIT_BRANCH=slice/v1/x run_check
  [ "$status" -eq 0 ]
  [[ $output == *"coverage v1/x: no done-when item"* ]]
}

@test "coverage: a change of the files the base lists needs no map" {
  bare_repo true
  printf 'deny\n' >deny.toml
  git add deny.toml
  git commit -q -m "a config"
  write_setup "$(fixture_with "gate.check=[\"$SHIPIT_BIN coverage -- true\"]" | sed '/^check = /a setup_files = ["deny.toml"]')"
  git checkout -q -b slice/v1/x
  printf 'weaker\n' >deny.toml
  git commit -q -am "config"
  run_check
  [ "$status" -eq 0 ]
  [[ $stderr == "shipit: this slice changes the checks (deny.toml); setup change"* ]]
  [[ $stderr != *"adds no done-when map"* ]]
}

@test "coverage: the list is the base tip's, also when the base added it after the slice forked" {
  bare_repo true
  printf 'deny\n' >deny.toml
  git add deny.toml
  git commit -q -m "a config"
  git checkout -q -b slice/v1/x
  local listed
  listed=$(fixture_with "gate.check=[\"$SHIPIT_BIN coverage -- true\"]" | sed '/^check = /a setup_files = ["deny.toml"]')
  write_setup "$listed"
  printf 'weaker\n' >deny.toml
  git commit -q -am "list and weaken"
  git checkout -q main
  git commit -q --allow-empty -m "the base moves on"
  write_setup "$listed"
  git checkout -q slice/v1/x
  run_check
  [ "$status" -eq 0 ]
  [[ $stderr == *"this slice changes the checks (deny.toml); setup change"* ]]
  [[ $stderr != *"adds no done-when map"* ]]
}
