#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

ROADMAP="$SHIPIT_SRC/tests/fixtures/roadmap.md"
GOAL="$SHIPIT_SRC/tests/fixtures/goal.md"
COVERAGE="$SHIPIT_SRC/tests/fixtures/coverage.toml"

# What `shipit format roadmap` prints for the fixtures; `docs/alpha/` is printed as `docs/alpha`.
PRINTED='{"roadmap":{"goal":"demo","elements":[{"kind":"slice","name":"alpha","after":[],"meets":["demo:DW1"],"owns":["src/alpha.rs","docs/alpha"],"test_drive":false},{"kind":"slice","name":"beta","after":["alpha"],"meets":["demo:DW2"],"owns":["src/beta.rs"],"test_drive":true},{"kind":"slice","name":"gamma","after":["alpha"],"meets":["demo:DW3"],"owns":["src/gamma.rs"],"test_drive":false},{"kind":"proof","name":"proof","after":["beta","gamma"],"meets":[],"how":"run by the owner"}]},"waves":[["alpha"],["beta","gamma"],["proof"]],"goal":["demo:DW1","demo:DW2","demo:DW3"]}'

# The formats need no repository and no setup: run in an empty folder.
setup() {
  cd "$BATS_TEST_TMPDIR"
}

@test "format roadmap: prints the parsed roadmap and its waves" {
  run --separate-stderr "$SHIPIT_BIN" format roadmap "$ROADMAP" "$GOAL"
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
  [ "$output" = "$PRINTED" ]
}

@test "format roadmap: without a goal file the goal prints as null" {
  run --separate-stderr "$SHIPIT_BIN" format roadmap "$ROADMAP"
  [ "$status" -eq 0 ]
  [[ $output == *'"waves":[["alpha"],["beta","gamma"],["proof"]],"goal":null}' ]]
}

