#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# a_review <slice>: a second opinion on the slice's current version, with an issue, a question
# and a nitpick; the repo is left on the slice's branch.
a_review() {
  make_work "slice/demo/$1" 1
  check_here
  rec second-opinion "$1" --author sonnet:high --by opus:high --result approve-after-fixes \
    --finding "issue (blocking): F1 text" --finding "question: F2 text" --finding "nitpick: F3 text"
  [ "$status" -eq 0 ]
}

@test "log: each event appends one line and keeps earlier lines" {
  make_slice_repo
  a_review beta
  local book
  book="$(store)/goals/demo/logbook.jsonl"
  lg beta note --text "a note"
  [ "$status" -eq 0 ]
  [ "$output" = "logged note for beta" ]
  lg beta known-issue --kind second-opinion --finding F1 --reason "later" --by sonnet:high
  [ "$status" -eq 0 ]
  [ "$output" = "logged known-issue for beta" ]
  lg beta answer --kind second-opinion --finding F2 --text "because" --by sonnet:high
  [ "$status" -eq 0 ]
  ship_entry beta
  ship_entry gamma
  lg proof shipped --note "run by the owner"
  [ "$status" -eq 0 ]
  [ "$output" = "logged shipped for proof" ]
  [ "$(wc -l <"$book")" -eq 7 ]
  [ "$(jq -r '[.slice, .event] | join(" ")' "$book" | tr '\n' ',')" = \
    "beta receipt,beta note,beta known-issue,beta answer,beta shipped,gamma shipped,proof shipped," ]
  [ "$(jq -r 'select(.event == "known-issue") | [.kind, .finding, .reason, .by] | join(" ")' "$book")" = \
    "second-opinion F1 later sonnet:high" ]
  head -n 2 "$book" >"$BATS_TEST_TMPDIR/first-two"
  lg beta note --text "one more"
  [ "$(wc -l <"$book")" -eq 8 ]
  cmp <(head -n 2 "$book") "$BATS_TEST_TMPDIR/first-two"
}

@test "log: a known issue names a finding of the current version's receipt" {
  make_slice_repo
  a_review beta
  lg beta known-issue --kind second-opinion --finding F9 --reason r --by sonnet:high
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: second-opinion has no finding F9" ]
  lg beta known-issue --kind second-opinion --finding F1 --reason r --by sonnet:high
  [ "$status" -eq 0 ]
  lg beta known-issue --kind second-opinion --finding F1 --reason r --by sonnet:high
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: second-opinion F1 is already closed" ]
  lg beta known-issue --kind informed-review --finding F1 --reason r --by sonnet:high
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: beta has no informed-review receipt for its current version" ]
  printf 'more\n' >>beta-1
  git commit -q -am "a new commit"
  check_here
  lg beta known-issue --kind second-opinion --finding F3 --reason r --by sonnet:high
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: beta has no second-opinion receipt for its current version" ]
}

@test "log: an answer closes only a question" {
  make_slice_repo
  a_review beta
  lg beta answer --kind second-opinion --finding F1 --text "no" --by sonnet:high
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: F1 is not a question: fix it, or log it as a known issue" ]
  lg beta known-issue --kind second-opinion --finding F2 --reason r --by sonnet:high
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: F2 is a question: answer it with shipit log beta answer" ]
}

@test "log: a usage mistake exits 2 and appends nothing" {
  make_slice_repo
  a_review beta
  lg beta note
  [ "$status" -eq 2 ]
  lg beta note --text hi --by sonnet:high
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: note does not take --by; "* ]]
  lg beta known-issue --kind second-opinion --finding F1 --reason r --by nonsense
  [ "$status" -eq 2 ]
  lg beta receipt --text x
  [ "$status" -eq 2 ]
  lg beta started --text x
  [ "$status" -eq 2 ]
  lg beta shipped
  [ "$status" -eq 2 ]
  lg beta shipped --merge HEAD --unmatched
  [ "$status" -eq 2 ]
  lg beta shipped --merge HEAD --note why
  [ "$status" -eq 2 ]
  lg nowhere note --text x
  [ "$status" -eq 2 ]
  [ "$(wc -l <"$(store)/goals/demo/logbook.jsonl")" -eq 1 ]
}

