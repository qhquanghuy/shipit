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

@test "a missing setup file exits 2 with exactly one message" {
  start_repo
  git rm -q .config/shipit.json
  git commit -q -m "drop setup"
  run_check
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at .config/shipit.json (see docs/setup.md)" ]
}

@test "arguments exit 2, including the removed --cap-from" {
  start_repo
  run_check --nope
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: unknown argument '--nope'; usage: shipit check" ]
  run_check --cap-from main
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: unknown argument '--cap-from'; usage: shipit check" ]
}

@test "a failing size count exits 2 with exactly one message and no receipt" {
  start_repo
  commit_files 1
  local shim="$BATS_TEST_TMPDIR/shim" real
  real=$(command -v git)
  mkdir "$shim"
  printf '#!%s\n[[ $1 == diff ]] && exit 128\nexec %q "$@"\n' "$BASH" "$real" >"$shim/git"
  chmod +x "$shim/git"
  PATH="$shim:$PATH" run_check
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: cannot count the changed files since "* ]]
  [ "$(printf '%s\n' "$stderr" | wc -l)" -eq 1 ]
  [[ $output != *"size:"* ]]
  no_receipt
}

@test "a branch that raises the cap is refused: exit 1, no command, no receipt" {
  start_repo ".check = $(checks "echo ran >> '$LOG'")"
  write_setup "$(jq -c ".check = $(checks "echo ran >> '$LOG'") | .cap = 100" "$FIXTURE")"
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: this slice weakens the checks (cap raised from 60 to 100); the owner must merge that setup change on main first" ]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "a branch that drops a check command is refused" {
  start_repo ".check = $(checks 'echo one' 'echo two')"
  write_setup "$(jq -c ".check = $(checks 'echo two')" "$FIXTURE")"
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: this slice weakens the checks (check command removed: echo one); the owner must merge that setup change on main first" ]
  no_receipt
}

@test "a branch that raises the cap and drops a check names both" {
  start_repo ".check = $(checks 'echo one')"
  write_setup "$(jq -c '.check = [] | .cap = 61' "$FIXTURE")"
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"(cap raised from 60 to 61; check command removed: echo one)"* ]]
}

@test "a branch that lowers the cap, adds a check or reorders them is fine" {
  start_repo ".check = $(checks 'echo one' 'echo two')"
  write_setup "$(jq -c ".check = $(checks 'echo two' 'echo extra' 'echo one') | .cap = 50" "$FIXTURE")"
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 2 files (cap 50)"* ]]
}

