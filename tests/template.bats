#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# A form needs no repository and no setup.
setup() {
  cd "$BATS_TEST_TMPDIR"
}

@test "template: a form is printed whole, from outside any repository" {
  run --separate-stderr "$SHIPIT_BIN" template decision-point
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
  [ "${lines[0]}" = '## Form' ]
  [[ $output == *'kind = "decision-point"'*'**Next:**'* ]]
}

@test "template: without a kind it lists the kinds" {
  run --separate-stderr "$SHIPIT_BIN" template
  [ "$status" -eq 0 ]
  [ "$output" = 'kinds: decision-point' ]
}

@test "template: an unknown kind and extra arguments exit 2" {
  run --separate-stderr "$SHIPIT_BIN" template review
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: unknown kind "review"; kinds: decision-point' ]
  run --separate-stderr "$SHIPIT_BIN" template decision-point more
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: usage: shipit template [kind]' ]
}