@test "format roadmap: an invalid roadmap exits 2 with every problem" {
  sed -e 's/^name = "beta"/name = "alpha"/' -e 's/^owns = \["src\/gamma.rs"\]/owns = ["\/gamma.rs"]/' \
    "$ROADMAP" >bad.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap bad.md
  [ "$status" -eq 2 ]
  [ -z "$output" ]
  [ "$stderr" = 'shipit: bad.md is invalid:
.slice[1].name: "alpha" is used twice
.slice[1].after[0]: "alpha" cannot come after itself
.slice[2].owns[0]: must be a path inside the repository, written like src/a.rs or docs/' ]
}

@test "format roadmap: an item nobody meets exits 2 with \"DW3: no slice\"" {
  sed 's/^meets = \["DW3"\]/meets = []/' "$ROADMAP" >unmet.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap unmet.md "$GOAL"
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: unmet.md is invalid:
DW3: no slice' ]
}

@test "format roadmap: a file that cannot be read with certainty exits 2" {
  printf '# Roadmap\n' >none.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap none.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: none.md has no toml block under "## Slices"' ]

  printf '## Slices\n```toml\nslice = [\n```\n' >broken.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap broken.md
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: broken.md is not valid TOML: "* ]]

  printf '## Slices\n```toml\ngoal = "x"\n' >unclosed.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap unclosed.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: unclosed.md: a fence opened at line 2 is never closed' ]

  printf '## Slices\n```toml\n```\n```toml\n```\n' >twice.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap twice.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: twice.md: more than one toml block under "## Slices"' ]

  printf '<!--\n## Slices\n```toml\ngoal = "x"\n```\n-->\n' >comment.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap comment.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: comment.md: raw HTML opens at line 1, which hides text from the reader' ]

  run --separate-stderr "$SHIPIT_BIN" format roadmap missing.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no file at missing.md' ]
}

@test "format roadmap: a goal that cannot be read with certainty exits 2" {
  printf '## Done-when\n\n1. a\n```\n2. b\n' >open-goal.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap "$ROADMAP" open-goal.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: goal: a fence opened at line 4 is never closed' ]

  printf '## Done-when\n' >empty-goal.md
  run --separate-stderr "$SHIPIT_BIN" format roadmap "$ROADMAP" empty-goal.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: goal: no numbered items under "## Done-when"' ]
}

@test "format roadmap: a chain of four thousand slices is read (a timeout of 20 seconds only stops a hang)" {
  {
    printf 'goal = "long"\n'
    for ((i = 3999; i >= 0; i--)); do
      printf '[[slice]]\nname = "s%d"\nafter = [%s]\nmeets = []\nowns = ["f/s%d.rs"]\ntest_drive = false\n' \
        "$i" "$([[ $i -eq 0 ]] || printf '"s%d"' $((i - 1)))" "$i"
    done
  } >chain.toml
  { printf '## Slices\n```toml\n'; cat chain.toml; printf '```\n'; } >chain.md
  run --separate-stderr timeout 20 "$SHIPIT_BIN" format roadmap chain.md
  [ "$status" -eq 0 ]
}

@test "format: an unknown form exits 2 with the usage" {
  run --separate-stderr "$SHIPIT_BIN" format
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: usage: shipit format <roadmap|map|findings|messages> …" ]
}

@test "format map: prints the items of a file and of a build-plan block" {
  want='{"DW2":["a single test"],"DW10":["status: a slice walks from planned to shipped","the_state_is_the_longest_chain_that_holds"]}'
  run --separate-stderr "$SHIPIT_BIN" format map "$COVERAGE"
  [ "$status" -eq 0 ]
  [ "$output" = "$want" ]

  {
    printf '# Build plan\n\n## Done-when coverage\n\n```toml\n'
    cat "$COVERAGE"
    printf '```\n'
  } >plan.MD
  run --separate-stderr "$SHIPIT_BIN" format map plan.MD
  [ "$status" -eq 0 ]
  [ "$output" = "$want" ]

  printf '## Done-when coverage\n\n```toml\n```\n' >none.md
  run --separate-stderr "$SHIPIT_BIN" format map none.md
  [ "$status" -eq 0 ]
  [ "$output" = '{}' ]
}

@test "format map: a bad map exits 2 and a block of another section is not borrowed" {
  printf 'DW4 = ["dw4 works"]\n' >bad.toml
  run --separate-stderr "$SHIPIT_BIN" format map bad.toml
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: bad.toml is invalid:
.DW4[0]: a test name must not carry a done-when id' ]

  printf '## Done-when coverage\n\n## Other\n\n```toml\nDW9 = ["x"]\n```\n' >plan.md
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

@test "format findings: every refused finding and result is named, one line each" {
  run --separate-stderr "$SHIPIT_BIN" format findings review approve "oops" $'issue (non-blocking): x\nissue (blocking): y'
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: finding "oops": must start with issue, suggestion, question or nitpick, then ": "
finding "issue (non-blocking): x\nissue (blocking): y": a finding is one line, without control characters' ]

  run --separate-stderr "$SHIPIT_BIN" format findings test-drive approve
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: approve is a review result" ]

  run --separate-stderr "$SHIPIT_BIN" format findings review maybe
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: "maybe" is not a result: approve, approve-after-fixes, rework, pass or findings' ]
}

# documented_messages <doc>: each code span in the "Message" column of the doc's tables.
documented_messages() {
  awk '
    /^\|/ {
      line = $0
      gsub(/\\\|/, "\001", line)
      n = split(line, cell, "|")
      if (!col) {
        for (i = 2; i < n; i++) {
          name = cell[i]
          gsub(/^ +| +$/, "", name)
          if (name == "Message") col = i
        }
        next
      }
      text = cell[col]
      gsub(/\001/, "|", text)
      m = split(text, part, "`")
      for (i = 2; i <= m; i += 2) print part[i]
      next
    }
    { col = 0 }
  ' "$1"
}

@test "docs/formats.md names every message the code prints, and no other" {
  # Printed by pure::rules, which the setup shares.
  shared=$'unknown key\nmissing\nmust be an object\n<file> is invalid:'
  in_code=$("$SHIPIT_BIN" format messages)
  [ -n "$in_code" ]
  run diff <(documented_messages "$SHIPIT_SRC/docs/formats.md" | LC_ALL=C sort -u) \
    <(printf '%s\n%s\n' "$in_code" "$shared" | LC_ALL=C sort -u)
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
}

@test "a message that the doc names and the code does not print is found" {
  sed 's/`a nitpick is never blocking`/`a nitpick is not blocking`/' "$SHIPIT_SRC/docs/formats.md" >doc.md
  [ "$(documented_messages doc.md | grep -c 'a nitpick is not blocking')" -eq 1 ]
  ! "$SHIPIT_BIN" format messages | grep -qxF 'a nitpick is not blocking'
}
