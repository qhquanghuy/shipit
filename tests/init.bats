#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

GUARDED='if command -v shipit >/dev/null 2>&1; then exec shipit pre-push "$@"; fi'

# on_path: shipit on PATH, as the installed hook needs it.
on_path() {
  mkdir -p "$BATS_TEST_TMPDIR/bin"
  ln -sf "$SHIPIT_BIN" "$BATS_TEST_TMPDIR/bin/shipit"
  PATH="$BATS_TEST_TMPDIR/bin:$PATH"
  export PATH
}

# configure: the identity every test repository commits with.
configure() {
  git config user.name "ShipIt Test"
  git config user.email "test@example.invalid"
  git config commit.gpgsign false
}

# new_repo: an empty repository on main as the current directory.
new_repo() {
  local repo
  repo=$(mktemp -d "$BATS_TEST_TMPDIR/work.XXXXXX")
  git init -q -b main "$repo"
  cd "$repo"
  configure
}

# new_project: a repository with a README on main, pushed to a bare origin, as the current directory.
new_project() {
  git init -q --bare "$BATS_TEST_TMPDIR/origin.git"
  new_repo
  echo hi >README
  git add README
  git commit -q -m "add a README"
  git remote add origin "$BATS_TEST_TMPDIR/origin.git"
  git push -q origin main
}

hook_file() {
  git rev-parse --path-format=absolute --git-path hooks/pre-push
}

# foreign_hook <body>: an executable pre-push hook that ShipIt did not write.
foreign_hook() {
  printf '%s\n' "$1" >"$(hook_file)"
  chmod +x "$(hook_file)"
}

@test "init in a new repository with no commits writes the starting setup and the hook" {
  new_repo
  run "$SHIPIT_BIN" init
  [ "$status" -eq 0 ]
  [ "${#lines[@]}" -le 30 ]
  [[ ${lines[0]} == "wrote .config/shipit.toml (base main)" ]]
  [ -x "$(hook_file)" ]
  [ "$(sed -n 2p "$(hook_file)")" = "# installed by shipit init" ]
  [ "$(sed -n 3p "$(hook_file)")" = "$GUARDED" ]
  [[ $output == *"No commits yet: make a first commit on main"* ]]
  [[ $output == *$'No commits yet: '*$'\nNext: '* ]]
  git add -A
  git commit -q -m "adopt"
  run_shipit setup
  [ "$status" -eq 0 ]
}

@test "the Next lines name pull request 1 and pull request 2" {
  new_project
  run "$SHIPIT_BIN" init
  [ "$status" -eq 0 ]
  [[ $output == *"open pull request 1 (the setup file only)"* ]]
  [[ $output == *"(pull request 2)"* ]]
  [[ $output != *"No commits yet"* ]]
  [[ $output != *"No origin/"* ]]
}

@test "an unpushed base is named in the output" {
  new_repo
  echo hi >README
  git add README
  git commit -q -m "add a README"
  run "$SHIPIT_BIN" init
  [ "$status" -eq 0 ]
  [[ $output == *"No origin/main yet: push main first"* ]]
}

@test "base from origin/HEAD" {
  git init -q --bare -b trunk "$BATS_TEST_TMPDIR/trunk.git"
  git clone -q "$BATS_TEST_TMPDIR/trunk.git" "$BATS_TEST_TMPDIR/seed"
  cd "$BATS_TEST_TMPDIR/seed"
  configure
  git checkout -q -b trunk
  echo hi >README
  git add README
  git commit -q -m "add a README"
  git push -q origin trunk
  git clone -q "$BATS_TEST_TMPDIR/trunk.git" "$BATS_TEST_TMPDIR/clone"
  cd "$BATS_TEST_TMPDIR/clone"
  git checkout -q -b work
  git branch other
  run "$SHIPIT_BIN" init
  [ "$status" -eq 0 ]
  [[ $output == *"wrote .config/shipit.toml (base trunk)"* ]]
}

@test "several local branches and no origin: exit 2 and nothing written" {
  new_repo
  echo hi >README
  git add README
  git commit -q -m "add a README"
  git branch other
  run --separate-stderr "$SHIPIT_BIN" init
  [ "$status" -eq 2 ]
  [[ $stderr == *"cannot tell the base branch; run shipit init --base <branch>"* ]]
  [ ! -e .config/shipit.toml ]
  [ ! -e "$(hook_file)" ]
}

@test "the CI job written by --ci-job passes actionlint" {
  new_repo
  run "$SHIPIT_BIN" init --ci-job --base main
  [ "$status" -eq 0 ]
  [ "$output" = "wrote .github/workflows/shipit.yml" ]
  run actionlint .github/workflows/shipit.yml
  [ "$status" -eq 0 ]
  grep -qF 'branches: ["main"]' .github/workflows/shipit.yml
}

