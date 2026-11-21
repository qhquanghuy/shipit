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
  lg proof shipped --note "run by the owner"
  [ "$status" -eq 0 ]
  [ "$output" = "logged shipped for proof" ]
  [ "$(wc -l <"$book")" -eq 5 ]
  [ "$(jq -r '[.slice, .event] | join(" ")' "$book" | tr '\n' ',')" = \
    "beta receipt,beta note,beta known-issue,beta answer,proof shipped," ]
  [ "$(jq -r 'select(.event == "known-issue") | [.kind, .finding, .reason, .by] | join(" ")' "$book")" = \
    "second-opinion F1 later sonnet:high" ]
  head -n 2 "$book" >"$BATS_TEST_TMPDIR/first-two"
  lg beta note --text "one more"
  [ "$(wc -l <"$book")" -eq 6 ]
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

@test "log: a proof step ships with its evidence" {
  make_slice_repo
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