@test "the cap in the receipt is the branch's own" {
  start_repo
  write_setup "$(jq -c '.check = [] | .cap = 40' "$FIXTURE")"
  run_check
  [ "$status" -eq 0 ]
  jq -e '.size.cap == 40' "$(store)"/receipts/check/*.json
}

@test "a base setup of another schema is compared on cap and check only" {
  start_repo
  git checkout -q main
  write_setup '{"cap": 60, "check": [], "from_the_future": {"a": 1}}'
  git checkout -q feature
  git merge -q --ff-only main
  write_setup "$(jq -c '.check = []' "$FIXTURE")"
  run_check
  [ "$status" -eq 0 ]
}

@test "a base setup with a bad cap or check is refused: exit 2" {
  start_repo
  git checkout -q main
  write_setup '{"cap": "60", "check": [1]}'
  git checkout -q feature
  git merge -q --ff-only main
  write_setup "$(jq -c '.check = []' "$FIXTURE")"
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: main:.config/shipit.json is invalid:"$'\n'".cap: must be an integer of at least 1"$'\n'".check: must be an array of strings" ]]
  git checkout -q main
  write_setup '{"cap": 60'
  git checkout -q feature
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: main:.config/shipit.json is not valid JSON" ]]
}

@test "a base without a setup file: a notice on stderr, then the check goes on" {
  local repo
  repo=$(mktemp -d "$BATS_TEST_TMPDIR/repo.XXXXXX")
  cd "$repo"
  git init -q -b main
  git config user.name t
  git config user.email t@example.invalid
  git config commit.gpgsign false
  echo readme >README
  git add README
  git commit -q -m "first"
  git checkout -q -b feature
  write_setup "$(jq -c '.check = []' "$FIXTURE")"
  commit_files 2
  run_check
  [ "$status" -eq 0 ]
  [ "$stderr" = "shipit: no setup file on main; nothing to compare" ]
  [[ $output == *"size: 3 files (cap 60)"* ]]
}

@test "the base setup is read from origin/<base> when it exists" {
  local up
  up=$(make_repo)
  cd "$up"
  write_setup "$(jq -c '.check = [] | .cap = 20' "$FIXTURE")"
  git clone -q "$up" "$BATS_TEST_TMPDIR/clone"
  cd "$BATS_TEST_TMPDIR/clone"
  git config user.name t
  git config user.email t@example.invalid
  git config commit.gpgsign false
  git checkout -q -b feature
  write_setup "$(jq -c '.check = [] | .cap = 30' "$FIXTURE")"
  git checkout -q main
  write_setup "$(jq -c '.check = [] | .cap = 100' "$FIXTURE")"
  git checkout -q feature
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"cap raised from 20 to 30"* ]]
}

@test "weakenings and the change verdict are decided in lib/check.jq" {
  run jq -L "$SHIPIT_SRC/lib" -nc 'include "check";
    [({base: {cap: 5, check: ["a", "b", "a"]}, branch: {cap: 5, check: ["b"]}} | weakenings),
     ({base: {cap: 5, check: []}, branch: {cap: 6, check: ["x"]}} | weakenings)]'
  [ "$output" = '[["check command removed: a"],["cap raised from 5 to 6"]]' ]
  run jq -L "$SHIPIT_SRC/lib" -nc 'include "check";
    {dirty: false, tree_before: "t", tree_after: "t", commit_before: "c", commit_after: "c"} as $ok
    | [$ok, ($ok | .dirty = true), ($ok | .tree_after = "u"), ($ok | .commit_after = "d"),
       ($ok | .dirty = true | .tree_after = "u")] | map(change_verdict | .kind)'
  [ "$output" = '["none","dirty","committed","moved_head","dirty"]' ]
}

@test "a skip-worktree file with local edits exits 3 and names the file" {
  start_repo
  commit_files 2
  git update-index --skip-worktree f1
  echo hidden >>f1
  [ -z "$(git status --porcelain)" ]
  run_check
  [ "$status" -eq 3 ]
  [[ $stderr == *"marked skip-worktree or assume-unchanged"*"(f1)"*"git update-index --no-skip-worktree --no-assume-unchanged"* ]]
  no_receipt
}

@test "an assume-unchanged file with local edits exits 3 and names the file" {
  start_repo
  commit_files 2
  git update-index --assume-unchanged f2
  echo hidden >>f2
  run_check
  [ "$status" -eq 3 ]
  [[ $stderr == *"(f2)"* ]]
  no_receipt
}

@test "a skip-worktree setup file cannot change what check reads" {
  start_repo
  commit_files 1
  git update-index --skip-worktree .config/shipit.json
  jq '.check = ["touch ran"] | .cap = 1000' "$FIXTURE" >.config/shipit.json
  run_check
  [ "$status" -eq 3 ]
  [ ! -e ran ]
  no_receipt
}

@test "after the flag is cleared, check passes again" {
  start_repo
  commit_files 1
  git update-index --skip-worktree f1
  run_check
  [ "$status" -eq 3 ]
  git update-index --no-skip-worktree f1
  run_check
  [ "$status" -eq 0 ]
}

@test "the setup commands run behind environment.run, one prefix call per command" {
  local wrap="$BATS_TEST_TMPDIR/wrap dir/wrap"
  mkdir -p "${wrap%/*}"
  printf '#!%s\nprintf "%%s\\n" "wrapped: $*" >>"%s"\nexec "${@:2}"\n' "$BASH" "$LOG" >"$wrap"
  chmod +x "$wrap"
  start_repo ".check = $(checks 'echo one' 'echo two') | .environment.run = $(checks "$wrap" skipme)"
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  [ "$(cat "$LOG")" = $'wrapped: skipme bash -c echo one\nwrapped: skipme bash -c echo two' ]
  [[ $output == *"check: echo one"* ]]
}

@test "a failing prefix fails the check and writes no receipt" {
  start_repo ".check = $(checks 'echo one') | .environment.run = $(checks false)"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"check failed (exit 1): echo one"* ]]
  no_receipt
}

@test "an empty environment.run runs the command as plain bash -c" {
  start_repo ".check = $(checks "echo \"\$0 \$#\" >> '$LOG'") | .environment.run = []"
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  [ "$(cat "$LOG")" = "bash 0" ]
}

# rename_files <count>: rename <count> committed files on the feature branch.
rename_files() {
  local i
  for i in $(seq 1 "$1"); do
    git mv "f$i" "g$i"
  done
  git commit -q -m "rename files"
}

@test "renaming 3 files counts 3, not 6" {
  start_repo
  git checkout -q main
  commit_files 3
  git checkout -q feature
  git merge -q --ff-only main
  git checkout -q -b renamed
  rename_files 3
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 3 files (cap 60)"* ]]
}

@test "renames count once even when diff.renames is false" {
  start_repo
  git checkout -q main
  commit_files 3
  git checkout -q feature
  git merge -q --ff-only main
  git checkout -q -b renamed
  git config diff.renames false
  rename_files 3
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 3 files (cap 60)"* ]]
}

@test "a base branch named like an option is passed to git as a name" {
  start_repo
  git update-ref refs/heads/-x main
  write_setup "$(jq -c '.check = [] | .base = "-x"' "$FIXTURE")"
  commit_files 2
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 3 files (cap 60)"* ]]
}

@test "a check that moves HEAD without changing the content is named" {
  start_repo ".check = $(checks 'git commit -q --allow-empty -m sneaky')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: a check changed the work: it moved HEAD to a new commit" ]
}
