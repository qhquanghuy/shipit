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
    printf '%s\n' "$i" >"work$i"
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
