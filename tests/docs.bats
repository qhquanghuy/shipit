#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# A human doc is kept only if this test checks what it names against the code: every `shipit
# <command>`, every --flag, every setup key path, every SHIPIT_ variable and every quoted
# `shipit: …` message. The goal (docs/goal.md) is approved text, not a reference, and is left out.
DOCS=(README.md docs/setup.md docs/store.md docs/vocab.md)

# Flags of git, nix and cargo that the docs mention; any other flag must be one of shipit's.
EXTERNAL_FLAGS=(--all-targets --branch --command --ignore-submodules --print-build-logs --release)

# files_in <root>: the reference docs found under <root>.
files_in() {
  local file
  for file in "${DOCS[@]}"; do
    [[ -f $1/$file ]] && printf '%s\n' "$1/$file"
  done
  return 0
}

# source_text: the Rust source with each line continued by a backslash joined to the next, so a
# message split over lines reads as one.
source_text() {
  cat "$SHIPIT_SRC"/src/main.rs "$SHIPIT_SRC"/src/*/*.rs |
    sed -e ':a' -e '/\\$/N; s/\\\n *//; ta'
}

# setup_paths: the key paths of a setup as `shipit setup` prints it, arrays flattened.
setup_paths() {
  local repo
  repo=$(make_repo)
  (cd "$repo" && "$SHIPIT_BIN" setup) |
    jq -r '[paths | map(select(type == "string")) | join(".")] | unique[]'
}

# check_docs <root>: print one line for each thing a doc names that the code does not have;
# status 1 if there is any.
check_docs() {
  local root=$1 files word flag path var message piece full_help src paths bad=0
  files=$(files_in "$root")
  [[ -n $files ]] || { echo "no docs under $root"; return 1; }
  full_help=$("$SHIPIT_BIN" help)
  src=$(source_text)
  paths=$(setup_paths)

  # shellcheck disable=SC2086
  for word in $(grep -ohE 'shipit [a-z][a-z-]*' $files | cut -d' ' -f2 | sort -u); do
    [[ $word == help ]] && continue
    "$SHIPIT_BIN" help "$word" >/dev/null 2>&1 || { echo "command: shipit $word"; bad=1; }
  done

  # shellcheck disable=SC2086
  for flag in $(grep -ohE -- '--[a-z][a-z-]*' $files | sort -u); do
    [[ " ${EXTERNAL_FLAGS[*]} " == *" $flag "* ]] && continue
    [[ $flag == --help || $flag == --version ]] && continue
    grep -qF -- "$flag" <<<"$full_help" || { echo "flag: $flag"; bad=1; }
  done

  # shellcheck disable=SC2086
  for path in $(grep -ohE '`(version|gate|agents|packs|owner)(\.[a-z_]+)*`' $files | tr -d '`' | sort -u); do
    [[ $path == version || $path =~ ^(gate|agents|packs|owner)$ ]] && continue
    grep -qxF -- "$path" <<<"$paths" || { echo "setup key: $path"; bad=1; }
  done

  # shellcheck disable=SC2086
  for var in $(grep -ohE 'SHIPIT_[A-Z_]+' $files | sort -u); do
    grep -qF -- "$var" <<<"$src" ||
      grep -qF -- "$var" "$SHIPIT_SRC/tests/helpers.bash" || { echo "variable: $var"; bad=1; }
  done

  # A message is checked piece by piece: what stays between its <placeholders> must be in the
  # source (the doc names the setup file where the code has the label of the ref).
  # shellcheck disable=SC2086
  while IFS= read -r message; do
    [[ -n $message ]] || continue
    message=${message#\`shipit: }
    message=${message%\`}
    message=${message//.config\/shipit.toml/<path>}
    while IFS= read -r piece; do
      [[ -z $piece ]] && continue
      grep -qF -- "$piece" <<<"$src" || { echo "message: $piece"; bad=1; }
    done < <(sed -E 's/<[^>]*>/\n/g' <<<"$message")
  done < <(grep -ohE '`shipit: [^`]*`' $files)

  return "$bad"
}

# copy_docs <root>: a copy of the reference docs to break.
copy_docs() {
  local file
  for file in "${DOCS[@]}"; do
    mkdir -p "$1/$(dirname "$file")"
    cp "$SHIPIT_SRC/$file" "$1/$file"
  done
}

@test "everything the reference docs name exists in the code" {
  run check_docs "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
}

@test "the example in docs/setup.md is a valid setup" {
  local repo
  repo=$(make_repo)
  cd "$repo"
  awk '/^```toml$/ { on = 1; next } /^```$/ { on = 0 } on' "$SHIPIT_SRC/docs/setup.md" >example.toml
  [ -s example.toml ]
  write_setup "$(cat example.toml)"
  run_shipit setup
  [ "$status" -eq 0 ]
}

@test "a renamed flag in a doc is found" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  sed -i 's/--base/--root/g' "$BATS_TEST_TMPDIR/docs/README.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"flag: --root"* ]]
}

@test "a command that does not exist in a doc is found" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  sed -i 's/shipit verify/shipit verifyy/' "$BATS_TEST_TMPDIR/docs/README.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"command: shipit verifyy"* ]]
}

@test "a renamed setup key in a doc is found" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  sed -i 's/`gate\.max_files`/`gate.max_file`/' "$BATS_TEST_TMPDIR/docs/docs/setup.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"setup key: gate.max_file"* ]]
}

@test "a renamed environment variable in a doc is found" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  sed -i 's/SHIPIT_CHECK/SHIPIT_CHEK/' "$BATS_TEST_TMPDIR/docs/docs/setup.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"variable: SHIPIT_CHEK"* ]]
}

@test "a quoted message that the code does not print is found" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  sed -i 's/the owner approves it by merging/the owner approves it by squashing/' "$BATS_TEST_TMPDIR/docs/README.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"message: "*"squashing"* ]]
}
