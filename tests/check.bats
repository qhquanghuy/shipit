#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# start_repo [path=value]...: a repo as the current directory. The setup file (the
# fixture with those values replaced, default: no check commands) is committed on
# main first, then the work happens on branch feature, so the setup is not a
# changed file.
start_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo"
  write_setup "$(fixture_with "${@:-gate.check=[]}")"
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

# checks <command>...: a TOML array of these commands.
checks() {
  jq -cn '$ARGS.positional' --args "$@"
}

# run_check <args>: run shipit check, stdout in $output, stderr in $stderr.
run_check() {
  run --separate-stderr "$SHIPIT_BIN" check "$@"
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
  start_repo "gate.check=$(checks "echo ran >> '$LOG'")"
  commit_files 61
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"61 files (cap 60)"* ]]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "a file named with spaces and one with a newline count one each" {
  start_repo 'gate.check=[]' 'gate.max_files=2'
  printf 'a\n' >"a file with spaces"
  printf 'b\n' >$'a\nnewline'
  git add -A
  git commit -q -m "odd names"
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 2 files (cap 2)"* ]]
  jq -e '.size.files == 2 and .base_given == false' "$(store)"/receipts/check/*.json
}

@test "commands run in order" {
  start_repo "gate.check=$(checks "echo one >> '$LOG'" "echo two >> '$LOG'" "echo three >> '$LOG'")"
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  [ "$(cat "$LOG")" = $'one\ntwo\nthree' ]
}

@test "check: each command sees SHIPIT_MERGE_BASE and SHIPIT_CHECK" {
  local one="printf '%s|%s\\n' \"\$SHIPIT_MERGE_BASE\" \"\$SHIPIT_CHECK\" >> '$LOG'"
  local two="printf '%s|%s\\n' \"\$SHIPIT_MERGE_BASE\" \"\$SHIPIT_CHECK\" >> '$LOG' # second"
  start_repo "gate.check=$(checks "$one" "$two")"
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  local base
  base=$(git merge-base main HEAD)
  [ "$(cat "$LOG")" = "$base|$one"$'\n'"$base|$two" ]
}

@test "the first failing command stops the run: exit 1, named, no receipt" {
  start_repo "gate.check=$(checks true false 'touch ran-after')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"check failed"*"false"* ]]
  [ ! -e ran-after ]
  no_receipt
}

@test "command output is shown" {
  start_repo "gate.check=$(checks 'echo visible-out' 'echo visible-err >&2')"
  commit_files 1
  run --separate-stderr "$SHIPIT_BIN" check
  [ "$status" -eq 0 ]
  [[ $output == *visible-out* ]]
  [[ $stderr == *visible-err* ]]
}

@test "a pass writes a receipt that matches the commit and tree" {
  start_repo "gate.check=$(checks true 'echo hi')"
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
  write_setup "$(fixture_with 'gate.check=[]')"
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
  jq -e --arg c "$(git rev-parse refs/remotes/origin/main)" '.base_ref == "refs/remotes/origin/main" and .base_commit == $c' "$(store)"/receipts/check/*.json
}

# clone_of_main <setup toml>: an upstream repo whose main holds the setup, and
# its clone as the current directory (identity configured, on main).
clone_of_main() {
  local up
  up=$(make_repo)
  cd "$up"
  write_setup "$1"
  git clone -q "$up" "$BATS_TEST_TMPDIR/clone"
  cd "$BATS_TEST_TMPDIR/clone"
  git config user.name t
  git config user.email t@example.invalid
  git config commit.gpgsign false
}

@test "local main ahead of origin/main: origin/main is used" {
  clone_of_main "$(fixture_with 'gate.check=[]' 'gate.max_files=9')"
  commit_files 5 m
  git checkout -q -b feature
  commit_files 3 f
  # against local main: 3 files; against origin/main: 8 files
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 8 files (cap 9)"* ]]
}

@test "local main and origin/main diverged: origin/main is used" {
  local up
  up=$(make_repo)
  cd "$up"
  write_setup "$(fixture_with 'gate.check=[]' 'gate.max_files=6')"
  git clone -q "$up" "$BATS_TEST_TMPDIR/clone"
  commit_files 5 m
  cd "$BATS_TEST_TMPDIR/clone"
  git config user.name t
  git config user.email t@example.invalid
  git config commit.gpgsign false
  commit_files 1 local
  git fetch -q origin
  git checkout -q -b feature origin/main
  commit_files 3 f
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 3 files (cap 6)"* ]]
}

@test "an unpushed setup change on the local base is not the baseline while origin/main exists" {
  clone_of_main "$(fixture_with 'gate.check=[]')"
  write_setup "$(fixture_with 'gate.check=[]' 'gate.max_files=40')"
  git checkout -q -b feature
  commit_files 2
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg gate.max_files)" ]
  no_receipt
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
  start_repo "gate.check=$(checks "pwd > '$LOG'")"
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

@test "a missing setup file exits 2 with exactly one message" {
  start_repo
  git rm -q .config/shipit.toml
  git commit -q -m "drop setup"
  run_check
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at .config/shipit.toml (see docs/setup.md)" ]
}

@test "an argument the command does not know exits 2 with the usage" {
  start_repo
  run_check --nope
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: unknown argument '--nope'; usage: shipit check [--base <name>]" ]
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

# changed_msg <fields> [base]: the refusal for a slice that changes <fields> of
# the setup.
changed_msg() {
  printf 'shipit: this slice changes the checks (%s); the owner merges setup changes on %s first (or merge/rebase %s if it moved on)' \
    "$1" "${2:-main}" "${2:-main}"
}

@test "a branch that raises the cap is refused: exit 1, no command, no receipt" {
  start_repo "gate.check=$(checks "echo ran >> '$LOG'")"
  write_setup "$(fixture_with "gate.check=$(checks "echo ran >> '$LOG'")" 'gate.max_files=100')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg gate.max_files)" ]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "every changed field is named, in the setup's order" {
  start_repo "gate.check=$(checks 'echo one')"
  write_setup "$(fixture_with 'gate.check=[]' 'gate.max_files=61' 'gate.environment.prefix=["env"]' 'agents.max_fix_rounds=3')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg 'gate.max_files, gate.check, gate.environment, agents.max_fix_rounds')" ]
}

@test "a base setup without a base string is refused: exit 2" {
  start_repo
  git checkout -q main
  write_setup $'version = 1\n[gate]\nbase = 1\nmax_files = "60"\ncheck = [1]\n[gate.environment]\nprefix = [2]'
  git checkout -q feature
  git merge -q --ff-only main
  write_setup "$(fixture_with 'gate.check=[]')"
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: main:.config/shipit.toml is invalid:"$'\n'".gate.base: must be a non-empty string" ]]
  git checkout -q main
  write_setup 'cap ='
  git checkout -q feature
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: main:.config/shipit.toml is not valid TOML" ]]
}

@test "a base without a setup file: a notice on stderr, then a setup-only slice goes on" {
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
  write_setup "$(fixture_with 'gate.check=[]')"
  run_check
  [ "$status" -eq 0 ]
  [ "$stderr" = "shipit: no setup file on main; nothing to compare" ]
  [[ $output == *"size: 1 files (cap 60)"* ]]
}

@test "a slice cannot name a branch with no setup file to skip every comparison" {
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
  git branch old
  write_setup "$(fixture_with "gate.check=$(checks 'false')")"
  git checkout -q -b feature
  write_setup "$(fixture_with "gate.check=$(checks "echo ran >> '$LOG'")" 'gate.base="old"')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: no setup file on old; nothing to compare
$(changed_msg 'version, gate.base, gate.max_files, gate.check, gate.environment, agents.max_fix_rounds, agents.effort_cap, agents.tiers, agents.roles, packs.skill, packs.domain, packs.model, owner.max_lines, owner.status_update_max_lines' old)" ]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "the base setup is read from origin/<base> when local <base> is behind it" {
  local up
  up=$(make_repo)
  cd "$up"
  write_setup "$(fixture_with 'gate.check=[]')"
  git clone -q "$up" "$BATS_TEST_TMPDIR/clone"
  write_setup "$(fixture_with 'gate.check=[]' 'gate.max_files=20')"
  cd "$BATS_TEST_TMPDIR/clone"
  git config user.name t
  git config user.email t@example.invalid
  git config commit.gpgsign false
  git fetch -q origin
  git checkout -q -b feature origin/main
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 1 files (cap 20)"* ]]
  [ -z "$stderr" ]
}

@test "--base overrides the base named in the setup" {
  start_repo
  git checkout -q -b release
  commit_files 5 r
  git checkout -q -b feature2
  commit_files 2 f
  # against release: 2 files; against main: 7 files
  run_check --base release
  [ "$status" -eq 0 ]
  [[ $output == *"size: 2 files (cap 60)"* ]]
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 7 files (cap 60)"* ]]
}

@test "--base: the setup is compared with the one on that base" {
  start_repo
  git checkout -q -b release main
  write_setup "$(fixture_with 'gate.check=[]' 'gate.max_files=10')"
  git checkout -q feature
  commit_files 1
  run_check --base release
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg gate.max_files release)" ]
}

@test "--base: the setup's own base field must match the base's setup" {
  start_repo
  git checkout -q -b release main
  write_setup "$(fixture_with 'gate.check=[]' 'gate.base="release"')"
  git checkout -q feature
  commit_files 1
  run_check --base release
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg gate.base release)" ]
}

@test "--base naming a missing branch exits 2" {
  start_repo
  commit_files 1
  run_check --base nope
  [ "$status" -eq 2 ]
  [[ $stderr == *'base branch "nope" not found (looked for origin/nope and nope)'* ]]
}

@test "a setup whose base is the branch being checked is refused: exit 2, no command, no receipt" {
  start_repo "gate.check=$(checks "echo ran >> '$LOG'")"
  write_setup "$(fixture_with "gate.check=$(checks "echo ran >> '$LOG'")" 'gate.base="feature"')"
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == 'shipit: base "feature" is the branch being checked'*"pass --base <name>"* ]]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "a fresh branch with nothing on it yet is refused until --base says the base" {
  start_repo
  run_check
  [ "$status" -eq 2 ]
  [[ $stderr == 'shipit: base "main" points at the commit being checked'* ]]
  run_check --base main
  [ "$status" -eq 0 ]
  [[ $output == *"size: 0 files (cap 60)"* ]]
}

@test "on the base branch itself with --base: size is 0 and the checks run" {
  start_repo "gate.check=$(checks "echo ran >> '$LOG'")"
  git checkout -q main
  run_check --base main
  [ "$status" -eq 0 ]
  [[ $output == *"size: 0 files (cap 60)"* ]]
  [ "$(cat "$LOG")" = ran ]
}

@test "a detached HEAD is the checked commit, not a branch name" {
  start_repo
  commit_files 2
  git checkout -q --detach
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 2 files (cap 60)"* ]]
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
  git update-index --skip-worktree .config/shipit.toml
  fixture_with 'gate.check=["touch ran"]' 'gate.max_files=1000' >.config/shipit.toml
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
  start_repo "gate.check=$(checks 'echo one' 'echo two')" "gate.environment.prefix=$(checks "$wrap" skipme)"
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  [ "$(cat "$LOG")" = $'wrapped: skipme bash -c echo one\nwrapped: skipme bash -c echo two' ]
  [[ $output == *"check: echo one"* ]]
}

@test "a failing prefix fails the check and writes no receipt" {
  start_repo "gate.check=$(checks 'echo one')" "gate.environment.prefix=$(checks false)"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == *"check failed (exit 1): echo one"* ]]
  no_receipt
}

@test "an empty environment.run runs the command as plain bash -c" {
  start_repo "gate.check=$(checks "echo \"\$0 \$#\" >> '$LOG'")" 'gate.environment.prefix=[]'
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

@test "a check that moves HEAD without changing the content is named" {
  start_repo "gate.check=$(checks 'git commit -q --allow-empty -m sneaky')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: a check changed the work: it moved HEAD to a new commit (check: git commit -q --allow-empty -m sneaky)" ]
  no_receipt
}

@test "a check that undoes an earlier check's edit is caught at the earlier check" {
  start_repo "gate.check=$(checks 'echo changed >> f1' 'git checkout -- f1' "echo third >> '$LOG'")"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: a check changed the work: f1 (check: echo changed >> f1)" ]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "a check that creates a file and a later one that deletes it is caught too" {
  start_repo "gate.check=$(checks 'touch junk' 'rm junk')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: a check changed the work: junk (check: touch junk)" ]
  no_receipt
}

@test "a check that sets the assume-unchanged flag is caught, even with no edit" {
  start_repo "gate.check=$(checks 'git update-index --assume-unchanged f1')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: a check changed the work: it marked tracked files skip-worktree or assume-unchanged (check: git update-index --assume-unchanged f1)" ]
  no_receipt
}

@test "a check that sets the skip-worktree flag is caught, even with no edit" {
  start_repo "gate.check=$(checks 'git update-index --skip-worktree f1')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == "shipit: a check changed the work: it marked tracked files skip-worktree or assume-unchanged (check: git update-index --skip-worktree f1)" ]]
  no_receipt
}

@test "a check that commits is named with the changed paths and the command" {
  start_repo "gate.check=$(checks 'echo more > new1 && git add new1 && git commit -q -m c')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: a check changed the work: new1 (it committed; check: echo more > new1 && git add new1 && git commit -q -m c)" ]
  no_receipt
}

@test "a check that renames a file lists both paths plainly" {
  start_repo "gate.check=$(checks 'git mv f1 g1')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: a check changed the work: g1, f1 (check: git mv f1 g1)" ]
}

@test "paths with spaces are listed unquoted" {
  start_repo "gate.check=$(checks "touch 'a b' $'tab\there'")"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [[ $stderr == "shipit: a check changed the work: a b, tab"$'\t'"here (check: "* ]]
}

@test "sparse checkout is refused with a clear message, before any other check" {
  start_repo
  mkdir sub
  echo x >sub/x
  echo y >y
  git add -A
  git commit -q -m "add sub and y"
  git sparse-checkout set --no-cone /y
  [ ! -e sub/x ]
  run_check
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: sparse checkout is not supported: checks would not see every file" ]
  no_receipt
  git sparse-checkout disable
  run_check
  [ "$status" -eq 0 ]
}

@test "check: a slice that changes tiers together with code is refused" {
  start_repo
  write_setup "$(fixture_with 'gate.check=[]' | sed 's/^model = "haiku"/model = "other"/')"
  commit_files 1
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg agents.tiers)" ]
  no_receipt
}

@test "check: a setup-only change of effort_cap is allowed and marked in the receipt" {
  start_repo
  write_setup "$(fixture_with 'gate.check=[]' 'agents.effort_cap="xhigh"')"
  run_check
  [ "$status" -eq 0 ]
  [ "$stderr" = "shipit: this slice changes the checks (agents.effort_cap); setup change: the owner approves it by merging" ]
  jq -e '.setup_change == true and .setup_fields == ["agents.effort_cap"]' "$(store)"/receipts/check/*.json
}

@test "check: a base tip that changed max_fix_rounds since the fork asks to merge or rebase" {
  start_repo
  commit_files 1
  git checkout -q main
  write_setup "$(fixture_with 'gate.check=[]' 'agents.max_fix_rounds=3')"
  git checkout -q feature
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg agents.max_fix_rounds)" ]
  no_receipt
}

@test "a slice that edits only the setup file may change the checks: a notice, a marked receipt, verify says so" {
  start_repo "gate.check=$(checks 'echo one')"
  write_setup "$(fixture_with "gate.check=$(checks 'echo one' "echo two >> '$LOG'")")"
  run_check
  [ "$status" -eq 0 ]
  [ "$stderr" = "shipit: this slice changes the checks (gate.check); setup change: the owner approves it by merging" ]
  [[ $output == *"size: 1 files (cap 60)"* ]]
  [ "$(cat "$LOG")" = two ]
  jq -e '.setup_change == true and .setup_fields == ["gate.check"] and (.commands | length) == 2' \
    "$(store)"/receipts/check/*.json
  run --separate-stderr "$SHIPIT_BIN" verify
  [ "$status" -eq 0 ]
  [[ $output == "valid receipt for "*" (setup change)" ]]
}

@test "a setup-only change that raises the cap runs with the new cap and is marked" {
  start_repo
  write_setup "$(fixture_with 'gate.check=[]' 'gate.max_files=100')"
  run_check
  [ "$status" -eq 0 ]
  [ "$stderr" = "shipit: this slice changes the checks (gate.max_files); setup change: the owner approves it by merging" ]
  [[ $output == *"size: 1 files (cap 100)"* ]]
  jq -e '.setup_change == true and .setup_fields == ["gate.max_files"] and .size.cap == 100' \
    "$(store)"/receipts/check/*.json
}

@test "a setup change together with a deleted file is refused" {
  start_repo "gate.check=$(checks "echo ran >> '$LOG'")"
  git checkout -q main
  printf 'x\n' >code
  git add code
  git commit -q -m "a file"
  git checkout -q feature
  git merge -q --ff-only main
  write_setup "$(fixture_with "gate.check=$(checks "echo ran >> '$LOG'" 'echo two')" 'gate.max_files=61')"
  git rm -q code
  git commit -q -m "drop the file"
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg 'gate.max_files, gate.check')" ]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "a plain receipt has no setup_change mark and verify prints no suffix" {
  start_repo
  commit_files 1
  run_check
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
  jq -e 'has("setup_change") | not' "$(store)"/receipts/check/*.json
  run --separate-stderr "$SHIPIT_BIN" verify
  [[ $output != *"setup change"* ]]
}

@test "a tag named like the base branch does not stand in for it" {
  start_repo
  commit_files 2
  git tag main HEAD
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 2 files (cap 60)"* ]]
  jq -e --arg c "$(git rev-parse refs/heads/main)" \
    '.base_ref == "refs/heads/main" and .base_commit == $c' "$(store)"/receipts/check/*.json
}

@test "a tag named origin/<base> does not stand in for the remote branch" {
  start_repo
  commit_files 2
  git tag origin/main HEAD
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"size: 2 files (cap 60)"* ]]
  jq -e '.base_ref == "refs/heads/main"' "$(store)"/receipts/check/*.json
}

@test "--base accepts the names git check-ref-format --branch accepts, and no revision" {
  start_repo
  local name git_says ours
  for name in main a/b @ @x a@b é x.y -x HEAD HEAD~1 HEAD^ main~2 a..b a//b /a a/ a. .a a/.b a.lock a/b.lock 'a b' 'a~' 'a^' 'a:' 'a?' 'a*' 'a[' 'a\b' 'a@{b' $'a\tb' $'a\x7fb'; do
    if git check-ref-format --branch "$name" >/dev/null 2>&1; then git_says=accepts; else git_says=refuses; fi
    run_check --base "$name"
    if [[ $stderr == *"is not a branch name"* ]]; then ours=refuses; else ours=accepts; fi
    [ "$git_says" = "$ours" ] || {
      printf '%q: git %s, shipit %s\n' "$name" "$git_says" "$ours" >&2
      return 1
    }
  done
  run_check --base HEAD~1
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: --base 'HEAD~1' is not a branch name (see git check-ref-format --branch); usage: shipit check [--base <name>]" ]
}

@test "a repository in a folder whose name is not UTF-8 works" {
  start_repo
  commit_files 1
  local bad="$BATS_TEST_TMPDIR/bad"$'\xff'
  mv "$PWD" "$bad"
  cd "$bad"
  run_check
  [ "$status" -eq 0 ]
  [[ $output == *"receipt written for "* ]]
  jq -e . "$(store)"/receipts/check/*.json
  run --separate-stderr "$SHIPIT_BIN" verify
  [ "$status" -eq 0 ]
}

@test "a submodule that git is told to ignore still counts as an uncommitted change" {
  start_repo
  local sub="$BATS_TEST_TMPDIR/sub"
  git init -q -b main "$sub"
  git -C "$sub" config user.name t
  git -C "$sub" config user.email t@example.invalid
  git -C "$sub" config commit.gpgsign false
  printf 's\n' >"$sub/s"
  git -C "$sub" add s
  git -C "$sub" commit -q -m s
  git -c protocol.file.allow=always submodule add -q "$sub" vendor
  git commit -q -m "add a submodule"
  git -C vendor config user.name t
  git -C vendor config user.email t@example.invalid
  git -C vendor config commit.gpgsign false
  git -C vendor commit -q --allow-empty -m inside
  git config diff.ignoreSubmodules all
  [ -z "$(git status --porcelain)" ]
  run_check
  [ "$status" -eq 3 ]
  [[ $stderr == "shipit: uncommitted changes: commit first"* ]]
  no_receipt
}

@test "a file system monitor that reports no changes does not hide an edit" {
  start_repo
  commit_files 1
  local hook="$BATS_TEST_TMPDIR/fsmonitor"
  printf '#!/bin/sh\nprintf "token\\0"\n' >"$hook"
  chmod +x "$hook"
  git config core.fsmonitor "$hook"
  git update-index --fsmonitor
  git status >/dev/null
  git status >/dev/null
  printf 'more\n' >>f1
  [ -z "$(git status --porcelain)" ]
  run_check
  [ "$status" -eq 3 ]
  [[ $stderr == "shipit: uncommitted changes: commit first"* ]]
  no_receipt
}

@test "a slice cannot name a branch that holds its own code to judge itself by" {
  start_repo "gate.check=$(checks 'echo ran')"
  commit_files 1
  git branch feat-code
  write_setup "$(fixture_with "gate.check=$(checks "echo ran >> '$LOG'")" 'gate.base="feat-code"')"
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg 'gate.base, gate.check' main)" ]
  [ ! -e "$LOG" ]
  no_receipt
}

@test "a base that moved on is not mistaken for the slice's change: refused, not marked" {
  start_repo "gate.check=$(checks 'true')"
  write_setup "$(fixture_with 'gate.check=["true"]' 'agents.roles.reader.tier=2')"
  git checkout -q main
  write_setup "$(fixture_with 'gate.check=["true", "test -f a"]')"
  git checkout -q feature
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg gate.check)" ]
  no_receipt
}

@test "a submodule that .gitmodules says to ignore still counts as a changed file" {
  start_repo "gate.check=$(checks 'true')"
  local sub="$BATS_TEST_TMPDIR/sub"
  git init -q -b main "$sub"
  git -C "$sub" config user.name t
  git -C "$sub" config user.email t@example.invalid
  git -C "$sub" config commit.gpgsign false
  printf 's\n' >"$sub/s"
  git -C "$sub" add s
  git -C "$sub" commit -q -m s
  git checkout -q main
  git -c protocol.file.allow=always submodule add -q "$sub" vendor
  git config -f .gitmodules submodule.vendor.ignore all
  git add .gitmodules
  git commit -q -m "add a submodule"
  git checkout -q feature
  git merge -q --ff-only main
  git -C vendor config user.name t
  git -C vendor config user.email t@example.invalid
  git -C vendor config commit.gpgsign false
  git -C vendor commit -q --allow-empty -m inside
  git add --force vendor
  write_setup "$(fixture_with 'gate.check=["true", "echo two"]')"
  run_check
  [ "$status" -eq 1 ]
  [ "$stderr" = "$(changed_msg gate.check)" ]
  no_receipt
}

@test "a base written as the remote branch says to name the branch" {
  start_repo 'gate.base="origin/main"' 'gate.check=[]'
  commit_files 1
  run_check
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: base branch "origin/main" not found (looked for origin/origin/main and origin/main); name the branch, not the remote: "main"' ]
}
