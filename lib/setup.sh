# shellcheck shell=bash
# The setup file: loading and validation. Needs lib/common.sh (die, repo_top,
# jq_lib). The schema itself is lib/setup.jq.

readonly SETUP_PATH='.config/shipit.json'

# setup_label <ref>: how messages name the file, with the ref when one is given.
setup_label() {
  if [[ -n ${1-} ]]; then
    printf '%s:%s\n' "$1" "$SETUP_PATH"
  else
    printf '%s\n' "$SETUP_PATH"
  fi
}

# setup_text <ref>: print the setup file text, from the working tree or from
# <ref>. Dies 2 when there is no such file.
setup_text() {
  local ref=${1-} top label
  top=$(repo_top)
  label=$(setup_label "$ref")
  if [[ -n $ref ]]; then
    git -C "$top" show "$ref:$SETUP_PATH" 2>/dev/null ||
      die 2 "no setup file at $label (run shipit init)"
  else
    [[ -f $top/$SETUP_PATH ]] || die 2 "no setup file at $label (run shipit init)"
    cat "$top/$SETUP_PATH"
  fi
}

# load_setup [ref]: print the setup as compact JSON. Dies 2 when the file is
# missing, is not JSON, or breaks the schema (every problem listed).
load_setup() {
  local ref=${1-} label text docs msg errors
  label=$(setup_label "$ref")
  text=$(setup_text "$ref")
  if ! docs=$(printf '%s\n' "$text" | jq -c -s . 2>&1); then
    msg=${docs#jq: error*: }
    die 2 "$label is not valid JSON: $msg"
  fi
  errors=$(printf '%s\n' "$docs" | jq_lib -r 'include "setup"; setup_errors | .[]')
  [[ -z $errors ]] || die 2 "$label is invalid:"$'\n'"$errors"
  printf '%s\n' "$docs" | jq -c '.[0]'
}
