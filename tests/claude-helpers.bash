# shellcheck shell=bash
# Helpers for the tests of the Claude Code hooks. Load with: load helpers, then load claude-helpers

# shipit_repo: a repo as make_repo makes it, with no check commands in its setup, so that
# shipit check is instant. It is committed on main.
shipit_repo() {
  local repo
  repo=$(make_repo) || return 1
  (cd "$repo" && write_setup "$(fixture_with 'gate.check=[]')") || return 1
  printf '%s\n' "$repo"
}

# hook_input <json-object>: a hook event; the fields given are merged over these defaults:
# hook_event_name PreToolUse, permission_mode default, session_id s1, tool_use_id t1.
hook_input() {
  local extra=${1:-}
  [[ -n $extra ]] || extra='{}'
  jq -cn --argjson extra "$extra" \
    '{hook_event_name: "PreToolUse", permission_mode: "default", session_id: "s1", tool_use_id: "t1"} + $extra'
}

# bash_input <cwd> <command> [mode]: the event of a Bash tool call.
bash_input() {
  hook_input "$(jq -cn --arg cwd "$1" --arg command "$2" --arg mode "${3:-default}" \
    '{cwd: $cwd, tool_name: "Bash", tool_input: {command: $command}, permission_mode: $mode}')"
}

# run_hook <command> <input> [folder]: run `shipit <command>` with the input on stdin, from the
# folder (default: a new folder outside any repository). Asserts exit 0 and that stdout is empty
# or one JSON object. Stdout is in $output, stderr in $stderr.
run_hook() {
  local command=$1 input=$2 folder=${3:-}
  if [[ -z $folder ]]; then
    folder=$(mktemp -d "${BATS_TEST_TMPDIR:?run under bats}/scratch.XXXXXX")
  fi
  run --separate-stderr bash -c 'cd "$1" && exec "$2" "$3"' _ "$folder" "$SHIPIT_BIN" "$command" <<<"$input"
  [ "$status" -eq 0 ]
  if [[ -n $output ]]; then
    jq -e 'type == "object"' <<<"$output" >/dev/null
  fi
}

# decision: the permissionDecision of the last hook output; reason: its reason.
decision() {
  jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$output"
}

reason() {
  jq -r '.hookSpecificOutput.permissionDecisionReason // empty' <<<"$output"
}

# agent_input <cwd> <subagent_type> <prompt>: the event of an Agent tool call.
agent_input() {
  hook_input "$(jq -cn --arg cwd "$1" --arg type "$2" --arg prompt "$3" \
    '{cwd: $cwd, tool_name: "Agent", tool_input: {subagent_type: $type, prompt: $prompt, description: "d"}, permission_mode: "auto"}')"
}