@test "--ci-job reads the setup's base when --base is not given" {
  new_project
  "$SHIPIT_BIN" init --no-hook >/dev/null
  git add -A
  git commit -q -m "adopt"
  run "$SHIPIT_BIN" init --ci-job
  [ "$status" -eq 0 ]
  grep -qF 'branches: ["main"]' .github/workflows/shipit.yml
}

@test "--ci-job with no setup and no --base: exit 2" {
  new_project
  run --separate-stderr "$SHIPIT_BIN" init --ci-job
  [ "$status" -eq 2 ]
  [ ! -e .github ]
}

@test "--ci-job when the file exists: exit 1, unchanged" {
  new_repo
  mkdir -p .github/workflows
  echo "mine" >.github/workflows/shipit.yml
  run --separate-stderr "$SHIPIT_BIN" init --ci-job --base main
  [ "$status" -eq 1 ]
  [[ $stderr == *".github/workflows/shipit.yml already exists; init never overwrites it"* ]]
  [ "$(cat .github/workflows/shipit.yml)" = "mine" ]
}

@test "an existing setup file: exit 1, unchanged, no hook; --hook-only installs the hook" {
  new_project
  mkdir -p .config
  echo "mine" >.config/shipit.toml
  run --separate-stderr "$SHIPIT_BIN" init
  [ "$status" -eq 1 ]
  [[ $stderr == *"a setup file already exists; init never overwrites it (in a new clone, run shipit init --hook-only)"* ]]
  [ "$(cat .config/shipit.toml)" = "mine" ]
  [ ! -e "$(hook_file)" ]
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [ -x "$(hook_file)" ]
  [ "$(cat .config/shipit.toml)" = "mine" ]
}

@test "our hook present and --hook-only: already installed" {
  new_project
  "$SHIPIT_BIN" init --hook-only >/dev/null
  cp "$(hook_file)" "$BATS_TEST_TMPDIR/before"
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ $output == *"pre-push hook: already installed"* ]]
  cmp "$(hook_file)" "$BATS_TEST_TMPDIR/before"
}

@test "from a git worktree copy the hook lands in the common hooks folder" {
  new_project
  git worktree add -q "$BATS_TEST_TMPDIR/wt" -b other
  cd "$BATS_TEST_TMPDIR/wt"
  "$SHIPIT_BIN" init --hook-only >/dev/null
  [ -x "$BATS_TEST_TMPDIR"/work.*/.git/hooks/pre-push ]
}

@test "two-PR adoption: the setup file alone on a branch checks and pushes" {
  new_project
  on_path
  "$SHIPIT_BIN" init >/dev/null
  git checkout -q -b slice/adopt
  git add .config/shipit.toml
  git commit -q -m "chore: setup file"
  run --separate-stderr "$SHIPIT_BIN" check
  [ "$status" -eq 0 ]
  [[ $stderr == *"no setup file on origin/main; nothing to compare"* ]]
  run git push -q origin slice/adopt
  [ "$status" -eq 0 ]
}

@test "end to end: a slice push is blocked without a receipt and passes after check" {
  new_project
  on_path
  "$SHIPIT_BIN" init >/dev/null
  git add .config/shipit.toml
  git commit -q -m "chore: setup file"
  git push -q origin main
  git checkout -q -b slice/x
  echo one >f1
  git add f1
  git commit -q -m "add f1"
  run git push -q origin slice/x
  [ "$status" -ne 0 ]
  [[ $output == *"push blocked: no check receipt for slice/x"* ]]
  "$SHIPIT_BIN" check >/dev/null
  run git push -q origin slice/x
  [ "$status" -eq 0 ]
}

@test "a tag, the base branch and a delete push without a receipt" {
  new_project
  on_path
  "$SHIPIT_BIN" init >/dev/null
  git add .config/shipit.toml
  git commit -q -m "chore: setup file"
  git push -q origin main
  git checkout -q -b slice/x
  echo one >f1
  git add f1
  git commit -q -m "add f1"
  "$SHIPIT_BIN" check >/dev/null
  git push -q origin slice/x
  git checkout -q main
  echo two >f2
  git add f2
  git commit -q -m "add f2 on the base"
  git tag v1
  run git push -q origin main v1
  [ "$status" -eq 0 ]
  run git push -q origin :slice/x
  [ "$status" -eq 0 ]
}

