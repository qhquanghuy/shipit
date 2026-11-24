#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

@test "summary: lists every receipt, known issue, done-when item and metric" {
  suite_repo
  plan_ready beta
  git checkout -q -b slice/demo/beta
  commit_map demo beta 'DW2 = ["one", "receipt: first"]'
  check_here
  "$SHIPIT_BIN" metric clone-ratio 2.1
  rec second-opinion beta --author sonnet:high --by opus:high --result approve-after-fixes \
    --finding "issue (blocking): a problem" --finding "nitpick: a detail"
  rec informed-review beta --author sonnet:high --by opus:high --result approve \
    --finding "question: why this?"
  lg beta known-issue --kind second-opinion --finding F1 --reason later --by sonnet:high
  lg beta answer --kind informed-review --finding F1 --text because --by sonnet:high
  run --separate-stderr "$SHIPIT_BIN" status --summary beta
  [ "$status" -eq 0 ]
  local key
  key=$(git diff --binary --full-index main HEAD | git patch-id --stable | cut -c1-12)
  [ "$(sed -n 1p <<<"$output")" = "### ShipIt: beta is checked" ]
  [[ $output == *"Done-when (.shipit/coverage/demo/beta.toml): demo:DW2 ✓ local (2 tests)"* ]]
  [[ $output == *"| check | $(git rev-parse 'HEAD^{tree}' | cut -c1-12) (current) | shipit | | | pass, size 1 files (cap 60), base main |"* ]]
  [[ $output == *"| second-opinion | $key (current) | opus high | sonnet high | tier | approve-after-fixes (must fix 1, nice to fix 1) |"* ]]
  [[ $output == *"| informed-review | $key (current) | opus high | sonnet high | tier | approve (must answer 1) |"* ]]
  [[ $output == *"Open findings: must fix 0, must answer 0, nice to fix 1"* ]]
  [[ $output == *"Stand-ins used: none"* ]]
  [[ $output == *"Metrics (outliers from local history): clone-ratio 2.1"* ]]
  [[ $output == *$'Known issues:\n- second-opinion F1 (must fix), logged by sonnet:high: a problem. Reason: later'* ]]
  [[ $output == *$'Known issues (not fixed, nice to fix):\n- second-opinion F2: a detail'* ]]
  [[ $output == *$'Answers:\n- informed-review F1, answered by sonnet:high: because'* ]]
}

@test "summary: an unknown slice exits 2" {
  make_slice_repo
  run --separate-stderr "$SHIPIT_BIN" status --summary nowhere
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no slice named "nowhere" in the roadmap of goal "demo"' ]
  run --separate-stderr "$SHIPIT_BIN" status --summary proof
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: "proof" is a proof step: it has no work to summarise' ]
  run --separate-stderr "$SHIPIT_BIN" status --summary beta --json
  [ "$status" -eq 2 ]
}

@test "summary: a slice with no branch and no map still has a summary" {
  make_slice_repo
  run --separate-stderr "$SHIPIT_BIN" status --summary alpha
  [ "$status" -eq 0 ]
  [ "$(sed -n 1p <<<"$output")" = "### ShipIt: alpha is planned" ]
  [[ $output == *"Done-when (.shipit/coverage/demo/alpha.toml): demo:DW1 ✗ not confirmed for this version"* ]]
  [[ $output == *"Known issues: none"* ]]
}
