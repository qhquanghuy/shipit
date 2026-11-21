#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# start_repo: a repo as the current directory, with no check commands, on
# branch feature with one committed file.
start_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo"
  write_setup "$(jq -c '.check = []' "$SHIPIT_SRC/tests/fixtures/setup-valid.json")"
  git checkout -q -b feature
  echo one >f1
  git add -A
  git commit -q -m "add f1"
}

# run_verify <args>: stdout in $output, stderr in $stderr.
run_verify() {
  run --separate-stderr "$SHIPIT_SRC/bin/shipit" verify "$@"
}

do_check() {
  "$SHIPIT_SRC/bin/shipit" check >/dev/null
}

short_tree() {
  git rev-parse --short "${1:-HEAD}^{tree}"
}

receipt_file() {
  git rev-parse --path-format=absolute --git-common-dir |
    sed "s|\$|/shipit/receipts/check/$(git rev-parse "${1:-HEAD}^{tree}").json|"
}

@test "after check, verify exits 0 and names the tree" {
  start_repo
  do_check
  run_verify
  [ "$status" -eq 0 ]
  [ "$output" = "valid receipt for $(short_tree)" ]
}

@test "verify works from a subfolder" {
  start_repo
  do_check
  mkdir sub
  cd sub
  run_verify
  [ "$status" -eq 0 ]
}

@test "a new commit with changed content has no receipt" {
  start_repo
  do_check
  echo two >f2
  git add -A
  git commit -q -m "add f2"
  run_verify
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: no receipt for this content: run shipit check" ]
}

@test "an amended message keeps the receipt valid" {
  start_repo
  do_check
  git commit -q --amend -m other
  run_verify
  [ "$status" -eq 0 ]
}

@test "an uncommitted change exits 1" {
  start_repo
  do_check
  echo more >>f1
  run_verify
  [ "$status" -eq 1 ]
  [[ $stderr == "shipit: uncommitted changes"* ]]
}

@test "an untracked file exits 1" {
  start_repo
  do_check
  echo new >untracked
  run_verify
  [ "$status" -eq 1 ]
  [[ $stderr == "shipit: uncommitted changes"* ]]
}

@test "verify of an old commit is valid while HEAD has no receipt" {
  start_repo
  do_check
  old=$(git rev-parse HEAD)
  echo two >f2
  git add -A
  git commit -q -m "add f2"
  run_verify
  [ "$status" -eq 1 ]
  run_verify "$old"
  [ "$status" -eq 0 ]
  [ "$output" = "valid receipt for $(short_tree "$old")" ]
}

@test "verify of a commit ignores the working tree" {
  start_repo
  do_check
  echo more >>f1
  run_verify HEAD
  [ "$status" -eq 0 ]
}

@test "an unknown commit exits 2" {
  start_repo
  run_verify nosuchthing
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: 'nosuchthing' is not a commit"* ]]
}

@test "an argument that looks like an option exits 2" {
  start_repo
  run_verify --all
  [ "$status" -eq 2 ]
}

@test "two arguments exit 2" {
  start_repo
  run_verify HEAD HEAD
  [ "$status" -eq 2 ]
  [[ $stderr == *"usage: shipit verify [commit]"* ]]
}

@test "a receipt from another working copy is valid" {
  start_repo
  do_check
  git worktree add -q -b second "$BATS_TEST_TMPDIR/second" HEAD
  cd "$BATS_TEST_TMPDIR/second"
  run_verify
  [ "$status" -eq 0 ]
  [ "$output" = "valid receipt for $(short_tree)" ]
}

@test "an empty receipt file exits 1" {
  start_repo
  do_check
  : >"$(receipt_file)"
  run_verify
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: no receipt for this content: run shipit check" ]
}

@test "a non-JSON receipt file exits 1" {
  start_repo
  do_check
  echo 'not json {' >"$(receipt_file)"
  run_verify
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: no receipt for this content: run shipit check" ]
}

@test "a receipt with a different key exits 1" {
  start_repo
  do_check
  jq '.key = "0000000000000000000000000000000000000000"' "$(receipt_file)" >"$BATS_TEST_TMPDIR/r.json"
  cp "$BATS_TEST_TMPDIR/r.json" "$(receipt_file)"
  run_verify
  [ "$status" -eq 1 ]
}

@test "receipts of the wrong shape exit 1" {
  start_repo
  do_check
  local bad
  for bad in '[]' 'null' '"text"' '42' '{"kind":"review","version":1}' '{"kind":"check","version":2}' \
    "{\"kind\":\"check\",\"version\":1,\"key\":\"$(git rev-parse 'HEAD^{tree}')\"} {}"; do
    printf '%s\n' "$bad" >"$(receipt_file)"
    run_verify
    [ "$status" -eq 1 ]
  done
}

@test "a directory where the receipt should be exits 1" {
  start_repo
  do_check
  rm "$(receipt_file)"
  mkdir "$(receipt_file)"
  run_verify
  [ "$status" -eq 1 ]
}

@test "a missing setup file exits 2 with exactly one message" {
  start_repo
  do_check
  git rm -q .config/shipit.json
  git commit -q -m "drop setup"
  run_verify
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at .config/shipit.json (see docs/setup.md)" ]
  run_verify HEAD~1
  [ "$status" -eq 0 ]
}

@test "verify of a commit reads the setup committed there, never the working tree" {
  start_repo
  do_check
  echo '{' >.config/shipit.json
  run_verify HEAD
  [ "$status" -eq 0 ]
  run_verify
  [ "$status" -eq 1 ]
  [[ $stderr == "shipit: uncommitted changes"* ]]
}

@test "an empty commit argument is a usage error" {
  start_repo
  run_verify ""
  [ "$status" -eq 2 ]
  [[ $stderr == *"usage: shipit verify [commit]"* ]]
}

@test "an invalid setup at the commit exits 2" {
  start_repo
  do_check
  git checkout -q -b broken
  write_setup '{"version": 2}'
  run_verify
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: .config/shipit.json is invalid:"* ]]
  run_verify feature
  [ "$status" -eq 0 ]
}

@test "verify with no receipts leaves no shipit folder behind" {
  start_repo
  run_verify
  [ "$status" -eq 1 ]
  [ ! -e "$(git rev-parse --path-format=absolute --git-common-dir)/shipit" ]
}
