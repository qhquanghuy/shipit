#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

FIXTURE="$SHIPIT_SRC/tests/fixtures/setup-valid.json"

# start_repo [jq filter]: a repo as the current directory. The setup file (the
# fixture changed by the filter, default: no check commands) is committed on
# main first, then the work happens on branch feature, so the setup is not a
# changed file.
start_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo"
  write_setup "$(jq -c "${1:-.check = []}" "$FIXTURE")"
  git checkout -q -b feature
}

# commit_files <count> [prefix]: commit <count> new files.
commit_files() {
  local i
  for i in $(seq 1 "$1"); do
    printf '%s\n' "$i" >"${2:-f}$i"
  done
  git add -A
  git commit -q -m "add files"
}

# checks <command>...: a jq filter that sets the check commands.
checks() {
  jq -cn '$ARGS.positional' --args "$@"
}

# run_check <args>: run shipit check, stdout in $output, stderr in $stderr.
run_check() {
  run --separate-stderr "$SHIPIT_SRC/bin/shipit" check "$@"
}

store() {
  git rev-parse --path-format=absolute --git-common-dir | sed 's|$|/shipit|'
}

no_receipt() {
  [ ! -e "$(store)/receipts" ]
}

LOG="$BATS_TEST_TMPDIR/log"

@test "an uncommitted change exits 3 and writes no receipt" {
  start_repo
  commit_files 1
  echo more >>f1
  run_check
  [ "$status" -eq 3 ]
  [[ $stderr == "shipit: uncommitted changes: commit first"* ]]
  no_receipt
}

@test "an untracked file exits 3 and writes no receipt" {
  start_repo
  commit_files 1
  echo new >untracked
  run_check
  [ "$status" -eq 3 ]
  no_receipt
}

@test "61 changed files with cap 60 exits 1, runs no command, writes no receipt" {
  start_repo ".check = $(checks "echo ran >> '$LOG'")"
  commit_files 61
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"61 files (cap 60)"* ]]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "exactly 60 changed files passes" {
  start_repo
  commit_files 60
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 60 files (cap 60)"* ]]
}

