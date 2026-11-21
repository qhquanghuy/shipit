#!/usr/bin/env bats

load helpers

@test "--version prints the version from Cargo.toml" {
  run_shipit --version
  [ "$status" -eq 0 ]
  [ "$output" = "shipit 0.1.0" ]
}

@test "an unknown command exits 2 and names shipit help" {
  run_shipit nosuchcommand
  [ "$status" -eq 2 ]
  [[ $output == shipit:* ]]
  [[ $output == *"shipit help"* ]]
}

@test "help and no arguments list check and verify, not the hidden setup" {
  run_shipit help
  [ "$status" -eq 0 ]
  local helped=$output
  run_shipit
  [ "$status" -eq 0 ]
  [ "$output" = "$helped" ]
  grep -qx '  check' <<<"$output"
  grep -qx '  verify' <<<"$output"
  ! grep -qx '  setup' <<<"$output"
}
