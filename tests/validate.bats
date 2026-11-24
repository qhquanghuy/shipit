#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

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

@test "validate: a review prints its result and the findings by class" {
  repo=$(make_repo)
  cd "$repo"
  {
    printf '## Form\n\n```toml\nkind = "second-opinion"\noutcome = "approve-after-fixes"\ninput = "the change"\n```\n\n'
    printf '## Summary\nA small change.\n\n## Security\n- none found: no input is read\n\n## Performance\n- not relevant: no loops\n\n'
    printf '## Findings\n- issue (blocking): a name is wrong\n- nitpick: a comma\n- question: why a loop?\n\n'
    printf '## Verified\n- `cargo test`: passed\n\n## Inferred\n- none\n\n## Unverifiable\n- none\n'
  } >review.md
  run --separate-stderr "$SHIPIT_BIN" validate second-opinion review.md
  [ "$status" -eq 0 ]
  [ "$output" = 'valid second-opinion: approve-after-fixes (1 must fix, 1 nice to fix, 1 must answer)' ]
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
  [ "${stderr##*$'\n'}" = '  too long: 15 lines (owner.max_lines is 5)' ]
}

@test "validate: example lines pass under the default cap and fail over owner.example_max_lines" {
  repo=$(make_repo)
  cd "$repo"
  write_message example.md '```example' 'one' 'two' 'three' '```'
  run --separate-stderr "$SHIPIT_BIN" validate decision-point example.md
  [ "$status" -eq 0 ]
  write_setup "$(fixture_with owner.max_lines=15 owner.status_update_max_lines=5 | sed 's/^\[owner\]$/&\nexample_max_lines = 2/')"
  run --separate-stderr "$SHIPIT_BIN" validate decision-point example.md
  [ "$status" -eq 1 ]
  [ "${stderr##*$'\n'}" = '  too many example lines: 5 (owner.example_max_lines is 2)' ]
}