@test "a file named with spaces and one with a newline count one each" {
  start_repo ".check = [] | .cap = 2"
  printf 'a\n' >"a file with spaces"
  printf 'b\n' >$'a\nnewline'
  git add -A
  git commit -q -m "odd names"
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 2 files (cap 2)"* ]]
  jq -e '.size.files == 2' "$(store)"/receipts/check/*.json
}

@test "commands run in order" {
  start_repo ".check = $(checks "echo one >> '$LOG'" "echo two >> '$LOG'" "echo three >> '$LOG'")"
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  [ "$(cat "$LOG")" = $'one\ntwo\nthree' ]
}

@test "the first failing command stops the run: exit 1, named, no receipt" {
  start_repo ".check = $(checks true false 'touch ran-after')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"check failed"*"false"* ]]
  [ ! -e ran-after ]
  no_receipt
}

@test "command output is shown" {
  start_repo ".check = $(checks 'echo visible-out' 'echo visible-err >&2')"
  commit_files 1
  run --separate-stderr "$SHIPIT_SRC/bin/shipit" check
  [ "$status" -eq 0 ]
  [[ $output == *visible-out* ]]
  [[ $stderr == *visible-err* ]]
}

@test "a check that creates a file fails and writes no receipt" {
  start_repo ".check = $(checks 'touch created')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"a check changed the work: created"* ]]
  no_receipt
}

@test "a check that modifies a tracked file fails and writes no receipt" {
  start_repo ".check = $(checks 'echo changed >> f1')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"a check changed the work: f1"* ]]
  no_receipt
}

@test "a check that makes a commit fails and writes no receipt" {
  start_repo ".check = $(checks 'echo x > made && git add made && git commit -q -m sneaky')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"a check changed the work: made"* ]]
  no_receipt
}

@test "a check that makes an empty commit fails and writes no receipt" {
  start_repo ".check = $(checks 'git commit -q --allow-empty -m sneaky')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"a check changed the work"* ]]
  no_receipt
}

@test "a pass writes a receipt that matches the commit and tree" {
  start_repo ".check = $(checks true 'echo hi')"
  commit_files 3
  local tree commit file
  tree=$(git rev-parse 'HEAD^{tree}')
  commit=$(git rev-parse HEAD)
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"receipt written for ${tree:0:7}"* ]]
  file="$(store)/receipts/check/$tree.json"
  [ -f "$file" ]
  jq -e --arg tree "$tree" --arg commit "$commit" '
    .kind == "check" and .version == 1 and .key == $tree and .commit == $commit
    and .size == {files: 3, cap: 60} and .commands == ["true", "echo hi"]
    and .tool == "shipit 0.1.0"
    and (.time | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"))' "$file"
  # no temporary file is left behind
  [ "$(ls -A "$(store)/receipts/check")" = "$tree.json" ]
}

@test "only origin/main present: size is counted against it" {
  local up
  up=$(make_repo)
  cd "$up"
  write_setup "$(jq -c '.check = []' "$FIXTURE")"
  git clone -q "$up" "$BATS_TEST_TMPDIR/clone"
  cd "$BATS_TEST_TMPDIR/clone"
  git config user.name t
  git config user.email t@example.invalid
  git config commit.gpgsign false
  git checkout -q -b feature
  commit_files 4
  git branch -q -D main
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 4 files (cap 60)"* ]]
}

@test "origin/main and a different main both present: origin/main is used" {
  local up
  up=$(make_repo)
  cd "$up"
  write_setup "$(jq -c '.check = [] | .cap = 6' "$FIXTURE")"
  git clone -q "$up" "$BATS_TEST_TMPDIR/clone"
  cd "$BATS_TEST_TMPDIR/clone"
  git config user.name t
  git config user.email t@example.invalid
  git config commit.gpgsign false
  commit_files 5 m
  git checkout -q -b feature
  commit_files 3 f
  # against local main: 3 files; against origin/main: 8 files
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"8 files (cap 6)"* ]]
  [[ $stderr == *origin/main* ]]
}

@test "no base branch at all: exit 2 with the message" {
  start_repo
  commit_files 1
  git branch -q -D main
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == *'base branch "main" not found (looked for origin/main and main)'* ]]
  no_receipt
}

@test "unrelated history: exit 2 no shared history" {
  start_repo
  git checkout -q --orphan other
  git commit -q -m "unrelated root"
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == *"no shared history with main"* ]]
  no_receipt
}

@test "from a subfolder, commands run at the top" {
  start_repo ".check = $(checks "pwd > '$LOG'")"
  mkdir sub
  echo x >sub/x
  git add -A
  git commit -q -m sub
  local top
  top=$(pwd -P)
  cd sub
  run_check
  [ "$status" -eq 0 ]
  [ "$(cat "$LOG")" = "$top" ]
}

@test "an empty check list passes on size alone" {
  start_repo
  commit_files 2
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"receipt written for"* ]]
  [ -f "$(store)/receipts/check/$(git rev-parse 'HEAD^{tree}').json" ]
}

@test "a missing setup file exits 2 and names shipit init" {
  start_repo
  git rm -q .config/shipit.json
  git commit -q -m "drop setup"
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == *"shipit init"* ]]
}

@test "unknown arguments and a bare --cap-from exit 2" {
  start_repo
  run_check --nope
  [ "$status" -eq 2 ]
  run_check --cap-from
  [ "$status" -eq 2 ]
  run_check --cap-from no-such-ref
  [ "$status" -eq 2 ]
  [[ $stderr == *"not a commit"* ]]
}

@test "--cap-from main uses main's cap when the branch raised it" {
  start_repo
  write_setup "$(jq -c '.check = [] | .cap = 100' "$FIXTURE")"
  commit_files 61
  run_check
  [ "$status" -eq 0 ]
  git reset -q --hard HEAD
  run_check --cap-from main
  [ "$status" -eq 1 ]
  [[ $stderr == *"62 files (cap 60)"* ]]
}

@test "--cap-from a ref without a setup file prints a notice and uses this cap" {
  start_repo
  git checkout -q --orphan nosetup
  git rm -rq -f .
  echo readme >README
  git add README
  git commit -q -m "no setup here"
  git checkout -q feature
  commit_files 2
  run_check --cap-from nosetup
  [ "$status" -eq 0 ]
  [[ $output == *"no setup file at nosetup; using this one"* ]]
  [[ $output == *"size: 2 files (cap 60)"* ]]
}
