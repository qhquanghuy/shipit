#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load claude-helpers

# A repo on main with a local bare remote, the tag v0.0.1 and the slice branches slice/v1/x
# (checked out) and slice/x.
setup() {
  REPO=$(shipit_repo)
  cd "$REPO" || return 1
  git init -q --bare "$BATS_TEST_TMPDIR/remote.git"
  git remote add origin "$BATS_TEST_TMPDIR/remote.git"
  git tag v0.0.1
  git branch slice/x
  git checkout -q -b slice/v1/x
}

# gate <command> [mode] [folder]: the ship gate on a Bash call in the repo.
gate() {
  run_hook claude-ship-gate "$(bash_input "$REPO" "$1" "${2:-default}")" "${3:-}"
}

@test "each ship command asks in auto mode" {
  local command
  for command in "git push origin main" "git push origin v0.0.1" "gh pr merge 1" "gh release create v9"; do
    gate "$command" auto
    [ "$(decision)" = ask ]
    [[ $(reason) == "ShipIt: only the owner ships. This command "* ]]
  done
  gate "git push origin main" auto
  [[ $(reason) == *"pushes to the base branch main"* ]]
  gate "git push origin v0.0.1" auto
  [[ $(reason) == *"pushes a tag"* ]]
}

@test "bypass mode denies a base push" {
  gate "git push origin main" bypassPermissions
  [ "$(decision)" = deny ]
  [[ $(reason) == *"this permission mode cannot ask the owner"* ]]
}

@test "bypass mode denies a push that names its branch with a substitution" {
  gate 'git push -u origin "$(git branch --show-current)"' bypassPermissions
  [ "$(decision)" = deny ]
  [[ $(reason) == *"name branches in plain text"* ]]
}

@test "a slice-branch push passes" {
  gate "git push -u origin slice/v1/x" auto
  [ -z "$output" ]
  gate "git push -u origin slice/v1/x" bypassPermissions
  [ -z "$output" ]
}

@test "a slice that names another base still asks for main" {
  write_setup "$(fixture_with 'gate.base="slice/x"')"
  gate "git push origin main" auto
  [ "$(decision)" = ask ]
  [[ $(reason) == *"pushes to the base branch main"* ]]
}

@test "the base branch's own setup names the base, and main still asks" {
  git checkout -q main
  write_setup "$(fixture_with 'gate.base="trunk"')"
  git branch trunk
  git checkout -q -b slice/v1/y
  gate "git push origin trunk" auto
  [ "$(decision)" = ask ]
  [[ $(reason) == *"pushes to the base branch trunk"* ]]
  gate "git push origin main" auto
  [ "$(decision)" = ask ]
}

@test "the repository comes from the input cwd, not the process folder" {
  local elsewhere
  elsewhere=$(mktemp -d "$BATS_TEST_TMPDIR/elsewhere.XXXXXX")
  run_hook claude-ship-gate "$(bash_input "$REPO" "git push origin main" auto)" "$elsewhere"
  [ "$(decision)" = ask ]
  local plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  run_hook claude-ship-gate "$(bash_input "$plain" "git push origin main" auto)" "$REPO"
  [ -z "$output" ]
}

@test "a cd into a ShipIt project from outside one is judged there" {
  local elsewhere
  elsewhere=$(mktemp -d "$BATS_TEST_TMPDIR/elsewhere.XXXXXX")
  run_hook claude-ship-gate "$(bash_input "$elsewhere" "cd $REPO && git push origin main" auto)" "$elsewhere"
  [ "$(decision)" = ask ]
}

@test "a Monitor command is judged" {
  run_hook claude-ship-gate "$(hook_input "$(jq -cn --arg cwd "$REPO" \
    '{cwd: $cwd, tool_name: "Monitor", tool_input: {command: "git push origin main"}, permission_mode: "auto"}')")"
  [ "$(decision)" = ask ]
}

@test "an mcp merge tool asks" {
  run_hook claude-ship-gate "$(hook_input "$(jq -cn --arg cwd "$REPO" \
    '{cwd: $cwd, tool_name: "mcp__github__merge_pull_request", tool_input: {}, permission_mode: "auto"}')")"
  [ "$(decision)" = ask ]
  [[ $(reason) == *"calls mcp__github__merge_pull_request"* ]]
  run_hook claude-ship-gate "$(hook_input "$(jq -cn --arg cwd "$REPO" \
    '{cwd: $cwd, tool_name: "mcp__github__list_issues", tool_input: {}}')")"
  [ -z "$output" ]
}

