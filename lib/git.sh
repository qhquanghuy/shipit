# shellcheck shell=bash
# Git facts about the work and its base. Needs lib/common.sh (die).

# ref_commit <ref>: print the commit <ref> names; fail quietly when there is none.
ref_commit() {
  git rev-parse --verify --quiet --end-of-options "$1^{commit}"
}

# base_ref <base>: print the ref the work is measured against. A local <base>
# that has everything origin/<base> has (it is ahead or equal, so the owner's
# unpushed work counts) wins; else origin/<base> if it resolves, else <base>
# if it resolves; otherwise die 2.
base_ref() {
  local base=$1
  local has_origin='' has_local=''
  ref_commit "origin/$base" >/dev/null && has_origin=1
  ref_commit "$base" >/dev/null && has_local=1
  if [[ -n $has_origin && -n $has_local ]]; then
    if git merge-base --is-ancestor --end-of-options "origin/$base" "$base"; then
      printf '%s\n' "$base"
    else
      printf 'origin/%s\n' "$base"
    fi
  elif [[ -n $has_origin ]]; then
    printf 'origin/%s\n' "$base"
  elif [[ -n $has_local ]]; then
    printf '%s\n' "$base"
  else
    die 2 "base branch \"$base\" not found (looked for origin/$base and $base)"
  fi
}

# current_branch: print the name of the checked-out branch; nothing when HEAD is
# detached.
current_branch() {
  git symbolic-ref --quiet --short HEAD || true
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

# refuse_hidden_edits <exit code>: die with <exit code> when a tracked file is
# marked skip-worktree or assume-unchanged.
refuse_hidden_edits() {
  local hidden hidden_line
  hidden=$(hidden_edit_paths) || exit
  [[ -z $hidden ]] && return 0
  hidden_line=$(printf '%s\n' "$hidden" | first_lines) || exit
  die "$1" "tracked files are marked skip-worktree or assume-unchanged, which hides edits from git ($hidden_line); clear the flag with 'git update-index --no-skip-worktree --no-assume-unchanged <file>' and run shipit again"
}

# refuse_sparse_checkout: die 2 when the repository is a sparse checkout, where
# files outside the pattern are missing and checks would not see them.
refuse_sparse_checkout() {
  local sparse
  sparse=$(git config --get --type=bool core.sparseCheckout || true)
  [[ $sparse != true ]] ||
    die 2 "sparse checkout is not supported: checks would not see every file"
}

# first_lines: read lines on stdin; print the first few on one line.
first_lines() {
  sed -n '1,5p' | paste -sd, - | sed 's/,/, /g'
}

# dirty_paths: the first few paths `git status` lists, on one line, read from
# the NUL-separated form so a rename lists both of its paths plainly and no
# name is quoted or split.
dirty_paths() {
  local entry path skip_next='' count=0 out=''
  while IFS= read -r -d '' entry; do
    if [[ -n $skip_next ]]; then
      skip_next=''
      path=$entry
    else
      path=${entry:3}
      [[ ${entry:0:2} == *[RC]* ]] && skip_next=1
    fi
    ((count += 1))
    ((count <= 5)) || continue
    out+="${out:+, }$path"
  done < <(git status --porcelain -z --untracked-files=normal)
  printf '%s\n' "$out"
}
