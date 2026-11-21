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
  run --separate-stderr "$SHIPIT_BIN" verify "$@"
}

do_check() {
  "$SHIPIT_BIN" check >/dev/null
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

@test "verify needs no setup file: a commit without one has no receipt, and says so" {
  start_repo
  do_check
  git rm -q .config/shipit.json
  git commit -q -m "drop setup"
  run_verify
  [ "$status" -eq 1 ]
  [[ $stderr == "shipit: no receipt for this content"* ]]
  run_verify HEAD~1
  [ "$status" -eq 0 ]
}

@test "verify never reads the setup: a receipt is valid for its tree even if the setup is broken" {
  start_repo
  do_check
  local receipt tree
  receipt=$(receipt_file)
  git checkout -q -b broken
  write_setup '{"version": 2'
  tree=$(git rev-parse 'HEAD^{tree}')
  jq -c --arg key "$tree" '.key = $key' "$receipt" >"${receipt%/*}/$tree.json"
  run_verify
  [ "$status" -eq 0 ]
  [ "$output" = "valid receipt for $(short_tree)" ]
  run_verify feature
  [ "$status" -eq 0 ]
  echo '{' >.config/shipit.json
  run_verify HEAD
  [ "$status" -eq 0 ]
}

@test "verify with no argument refuses a skip-worktree file, like check" {
  start_repo
  do_check
  git update-index --skip-worktree f1
  echo hidden >>f1
  run_verify
  [ "$status" -eq 1 ]
  [[ $stderr == *"marked skip-worktree or assume-unchanged"*"(f1)"*"git update-index --no-skip-worktree --no-assume-unchanged"* ]]
  run_verify HEAD
  [ "$status" -eq 0 ]
}

@test "verify with no argument refuses an assume-unchanged file with the same message as check" {
  start_repo
  do_check
  git update-index --assume-unchanged f1
  run_verify
  [ "$status" -eq 1 ]
  local verify_msg="$stderr"
  run --separate-stderr "$SHIPIT_BIN" check
  [ "$status" -eq 3 ]
  [ "$stderr" = "$verify_msg" ]
  [[ $verify_msg == *"(f1)"* ]]
}

@test "an empty commit argument is a usage error" {
  start_repo
  run_verify ""
  [ "$status" -eq 2 ]
  [[ $stderr == *"usage: shipit verify [commit]"* ]]
}

@test "verify with no receipts leaves no shipit folder behind" {
  start_repo
  run_verify
  [ "$status" -eq 1 ]
  [ ! -e "$(git rev-parse --path-format=absolute --git-common-dir)/shipit" ]
}