@test "a teammate without shipit pushes: the hook exits 0" {
  new_project
  on_path
  "$SHIPIT_BIN" init >/dev/null
  git add .config/shipit.toml
  git commit -q -m "chore: setup file"
  git push -q origin main
  git checkout -q -b slice/x
  echo one >f1
  git add f1
  git commit -q -m "add f1"
  mkdir -p "$BATS_TEST_TMPDIR/bare-path"
  for tool in git sh; do
    ln -sf "$(command -v "$tool")" "$BATS_TEST_TMPDIR/bare-path/$tool"
  done
  run env PATH="$BATS_TEST_TMPDIR/bare-path" sh "$(hook_file)" origin /x </dev/null
  [ "$status" -eq 0 ]
  run env PATH="$BATS_TEST_TMPDIR/bare-path" git push -q origin slice/x
  [ "$status" -eq 0 ]
}

@test "a foreign sh hook in .git/hooks gets the guarded line once" {
  new_project
  foreign_hook $'#!/bin/sh\necho foreign'
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ $output == "added to $(hook_file): $GUARDED" ]]
  [ "$(grep -cxF "$GUARDED" "$(hook_file)")" -eq 1 ]
  [ "$(head -n 2 "$(hook_file)")" = $'#!/bin/sh\necho foreign' ]
  cp "$(hook_file)" "$BATS_TEST_TMPDIR/before"
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ $output == *"already installed"* ]]
  cmp "$(hook_file)" "$BATS_TEST_TMPDIR/before"
}

# refuses <name> <hook body> <reason>
refuses() {
  new_project
  foreign_hook "$2"
  cp "$(hook_file)" "$BATS_TEST_TMPDIR/before"
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ $output == "not added to $(hook_file) ($3); add this line yourself, before anything reads git's input: $GUARDED" ]]
  cmp "$(hook_file)" "$BATS_TEST_TMPDIR/before"
}

@test "a python hook: not a sh or bash script" {
  refuses python $'#!/usr/bin/python3\nprint("x")' "it is not a sh or bash script"
}

@test "a hook with exit 0 at its end: printed" {
  refuses exit $'#!/bin/sh\necho x\nexit 0' "a line starts with exit or exec, so a line after it never runs"
}

@test "a hook with exec: printed" {
  refuses exec $'#!/bin/sh\nexec other-tool "$@"' "a line starts with exit or exec, so a line after it never runs"
}

@test "a git lfs hook: printed" {
  refuses lfs $'#!/bin/sh\ngit lfs pre-push "$@"' "it uses git lfs or reads git's input, which ShipIt's line needs too"
}

@test "a hook that reads stdin: printed" {
  refuses stdin $'#!/bin/sh\nwhile read a b c d; do echo "$a"; done' "it uses git lfs or reads git's input, which ShipIt's line needs too"
}

# sums: a checksum of every file of the working tree except .git, one per line.
sums() {
  find . -path ./.git -prune -o -type f -print0 | sort -z | xargs -0 sha256sum
}

# same_except [<file>]: the working tree is as `before=$(sums)` found it, but for that file.
same_except() {
  local file=${1:-/nonexistent}
  diff <(grep -vF -- "  ./$file" <<<"$before") <(grep -vF -- "  ./$file" <<<"$(sums)")
}

@test "core.hooksPath hook: the guarded line is appended once" {
  new_project
  mkdir hooks
  printf '#!/bin/sh\necho hi\n' >hooks/pre-push
  chmod +x hooks/pre-push
  git add hooks
  git commit -q -m "add hooks"
  git config core.hooksPath hooks
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ ${lines[0]} == "added to $PWD/hooks/pre-push: $GUARDED" ]]
  [[ ${lines[1]} == "commit it: $PWD/hooks/pre-push is tracked" ]]
  [ "$(grep -cxF "$GUARDED" hooks/pre-push)" -eq 1 ]
  [ -x hooks/pre-push ]
  same_except hooks/pre-push
  run "$SHIPIT_BIN" init --hook-only
  [[ $output == "pre-push hook: already installed" ]]
  [ "$(grep -cxF "$GUARDED" hooks/pre-push)" -eq 1 ]
}

@test "core.hooksPath outside the project: printed, nothing written" {
  new_project
  mkdir "$BATS_TEST_TMPDIR/global-hooks"
  printf '#!/bin/sh\necho hi\n' >"$BATS_TEST_TMPDIR/global-hooks/pre-push"
  cp "$BATS_TEST_TMPDIR/global-hooks/pre-push" "$BATS_TEST_TMPDIR/global-before"
  git config core.hooksPath "$BATS_TEST_TMPDIR/global-hooks"
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ $output == "not added to $BATS_TEST_TMPDIR/global-hooks/pre-push (it is outside this project); add this line yourself, before anything reads git's input: $GUARDED" ]]
  cmp "$BATS_TEST_TMPDIR/global-hooks/pre-push" "$BATS_TEST_TMPDIR/global-before"
  same_except
  rm "$BATS_TEST_TMPDIR/global-hooks/pre-push"
  run "$SHIPIT_BIN" init --hook-only
  [[ $output == *"(it is outside this project)"* ]]
  [ ! -e "$BATS_TEST_TMPDIR/global-hooks/pre-push" ]
}

