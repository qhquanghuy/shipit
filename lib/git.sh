# shellcheck shell=bash
# Git facts about the work and its base. Needs lib/common.sh (die).

# base_ref <base>: print origin/<base> if it resolves, else <base> if it
# resolves; otherwise die 2.
base_ref() {
  local base=$1
  if git rev-parse --verify --quiet --end-of-options "origin/$base^{commit}" >/dev/null; then
    printf 'origin/%s\n' "$base"
  elif git rev-parse --verify --quiet --end-of-options "$base^{commit}" >/dev/null; then
    printf '%s\n' "$base"
  else
    die 2 "base branch \"$base\" not found (looked for origin/$base and $base)"
  fi
}

# merge_base <ref>: print the merge base of <ref> and HEAD; die 2 if they share
# no history.
merge_base() {
  git merge-base --end-of-options "$1" HEAD 2>/dev/null ||
    die 2 "no shared history with $1"
}

# changed_files_count <merge base>: print how many files HEAD changes since
# <merge base>. Counted from NUL-separated names, so any file name counts as
# one. A rename counts as one file (-M, so the user's diff.renames setting does
# not matter). Dies 2 when git cannot make the diff.
changed_files_count() {
  local count
  count=$(git diff -z --name-only -M --end-of-options "$1" HEAD | tr -cd '\0' | wc -c | tr -d '[:space:]') ||
    die 2 "cannot count the changed files since $1"
  printf '%s\n' "$count"
}

# hidden_edit_paths: print the tracked files that are marked skip-worktree or
# assume-unchanged, one per line. Git does not report edits to these, so a clean
# `git status` proves nothing about them.
hidden_edit_paths() {
  git ls-files -v -z | tr '\0' '\n' | sed -n -E 's/^[a-zS] //p' ||
    die 2 "cannot list the tracked files"
}