@test "log: shipped needs the merge of this slice's reviewed change" {
  make_slice_repo
  git init -q --bare "$BATS_TEST_TMPDIR/origin.git"
  git remote add origin "$BATS_TEST_TMPDIR/origin.git"
  git push -q origin main
  reviewed beta
  reviewed gamma
  local before merged
  before=$(git rev-parse main)
  git merge -q --no-ff slice/demo/beta -m "merge beta"
  merged=$(git rev-parse HEAD)
  lg beta shipped --merge "$merged"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $merged is not a commit on origin/main" ]
  git push -q origin main
  lg alpha shipped --merge "$merged"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: alpha has no review receipt: nothing was reviewed to ship" ]
  lg beta shipped --merge "$before"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $before is not the merge of beta's reviewed change into origin/main" ]
  lg gamma shipped --merge "$merged"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $merged is not the merge of gamma's reviewed change into origin/main" ]
  lg beta shipped --merge "$merged"
  [ "$status" -eq 0 ]
  [ "$output" = "logged shipped for beta" ]
  [ "$(jq -r 'select(.event == "shipped") | [.slice, .commit, .matched] | join(" ")' "$(store)/goals/demo/logbook.jsonl")" = \
    "beta $merged true" ]
  lg gamma shipped --merge "$merged"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $merged is already named by beta's shipped entry" ]
  git merge -q --squash slice/demo/gamma
  git commit -q -m "squash gamma"
  git push -q origin main
  lg gamma shipped --merge "$(git rev-parse HEAD)"
  [ "$status" -eq 0 ]
}

@test "log: a squash after a nearby base change is refused, then passes with --unmatched --note" {
  make_slice_repo
  seq 1 9 >shared.txt
  git add shared.txt
  git commit -q -m "a shared file"
  git checkout -q -b slice/demo/beta
  sed -i '5s/.*/five/' shared.txt
  git commit -q -am "change line five"
  check_here
  "$SHIPIT_BIN" receipt informed-review beta --author sonnet:high --by opus:high --result approve
  git checkout -q main
  sed -i '3s/.*/three/' shared.txt
  git commit -q -am "change line three"
  git merge -q --squash slice/demo/beta
  git commit -q -m "squash beta"
  lg beta shipped --merge HEAD
  [ "$status" -eq 1 ]
  [[ $stderr == *"is not the merge of beta's reviewed change into main" ]]
  lg beta shipped --merge HEAD --unmatched
  [ "$status" -eq 2 ]
  lg beta shipped --merge HEAD --unmatched --note "line three moved"
  [ "$status" -eq 0 ]
  [ "$(jq -r 'select(.event == "shipped") | [.matched, .note] | join(" ")' "$(store)/goals/demo/logbook.jsonl")" = \
    "false line three moved" ]
  st
  [[ $output == *"shipped (demo): beta (merge not matched: line three moved)"* ]]
  run --separate-stderr "$SHIPIT_BIN" status --summary beta
  [ "$(sed -n 1p <<<"$output")" = "### ShipIt: beta is shipped (merge not matched: line three moved)" ]
}

@test "log: a proof step ships only after every slice it follows has shipped" {
  make_slice_repo
  lg proof shipped --note "too early"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: proof is not ready: wait for beta, gamma to ship" ]
  ship_entry beta
  lg proof shipped --note "still early"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: proof is not ready: wait for gamma to ship" ]
  ship_entry gamma
  lg proof shipped --note "now"
  [ "$status" -eq 0 ]
}

@test "log: a proof step ships with its evidence" {
  make_slice_repo
  ship_entry beta
  ship_entry gamma
  lg proof shipped --note "the owner ran it"
  [ "$status" -eq 0 ]
  lg proof shipped --merge HEAD
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: "proof" is a proof step: it ships with --note <evidence>' ]
  lg beta shipped --note "evidence"
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: "beta" is a slice: it ships with --merge <commit>' ]
}

@test "log: 40 parallel appends from two working copies give 40 entries" {
  make_slice_repo
  git worktree add -q "$BATS_TEST_TMPDIR/wt2" -b other
  local i
  (for i in $(seq 1 20); do "$SHIPIT_BIN" log alpha note --text "a$i" >/dev/null; done) &
  (cd "$BATS_TEST_TMPDIR/wt2" && for i in $(seq 1 20); do "$SHIPIT_BIN" log beta note --text "b$i" >/dev/null; done) &
  wait
  local book
  book="$(store)/goals/demo/logbook.jsonl"
  [ "$(wc -l <"$book")" -eq 40 ]
  [ "$(jq -c . "$book" | wc -l)" -eq 40 ]
  [ "$(jq -r 'select(.slice == "alpha") | .text' "$book" | wc -l)" -eq 20 ]
}

@test "log: an entry over 4096 bytes exits 2 and appends nothing" {
  make_slice_repo
  lg beta note --text "$(head -c 5000 /dev/zero | tr '\0' x)"
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: entry too long; shorten the text" ]
  [ ! -e "$(store)/goals/demo/logbook.jsonl" ]
}

