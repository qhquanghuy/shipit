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

@test "template: every review and report form ends with the three claim lists, in order" {
  for kind in review informed-review second-opinion plan-review explanation scout-report test-drive; do
    run --separate-stderr "$SHIPIT_BIN" template "$kind"
    [ "$status" -eq 0 ]
    [ "${lines[0]}" = '## Form' ]
    [[ $output == *"kind = \"$kind\""* ]]
    [ "$(grep '^## ' <<<"$output" | tail -3)" = $'## Verified\n## Inferred\n## Unverifiable' ]
  done
  run --separate-stderr "$SHIPIT_BIN" template review
  [[ $output == *$'\n## Reading path\n'*$'\n## Other files\n'*$'\n## Look here\n'* ]]
}

@test "template: without a kind it lists the kinds" {
  run --separate-stderr "$SHIPIT_BIN" template
  [ "$status" -eq 0 ]
  [ "$output" = 'kinds: build-plan, decision-point, explanation, informed-review, known-issue, model-advice, plan-review, review, roadmap, scout-report, second-opinion, test-drive' ]
}

@test "template: an unknown kind and extra arguments exit 2" {
  run --separate-stderr "$SHIPIT_BIN" template nonsense
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: unknown kind "nonsense"; kinds: build-plan, decision-point, explanation, informed-review, known-issue, model-advice, plan-review, review, roadmap, scout-report, second-opinion, test-drive' ]
  run --separate-stderr "$SHIPIT_BIN" template decision-point more
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: usage: shipit template [kind]' ]
}
