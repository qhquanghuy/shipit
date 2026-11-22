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

@test "a one-segment slice branch push asks" {
  gate "git push -u origin slice/x" auto
  [ "$(decision)" = ask ]
}

@test "a bare push from a slice branch asks" {
  gate "git push" auto
  [ "$(decision)" = ask ]
  gate "git push origin HEAD" bypassPermissions
  [ "$(decision)" = deny ]
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

@test "a heredoc commit message and plain read-only forms pass with no prompt" {
  gate $'git commit -m "$(cat <<\'EOF\'\nfix: git push origin main\nEOF\n)"' bypassPermissions
  [ -z "$output" ]
  gate "git log @{u}.." bypassPermissions
  [ -z "$output" ]
  gate "git diff HEAD~1" bypassPermissions
  [ -z "$output" ]
}

@test "a command that moves the branch before a push asks" {
  gate "git checkout main && git merge slice/v1/x && git push" auto
  [ "$(decision)" = ask ]
  gate "git checkout main && git merge slice/v1/x && git push" bypassPermissions
  [ "$(decision)" = deny ]
}

@test "shipit complete with --confirm asks in auto mode and is denied in bypass mode" {
  gate "shipit complete demo --confirm" auto
  [ "$(decision)" = ask ]
  [[ $(reason) == *"confirms a goal completion (shipit complete --confirm), which only the owner may do"* ]]
  gate "shipit complete demo --confirm" bypassPermissions
  [ "$(decision)" = deny ]
}

@test "shipit complete without --confirm passes" {
  gate "shipit complete demo" bypassPermissions
  [ -z "$output" ]
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
