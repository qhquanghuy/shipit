#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load claude-helpers

# A repo on main with the slice branch slice/v1/x, ShipIt's hooks installed, and shipit first on the
# PATH, as git runs the hook with it.
setup() {
  REPO=$(shipit_repo)
  export CLAUDE_PLUGIN_DATA="$BATS_TEST_TMPDIR/plugin-data"
  git -C "$REPO" branch slice/v1/x
  mkdir -p "$BATS_TEST_TMPDIR/bin"
  ln -sf "$SHIPIT_BIN" "$BATS_TEST_TMPDIR/bin/shipit"
  PATH="$BATS_TEST_TMPDIR/bin:$PATH"
  export PATH
  (cd "$REPO" && "$SHIPIT_BIN" init --hook-only >/dev/null)
}

MAKER_CALL='{"model":"sonnet","effort":"high","isolation":"worktree","run_in_background":true}'

# maker_call <tool_use_id> <branch>: the Agent call of an isolated maker for that slice branch.
maker_call() {
  agent_input "$REPO" shipit:maker "shipit-branch: $2" "$MAKER_CALL" | jq -c --arg id "$1" '.tool_use_id = $id'
}

# start_maker <tool_use_id> <branch>: the agent gate admits the call, which records its start.
start_maker() {
  run_hook claude-agent-gate "$(maker_call "$1" "$2")" "$REPO"
  [ -z "$output" ]
}

# launched <tool_use_id> <agent_id> <branch>: the call returns at once with the agent's id.
launched() {
  run_hook claude-agent-count "$(maker_call "$1" "$3" |
    jq -c --arg agent "$2" '.hook_event_name = "PostToolUse" | .tool_response = {status: "async_launched", agentId: $agent}')" "$REPO"
}

# maker_started <agent_id> <cwd>: the agent starts, or is started again, in that folder.
maker_started() {
  run_hook claude-agent-count "$(hook_input "$(jq -cn --arg cwd "$2" --arg agent "$1" \
    '{hook_event_name: "SubagentStart", cwd: $cwd, agent_id: $agent, agent_type: "shipit:maker"}')")" "$REPO"
}

# agent_tree <agent_id>: the worktree Claude Code makes for the agent, on its own branch; its path is printed.
agent_tree() {
  git -C "$REPO" worktree add -q "$REPO/.claude/worktrees/agent-$1" -b "worktree-agent-$1" main
  printf '%s\n' "$REPO/.claude/worktrees/agent-$1"
}

# commit_here <message>: stage a new file and commit; stderr is in $stderr, the exit code in $status.
commit_here() {
  printf '%s\n' "$1" >"./file-$1.txt"
  git add -A
  run --separate-stderr git commit -q -m "$1"
}

@test "pre-commit: an isolated maker commits only on its slice branch" {
  local tree head
  start_maker t1 slice/v1/x
  tree=$(agent_tree a1)
  maker_started a1 "$tree"
  launched t1 a1 slice/v1/x
  cd "$tree"
  head=$(git rev-parse HEAD)
  commit_here off-branch
  [ "$status" -ne 0 ]
  [[ $stderr == *"shipit: this worktree is shipit:maker's for slice/v1/x; commit there: git switch slice/v1/x (docs/claude-code.md, Parallel makers)"* ]]
  [ "$(git rev-parse HEAD)" = "$head" ]
  git switch -q slice/v1/x
  commit_here on-branch
  [ "$status" -eq 0 ]
  [[ $stderr != *"shipit:"* ]]
  [ "$(git rev-parse HEAD)" != "$head" ]
  cd "$REPO"
  commit_here on-main
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
}

@test "pre-commit: a maker whose slice is not known is warned, and a resumed maker too" {
  local tree
  start_maker t2 slice/v1/x
  tree=$(agent_tree a2)
  maker_started a2 "$tree"
  cd "$tree"
  commit_here unknown
  [ "$status" -eq 0 ]
  [ "$stderr" = "shipit: not checked: this maker's slice branch is not known; start it again with run_in_background (docs/claude-code.md, Parallel makers)" ]
  tree=$(agent_tree a3)
  cd "$tree"
  git switch -q slice/v1/x
  commit_here unpaired
  [ "$status" -eq 0 ]
  [ "$stderr" = "shipit: not checked: no maker record names this worktree, but a maker was started for slice/v1/x (docs/claude-code.md, Parallel makers)" ]
  cd "$REPO"
  maker_started a2 "$REPO"
  [ "$(jq -r '.hookSpecificOutput.hookEventName' <<<"$output")" = SubagentStart ]
  [ "$(jq -r '.hookSpecificOutput.additionalContext' <<<"$output")" = "ShipIt: not checked: agent a2 is an isolated shipit:maker started again outside its worktree; stop and tell the lead to start a new maker (docs/claude-code.md, Parallel makers)" ]
}
