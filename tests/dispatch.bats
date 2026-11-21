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

@test "command names with a slash or .. exit 2" {
  local name
  for name in ../bin/shipit "a/b" ".." "../x" "/etc/passwd" "UPPER" "1abc" "-x" ""; do
    run_shipit "$name"
    [ "$status" -eq 2 ] || { echo "name '$name' gave status $status"; return 1; }
    [[ $output == *"shipit help"* ]]
  done
}
