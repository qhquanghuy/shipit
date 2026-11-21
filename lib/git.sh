# shellcheck shell=bash
# Git facts about the work and its base. Needs lib/common.sh (die).

# base_ref <base>: print origin/<base> if it resolves, else <base> if it
# resolves; otherwise die 2.
base_ref() {
  local base=$1
  if git rev-parse --verify --quiet "origin/$base^{commit}" >/dev/null; then
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
  git merge-base "$1" HEAD 2>/dev/null ||
    die 2 "no shared history with $1"
}

# changed_files_count <merge base>: print how many files HEAD changes since
# <merge base>. Counted from NUL-separated names, so any file name counts as
# one. Renames are not detected: the old and the new name each count.
changed_files_count() {
  git diff -z --name-only --no-renames "$1" HEAD | tr -cd '\0' | wc -c | tr -d '[:space:]'
  printf '\n'
}
