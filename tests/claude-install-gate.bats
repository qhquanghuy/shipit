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
  for command in "pip install requests" "sudo npm install -g left-pad" "cargo install ripgrep" "cd d && pacman -Syu x" "bash -c \"npm install -g foo\"" "sh -c 'pip install --user x'"; do
    gate "$command"
    [ "$(decision)" = deny ]
    [[ $(reason) == "ShipIt: never install globally on this machine ("* ]]
    [[ $(reason) == *"Use what the project provides (for example its nix develop shell)."* ]]
  done
  gate "pip install requests"
  [[ $(reason) == *'("pip install")'* ]]
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

# A commit from before the setup file: the repository is still the project's.
old_commit() {
  git commit-tree -m old "$(git hash-object -t tree /dev/null)"
}

# denied_in <folder>: the install gate refuses `pip install requests` there; passed_in: it says nothing.
denied_in() {
  run_hook claude-install-gate "$(bash_input "$1" "pip install requests" bypassPermissions)" "$1"
  [ "$(decision)" = deny ]
}

passed_in() {
  run_hook claude-install-gate "$(bash_input "$1" "pip install requests" bypassPermissions)" "$1"
  [ -z "$output" ]
}

@test "a worktree or a checkout from before the setup file is judged as the project" {
  local old wt="$BATS_TEST_TMPDIR/w"
  old=$(old_commit)
  git worktree add -q --detach "$wt" "$old"
  denied_in "$wt"
  git checkout -q --detach "$old"
  [ ! -e .config/shipit.toml ]
  denied_in "$REPO"
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
  passed_in "$plain"
  # On the default branch, a checkout from before it is still the project.
  git -C "$plain" checkout -q --detach
  git -C "$plain" branch -q -f main side
  git -C "$plain" checkout -q --detach "$(git -C "$plain" rev-list --max-parents=0 side)"
  denied_in "$plain"
  # The store folder of the repository makes it one too.
  local store="$BATS_TEST_TMPDIR/store"
  git init -q -b main "$store"
  passed_in "$store"
  mkdir "$store/.git/shipit"
  denied_in "$store"
}

@test "origin's main stands in for the default branch when origin/HEAD is missing and main is gone" {
  git init -q --bare "$BATS_TEST_TMPDIR/remote.git"
  git remote add origin "$BATS_TEST_TMPDIR/remote.git"
  git push -q origin main
  git fetch -q origin
  git symbolic-ref --quiet refs/remotes/origin/HEAD && git remote set-head origin -d
  local old
  old=$(old_commit)
  git checkout -q --detach "$old"
  git branch -q -D main
  denied_in "$REPO"
}
