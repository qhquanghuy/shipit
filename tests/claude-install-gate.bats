#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load claude-helpers

setup() {
  REPO=$(shipit_repo)
  cd "$REPO" || return 1
}

# gate <command> [folder]: the install gate on a Bash call in the repo.
gate() {
  run_hook claude-install-gate "$(bash_input "$REPO" "$1" bypassPermissions)" "${2:-}"
}

@test "a global install is denied with the recipe" {
  local command
  for command in "pip install requests" "sudo npm install -g left-pad" "cargo install ripgrep" "cd d && pacman -Syu x"; do
    gate "$command"
    [ "$(decision)" = deny ]
    [[ $(reason) == "ShipIt: never install globally on this machine ("* ]]
    [[ $(reason) == *"Use what the project provides (for example its nix develop shell)."* ]]
  done
  gate "pip install requests"
  [[ $(reason) == *'("pip install")'* ]]
}

@test "ordinary commands and text about installing pass" {
  gate "cargo build && nix develop -c cargo test"
  [ -z "$output" ]
  gate "npm install"
  [ -z "$output" ]
  gate $'git commit -m "$(cat <<\'EOF\'\ndocument pip install\nEOF\n)"'
  [ -z "$output" ]
  gate $'cat > notes.md <<\'EOF\'\npip install x\nEOF'
  [ -z "$output" ]
  gate $'bash <<\'EOF\'\npip install x\nEOF'
  [ "$(decision)" = deny ]
}

@test "the machine notes replace the list and the recipe" {
  mkdir -p "$HOME/.config/shipit"
  printf 'Missing tool: use the gadget shell.\nNever: `frob bar`\n' >"$HOME/.config/shipit/machine.md"
  gate "frob bar x"
  [ "$(decision)" = deny ]
  [[ $(reason) == 'ShipIt: never install globally on this machine ("frob bar"). use the gadget shell.' ]]
  gate "pip install x"
  [ -z "$output" ]
}

@test "bad stdin in a ShipIt project denies" {
  run --separate-stderr bash -c 'cd "$1" && CLAUDE_PROJECT_DIR="$2" exec "$3" claude-install-gate <<<"not json"' \
    _ "$BATS_TEST_TMPDIR" "$REPO" "$SHIPIT_BIN"
  [ "$status" -eq 0 ]
  [ "$(decision)" = deny ]
  [[ $(reason) == "ShipIt cannot read the hook input ("*")." ]]
  run --separate-stderr bash -c 'cd "$1" && unset CLAUDE_PROJECT_DIR && exec "$2" claude-install-gate <<<"{}"' \
    _ "$REPO" "$SHIPIT_BIN"
  [ "$status" -eq 0 ]
  [ "$(decision)" = deny ]
}

@test "bad stdin outside a ShipIt project allows" {
  run_hook claude-install-gate "not json"
  [ -z "$output" ]
  run_hook claude-install-gate "{}"
  [ -z "$output" ]
}

@test "outside a ShipIt project every command allows" {
  local plain="$BATS_TEST_TMPDIR/plain"
  git init -q "$plain"
  run_hook claude-install-gate "$(bash_input "$plain" "pip install requests" bypassPermissions)" "$plain"
  [ -z "$output" ]
}

@test "a call with no command in a ShipIt project is a bad input" {
  run_hook claude-install-gate "$(hook_input "$(jq -cn --arg cwd "$REPO" '{cwd: $cwd, tool_name: "Bash"}')")"
  [ "$(decision)" = deny ]
  [[ $(reason) == *"has no tool_input.command"* ]]
}