@test "shipit complete with --confirm asks in auto mode and is denied in bypass mode" {
  gate "shipit complete demo --confirm" auto
  [ "$(decision)" = ask ]
  [[ $(reason) == *"confirms a goal completion (shipit complete --confirm), which only the owner may do"* ]]
  gate "shipit complete demo --confirm" bypassPermissions
  [ "$(decision)" = deny ]
}

@test "bad stdin in a ShipIt project denies" {
  run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" exec "$3" claude-ship-gate <<<"not json"' \
    _ "$BATS_TEST_TMPDIR" "$REPO" "$SHIPIT_BIN"
  [ "$status" -eq 0 ]
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt cannot read the hook input ("* ]]
  run --separate-stderr bash -c 'cd "$1" && unset CLAUDE_PROJECT_DIR && exec "$2" claude-ship-gate <<<"{}"' \
    _ "$REPO" "$SHIPIT_BIN"
  [ "$status" -eq 0 ]
  [ "$(decision)" = deny ]
}

@test "bad stdin outside a ShipIt project allows" {
  run_hook claude-ship-gate "not json"
  [ -z "$output" ]
  run_hook claude-ship-gate "{}"
  [ -z "$output" ]
}

@test "outside a ShipIt project every command allows" {
  local plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  run_hook claude-ship-gate "$(bash_input "$plain" "git push origin main" bypassPermissions)" "$plain"
  [ -z "$output" ]
  run_hook claude-ship-gate "$(bash_input "$plain" "gh pr merge 1" bypassPermissions)" "$plain"
  [ -z "$output" ]
}

@test "an unknown tool without a command in a ShipIt project is a bad input" {
  run_hook claude-ship-gate "$(hook_input "$(jq -cn --arg cwd "$REPO" '{cwd: $cwd, tool_name: "Bash", permission_mode: "auto"}')")"
  [ "$(decision)" = ask ]
  [[ $(reason) == *"has no tool_input.command"* ]]
}

@test "an argument is a usage error" {
  run --separate-stderr "$SHIPIT_BIN" claude-ship-gate extra
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: usage: shipit claude-ship-gate"* ]]
}

@test "the gate writes nothing" {
  local before after store_before store_after
  before=$(git status --porcelain)
  store_before=$(find "$(git rev-parse --git-common-dir)" -type f -newer "$REPO/.config/shipit.toml" | wc -l)
  gate "git push origin main" auto
  gate "git push -u origin slice/v1/x" auto
  after=$(git status --porcelain)
  store_after=$(find "$(git rev-parse --git-common-dir)" -type f -newer "$REPO/.config/shipit.toml" | wc -l)
  [ "$before" = "$after" ]
  [ "$store_before" = "$store_after" ]
}

@test "the common pass case runs no git" {
  local shims="$BATS_TEST_TMPDIR/shims"
  mkdir -p "$shims"
  printf '#!/bin/sh\necho "$@" >>%s/calls\nexec %s "$@"\n' "$shims" "$(command -v git)" >"$shims/git"
  chmod +x "$shims/git"
  run --separate-stderr bash -c 'cd "$1" && PATH="$2:$PATH" exec "$3" claude-ship-gate <<<"$4"' \
    _ "$BATS_TEST_TMPDIR" "$shims" "$SHIPIT_BIN" "$(bash_input "$REPO" "ls -la")"
  [ "$status" -eq 0 ]
  [ -z "$output" ]
  [ ! -e "$shims/calls" ]
}

@test "a slice push asks when the config can send it elsewhere" {
  local config
  for config in "remote.origin.push refs/heads/slice/v1/x:refs/heads/main" "push.default matching" "push.default upstream" "help.autocorrect immediate"; do
    # shellcheck disable=SC2086
    git config $config
    gate "git push -u origin slice/v1/x" bypassPermissions
    [ "$(decision)" = deny ]
    [[ $(reason) == *"can send it elsewhere"* ]]
    git config --unset "${config%% *}"
  done
  gate "git push -u origin slice/v1/x" bypassPermissions
  [ -z "$output" ]
}

@test "a command that changes the config or the remotes asks, and a read of them passes" {
  gate "git config remote.origin.push refs/heads/slice/v1/x:refs/heads/main" bypassPermissions
  [ "$(decision)" = deny ]
  gate "git remote set-url origin /elsewhere" bypassPermissions
  [ "$(decision)" = deny ]
  gate "git config --get remote.origin.url && git remote -v" bypassPermissions
  [ -z "$output" ]
}

@test "a push aimed at a ShipIt project from outside one asks" {
  local elsewhere
  elsewhere=$(mktemp -d "$BATS_TEST_TMPDIR/elsewhere.XXXXXX")
  for command in "git -C $REPO push origin main" "GIT_DIR=$REPO/.git git push origin main" "git --git-dir=$REPO/.git push origin main"; do
    run_hook claude-ship-gate "$(bash_input "$elsewhere" "$command" bypassPermissions)" "$elsewhere"
    [ "$(decision)" = deny ]
  done
}

@test "an mcp write to a branch asks unless the branch is a slice branch" {
  local tool branch
  for tool in create_or_update_file delete_file push_files; do
    for branch in main feature; do
      run_hook claude-ship-gate "$(hook_input "$(jq -cn --arg cwd "$REPO" --arg tool "mcp__github__$tool" --arg branch "$branch" \
        '{cwd: $cwd, tool_name: $tool, tool_input: {branch: $branch}, permission_mode: "bypassPermissions"}')")"
      [ "$(decision)" = deny ]
    done
    run_hook claude-ship-gate "$(hook_input "$(jq -cn --arg cwd "$REPO" --arg tool "mcp__github__$tool" \
      '{cwd: $cwd, tool_name: $tool, tool_input: {branch: "slice/v1/x"}, permission_mode: "bypassPermissions"}')")"
    [ -z "$output" ]
  done
}

@test "twenty thousand messages are judged in time" {
  local input="$BATS_TEST_TMPDIR/big.json"
  {
    printf '{"cwd":"%s","tool_name":"Bash","permission_mode":"auto","tool_input":{"command":"git commit' "$REPO"
    yes ' -m \"x\"' | head -n 20000 | tr -d '\n'
    printf '"}}'
  } >"$input"
  run --separate-stderr timeout 10 "$SHIPIT_BIN" claude-ship-gate <"$input"
  [ "$status" -eq 0 ]
  [ -z "$output" ]
}

# push.default=upstream sends a branch to the branch it tracks: main, for a slice cut from it.
@test "a slice cut from main asks under push.default=upstream, and not under simple" {
  git push -q origin main
  git fetch -q origin
  git checkout -q -b slice/v1/z origin/main
  printf 'z\n' >z.txt
  git add z.txt
  git commit -q -m "z"
  git config push.default upstream
  gate "git push origin slice/v1/z" bypassPermissions
  [ "$(decision)" = deny ]
  [[ $(reason) == *"can send it elsewhere"* ]]
  git config push.default simple
  gate "git push origin slice/v1/z" bypassPermissions
  [ -z "$output" ]
}

@test "git commands that are not on the list ask, and the ones on it pass" {
  local command
  for command in "git -c core.fsmonitor='g\"\"it push origin main #' status" "git rebase --exec 'g\"\"it push origin main' HEAD~1" "git submodule foreach 'g\"\"it push origin main'" "git psuh origin main"; do
    gate "$command" bypassPermissions
    [ "$(decision)" = deny ]
  done
  for command in "git status" "git add -A && git commit -m 'x' && git push -u origin slice/v1/x" "git add config" "git log -- config" "git fetch"; do
    gate "$command" bypassPermissions
    [ -z "$output" ]
  done
}

@test "a cd to a folder under the home folder is read and judged there" {
  local elsewhere
  elsewhere=$(mktemp -d "$BATS_TEST_TMPDIR/elsewhere.XXXXXX")
  cp -r "$REPO" "$HOME/proj"
  run_hook claude-ship-gate "$(bash_input "$elsewhere" "cd ~/proj && git push origin main" bypassPermissions)" "$elsewhere"
  [ "$(decision)" = deny ]
  gate "cd ~/proj && git status" bypassPermissions
  [ -z "$output" ]
  gate "cd ~/proj && git push origin main" bypassPermissions
  [ "$(decision)" = deny ]
}

@test "a cd to another folder never takes the working folder out of the judgement" {
  local plain="$BATS_TEST_TMPDIR/plain" command
  git init -q "$plain"
  for command in \
    "cd $plain && git push origin main" \
    "cd $plain && bash -c 'cd $REPO && git push origin main'" \
    "cd $plain && env -S 'git -C $REPO push origin main'" \
    "cd $plain && gh pr merge 1 -R owner/repo" \
    "cd $plain && cp evil $REPO/.git/hooks/pre-commit" \
    "git push origin main && cd $plain"; do
    gate "$command" bypassPermissions
    [ "$(decision)" = deny ]
  done
}

# A commit from before the setup file: the repository is still the project's.
old_commit() {
  git commit-tree -m old "$(git hash-object -t tree /dev/null)"
}

@test "a worktree or a checkout from before the setup file is judged as the project" {
  local old wt="$BATS_TEST_TMPDIR/w"
  old=$(old_commit)
  git worktree add -q --detach "$wt" "$old"
  run_hook claude-ship-gate "$(bash_input "$wt" "git push origin main" bypassPermissions)" "$wt"
  [ "$(decision)" = deny ]
  gate "cd $wt && git push origin main" bypassPermissions
  [ "$(decision)" = deny ]
  git checkout -q --detach "$old"
  [ ! -e .config/shipit.toml ]
  for command in "git push origin main" "gh pr merge 1"; do
    gate "$command" bypassPermissions
    [ "$(decision)" = deny ]
  done
}

@test "a repository is a project by its default branch or its store, and not by another branch" {
  local plain="$BATS_TEST_TMPDIR/other"
  git init -q -b main "$plain"
  git -C "$plain" config user.name t
  git -C "$plain" config user.email t@example.invalid
  git -C "$plain" commit -q --allow-empty -m first
  git -C "$plain" checkout -q -b side
  mkdir -p "$plain/.config"
  cp "$REPO/.config/shipit.toml" "$plain/.config/shipit.toml"
  git -C "$plain" add .config
  git -C "$plain" commit -q -m setup
  git -C "$plain" checkout -q main
  # The setup file is on a branch that is not the default: no project.
  run_hook claude-ship-gate "$(bash_input "$plain" "git push origin main" bypassPermissions)" "$plain"
  [ -z "$output" ]
  # On the default branch, a checkout from before it is still the project.
  git -C "$plain" checkout -q --detach
  git -C "$plain" branch -q -f main side
  git -C "$plain" checkout -q --detach "$(git -C "$plain" rev-list --max-parents=0 side)"
  run_hook claude-ship-gate "$(bash_input "$plain" "git push origin main" bypassPermissions)" "$plain"
  [ "$(decision)" = deny ]
  # The store folder of the repository makes it one too.
  local store="$BATS_TEST_TMPDIR/store"
  git init -q -b main "$store"
  run_hook claude-ship-gate "$(bash_input "$store" "git push origin main" bypassPermissions)" "$store"
  [ -z "$output" ]
  mkdir "$store/.git/shipit"
  run_hook claude-ship-gate "$(bash_input "$store" "git push origin main" bypassPermissions)" "$store"
  [ "$(decision)" = deny ]
}

@test "origin's main stands in for the default branch when origin/HEAD is missing and main is gone" {
  git push -q origin main
  git fetch -q origin
  git symbolic-ref --quiet refs/remotes/origin/HEAD && git remote set-head origin -d
  local old
  old=$(old_commit)
  git checkout -q --detach "$old"
  git branch -q -D main
  gate "git push origin main" bypassPermissions
  [ "$(decision)" = deny ]
}

@test "what could empty the rule that a repository is a project asks" {
  local command
  for command in "git branch -D main" "git branch -m main old" "git branch -rd origin/main" "git fetch . +x:refs/remotes/origin/main"; do
    gate "$command" bypassPermissions
    [ "$(decision)" = deny ]
  done
}

# by <type> <command> [mode]: the ship gate on a Bash call of a subagent of that type, in the repo.
by() {
  run_hook claude-ship-gate "$(as_agent "$1" "$(bash_input "$REPO" "$2" "${3:-default}")")"
}

# edits <who> <tool> <path> [mode]: the ship gate on an edit call; who is `main` or a subagent type.
edits() {
  local input
  input=$(edit_input "$REPO" "$2" "$3")
  if [[ $1 != main ]]; then
    input=$(as_agent "$1" "$input")
  fi
  run_hook claude-ship-gate "$input"
}

@test "ship gate: on a slice branch only the maker moves the work, with its trailer" {
  local maker='--trailer "ShipIt-Made-By: shipit:maker"' who command
  by shipit:maker "git commit -m x $maker"
  [ -z "$output" ]
  by shipit:maker "git commit -m x --trailer=\"ShipIt-Made-By: shipit:maker\""
  [ -z "$output" ]
  by shipit:maker "git rebase main"
  [ -z "$output" ]
  for command in 'git commit -m x' 'git commit -m x --trailer "ShipIt-Made-By: owner"' \
    "git -c a=b commit -m x $maker" "env git commit -m x $maker" "nix develop -c git commit -m x $maker" \
    "sh -c 'git commit -m x $maker'" "git commit -m \"\$(cat m)\" $maker"; do
    by shipit:maker "$command"
    [ "$(decision)" = deny ]
    [[ $(reason) == 'ShipIt: a maker commits with one plain command that carries its trailer: git commit -m "<message>" --trailer "ShipIt-Made-By: shipit:maker".' ]]
  done
  for who in shipit:slice shipit:work-reviewer; do
    for command in "git commit -m x $maker" 'git cherry-pick X' 'git reset --hard X' 'git revert X' \
      'git merge X' 'git rebase main' 'git am p.patch' 'git pull'; do
      by "$who" "$command"
      [ "$(decision)" = deny ]
      [[ $(reason) == "ShipIt: on a slice branch only shipit:maker commits or moves the work, so that a receipt names who made it. Start shipit:maker for this change." ]]
    done
  done
  by shipit:slice 'git log --grep commit'
  [ -z "$output" ]
  gate 'git commit -m x --trailer "ShipIt-Made-By: owner"' default
  [ "$(decision)" = ask ]
  [[ $(reason) == 'ShipIt: only shipit:maker changes a slice'"'"'s work. Answer yes only if you, the owner, make this change yourself; commit it with --trailer "ShipIt-Made-By: owner".' ]]
  gate 'git commit -m x --trailer "ShipIt-Made-By: owner"' bypassPermissions
  [ "$(decision)" = deny ]
  gate "git commit -m x $maker" default
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt: only shipit:maker may write its own trailer; the main session is not the maker." ]]
  gate 'git commit -m x' default
  [ "$(decision)" = ask ]
  gate 'git commit -m "$(cat m)"' default
  [ "$(decision)" = ask ]
  gate 'echo git commit' default
  [ -z "$output" ]
  # a folder of its own: the main copy is on main, the slice is a worktree
  git checkout -q main
  git worktree add -q -b slice/v1/w "$BATS_TEST_TMPDIR/wt"
  gate "git -C $BATS_TEST_TMPDIR/wt commit -m x" default
  [ "$(decision)" = ask ]
  gate "cd $BATS_TEST_TMPDIR/wt && git commit -m x" default
  [ "$(decision)" = ask ]
  by shipit:slice "git -C $BATS_TEST_TMPDIR/wt reset --hard X"
  [ "$(decision)" = deny ]
  # off a slice branch there is no author answer
  git checkout -q -b adopt/setup
  for who in main shipit:slice shipit:maker; do
    for command in 'git commit -m x' 'git rebase main' "env git commit -m x"; do
      if [[ $who == main ]]; then gate "$command" default; else by "$who" "$command"; fi
      [ -z "$output" ]
    done
  done
  git checkout -q main
  gate 'git commit -m x' default
  [ -z "$output" ]
}

@test "ship gate: on a slice branch only the maker edits what git would commit" {
  local wt="$BATS_TEST_TMPDIR/wt" tool
  git checkout -q main
  printf '.shipit/td/\n.shipit/drive/\nnotes/\ntarget/\n*.lock\n' >.gitignore
  printf 'echo hi\n' >greet.sh
  printf 'x\n' >x.lock
  git add .gitignore greet.sh
  git add -f x.lock
  git commit -q -m "files"
  git worktree add -q -b slice/v1/w "$wt"
  edits main Edit "$wt/greet.sh"
  [ "$(decision)" = ask ]
  [[ $(reason) == 'ShipIt: only shipit:maker changes a slice'"'"'s work. Answer yes only if you, the owner, make this change yourself; commit it with --trailer "ShipIt-Made-By: owner".' ]]
  edits shipit:slice Edit "$wt/greet.sh"
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt: on a slice branch only shipit:maker edits the work (a file git would commit), so that a receipt names who made it. Start shipit:maker for this change." ]]
  edits shipit:maker Edit "$wt/greet.sh"
  [ -z "$output" ]
  for tool in Edit Write NotebookEdit; do
    edits shipit:slice "$tool" "$wt/greet.sh"
    [ "$(decision)" = deny ]
  done
  edits main Write "$wt/.shipit/td/alpha/x.md"
  [ -z "$output" ]
  edits shipit:slice Write "$wt/notes/x.md"
  [ -z "$output" ]
  edits shipit:slice Write "$wt/target/x"
  [ -z "$output" ]
  edits main Edit "$wt/x.lock"
  [ "$(decision)" = ask ]
  edits shipit:slice Edit "$wt/x.lock"
  [ "$(decision)" = deny ]
  edits main Write "$wt/new/folder/f.txt"
  [ "$(decision)" = ask ]
  edits shipit:slice Write "$BATS_TEST_TMPDIR/outside.txt"
  [ -z "$output" ]
  edits shipit:slice Edit "$REPO/greet.sh"
  [ -z "$output" ]
}
