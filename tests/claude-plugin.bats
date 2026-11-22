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

# doc_lines <kind>: the commands of the fenced blocks in docs/claude-code.md marked `sh <kind>`.
doc_lines() {
  awk -v kind="$1" '$0 == "```sh " kind { on = 1; next } /^```/ { on = 0 } on' "$PLUGIN/docs/claude-code.md"
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
  jq -e '.hooks.SessionStart | map(select(.hooks[0].args[0] == "claude-agent-count")) | .[0].matcher == "startup"' "$PLUGIN/hooks/hooks.json"
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
  [ "$(jq -r '.hooks[][].hooks[] | select(.command | endswith("/bin/shipit")) | .args[0]' "$PLUGIN/hooks/hooks.json" | sort -u | wc -l)" -eq 4 ]
}

@test "one call through the launcher asks for a base push" {
  plugin_copy
  mkdir -p "$COPY/target/plugin/shipit/bin"
  ln -s "$SHIPIT_BIN" "$COPY/target/plugin/shipit/bin/shipit"
  local repo
  repo=$(shipit_repo)
  run --separate-stderr bash -c 'cd "$1" && exec "$2" claude-ship-gate <<<"$3"' \
    _ "$BATS_TEST_TMPDIR" "$COPY/bin/shipit" "$(bash_input "$repo" "git push origin main" auto)"
  [ "$status" -eq 0 ]
  [ "$(decision)" = ask ]
}

@test "launcher without a build: the Bash gates of a ShipIt project block, the rest allow" {
  plugin_copy
  local repo plain hook
  repo=$(shipit_repo)
  plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  for hook in claude-ship-gate claude-install-gate; do
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
  for hook in claude-ship-gate claude-install-gate claude-agent-gate claude-agent-count; do
    run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" exec "$3" "$4" <<<"{}"' \
      _ "$BATS_TEST_TMPDIR" "$plain" "$COPY/bin/shipit" "$hook"
    [ "$status" -eq 0 ]
    [ -z "$output$stderr" ]
  done
  run --separate-stderr bash -c 'cd "$1" && unset CLAUDE_PROJECT_DIR && exec "$2" claude-ship-gate <<<"{}"' \
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

@test "the four agents are named by their files, and the two gated ones are in the hook code" {
  local names
  names=$(cd "$PLUGIN/agents" && printf '%s\n' *.md | sed 's/\.md$//' | sort | tr '\n' ' ')
  [ "$names" = "plan-reviewer reader tester work-reviewer " ]
  grep -q '"shipit:work-reviewer"' "$PLUGIN/src/pure/claude_hook.rs"
  grep -q '"shipit:tester"' "$PLUGIN/src/pure/claude_hook.rs"
  ! grep -q 'shipit:plan-reviewer\|shipit:reader' "$PLUGIN/src/pure/claude_hook.rs" ||
    ! grep -q 'const .*"shipit:\(plan-reviewer\|reader\)"' "$PLUGIN/src/pure/claude_hook.rs"
}

@test "an agent file holds a description, no model and no effort" {
  local file
  for file in "$PLUGIN"/agents/*.md; do
    [ "$(sed -n 1p "$file")" = "---" ]
    grep -q '^description: .' "$file"
    ! grep -qE '^(model|effort|name):' "$file"
  done
  grep -q '^tools: Read, Grep, Glob, Bash$' "$PLUGIN/agents/plan-reviewer.md"
  grep -q '^tools: Read, Grep, Glob, Bash$' "$PLUGIN/agents/reader.md"
  ! grep -q '^tools:' "$PLUGIN/agents/work-reviewer.md"
  ! grep -q '^tools:' "$PLUGIN/agents/tester.md"
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

@test "the push form the doc calls silent passes the ship gate, and the forms it says ask do ask" {
  local repo line
  repo=$(shipit_repo)
  cd "$repo"
  git checkout -q -b slice/v1/demo
  [ -n "$(doc_lines silent)" ]
  [ -n "$(doc_lines asks)" ]
  while IFS= read -r line; do
    run_hook claude-ship-gate "$(bash_input "$repo" "$line" bypassPermissions)"
    [ -z "$output" ]
  done < <(doc_lines silent)
  while IFS= read -r line; do
    run_hook claude-ship-gate "$(bash_input "$repo" "$line" auto)"
    [ "$(decision)" = ask ]
    run_hook claude-ship-gate "$(bash_input "$repo" "$line" bypassPermissions)"
    [ "$(decision)" = deny ]
  done < <(doc_lines asks)
}

# The tests feed the gates a tool name; a gate only judges what its matcher routes to it. A
# matcher of plain names is an exact match, any other is a regular expression.
@test "the matchers name every tool the gate tests feed" {
  local name matched
  matched() {
    jq -e --arg tool "$1" --arg gate "$2" \
      '[.hooks.PreToolUse[] | select(.matcher as $m | if ($m | test("^[A-Za-z_|]+$")) then ($tool | test("^(" + $m + ")$")) else ($tool | test($m)) end) | .hooks[].args[0]] | index($gate) != null' \
      "$PLUGIN/hooks/hooks.json" >/dev/null
  }
  while IFS= read -r name; do
    matched "$name" claude-ship-gate
  done < <({ grep -ohE 'tool_name: "[^"]+"' "$PLUGIN/tests/claude-ship-gate.bats" "$PLUGIN/tests/claude-helpers.bash" | cut -d'"' -f2 | grep -v '^Agent$'; echo Bash; echo Monitor; } | sort -u)
  matched Bash claude-install-gate
  matched Monitor claude-install-gate
  matched Agent claude-agent-gate
  matched Task claude-agent-gate
  ! matched Read claude-ship-gate
}