@test "validate: a missing file, an unknown kind and a missing argument exit 2" {
  repo=$(make_repo)
  cd "$repo"
  run --separate-stderr "$SHIPIT_BIN" validate decision-point nowhere.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no file at nowhere.md' ]
  write_message out.md
  run --separate-stderr "$SHIPIT_BIN" validate nonsense out.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: unknown kind "nonsense"; kinds: build-plan, decision-point, explanation, informed-review, known-issue, model-advice, plan-review, review, roadmap, scout-report, second-opinion, test-drive' ]
  run --separate-stderr "$SHIPIT_BIN" validate decision-point
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: usage: shipit validate <kind> <file> [--goal <name> | --goal-file <path>]' ]
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

# write_roadmap <file>: the formats' roadmap fixture, wrapped as this kind's output.
write_roadmap() {
  {
    printf '## Form\n\n```toml\nkind = "roadmap"\noutcome = "ready-for-review"\ninput = "a goal"\n```\n\n'
    cat "$SHIPIT_SRC/tests/fixtures/roadmap.md"
    printf '\n## Goal\nThe demo goal.\n\n## Shared contracts\n- none\n\n## Owner decisions\n- none\n\n## Risks\n- none\n'
  } >"$1"
}

# bigger_goal: the goal fixture with a fourth done-when item that no slice meets.
bigger_goal() {
  sed 's/^3\. The third item\./3. The third item.\n4. The fourth item./' "$SHIPIT_SRC/tests/fixtures/goal.md"
}

@test "validate: a roadmap needs its goal, and only a roadmap takes one" {
  repo=$(make_repo)
  cd "$repo"
  write_roadmap road.md
  write_message out.md
  run --separate-stderr "$SHIPIT_BIN" validate roadmap road.md
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: the roadmap needs its goal: shipit validate roadmap <file> --goal <name> or --goal-file <path>' ]
  run --separate-stderr "$SHIPIT_BIN" validate decision-point out.md --goal demo
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: --goal and --goal-file are only for the roadmap, and only one of them' ]
  run --separate-stderr "$SHIPIT_BIN" validate roadmap road.md --goal demo --goal-file "$SHIPIT_SRC/tests/fixtures/goal.md"
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: --goal and --goal-file are only for the roadmap, and only one of them" ]
}

@test "validate: a roadmap is held against its goal file, read from the caller's folder" {
  repo=$(make_repo)
  mkdir -p "$repo/sub"
  cd "$repo/sub"
  write_roadmap ../road.md
  cp "$SHIPIT_SRC/tests/fixtures/goal.md" ../goal.md
  run --separate-stderr "$SHIPIT_BIN" validate roadmap ../road.md --goal-file ../goal.md
  [ "$status" -eq 0 ]
  [ "$output" = 'valid roadmap: ready-for-review' ]
  bigger_goal >../bigger.md
  run --separate-stderr "$SHIPIT_BIN" validate roadmap ../road.md --goal-file ../bigger.md
  [ "$status" -eq 1 ]
  [ "$stderr" = 'shipit: ../road.md is not a valid roadmap:
  DW4: no slice' ]
}

@test "validate: --goal reads the newest version of the goal in the store" {
  repo=$(make_repo)
  cd "$repo"
  write_roadmap road.md
  store=$(git rev-parse --path-format=absolute --git-common-dir)/shipit/goals/demo
  mkdir -p "$store"
  bigger_goal >"$store/goal.v9.md"
  cp "$SHIPIT_SRC/tests/fixtures/goal.md" "$store/goal.v10.md"
  run --separate-stderr "$SHIPIT_BIN" validate roadmap road.md --goal demo
  [ "$status" -eq 0 ]
  run --separate-stderr "$SHIPIT_BIN" validate roadmap road.md --goal nonesuch
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no goal "nonesuch" in the store' ]
}

# add_slice <file> <name> <path>: a slice after alpha, owning <path>, before the proof step.
add_slice() {
  sed -i "/^\[\[proof\]\]\$/i [[slice]]\nname = \"$2\"\nafter = [\"alpha\"]\nmeets = []\nowns = [\"$3\"]\ntest_drive = false\n" "$1"
}

# delta_repo: a stored roadmap v1 whose alpha has shipped and whose beta has started.
delta_repo() {
  make_slice_repo
  cp "$SHIPIT_SRC/tests/fixtures/goal.md" "$(store)/goals/demo/goal.v1.md"
  ship_entry alpha
  logged_start beta
}

@test "validate: a roadmap that changes a started or shipped slice's entry is refused by validate and store put, naming each" {
  delta_repo
  write_roadmap a.md
  sed -i -e '/^name = "alpha"$/a split_into = ["gamma"]' -e 's/"beta"/"beta-two"/g' -e 's|src/gamma.rs|src/gamma/|' a.md
  run --separate-stderr "$SHIPIT_BIN" validate roadmap a.md --goal demo
  [ "$status" -eq 1 ]
  [ -z "$output" ]
  [ "$stderr" = 'shipit: a.md is not a valid roadmap:
  slice alpha has shipped: its entry cannot change
  slice beta has started: its entry cannot be removed' ]
  run --separate-stderr "$SHIPIT_BIN" store put roadmap a.md --goal demo
  [ "$status" -eq 1 ]
  [ "$stderr" = 'shipit: a.md is not a valid roadmap:
  slice alpha has shipped: its entry cannot change
  slice beta has started: its entry cannot be removed' ]
  [ ! -e "$(store)/goals/demo/roadmap.v2.md" ]
  write_roadmap b.md
  sed -i -e '/^name = "beta"$/a split_into = ["beta-a"]' -e 's|"src/beta.rs"\]|"src/beta.rs", "src/beta2.rs"]|' b.md
  add_slice b.md beta-a src/beta.rs
  run --separate-stderr "$SHIPIT_BIN" validate roadmap b.md --goal demo
  [ "$status" -eq 1 ]
  [ "$stderr" = 'shipit: b.md is not a valid roadmap:
  slice beta has started: its entry may only gain split_into' ]
}

@test "validate: a started slice may gain split_into naming its parts, and the version is stored" {
  delta_repo
  write_roadmap c.md
  sed -i '/^name = "beta"$/a split_into = ["beta-a", "beta-b"]' c.md
  add_slice c.md beta-a src/beta.rs
  add_slice c.md beta-b src/beta/
  run --separate-stderr "$SHIPIT_BIN" validate roadmap c.md --goal demo
  [ "$status" -eq 0 ]
  [ "$output" = 'valid roadmap: ready-for-review' ]
  run --separate-stderr "$SHIPIT_BIN" store put roadmap c.md --goal demo
  [ "$status" -eq 0 ]
  [ "$output" = "stored $(store)/goals/demo/roadmap.v2.md" ]
}

@test "validate: a goal with no stored roadmap compares no entry, even with a shipped slice" {
  repo=$(make_repo)
  cd "$repo"
  mkdir -p "$(store)/goals/demo"
  cp "$SHIPIT_SRC/tests/fixtures/goal.md" "$(store)/goals/demo/goal.v1.md"
  ship_entry alpha
  write_roadmap road.md
  sed -i 's|, "docs/alpha/"||' road.md
  run --separate-stderr "$SHIPIT_BIN" validate roadmap road.md --goal demo
  [ "$status" -eq 0 ]
  [ "$output" = 'valid roadmap: ready-for-review' ]
}

@test "validate: entries that cannot be told apart stop a stored roadmap with exit 2 and refuse a draft with exit 1" {
  make_slice_repo
  cp "$SHIPIT_SRC/tests/fixtures/goal.md" "$(store)/goals/demo/goal.v1.md"
  sed -i 's/^\[\[slice\]\]$/[["slice"]]/' "$(store)/goals/demo/roadmap.v1.md"
  write_roadmap road.md
  run --separate-stderr "$SHIPIT_BIN" validate roadmap road.md --goal demo
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: $(store)/goals/demo/roadmap.v1.md: cannot tell the entries of the Slices block apart; write each header as a plain [[slice]] or [[proof]] line" ]
  cp "$SHIPIT_SRC/tests/fixtures/roadmap.md" "$(store)/goals/demo/roadmap.v1.md"
  sed -i 's/^\[\[slice\]\]$/[["slice"]]/' road.md
  run --separate-stderr "$SHIPIT_BIN" validate roadmap road.md --goal demo
  [ "$status" -eq 1 ]
  [ "$stderr" = 'shipit: road.md is not a valid roadmap:
  road.md: cannot tell the entries of the Slices block apart; write each header as a plain [[slice]] or [[proof]] line' ]
}
