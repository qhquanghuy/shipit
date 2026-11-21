# shellcheck shell=bash
# The setup file: loading and validation. Needs lib/common.sh (die, repo_top,
# jq_lib). The schema itself is lib/setup.jq; lib/check.jq has the little that
# is checked of a base branch's setup.

readonly SETUP_PATH='.config/shipit.json'

# setup_label <ref>: how messages name the file; the ref is named unless it is
# HEAD.
setup_label() {
  if [[ -z ${1-} || $1 == HEAD ]]; then
    printf '%s\n' "$SETUP_PATH"
  else
    printf '%s:%s\n' "$1" "$SETUP_PATH"
  fi
}

# setup_text <ref>: print the setup file as committed at <ref> (default HEAD),
# never the working tree. Dies 2 when there is no such file.
setup_text() {
  local ref=${1:-HEAD} top label
  top=$(repo_top) || exit
  label=$(setup_label "$ref") || exit
  git -C "$top" show --end-of-options "$ref:$SETUP_PATH" 2>/dev/null ||
    die 2 "no setup file at $label (see docs/setup.md)"
}

# load_setup [ref]: print the setup committed at <ref> (default HEAD) as
# compact JSON. Dies 2 when the file is missing, is not JSON, or breaks the
# schema (every problem listed).
load_setup() {
  local ref=${1:-HEAD} label text docs msg errors
  label=$(setup_label "$ref") || exit
  text=$(setup_text "$ref") || exit
  if ! docs=$(printf '%s\n' "$text" | jq -c -s . 2>&1); then
    msg=${docs#jq: error*: }
    die 2 "$label is not valid JSON: $msg"
  fi
  errors=$(printf '%s\n' "$docs" | jq_lib -r 'include "setup"; setup_errors | .[]') || exit
  [[ -z $errors ]] || die 2 "$label is invalid:"$'\n'"$errors"
  printf '%s\n' "$docs" | jq -c '.[0]'
}

# load_baseline <ref>: print {"cap": …, "check": […]} from the setup committed
# at <ref>, a base branch. Only those two fields are validated, so a change of
# the schema never breaks it. Prints "null" (and a notice on stderr) when <ref>
# has no setup file. Dies 2 when the file is there but those fields are wrong.
load_baseline() {
  local ref=$1 label text docs errors
  label=$(setup_label "$ref") || exit
  if ! git cat-file -e --end-of-options "$ref:$SETUP_PATH" 2>/dev/null; then
    printf 'shipit: no setup file on %s; nothing to compare\n' "$ref" >&2
    printf 'null\n'
    return 0
  fi
  text=$(setup_text "$ref") || exit
  docs=$(printf '%s\n' "$text" | jq -c -s . 2>/dev/null) ||
    die 2 "$label is not valid JSON"
  errors=$(printf '%s\n' "$docs" | jq_lib -r 'include "check"; baseline_errors | .[]') || exit
  [[ -z $errors ]] || die 2 "$label is invalid:"$'\n'"$errors"
  printf '%s\n' "$docs" | jq -c '.[0] | {cap, check}'
}
