#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load claude-helpers

# A repo on main whose setup has no checks and the default cap of 4 agents, and a plugin data
# folder for the ledger.
setup() {
  REPO=$(shipit_repo)
  DATA="$BATS_TEST_TMPDIR/plugin-data"
  export CLAUDE_PLUGIN_DATA="$DATA"
  cd "$REPO" || return 1
}

# call <tool_use_id> [session]: the cap gate on an Agent call (a role that needs no receipt).
call() {
  run_hook claude-agent-gate "$(agent_input "$REPO" general-purpose "Do it." |
    jq -c --arg id "$1" --arg session "${2:-s1}" '.tool_use_id = $id | .session_id = $session')"
}

# event <name> <json fields>: a bookkeeping event; no tool_name and no tool_input.
event() {
  run_hook claude-agent-count "$(hook_input "$(jq -cn --arg name "$1" --arg cwd "$REPO" \
    --argjson fields "${2:-{\}}" '{hook_event_name: $name, cwd: $cwd} + $fields')")"
}

admitted() {
  [ -z "$output" ]
}

refused() {
  [ "$(decision)" = deny ]
  [[ $(reason) == *"([agents.parallel] agents)"* ]]
}

# fill <n>: n admitted calls, t1…tn.
fill() {
  local i
  for i in $(seq 1 "$1"); do
    call "t$i"
    admitted
  done
}

entries() {
  find "$DATA/agents/${1:-s1}" -type f \( -name 'r-*' -o -name 'a-*' \) -printf '%f\n' | sort
}

@test "eight agents started at once: exactly four admitted" {
  local i input
  input=$(agent_input "$REPO" general-purpose "Do it.")
  for i in $(seq 1 8); do
    (
      jq -c --arg id "t$i" '.tool_use_id = $id' <<<"$input" |
        "$SHIPIT_BIN" claude-agent-gate >"$BATS_TEST_TMPDIR/out.$i"
    ) &
  done
  wait
  local empty=0 denied=0
  for i in $(seq 1 8); do
    if [[ ! -s $BATS_TEST_TMPDIR/out.$i ]]; then
      empty=$((empty + 1))
    elif jq -e '.hookSpecificOutput.permissionDecision == "deny" and (.hookSpecificOutput.permissionDecisionReason | contains("[agents.parallel] agents"))' \
      "$BATS_TEST_TMPDIR/out.$i" >/dev/null; then
      denied=$((denied + 1))
    fi
  done
  [ "$empty" -eq 4 ]
  [ "$denied" -eq 4 ]
  [ "$(entries | wc -l)" -eq 4 ]
}

@test "a start, then a stop, frees a place" {
  fill 4
  call t5
  refused
  event SubagentStart '{"session_id":"s1","agent_id":"a1"}'
  call t5
  refused
  [ "$(entries | grep -c '^a-')" -eq 1 ]
  [ "$(entries | grep -c '^r-')" -eq 3 ]
  event SubagentStop '{"session_id":"s1","agent_id":"a1"}'
  call t5
  admitted
  event SubagentStop '{"session_id":"s1","agent_id":"nobody"}'
}

@test "a failed Agent call frees its reservation" {
  fill 4
  event PostToolUseFailure '{"session_id":"s1","tool_use_id":"t2"}'
  call t5
  admitted
  call t6
  refused
}

@test "Stop drops what background_tasks does not list" {
  fill 3
  event SubagentStart '{"session_id":"s1","agent_id":"a1"}'
  event SubagentStart '{"session_id":"s1","agent_id":"a2"}'
  event SubagentStart '{"session_id":"s1","agent_id":"a3"}'
  call t4
  admitted
  [ "$(entries | grep -c '^a-')" -eq 3 ]
  event Stop '{"session_id":"s1","background_tasks":[{"type":"subagent"},{"type":"shell"}]}'
  [ "$(entries | grep -c '^a-')" -eq 1 ]
  [ "$(entries | grep -c '^r-')" -eq 0 ]
  event Stop '{"session_id":"s1"}'
  [ "$(entries | grep -c '^a-')" -eq 1 ]
}

