#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# Each command of the slice, with arguments that pass its own parsing, so that the setup is the
# first thing it needs.
COMMANDS=(
  "receipt plan-review beta --author opus:high --by fable:high --result approve"
  "log beta note --text a-note"
  "status"
  "start alpha"
  "coverage -- true"
  "metric clone-ratio 1"
  "complete demo"
)

@test "new commands: a missing setup stops each" {
  make_slice_repo
  git rm -q .config/shipit.toml
  git commit -q -m "drop setup"
  local words
  for words in "${COMMANDS[@]}"; do
    # shellcheck disable=SC2086
    run --separate-stderr "$SHIPIT_BIN" $words
    [ "$status" -eq 2 ]
    [ "$stderr" = "shipit: no setup file at .config/shipit.toml (see docs/setup.md)" ]
  done
}
