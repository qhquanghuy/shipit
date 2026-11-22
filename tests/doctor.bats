#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# tools_dir: a folder holding only git, sh and sleep, to stand for PATH.
tools_dir() {
  local dir="$BATS_TEST_TMPDIR/tools" name
  mkdir -p "$dir"
  for name in git sh sleep; do
    ln -sf "$(command -v "$name")" "$dir/$name"
  done
  printf '%s\n' "$dir"
}

# fake_tool <dir> <name> <body>
fake_tool() {
  printf '#!/bin/sh\n%s\n' "$3" >"$1/$2"
  chmod +x "$1/$2"
}

# doctor_with <path> [<args>]: stdout in $output.
doctor_with() {
  local path=$1
  shift
  run env PATH="$path" "$SHIPIT_BIN" doctor "$@"
}

@test "tools: a fake tool's version, a missing one, one that hangs" {
  local dir start
  dir=$(tools_dir)
  fake_tool "$dir" rg 'echo ripgrep 14.1.0'
  fake_tool "$dir" uv 'sleep 10'
  start=$SECONDS
  doctor_with "$dir"
  [ "$status" -eq 0 ]
  [ $((SECONDS - start)) -lt 5 ]
  grep -qxF '  rg ripgrep 14.1.0' <<<"$output"
  grep -qxF '  jq missing' <<<"$output"
  grep -qxF '  uv no answer' <<<"$output"
  grep -q '^  git git version ' <<<"$output"
}

@test "in a repository with a valid setup: setup ok" {
  local repo
  repo=$(make_repo)
  cd "$repo"
  run "$SHIPIT_BIN" doctor
  [ "$status" -eq 0 ]
  grep -qxF 'setup: .config/shipit.toml ok' <<<"$output"
}

@test "in a repository with an invalid setup, doctor reports the first problem and exits 0" {
  local repo
  repo=$(make_repo)
  cd "$repo"
  printf 'version = 1\n' >.config/shipit.toml
  git commit -q -am "break the setup"
  run "$SHIPIT_BIN" doctor
  [ "$status" -eq 0 ]
  grep -q '^setup: .config/shipit.toml invalid: ' <<<"$output"
  [ "$(grep -c '^setup: ' <<<"$output")" -eq 1 ]
}

@test "outside a repository and without a setup, doctor exits 0 and says so" {
  local repo
  cd "$BATS_TEST_TMPDIR"
  run "$SHIPIT_BIN" doctor
  [ "$status" -eq 0 ]
  grep -qxF 'setup: not in a git repository' <<<"$output"
  grep -qxF 'machine notes: no machine notes yet (built-in list and recipe)' <<<"$output"
  repo=$(make_repo)
  cd "$repo"
  git rm -q .config/shipit.toml
  git commit -q -m "no setup"
  run "$SHIPIT_BIN" doctor
  [ "$status" -eq 0 ]
  grep -qxF 'setup: .config/shipit.toml missing' <<<"$output"
}

@test "with machine notes, their recipe and never-list are shown" {
  mkdir -p "$HOME/.config/shipit"
  printf '%s\n' '# notes' 'Missing tool: ask Ada' 'Never: `pip install`, `foo bar -x`' >"$HOME/.config/shipit/machine.md"
  cd "$BATS_TEST_TMPDIR"
  run "$SHIPIT_BIN" doctor
  [ "$status" -eq 0 ]
  grep -qxF 'machine notes: ~/.config/shipit/machine.md' <<<"$output"
  grep -qxF 'missing tool: ask Ada' <<<"$output"
  grep -qxF 'never: pip install, foo bar -x' <<<"$output"
}

@test "the example in docs/machine-notes.md is read as the notes" {
  mkdir -p "$HOME/.config/shipit"
  awk '/^```$/ { n++; next } n == 1' "$SHIPIT_SRC/docs/machine-notes.md" >"$HOME/.config/shipit/machine.md"
  [ -s "$HOME/.config/shipit/machine.md" ]
  cd "$BATS_TEST_TMPDIR"
  run "$SHIPIT_BIN" doctor
  [ "$status" -eq 0 ]
  grep -q '^missing tool: use `nix shell nixpkgs#<tool> --command <command>` for one run' <<<"$output"
  grep -qxF 'never: pip install, npm install -g, cargo install' <<<"$output"
}

@test "a home folder that belongs to someone else: the agent config cannot be created" {
  cd "$BATS_TEST_TMPDIR"
  run env HOME=/ "$SHIPIT_BIN" doctor
  [ "$status" -eq 0 ]
  grep -qxF 'agent config: ~/.claude/CLAUDE.md missing (cannot be created)' <<<"$output"
}

@test "an argument: exit 2 with the usage" {
  run --separate-stderr "$SHIPIT_BIN" doctor now
  [ "$status" -eq 2 ]
  [[ $stderr == *"usage: shipit doctor"* ]]
}
