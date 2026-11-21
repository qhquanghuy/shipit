#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# write_message <file> <extra line>...: a valid decision point, then the extra lines.
write_message() {
  local file=$1
  shift
  {
    printf '## Form\n\n```toml\nkind = "decision-point"\noutcome = "waiting-for-the-owner"\ninput = "a choice"\n```\n\n'
    printf '[shipit · demo · plan]\n\n'
    printf '**What:** pick one.\n**Context:** a short reason.\n**Recommendation:** the first.\n'
    printf '%s\n' "$@"
    printf '**Next:** the lead goes on.\n'
  } >"$file"
}

@test "validate: a message is read from the caller's folder, in a subfolder of the repo" {
  repo=$(make_repo)
  mkdir -p "$repo/sub"
  write_message "$repo/out.md"
  cd "$repo/sub"
  run --separate-stderr "$SHIPIT_BIN" validate decision-point ../out.md
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
  [ "$output" = "valid decision-point: waiting-for-the-owner" ]
}

@test "validate: an invalid output exits 1 and lists each problem" {
  repo=$(make_repo)
  cd "$repo"
  printf '## Form\n\n```toml\nkind = "decision-point"\noutcome = "waiting-for-the-owner"\ninput = "x"\n```\n\n[shipit · demo]\n\n**What:** a question\n**Next:** later\n' >bad.md
  run --separate-stderr "$SHIPIT_BIN" validate decision-point bad.md
  [ "$status" -eq 1 ]
  [ -z "$output" ]
  [ "$stderr" = 'shipit: bad.md is not a valid decision-point:
  label: the line after the Form section must be [project · slice · phase]
  part "Context" is missing
  part "Recommendation" is missing' ]
}

@test "validate: a message over owner.max_lines fails with the exact line" {
  repo=$(make_repo)
  cd "$repo"
  write_setup "$(fixture_with owner.max_lines=5 owner.status_update_max_lines=5)"
  write_message long.md
  run --separate-stderr "$SHIPIT_BIN" validate decision-point long.md
  [ "$status" -eq 1 ]
  [ "${stderr##*$'\n'}" = '  too long: 8 lines (owner.max_lines is 5)' ]
}

@test "validate: a missing file, an unknown kind and a missing argument exit 2" {
  repo=$(make_repo)
  cd "$repo"
  run --separate-stderr "$SHIPIT_BIN" validate decision-point nowhere.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no file at nowhere.md' ]
  write_message out.md
  run --separate-stderr "$SHIPIT_BIN" validate review out.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: unknown kind "review"; kinds: decision-point' ]
  run --separate-stderr "$SHIPIT_BIN" validate decision-point
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: usage: shipit validate <kind> <file>' ]
}

@test "validate: a missing setup stops it" {
  repo=$(make_repo)
  cd "$repo"
  write_message out.md
  git rm -q .config/shipit.toml
  git commit -q -m "drop setup"
  run --separate-stderr "$SHIPIT_BIN" validate decision-point out.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no setup file at .config/shipit.toml (see docs/setup.md)' ]
}