@test "core.hooksPath inside the project through a link that leaves it: printed" {
  new_project
  mkdir "$BATS_TEST_TMPDIR/elsewhere"
  ln -s "$BATS_TEST_TMPDIR/elsewhere" linked-hooks
  git config core.hooksPath linked-hooks
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ $output == *"(it is outside this project)"* ]]
  [ ! -e "$BATS_TEST_TMPDIR/elsewhere/pre-push" ]
}

@test "husky: .husky/pre-push gets the guarded line" {
  new_project
  mkdir -p .husky/_
  printf 'npm test\n' >.husky/pre-push
  git config core.hooksPath .husky/_
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ ${lines[0]} == "added to $PWD/.husky/pre-push: $GUARDED" ]]
  [ "$(head -n 1 .husky/pre-push)" = "npm test" ]
  [ "$(tail -n 1 .husky/pre-push)" = "$GUARDED" ]
  same_except .husky/pre-push
}

@test "lefthook: lefthook-local.yml is created and lefthook install is printed" {
  new_project
  printf 'pre-commit:\n  commands: {}\n' >lefthook.yml
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ ${lines[0]} == "wrote lefthook-local.yml" ]]
  [[ $output == *"run lefthook install"* ]]
  [[ $output == *"add lefthook-local.yml to .gitignore (it is per person)"* ]]
  grep -qF "shipit pre-push {1} {2}" lefthook-local.yml
  grep -qF "use_stdin: true" lefthook-local.yml
  same_except lefthook-local.yml
  printf 'lefthook-local.yml\n' >.gitignore
  run "$SHIPIT_BIN" init --hook-only
  [[ $output == "pre-push hook: already installed" ]]
}

@test "lefthook with .lefthook.yml: .lefthook-local.yml is created" {
  new_project
  printf 'pre-commit:\n  commands: {}\n' >.lefthook.yml
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ ${lines[0]} == "wrote .lefthook-local.yml" ]]
  [ ! -e lefthook-local.yml ]
  same_except .lefthook-local.yml
}

@test "lefthook with a pre-push key in the local file: printed, unchanged" {
  new_project
  printf 'pre-commit:\n  commands: {}\n' >lefthook.yml
  printf 'pre-push:\n  commands:\n    mine:\n      run: echo mine\n' >lefthook-local.yml
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ ${lines[0]} == "not added to lefthook-local.yml (lefthook-local.yml already has a pre-push entry); add it yourself:" ]]
  [[ $output == *"  # only one command receives git's input when several use use_stdin"* ]]
  same_except
}

@test "lefthook-local.yml without pre-push: the entry is appended" {
  new_project
  printf 'pre-commit:\n  commands: {}\n' >lefthook.yml
  printf 'output:\n  - success' >lefthook-local.yml
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ ${lines[0]} == "added the pre-push entry to lefthook-local.yml" ]]
  [ "$(head -n 2 lefthook-local.yml)" = $'output:\n  - success' ]
  [ "$(sed -n 3p lefthook-local.yml)" = "pre-push:" ]
  same_except lefthook-local.yml
  cp lefthook-local.yml "$BATS_TEST_TMPDIR/before-local"
  run "$SHIPIT_BIN" init --hook-only
  [[ $output == *"pre-push hook: already installed"* ]]
  cmp lefthook-local.yml "$BATS_TEST_TMPDIR/before-local"
}

@test "lefthook.toml: printed, nothing written" {
  new_project
  printf '[pre-commit]\n' >lefthook.toml
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ ${lines[0]} == "not added to lefthook.toml (init writes lefthook-local.yml only next to lefthook.yml or .lefthook.yml); add it yourself:" ]]
  same_except
}

@test "pre-commit: the snippet is printed, nothing written" {
  new_project
  printf 'repos: []\n' >.pre-commit-config.yaml
  before=$(sums)
  run "$SHIPIT_BIN" init --hook-only
  [ "$status" -eq 0 ]
  [[ ${lines[0]} == "not added to .pre-commit-config.yaml ("*"add this entry under repos:); add it yourself:" ]]
  [[ $output == *"id: shipit"* ]]
  [[ $output == *"# pre-commit runs it once per push, for one ref"* ]]
  same_except
  [ ! -e "$(hook_file)" ]
}

@test "--no-hook writes the setup only, whatever the manager" {
  new_project
  printf 'pre-commit:\n  commands: {}\n' >lefthook.yml
  before=$(sums)
  run "$SHIPIT_BIN" init --no-hook
  [ "$status" -eq 0 ]
  [ ! -e lefthook-local.yml ]
  [ ! -e "$(hook_file)" ]
  same_except .config/shipit.toml
}
