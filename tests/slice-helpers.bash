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
