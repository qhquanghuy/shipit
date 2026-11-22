#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
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

@test "help and no arguments list check and verify with their summaries, not the hidden setup" {
  run_shipit help
  [ "$status" -eq 0 ]
  local helped=$output
  run_shipit
  [ "$status" -eq 0 ]
  [ "$output" = "$helped" ]
  grep -qE '^  check \[--base <name>\]  +Refuses a dirty tree' <<<"$output"
  grep -qE '^  verify \[commit\]  +Says whether a valid check receipt exists' <<<"$output"
  run ! grep -q 'usage: shipit setup' <<<"$helped"
  run ! grep -qE '^  setup' <<<"$helped"
}

@test "help <command> gives the usage, the setup need and the exit codes, for hidden commands too" {
  run_shipit help check
  [ "$status" -eq 0 ]
  [[ $output == "usage: shipit check [--base <name>]"* ]]
  [[ $output == *$'needs a setup file: yes\nexit codes:\n  0  receipt written'* ]]
  [[ $output == *$'\n  3  uncommitted changes'* ]]
  run_shipit --help verify
  [ "$status" -eq 0 ]
  [[ $output == *"needs a setup file: no"* ]]
  [[ $output != *"  3  "* ]]
  run_shipit help setup
  [ "$status" -eq 0 ]
  [[ $output == "usage: shipit setup [ref]"* ]]
}

# The commands are the files in src/commands, as build.rs finds them, so hidden ones are run too.
@test "a command stops with exit 2 and no setup file exactly when it says it needs one" {
  local repo file name needs
  repo=$(make_repo)
  cd "$repo"
  git rm -q .config/shipit.toml
  git commit -q -m "drop the setup"
  for file in "$SHIPIT_SRC"/src/commands/*.rs; do
    name=$(basename "$file" .rs)
    name=${name//_/-}
    run_shipit help "$name"
    [ "$status" -eq 0 ]
    needs=no
    [[ $output == *"needs a setup file: yes"* ]] && needs=yes
    run --separate-stderr "$SHIPIT_BIN" "$name"
    if [[ $needs == yes ]]; then
      [ "$status" -eq 2 ]
      [[ $stderr == *"no setup file at .config/shipit.toml"* ]]
    else
      [[ $stderr != *"no setup file"* ]]
    fi
  done
}

@test "help of an unknown command or with too many arguments exits 2" {
  run_shipit help nosuchcommand
  [ "$status" -eq 2 ]
  [[ $output == *"unknown command 'nosuchcommand'"* ]]
  run_shipit help check verify
  [ "$status" -eq 2 ]
  [[ $output == *"usage: shipit help [command]"* ]]
}