@test "startup clears the ledger and removes ledgers older than 7 days" {
  fill 1
  mkdir -p "$DATA/agents/old" "$DATA/agents/young"
  date -u -d '8 days ago' +%Y-%m-%dT%H:%M:%SZ >"$DATA/agents/old/started"
  date -u +%Y-%m-%dT%H:%M:%SZ >"$DATA/agents/young/started"
  event SessionStart '{"session_id":"s1","source":"startup"}'
  [ ! -e "$DATA/agents/s1" ]
  [ ! -e "$DATA/agents/old" ]
  [ -d "$DATA/agents/young" ]
}

@test "compact and resume keep the ledger" {
  fill 2
  local source
  for source in compact resume clear fork; do
    event SessionStart "{\"session_id\":\"s1\",\"source\":\"$source\"}"
    [ "$(entries | wc -l)" -eq 2 ]
  done
  call t3
  admitted
  call t4
  admitted
  call t5
  refused
}

@test "each bookkeeping event is read without tool_name or tool_input" {
  fill 1
  event SubagentStart '{"session_id":"s1","agent_id":"a1"}'
  [ "$(entries | grep -c '^a-a1')" -eq 1 ]
  [ -z "$stderr" ]
  event Stop '{"session_id":"s1","background_tasks":[{"type":"subagent"}]}'
  [ "$(entries | grep -c '^a-a1')" -eq 1 ]
  event PostToolUseFailure '{"session_id":"s1","tool_use_id":"t9"}'
  event SubagentStop '{"session_id":"s1","agent_id":"a1"}'
  [ "$(entries | wc -l)" -eq 0 ]
  [ -z "$stderr" ]
}

@test "an unsafe id is a bad input" {
  call "../x"
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt: the agent cannot start: the hook input has no safe tool_use_id." ]]
  call t1 "../escape"
  [ "$(decision)" = deny ]
  [[ $(reason) == *"no safe session_id"* ]]
  event SubagentStart '{"session_id":"s1","agent_id":"a/b"}'
  [ ! -e "$DATA/agents/escape" ]
  mkdir -p "$DATA/agents/s1"
  event SubagentStart '{"session_id":"s1","agent_id":"a/b"}'
  [[ $stderr == "shipit: agent count: the hook input has no safe agent_id" ]]
  [ "$(find "$DATA" -type f -name 'a-*' | wc -l)" -eq 0 ]
}

@test "without CLAUDE_PLUGIN_DATA the gate denies" {
  unset CLAUDE_PLUGIN_DATA
  call t1
  [ "$(decision)" = deny ]
  [ "$(reason)" = "ShipIt cannot count the running agents: CLAUDE_PLUGIN_DATA is not set." ]
  event SubagentStart '{"session_id":"s1","agent_id":"a1"}'
  [[ $stderr == "shipit: agent count: CLAUDE_PLUGIN_DATA is not set" ]]
}

@test "outside a ShipIt project no ledger is written" {
  local plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  run_hook claude-agent-gate "$(agent_input "$plain" general-purpose "Do it.")" "$plain"
  [ -z "$output" ]
  [ ! -e "$DATA" ]
  event SubagentStart '{"session_id":"s1","agent_id":"a1"}'
  [ ! -e "$DATA/agents/s1" ]
}

@test "an invalid setup file denies the agent start" {
  git checkout -q -b slice/v1/x
  printf 'not = [valid\n' >.config/shipit.toml
  git commit -q -am "break the setup"
  call t1
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt: the agent cannot start: "*"shipit.toml"* ]]
  [ ! -e "$DATA" ]
}

@test "a slice cannot raise its own cap" {
  git checkout -q -b slice/v1/x
  write_setup "$(fixture_with 'gate.check=[]')"$'\n[agents.parallel]\nagents = 100'
  fill 4
  call t5
  refused
}

@test "the cap is read from the base branch's setup" {
  write_setup "$(fixture_with 'gate.check=[]')"$'\n[agents.parallel]\nagents = 2'
  git checkout -q -b slice/v1/x
  fill 2
  call t3
  [ "$(decision)" = deny ]
  [[ $(reason) == *"the setup allows 2 "* ]]
}

@test "the review gate comes before the cap" {
  git checkout -q -b slice/v1/x
  run_hook claude-agent-gate "$(agent_input "$REPO" shipit:work-reviewer $'shipit-branch: slice/v1/x\nReview.')"
  [ "$(decision)" = deny ]
  [[ $(reason) == *"needs a check receipt"* ]]
  [ ! -e "$DATA" ]
}
