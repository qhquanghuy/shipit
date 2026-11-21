#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

ROADMAP="$SHIPIT_SRC/tests/fixtures/roadmap.md"
GOAL="$SHIPIT_SRC/tests/fixtures/goal.md"

# What `shipit format roadmap` prints for the fixtures.
PRINTED='{"roadmap":{"goal":"demo","elements":[{"kind":"slice","name":"alpha","after":[],"meets":["demo:DW1"],"owns":["src/alpha.rs","docs/alpha/"],"test_drive":false,"branch":"slice/demo/alpha"},{"kind":"slice","name":"beta","after":["alpha"],"meets":["demo:DW2"],"owns":["src/beta.rs"],"test_drive":true,"branch":"slice/demo/beta"},{"kind":"slice","name":"gamma","after":["alpha"],"meets":["demo:DW3"],"owns":["src/gamma.rs"],"test_drive":false,"branch":"slice/demo/gamma"},{"kind":"proof","name":"proof","after":["beta","gamma"],"meets":[],"how":"run by the owner"}]},"waves":[["alpha"],["beta","gamma"],["proof"]],"goal":["demo:DW1","demo:DW2","demo:DW3"]}'

setup() {
  cd "$(make_repo)"
}

@test "format roadmap: prints the parsed roadmap and its waves" {
  run --separate-stderr "$SHIPIT_BIN" format roadmap "$ROADMAP" --goal "$GOAL"
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
  [ "$output" = "$PRINTED" ]
}

@test "format roadmap: without a goal the goal prints as null" {
  run --separate-stderr "$SHIPIT_BIN" format roadmap "$ROADMAP"
  [ "$status" -eq 0 ]
  [[ $output == *'"waves":[["alpha"],["beta","gamma"],["proof"]],"goal":null}' ]]
}

@test "format roadmap: an invalid roadmap exits 2 with every problem" {
  sed -e 's/^name = "beta"/name = "alpha"/' -e 's/^owns = \["src\/gamma.rs"\]/owns = []/' \
    "$ROADMAP" >bad.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap bad.md
  [ "$status" -eq 2 ]
  [ -z "$output" ]
  [ "$stderr" = 'shipit: bad.md is invalid:
.slice[1].name: "alpha" is used twice
.slice[1].after[0]: "alpha" cannot come after itself
.slice[2].owns: must list at least one path' ]
}

@test "format roadmap: a roadmap without a goal or with a branch outside its goal exits 2" {
  sed '/^goal = /d' "$ROADMAP" >nogoal.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap nogoal.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: nogoal.md is invalid:
.goal: missing' ]

  sed 's#slice/demo/beta#slice/beta#' "$ROADMAP" >branch.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap branch.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: branch.md is invalid:
.slice[1].branch: must be "slice/demo/beta"' ]
}

@test "format roadmap: an item nobody meets exits 2 with \"DW3: no slice\"" {
  sed 's/^meets = \["DW3"\]/meets = []/' "$ROADMAP" >unmet.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap unmet.md --goal "$GOAL"
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: unmet.md is invalid:
DW3: no slice' ]
}

@test "format roadmap: a file without the block, without TOML or missing exits 2" {
  printf '# Roadmap\n' >none.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap none.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: none.md has no toml block under "## Slices"' ]

  printf '## Slices\n```toml\nslice = [\n```\n' >broken.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap broken.md
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: broken.md is not valid TOML: "* ]]

  run --separate-stderr "$SHIPIT_BIN" format roadmap missing.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no file at missing.md' ]
}

@test "format roadmap: a goal without items exits 2" {
  printf '## Done-when\n' >empty-goal.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap "$ROADMAP" --goal empty-goal.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: goal: no numbered items under "## Done-when"' ]
}

@test "format: a missing setup stops it" {
  git rm -q .config/shipit.toml
  git commit -q -m "drop setup"
  run --separate-stderr "$SHIPIT_BIN" format roadmap "$ROADMAP"
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at .config/shipit.toml (see docs/setup.md)" ]
}

@test "format: an unknown form exits 2 with the usage" {
  run --separate-stderr "$SHIPIT_BIN" format
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: usage: shipit format <roadmap|map|findings> …" ]
}

COVERAGE="$SHIPIT_SRC/tests/fixtures/coverage.toml"

@test "format map: prints the items of a file and of a build-plan block" {
  want='{"DW2":["a single test"],"DW10":["status: a slice walks from planned to shipped","the_state_is_the_longest_chain_that_holds"]}'
  run --separate-stderr "$SHIPIT_BIN" format map "$COVERAGE"
  [ "$status" -eq 0 ]
  [ "$output" = "$want" ]

  {
    printf '# Build plan\n\n## Done-when coverage\n\n```toml\n'
    cat "$COVERAGE"
    printf '```\n'
  } >plan.md
  run --separate-stderr "$SHIPIT_BIN" format map plan.md
  [ "$status" -eq 0 ]
  [ "$output" = "$want" ]

  printf '## Done-when coverage\n\n```toml\n```\n' >none.md
  run --separate-stderr "$SHIPIT_BIN" format map none.md
  [ "$status" -eq 0 ]
  [ "$output" = '{}' ]
}

@test "format map: a bad map exits 2 with every problem" {
  printf 'DW4 = ["dw4 works"]\nnope = ["a"]\n' >bad.toml
  run --separate-stderr "$SHIPIT_BIN" format map bad.toml
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: bad.toml is invalid:
.DW4[0]: a test name must not carry a done-when id
.nope: must be a done-when id such as "DW4"' ]

  printf '# Plan\n' >plan.md
  run --separate-stderr "$SHIPIT_BIN" format map plan.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: plan.md has no toml block under "## Done-when coverage"' ]
}

@test "format findings: prints the result and the parsed findings" {
  run --separate-stderr "$SHIPIT_BIN" format findings review approve-after-fixes \
    "issue (blocking, security): a hole" "question: why?"
  [ "$status" -eq 0 ]
  [ "$output" = '{"result":"approve-after-fixes","findings":[{"id":"F1","label":"issue","class":"must fix","perspectives":["security"],"text":"a hole"},{"id":"F2","label":"question","class":"must answer","perspectives":[],"text":"why?"}]}' ]
}

@test "format findings: a rework without a must-fix finding exits 2" {
  run --separate-stderr "$SHIPIT_BIN" format findings review rework "nitpick: a name"
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: rework needs a must-fix finding" ]
}

@test "format findings: every refused finding and result is named" {
  run --separate-stderr "$SHIPIT_BIN" format findings review approve "oops" "issue: x"
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: finding "oops": must start with issue, suggestion, question or nitpick, then ": "
finding "issue: x": issue and suggestion need (blocking) or (non-blocking)' ]

  run --separate-stderr "$SHIPIT_BIN" format findings test-drive approve
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: approve is a review result" ]

  run --separate-stderr "$SHIPIT_BIN" format findings review maybe
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: "maybe" is not a result: approve, approve-after-fixes, rework, pass or findings' ]
}
