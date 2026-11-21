#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

ZEROS=0000000000000000000000000000000000000000

# start_repo: a repo as the current directory, with no check commands, on a branch slice/x
# with one committed file.
start_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo"
  write_setup "$(fixture_with 'gate.check=[]')"
  git checkout -q -b slice/x
  echo one >f1
  git add -A
  git commit -q -m "add f1"
}

# push_line <branch> [<commit>]: the line git feeds the hook for a push of that branch.
push_line() {
  printf 'refs/heads/%s %s refs/heads/%s %s\n' "$1" "$(git rev-parse "${2:-HEAD}")" "$1" "$ZEROS"
}

# hook <stdin text>: run the hook as git does.
hook() {
  run --separate-stderr bash -c '"$SHIPIT_BIN" pre-push origin /x <<<"$1"' _ "$1"
}

@test "a slice branch without a receipt: exit 1 naming the branch and run shipit check" {
  start_repo
  hook "$(push_line slice/x)"
  [ "$status" -eq 1 ]
  [[ $stderr == *"push blocked: no check receipt for slice/x ($(git rev-parse --short HEAD))"* ]]
  [[ $stderr == *"run shipit check"* ]]
}

@test "a branch not named slice/* passes without a receipt, with the notice" {
  start_repo
  git branch -m fix/x
  hook "$(push_line fix/x)"
  [ "$status" -eq 0 ]
  [ "$output" = "" ]
  [[ $stderr == *"shipit: fix/x is not a slice/* branch; not checked"* ]]
}

@test "after shipit check the same line passes: exit 0, no output" {
  start_repo
  "$SHIPIT_BIN" check >/dev/null
  hook "$(push_line slice/x)"
  [ "$status" -eq 0 ]
  [ "$output" = "" ]
  [ "$stderr" = "" ]
}

@test "one line with a receipt and one without: exit 1" {
  start_repo
  "$SHIPIT_BIN" check >/dev/null
  local good
  good=$(git rev-parse HEAD)
  git checkout -q -b slice/y
  echo two >f2
  git add -A
  git commit -q -m "add f2"
  hook "$(push_line slice/x "$good")"$'\n'"$(push_line slice/y)"
  [ "$status" -eq 1 ]
  [[ $stderr == *"no check receipt for slice/y"* ]]
}

@test "a three-field line: exit 2 with the line as JSON" {
  start_repo
  hook "refs/heads/slice/x $(git rev-parse HEAD) refs/heads/slice/x"
  [ "$status" -eq 2 ]
  [[ $stderr == *'unexpected input from git: "refs/heads/slice/x '* ]]
}

@test "a tag push from a branch without a setup file passes" {
  local repo
  repo=$(make_repo)
  cd "$repo"
  git rm -q .config/shipit.toml
  git commit -q -m "drop the setup"
  hook "refs/tags/v1 $(git rev-parse HEAD) refs/tags/v1 $ZEROS"
  [ "$status" -eq 0 ]
  [ "$stderr" = "" ]
}

@test "a delete push passes without reading anything" {
  local repo
  repo=$(make_repo)
  cd "$repo"
  hook "(delete) $ZEROS refs/heads/slice/x $(git rev-parse HEAD)"
  [ "$status" -eq 0 ]
  [ "$output" = "" ]
  [ "$stderr" = "" ]
}

@test "a branch whose commit has no setup file passes with the notice" {
  local repo
  repo=$(make_repo)
  cd "$repo"
  git checkout -q -b slice/x
  git rm -q .config/shipit.toml
  git commit -q -m "drop the setup"
  hook "$(push_line slice/x)"
  [ "$status" -eq 0 ]
  [[ $stderr == *"shipit: slice/x has no setup file at $(git rev-parse --short HEAD); not checked (ShipIt is not adopted there)"* ]]
}

@test "an invalid setup at the pushed commit: exit 2" {
  start_repo
  printf 'version = 1\n' >.config/shipit.toml
  git commit -q -am "break the setup"
  hook "$(push_line slice/x)"
  [ "$status" -eq 2 ]
  [[ $stderr == *" is invalid:"* ]]
}

@test "a push of the base branch passes without a receipt" {
  start_repo
  hook "$(push_line main "$(git rev-parse main)")"
  [ "$status" -eq 0 ]
}

@test "a receipt made with --base: the push passes and stderr names the base" {
  start_repo
  "$SHIPIT_BIN" check --base main >/dev/null
  hook "$(push_line slice/x)"
  [ "$status" -eq 0 ]
  [[ $stderr == *"shipit: slice/x: valid receipt for "*"(--base refs/heads/main)"* ]]
}

@test "two arguments are required" {
  run --separate-stderr "$SHIPIT_BIN" pre-push origin </dev/null
  [ "$status" -eq 2 ]
  [[ $stderr == *"usage: shipit pre-push <remote> <url>"* ]]
}
