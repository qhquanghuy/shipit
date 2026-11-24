# shellcheck shell=bash
# Helpers for the tests of receipt, log, status, start, coverage, metric and complete.
# Load with: load helpers, then load slice-helpers

# store: the ShipIt store of the current repo.
store() {
  printf '%s/shipit\n' "$(git rev-parse --path-format=absolute --git-common-dir)"
}

# add_goal <name>: the goal <name> in the store, with the roadmap of tests/fixtures/roadmap.md
# (slices alpha, beta, gamma and the proof step "proof") as its first version.
add_goal() {
  local dir
  dir="$(store)/goals/$1"
  mkdir -p "$dir"
  sed "s/^goal = \"demo\"/goal = \"$1\"/" "$SHIPIT_SRC/tests/fixtures/roadmap.md" >"$dir/roadmap.v1.md"
}

# make_slice_repo [path=value]...: a repo as the current directory. Its setup (the fixture with
# those values replaced; default: no check commands) is committed on main, and the store has
# the goal demo.
make_slice_repo() {
  local repo edits=("$@")
  [[ ${#edits[@]} -gt 0 ]] || edits=('gate.check=[]')
  repo=$(make_repo)
  cd "$repo" || return 1
  write_setup "$(fixture_with "${edits[@]}")"
  add_goal demo
}

# write_build_plan <slice> <text> [goal]: the build plan of a slice, in the store.
write_build_plan() {
  local dir
  dir="$(store)/goals/${3:-demo}/slices/$1"
  mkdir -p "$dir"
  printf '%s\n' "$2" >"$dir/build-plan.md"
}

# make_work <branch> <count>: a new branch from main with <count> new files in one commit.
make_work() {
  local i
  git checkout -q -b "$1" main
  for i in $(seq 1 "$2"); do
    printf '%s\n' "$1 $i" >"${1##*/}-$i"
  done
  git add -A
  git commit -q -m "work on $1"
}

# rec <args>: run shipit receipt, stdout in $output, stderr in $stderr.
rec() {
  run --separate-stderr "$SHIPIT_BIN" receipt "$@"
}

# check_here: shipit check on the current branch; the test stops when it does not pass.
check_here() {
  "$SHIPIT_BIN" check >/dev/null 2>&1 || {
    "$SHIPIT_BIN" check >&2
    return 1
  }
}

# patch_id_of <rev>: the patch id of the change from main to <rev>, as a plain diff gives it.
patch_id_of() {
  git diff --binary --full-index "$(git merge-base main "$1")" "$1" | git patch-id --stable | cut -d' ' -f1
}

# reviewed <slice> [count]: slice/demo/<slice> with <count> new files, checked and with an
# informed review; the repo is back on main.
reviewed() {
  make_work "slice/demo/$1" "${2:-1}"
  check_here
  "$SHIPIT_BIN" receipt informed-review "$1" --author sonnet:high --by opus:high --result approve >/dev/null
  git checkout -q main
}

# lg <args>: run shipit log, stdout in $output, stderr in $stderr.
lg() {
  run --separate-stderr "$SHIPIT_BIN" log "$@"
}

# st <args>: run shipit status, stdout in $output, stderr in $stderr.
st() {
  run --separate-stderr "$SHIPIT_BIN" status "$@"
}

# plan_ready <slice> [goal]: a build plan for the slice with a counting plan review.
plan_ready() {
  write_build_plan "$1" "the plan of ${2:-demo}/$1" "${2:-demo}"
  "$SHIPIT_BIN" receipt plan-review "$1" --goal "${2:-demo}" --author opus:high --by fable:high --result approve >/dev/null
}

# ship_entry <slice> [goal]: a shipped line in the goal's logbook, as the owner's merge leaves it.
ship_entry() {
  local book
  book="$(store)/goals/${2:-demo}/logbook.jsonl"
  mkdir -p "$(dirname "$book")"
  printf '{"slice":"%s","event":"shipped","commit":"%s","matched":true,"time":"2026-11-21T10:00:00Z","tool":"shipit 0.1.0"}\n' \
    "$1" "$(printf '%040d' 0)" >>"$book"
}

# the_line <slice> [goal]: the status line of one slice, from $output.
the_line() {
  # shellcheck disable=SC2154 # `output` is set by bats `run`
  grep -F "${2:-demo}/$1:" <<<"$output"
}

# suite_repo [program]: the repo whose only check is the coverage line over the committed script
# fake-suite, which prints libtest and TAP lines (and a skipped one); the store has the goal demo.
suite_repo() {
  make_slice_repo "gate.check=[\"$SHIPIT_BIN coverage -- ${1:-bash fake-suite}\"]"
  cat >fake-suite <<'SUITE'
echo "test alpha::tests::one ... ok"
echo "ok 1 receipt: first"
echo "ok 2 receipt: skipped # skip not run"
echo "test alpha::tests::slow ... ignored"
SUITE
  git add fake-suite
  git commit -q -m "a suite"
}

# commit_map <goal> <slice> <toml>: commit the map of the slice.
commit_map() {
  mkdir -p ".shipit/coverage/$1"
  printf '%s\n' "$3" >".shipit/coverage/$1/$2.toml"
  git add .shipit
  git commit -q -m "a map"
}

# fake_gh: a gh first on PATH. It logs `issue create <title>` for each call to $GH_LOG and prints
# an issue address that carries the number of the call; GH_FAIL_AT=<n> makes the n-th call fail.
fake_gh() {
  local bin="$BATS_TEST_TMPDIR/ghbin"
  mkdir -p "$bin"
  GH_LOG="$BATS_TEST_TMPDIR/gh.log"
  export GH_LOG
  printf '#!%s\n' "$BASH" >"$bin/gh"
  cat >>"$bin/gh" <<'GH'
printf '%s %s %s\n' "$1" "$2" "$4" >>"$GH_LOG"
n=$(wc -l <"$GH_LOG")
if [[ ${GH_FAIL_AT:-} == "$n" ]]; then
  echo "gh: something went wrong" >&2
  exit 1
fi
echo "https://example.invalid/issues/$n"
GH
  chmod +x "$bin/gh"
  PATH="$bin:$PATH"
}

# logged_start <slice> [rule]: a started line in the goal's logbook; with `rule` as `shipit start`
# writes it from the author rule on, without it as the older tool wrote it.
logged_start() {
  local book rule=''
  book="$(store)/goals/demo/logbook.jsonl"
  mkdir -p "$(dirname "$book")"
  [[ ${2:-} == rule ]] && rule=',"author_rule":true'
  printf '{"slice":"%s","event":"started","branch":"slice/demo/%s"%s,"time":"2026-11-21T10:00:00Z","tool":"shipit 0.1.0"}\n' \
    "$1" "$1" "$rule" >>"$book"
}

# plain_commit <file>: a commit of one new file with no trailer.
plain_commit() {
  printf '%s\n' "$1" >"$1"
  git add "$1"
  git commit -q -m "plain $1"
}

# old_author <kind> <model> [fresh]: the receipt and its logbook line of the last receipt of that
# kind, as the tool wrote them before the author of the work had to be the setup's maker: the
# author is <model>:high, not the maker, and the rank is "fresh" when asked.
old_author() {
  local file rank
  file=$(find "$(store)/receipts" -name "$1.json" | head -n 1)
  rank=$(jq -r .rank "$file")
  [[ ${3:-} == fresh ]] && rank=fresh
  jq --arg m "$2" --arg r "$rank" \
    '.author = {model: $m, effort: "high", tier: ({haiku: 1, sonnet: 2, opus: 3, fable: 4}[$m]), stand_in: false} | .author_is_maker = false | .rank = $r' \
    "$file" >"$file.new"
  mv "$file.new" "$file"
  local book
  book="$(store)/goals/demo/logbook.jsonl"
  jq -c --arg k "$1" --arg m "$2" --arg r "$rank" \
    'if .kind == $k then .author = ($m + ":high") | .rank = $r else . end' "$book" >"$book.new"
  mv "$book.new" "$book"
}