@test "log: a slice cannot be logged as shipped twice" {
  make_slice_repo
  reviewed beta
  git merge -q --no-ff slice/demo/beta -m "merge beta"
  lg beta shipped --merge HEAD
  [ "$status" -eq 0 ]
  lg beta shipped --merge HEAD --unmatched --note "again"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: beta is already shipped" ]
  ship_entry gamma
  lg proof shipped --note "once"
  [ "$status" -eq 0 ]
  lg proof shipped --note "twice"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: proof is already shipped" ]
  [ "$(jq -r 'select(.event == "shipped") | .slice' "$(store)/goals/demo/logbook.jsonl" | wc -l)" -eq 3 ]
}

@test "log: --unmatched still needs a commit on the base, after the reviewed change left it" {
  make_slice_repo
  git init -q --bare "$BATS_TEST_TMPDIR/origin.git"
  git remote add origin "$BATS_TEST_TMPDIR/origin.git"
  git push -q origin main
  reviewed beta
  local root merged
  root=$(git rev-list --max-parents=0 main)
  git merge -q --no-ff slice/demo/beta -m "merge beta"
  merged=$(git rev-parse HEAD)
  lg beta shipped --merge "$merged" --unmatched --note "not on the base yet"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $merged is not a commit on origin/main" ]
  git push -q origin main
  lg beta shipped --merge "$root" --unmatched --note "the root commit"
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: $root is not after the reviewed change left origin/main" ]
  lg beta shipped --merge "$merged" --unmatched --note "an honest merge"
  [ "$status" -eq 0 ]
}

@test "log: text with a newline or a control character is refused" {
  make_slice_repo
  lg beta note --text $'one\ntwo'
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: --text must be one line, without control characters; usage: shipit log"* ]]
  lg beta known-issue --kind second-opinion --finding F1 --reason $'late\e[31m' --by opus:high
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: --reason must be one line, without control characters; "* ]]
  lg beta shipped --merge HEAD --unmatched --note $'x\n### y'
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: --note must be one line, without control characters; "* ]]
  lg beta known-issue --kind second-opinion --finding F1 --reason r --by $'opus\e:high'
  [ "$status" -eq 2 ]
  [ ! -e "$(store)/goals/demo/logbook.jsonl" ]
}

@test "log: status and the summary print the text of an old entry escaped" {
  make_slice_repo
  mkdir -p "$(store)/goals/demo"
  printf '{"slice":"beta","event":"shipped","commit":"%040d","matched":false,"note":"x\\n### ShipIt: alpha is ready to ship\\u001b[31m","time":"2026-11-21T10:00:00Z","tool":"shipit 0.1.0"}\n' 0 \
    >"$(store)/goals/demo/logbook.jsonl"
  st
  [ "$status" -eq 0 ]
  [[ $output == *'shipped (demo): beta (merge not matched: x\n### ShipIt: alpha is ready to ship\u{1b}[31m)'* ]]
  [[ $output != *$'\e'* ]]
  run --separate-stderr "$SHIPIT_BIN" status --summary beta
  [ "$status" -eq 0 ]
  [[ $output != *$'\e'* ]]
  [ "$(grep -c '^### ShipIt:' <<<"$output")" -eq 1 ]
}

@test "log: raise records a reason, status then asks for the second opinion" {
  suite_repo
  plan_ready alpha
  mapped_work alpha DW1
  check_here
  "$SHIPIT_BIN" receipt informed-review alpha --author sonnet:high --by opus:high --result approve >/dev/null
  st
  [[ $(the_line alpha) == "demo/alpha: ready to ship."* ]]
  local book before
  book="$(store)/goals/demo/logbook.jsonl"
  lg alpha raise --reason "touches the agent gate"
  [ "$status" -eq 0 ]
  [ "$output" = "logged raise for alpha" ]
  [ "$(jq -c 'select(.event == "raise") | [.slice, .reason]' "$book")" = '["alpha","touches the agent gate"]' ]
  st
  [ "$(the_line alpha)" = "demo/alpha: checked. Next: missing: second opinion. Review: full (raised: touches the agent gate). Started without shipit start." ]
  "$SHIPIT_BIN" receipt second-opinion alpha --author sonnet:high --by opus:high --result approve >/dev/null
  st
  [[ $(the_line alpha) == "demo/alpha: ready to ship."* ]]
  before=$(wc -l <"$book")
  lg alpha raise
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: raise needs --reason; "* ]]
  lg alpha raise --reason "  "
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: raise needs a --reason that is not blank; "* ]]
  lg alpha raise --reason x --text y
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: raise does not take --text; "* ]]
  lg proof raise --reason x
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: "proof" is a proof step: it has no review level' ]
  [ "$(wc -l <"$book")" -eq "$before" ]
}
