#!/usr/bin/env bats

load helpers

@test "HOME points at a temp folder" {
  [[ $HOME == "$BATS_TEST_TMPDIR"/* ]]
  [ -d "$HOME" ]
}

@test "make_repo makes a repo on main with identity and a committed valid setup file" {
  local repo
  repo=$(make_repo)
  [ "$(git -C "$repo" rev-parse --abbrev-ref HEAD)" = main ]
  [ "$(git -C "$repo" config user.name)" != "" ]
  [ "$(git -C "$repo" config user.email)" != "" ]
  [ "$(git -C "$repo" rev-list --count HEAD)" -eq 1 ]
  [ "$(git -C "$repo" ls-tree -r --name-only HEAD)" = ".config/shipit.toml" ]
  [ -z "$(git -C "$repo" status --porcelain --untracked-files=normal)" ]
  diff <(valid_setup) "$repo/.config/shipit.toml"
}

@test "write_setup writes the setup file at the top and commits it" {
  local repo
  repo=$(make_repo)
  mkdir -p "$repo/sub"
  cd "$repo/sub"
  write_setup "version = 1"
  [ "$(cat "$repo/.config/shipit.toml")" = "version = 1" ]
  [ "$(git -C "$repo" rev-list --count HEAD)" -eq 2 ]
  [ -z "$(git -C "$repo" status --porcelain --untracked-files=normal)" ]
}

@test "fixture_with replaces a top-level key and a key in a table, and nothing else" {
  run fixture_with 'gate.max_files=100' 'gate.environment.prefix=["env"]' 'agents.roles.reader.tier=2'
  [ "$status" -eq 0 ]
  diff <(printf '%s\n' "$output") <(sed \
    -e 's/^max_files = 60$/max_files = 100/' \
    -e 's/^prefix = \[\]$/prefix = ["env"]/' \
    -e '/^\[agents.roles.reader\]$/{n;s/.*/tier = 2/;}' "$FIXTURE")
}

@test "fixture_with fails on a path the fixture has no line for" {
  run fixture_with 'nonesuch=1'
  [ "$status" -eq 1 ]
  [[ $output == *"no line for nonesuch=1"* ]]
}
