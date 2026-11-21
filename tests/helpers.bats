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
  [ "$(git -C "$repo" ls-tree -r --name-only HEAD)" = ".config/shipit.json" ]
  [ -z "$(git -C "$repo" status --porcelain --untracked-files=normal)" ]
  jq -e '.version == 1 and .base == "main" and (.tiers | length) == 4' "$repo/.config/shipit.json"
}

@test "write_setup writes the setup file at the top and commits it" {
  local repo
  repo=$(make_repo)
  mkdir -p "$repo/sub"
  cd "$repo/sub"
  write_setup '{"version": 1}'
  [ "$(jq -c . "$repo/.config/shipit.json")" = '{"version":1}' ]
  [ "$(git -C "$repo" rev-list --count HEAD)" -eq 2 ]
  [ -z "$(git -C "$repo" status --porcelain --untracked-files=normal)" ]
}

@test "run_shipit runs this working copy's bin/shipit" {
  run_shipit --version
  [ "$status" -eq 0 ]
  [ "$output" = "shipit $(<"$SHIPIT_SRC/VERSION")" ]
}
