#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load claude-helpers

# The repository root is the plugin root.
PLUGIN=$SHIPIT_SRC

# plugin_copy: a copy of the launcher, the build script and the hooks, as an installed plugin
# folder; its path is in $COPY. Without a build, as the plugin is when first installed.
plugin_copy() {
  COPY=$(mktemp -d "$BATS_TEST_TMPDIR/plugin.XXXXXX")
  COPY=$(cd -P "$COPY" && pwd)
  mkdir -p "$COPY/bin" "$COPY/scripts" "$COPY/hooks"
  cp "$PLUGIN/bin/shipit" "$COPY/bin/shipit"
  cp "$PLUGIN/scripts/shipit-build" "$COPY/scripts/shipit-build"
  cp "$PLUGIN/hooks/hooks.json" "$COPY/hooks/hooks.json"
}

# stub_nix <exit status>: a nix on a new PATH folder ($STUBS) that records its arguments in
# $STUBS/calls and, when it succeeds, makes the build the --out-link names.
stub_nix() {
  STUBS=$(mktemp -d "$BATS_TEST_TMPDIR/stubs.XXXXXX")
  cat >"$STUBS/nix" <<STUB
#!/bin/sh
printf '%s\n' "\$*" >>"$STUBS/calls"
if [ $1 -eq 0 ]; then
  while [ \$# -gt 0 ]; do
    if [ "\$1" = --out-link ]; then
      mkdir -p "\$2/bin"
      printf '#!/bin/sh\n' >"\$2/bin/shipit"
      chmod +x "\$2/bin/shipit"
    fi
    shift
  done
fi
exit $1
STUB
  chmod +x "$STUBS/nix"
}

@test "the manifests parse and hold their fields" {
  jq -e '.name == "shipit" and (has("version") | not) and .license == "MIT" and (.keywords | length > 0) and (.author.name | length > 0) and (.description | length > 0)' \
    "$PLUGIN/.claude-plugin/plugin.json"
  jq -e '.name == "shipit" and (.owner.name | length > 0) and (.description | length > 0) and (.plugins | length == 1) and .plugins[0].name == "shipit" and .plugins[0].source == "./"' \
    "$PLUGIN/.claude-plugin/marketplace.json"
}

@test "every hook is in exec form and runs an executable file of the plugin" {
  local command file
  while IFS= read -r command; do
    [[ $command == '${CLAUDE_PLUGIN_ROOT}/'* ]]
    file="$PLUGIN/${command#'${CLAUDE_PLUGIN_ROOT}/'}"
    [ -x "$file" ]
  done < <(jq -r '.hooks[][].hooks[].command' "$PLUGIN/hooks/hooks.json" | sort -u)
  jq -e '[.hooks[][].hooks[] | .type == "command" and (.args | type == "array") and (.timeout > 0)] | all' "$PLUGIN/hooks/hooks.json"
  jq -e '[.hooks.PreToolUse[].hooks[] | .onFailure == "block" and .timeout == 10] | all' "$PLUGIN/hooks/hooks.json"
  jq -e '[.hooks | to_entries[] | select(.key != "PreToolUse") | .value[].hooks[] | .onFailure == "continue"] | all' "$PLUGIN/hooks/hooks.json"
  # The ledger is cleared at startup and at resume, so the count hook must be run for both.
  jq -e '.hooks.SessionStart | map(select(.hooks[0].args[0] == "claude-agent-count")) | .[0].matcher == "startup|resume"' "$PLUGIN/hooks/hooks.json"
}

@test "every hook command is known to the binary" {
  local arg folder
  folder=$(mktemp -d "$BATS_TEST_TMPDIR/scratch.XXXXXX")
  cd "$folder"
  while IFS= read -r arg; do
    run --separate-stderr "$SHIPIT_BIN" "$arg" <<<"{}"
    [ "$status" -eq 0 ]
    [ -z "$output" ]
    [[ $stderr != *"unknown command"* ]]
  done < <(jq -r '.hooks[][].hooks[] | select(.command | endswith("/bin/shipit")) | .args[0]' "$PLUGIN/hooks/hooks.json" | sort -u)
  [ "$(jq -r '.hooks[][].hooks[] | select(.command | endswith("/bin/shipit")) | .args[0]' "$PLUGIN/hooks/hooks.json" | sort -u | wc -l)" -eq 3 ]
}

@test "one call through the launcher denies a global install" {
  plugin_copy
  mkdir -p "$COPY/target/plugin/shipit/bin"
  ln -s "$SHIPIT_BIN" "$COPY/target/plugin/shipit/bin/shipit"
  local repo
  repo=$(shipit_repo)
  run --separate-stderr bash -c 'cd "$1" && exec "$2" claude-install-gate <<<"$3"' \
    _ "$BATS_TEST_TMPDIR" "$COPY/bin/shipit" "$(bash_input "$repo" "pip install requests" auto)"
  [ "$status" -eq 0 ]
  [ "$(decision)" = deny ]
}

@test "launcher without a build: the install gate of a ShipIt project blocks, the rest allow" {
  plugin_copy
  local repo plain hook
  repo=$(shipit_repo)
  plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  for hook in claude-install-gate; do
    run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" exec "$3" "$4" <<<"{}"' \
      _ "$BATS_TEST_TMPDIR" "$repo" "$COPY/bin/shipit" "$hook"
    [ "$status" -eq 2 ]
    [ -z "$output" ]
    [[ $stderr == "ShipIt: the shipit tool is not built for this plugin yet ($COPY/target/plugin/shipit/bin/shipit). Start a new session, or run: nix build '$COPY#default' --out-link '$COPY/target/plugin/shipit'" ]]
  done
  for hook in claude-agent-gate claude-agent-count; do
    run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" exec "$3" "$4" <<<"{}"' \
      _ "$BATS_TEST_TMPDIR" "$repo" "$COPY/bin/shipit" "$hook"
    [ "$status" -eq 0 ]
    [ -z "$output$stderr" ]
  done
  for hook in claude-install-gate claude-agent-gate claude-agent-count; do
    run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" exec "$3" "$4" <<<"{}"' \
      _ "$BATS_TEST_TMPDIR" "$plain" "$COPY/bin/shipit" "$hook"
    [ "$status" -eq 0 ]
    [ -z "$output$stderr" ]
  done
  run --separate-stderr bash -c 'cd "$1" && unset CLAUDE_PROJECT_DIR && exec "$2" claude-install-gate <<<"{}"' \
    _ "$repo" "$COPY/bin/shipit"
  [ "$status" -eq 2 ]
  run --separate-stderr "$COPY/bin/shipit" check
  [ "$status" -eq 2 ]
  [[ $stderr == "ShipIt: the shipit tool is not built for this plugin yet"* ]]
}

@test "shipit-build with a stub nix builds once, outside a ShipIt project too" {
  plugin_copy
  stub_nix 0
  local plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  run --separate-stderr bash -c 'cd "$1" && PATH="$2:$PATH" exec "$3"' _ "$plain" "$STUBS" "$COPY/scripts/shipit-build"
  [ "$status" -eq 0 ]
  [ -z "$output$stderr" ]
  [ "$(cat "$STUBS/calls")" = "build $COPY#default --out-link $COPY/target/plugin/shipit --quiet" ]
  [ -x "$COPY/target/plugin/shipit/bin/shipit" ]
  run --separate-stderr bash -c 'cd "$1" && PATH="$2:$PATH" exec "$3"' _ "$plain" "$STUBS" "$COPY/scripts/shipit-build"
  [ "$status" -eq 0 ]
  [ "$(wc -l <"$STUBS/calls")" -eq 1 ]
}

@test "shipit-build of a checkout builds again" {
  plugin_copy
  stub_nix 0
  git init -q "$COPY"
  local i
  for i in 1 2; do
    run --separate-stderr bash -c 'cd "$1" && PATH="$2:$PATH" exec "$3"' _ "$BATS_TEST_TMPDIR" "$STUBS" "$COPY/scripts/shipit-build"
    [ "$status" -eq 0 ]
  done
  [ "$(wc -l <"$STUBS/calls")" -eq 2 ]
}

@test "a failed build says so in a ShipIt project and is silent elsewhere" {
  plugin_copy
  stub_nix 1
  local repo plain
  repo=$(shipit_repo)
  plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" PATH="$3:$PATH" exec "$4"' _ "$BATS_TEST_TMPDIR" "$repo" "$STUBS" "$COPY/scripts/shipit-build"
  [ "$status" -eq 2 ]
  [ -z "$output" ]
  [ "$(wc -l <<<"$stderr")" -eq 1 ]
  [[ $stderr == "ShipIt: cannot build the shipit tool. Run: nix build '$COPY#default' --out-link '$COPY/target/plugin/shipit'" ]]
  run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" PATH="$3:$PATH" exec "$4"' _ "$BATS_TEST_TMPDIR" "$plain" "$STUBS" "$COPY/scripts/shipit-build"
  [ "$status" -eq 0 ]
  [ -z "$output$stderr" ]
}

@test "without nix a build fails the same way" {
  plugin_copy
  local repo tools tool
  repo=$(shipit_repo)
  tools=$(mktemp -d "$BATS_TEST_TMPDIR/tools.XXXXXX")
  for tool in dirname readlink git mkdir; do
    ln -s "$(command -v "$tool")" "$tools/$tool"
  done
  run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" PATH="$3" exec "$4" "$5"' \
    _ "$BATS_TEST_TMPDIR" "$repo" "$tools" "$(command -v bash)" "$COPY/scripts/shipit-build"
  [ "$status" -eq 2 ]
  [[ $stderr == "ShipIt: cannot build the shipit tool."* ]]
}

@test "the eight agents are named by their files, and the gated ones are in the hook code" {
  local names
  names=$(cd "$PLUGIN/agents" && printf '%s\n' *.md | sed 's/\.md$//' | sort | tr '\n' ' ')
  [ "$names" = "maker model-scout plan-reviewer reader scout slice tester work-reviewer " ]
  grep -q '"shipit:work-reviewer"' "$PLUGIN/src/pure/claude_hook.rs"
  grep -q '"shipit:tester"' "$PLUGIN/src/pure/claude_hook.rs"
  run ! grep -q 'const .*"shipit:\(plan-reviewer\|reader\|scout\|model-scout\|slice\)"' "$PLUGIN/src/pure/claude_hook.rs"
}

@test "an agent file holds a description, no model and no effort, and only the slice agent may start agents" {
  local file name
  for file in "$PLUGIN"/agents/*.md; do
    [ "$(sed -n 1p "$file")" = "---" ]
    grep -q '^description: .' "$file"
    run ! grep -qE '^(model|effort|name):' "$file"
    name=$(basename "$file" .md)
    if [ "$name" = slice ]; then
      run ! grep -q '^tools:' "$file"
    else
      grep -q '^tools: ' "$file"
      run ! grep -q '^tools:.*Agent' "$file"
    fi
  done
  grep -q '^tools: Read, Grep, Glob, Bash$' "$PLUGIN/agents/plan-reviewer.md"
  grep -q '^tools: Read, Grep, Glob, Bash$' "$PLUGIN/agents/reader.md"
  grep -q '^tools: Read, Grep, Glob, Bash$' "$PLUGIN/agents/scout.md"
  grep -q '^tools: Read, Grep, Glob, Bash$' "$PLUGIN/agents/work-reviewer.md"
  grep -q '^tools: Read, Grep, Glob, Bash, WebSearch, WebFetch$' "$PLUGIN/agents/model-scout.md"
  grep -q '^tools: Read, Grep, Glob, Bash, Write, Edit, Skill$' "$PLUGIN/agents/maker.md"
  grep -q '^tools: Read, Grep, Glob, Bash, Write, Edit, Skill$' "$PLUGIN/agents/tester.md"
}

@test "the agent files pass the lint" {
  local repo
  repo=$(make_repo)
  mkdir -p "$repo/agents" "$repo/harness" "$repo/docs"
  cp "$PLUGIN"/agents/*.md "$repo/agents/"
  cp "$PLUGIN/harness/claude-code.txt" "$repo/harness/"
  cp "$PLUGIN/docs/vocab.md" "$repo/docs/"
  git -C "$repo" add -A
  git -C "$repo" commit -q -m "agents"
  cd "$repo"
  run_shipit lint
  [ "$status" -eq 0 ]
  [[ $output == "lint: "*" files clean" ]]
}

@test "the harness list matches files, and lists every file of the plugin" {
  local glob file listed
  while IFS= read -r glob; do
    [[ -z $glob || $glob == '#'* ]] && continue
    # shellcheck disable=SC2086
    compgen -G "$PLUGIN/$glob" >/dev/null
  done <"$PLUGIN/harness/claude-code.txt"
  while IFS= read -r file; do
    listed=no
    while IFS= read -r glob; do
      [[ -z $glob || $glob == '#'* ]] && continue
      # shellcheck disable=SC2053
      [[ $file == $glob ]] && listed=yes
    done <"$PLUGIN/harness/claude-code.txt"
    [ "$listed" = yes ] || { echo "not listed: $file"; return 1; }
  done < <(cd "$PLUGIN" && find .claude-plugin hooks agents bin scripts src/commands -type f \( -path 'src/commands/*' -name 'claude_*.rs' -o -not -path 'src/commands/*' \) | sed 's|^\./||' | sort)
}

@test "validate prints only the accepted warnings" {
  command -v claude >/dev/null || skip "claude is not on the PATH"
  run claude plugin validate "$PLUGIN"
  [ "$status" -eq 0 ]
  [[ $output == *"Validation passed with warnings"* ]]
  [[ $output != *"✘ Found"*"problems"*"Validating"* ]]
  [ "$(grep -c '❯ version:\|❯ plugins\[0\] plugin.json → version:' <<<"$output")" -eq 2 ]
  [ "$(grep -c '❯ root: CLAUDE.md at the plugin root' <<<"$output")" -eq 1 ]
  [ "$(grep -c 'Found [0-9]* warning:' <<<"$output")" -eq 3 ]
  grep -q 'agents: shipit:work-reviewer' <<<"$output"
}

# hook_table [hooks file]: the PreToolUse table, one line: each matcher with the gates it runs.
hook_table() {
  jq -c '[.hooks.PreToolUse[] | [.matcher, [.hooks[].args[0]]]]' "${1:-$PLUGIN/hooks/hooks.json}"
}

# post_tool_use_ok <hooks file>: PostToolUse runs only the agent count, on Agent|Task, and never blocks.
post_tool_use_ok() {
  jq -e '.hooks.PostToolUse == [{matcher: "Agent|Task", hooks: [{type: "command", command: "${CLAUDE_PLUGIN_ROOT}/bin/shipit", args: ["claude-agent-count"], timeout: 10, onFailure: "continue"}]}]' "$1"
}

@test "the hook table holds only the agent gate and the install gate before a tool runs, and no hook on an event that can refuse" {
  local hooks=$PLUGIN/hooks/hooks.json broken=$BATS_TEST_TMPDIR/hooks.json
  [ "$(hook_table)" = '[["Agent|Task",["claude-agent-gate"]],["Bash|Monitor",["claude-install-gate"]]]' ]
  # No other event: PermissionRequest and UserPromptSubmit can refuse or block, and have no hook; PostToolUse
  # runs only the agent count, on Agent|Task, and never blocks.
  [ "$(jq -c '.hooks | keys' "$hooks")" = '["PostToolUse","PostToolUseFailure","PreToolUse","SessionStart","Stop","SubagentStart","SubagentStop"]' ]
  post_tool_use_ok "$hooks"
  jq '.hooks.PostToolUse[0].hooks[0].args = ["claude-form-gate"]' "$hooks" >"$broken"
  run post_tool_use_ok "$broken"
  [ "$status" -ne 0 ]
  jq '.hooks.PostToolUse[0].hooks[0].onFailure = "block"' "$hooks" >"$broken"
  run post_tool_use_ok "$broken"
  [ "$status" -ne 0 ]
  # Stop and SubagentStop can block too: there only the agent count runs, and it never blocks.
  jq -e '[.hooks.Stop[], .hooks.SubagentStop[] | .hooks[] | .args[0] == "claude-agent-count" and .onFailure == "continue"] | all' "$hooks"
  # A gate added with no matcher runs on every tool: the table shows it.
  jq '.hooks.PreToolUse += [{hooks: [{type: "command", command: "x", args: ["claude-form-gate"], timeout: 10, onFailure: "block"}]}]' "$hooks" >"$broken"
  [ "$(hook_table "$broken")" != '[["Agent|Task",["claude-agent-gate"]],["Bash|Monitor",["claude-install-gate"]]]' ]
  jq '.hooks.PermissionRequest = [{hooks: [{type: "command", command: "x", args: ["claude-form-gate"], timeout: 10, onFailure: "block"}]}]' "$hooks" >"$broken"
  [ "$(jq -c '.hooks | keys' "$broken")" != '["PostToolUse","PostToolUseFailure","PreToolUse","SessionStart","Stop","SubagentStart","SubagentStop"]' ]
}
