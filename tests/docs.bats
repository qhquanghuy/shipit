#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# This test reads README.md, every file in docs/ and every skill of skills/ and checks that what they
# name exists in the code. A new doc or skill is covered without editing this file.
#
# Checked, in every one of those files (docs/goal.md included):
#   - each --flag is in the output of `shipit help`, or is one of the git, nix and cargo flags
#     listed below;
#   - each backticked setup key path (such as `gate.max_files`) is in the output of `shipit setup`;
#   - each SHIPIT_ variable is in src/ or in tests/helpers.bash.
# Checked, in every file except docs/goal.md (which names planned commands and is approved text):
#   - each `shipit <word>` is a command `shipit help <word>` knows;
#   - each backticked `shipit: …` message, read across line breaks, has every piece between its
#     <placeholders> somewhere in the source of src/ (tests and comments included).
# Not checked: what a doc says a command does, exit codes, which commands need no setup (all of
# these come from `shipit help <command>`, which cannot drift), messages without the `shipit: `
# prefix, receipt fields, store paths and default values.

# Flags of git, gh, nix, cargo and claude (--allowed reads the start of --allowedTools; --add-dir) that the docs mention; any other flag must be one of shipit's.
EXTERNAL_FLAGS=(--all-targets --branch --check --command --ignore-submodules --print-build-logs --release --pager --pre --short --template --numstat --separate-git-dir --stat --target-directory --body-file --allowed --add-dir --title --git-common-dir --name-status --comments --detach --format --git-dir --work-tree)

# files_in <root>: README.md, every file in docs/ and every skill under <root>.
files_in() {
  local file
  for file in "$1"/README.md "$1"/docs/*.md "$1"/skills/*/SKILL.md; do
    [[ -f $file ]] && printf '%s\n' "$file"
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
  local root=$1 files others word flag path var message piece full_help src paths bad=0
  files=$(files_in "$root")
  [[ -n $files ]] || { echo "no docs under $root"; return 1; }
  others=$(grep -v '/docs/goal\.md$' <<<"$files" || true)
  full_help=$("$SHIPIT_BIN" help)
  src=$(source_text)
  paths=$(setup_paths)

  # shellcheck disable=SC2086
  for word in $(grep -ohE 'shipit [a-z][a-z-]*' $others | cut -d' ' -f2 | sort -u); do
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
  # source (the doc names the setup file where the code has the label of the ref). The docs are
  # joined into one line first, so a message broken over two lines is read whole.
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
  done < <(cat $others | tr '\n' ' ' | grep -ohE '`shipit: [^`]*`' | sed -E 's/  +/ /g')

  return "$bad"
}

# copy_docs <root>: a copy of README.md, docs/ and skills/ to break.
copy_docs() {
  mkdir -p "$1/docs"
  cp "$SHIPIT_SRC/README.md" "$1/README.md"
  cp "$SHIPIT_SRC"/docs/*.md "$1/docs/"
  if [[ -d $SHIPIT_SRC/skills ]]; then cp -r "$SHIPIT_SRC/skills" "$1/skills"; fi
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

@test "a message broken over two lines of a doc is read whole" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  sed -i 's/^setup change: the owner approves/setup changed: the owner approves/' "$BATS_TEST_TMPDIR/docs/docs/setup.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"message: "* ]]
}

@test "a doc added later is checked without editing the test" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  printf 'Run `shipit check --nope` and see `gate.nothing`.\n' >"$BATS_TEST_TMPDIR/docs/docs/later.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"flag: --nope"* ]]
  [[ $output == *"setup key: gate.nothing"* ]]
}

@test "docs/goal.md is checked for keys, flags and variables, but may name planned commands" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  printf 'The `shipit complete` command, planned, with `gate.max_files`.\n' >>"$BATS_TEST_TMPDIR/docs/docs/goal.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 0 ]
  printf 'Also `gate.max_file` and --no-such-flag and SHIPIT_NOTHING.\n' >>"$BATS_TEST_TMPDIR/docs/docs/goal.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"setup key: gate.max_file"* ]]
  [[ $output == *"flag: --no-such-flag"* ]]
  [[ $output == *"variable: SHIPIT_NOTHING"* ]]
}

@test "a command that does not exist in a skill is found" {
  copy_docs "$BATS_TEST_TMPDIR/docs"
  mkdir -p "$BATS_TEST_TMPDIR/docs/skills/x"
  printf -- '---\nname: x\n---\nRun `shipit verifyy` now.\n' >"$BATS_TEST_TMPDIR/docs/skills/x/SKILL.md"
  run check_docs "$BATS_TEST_TMPDIR/docs"
  [ "$status" -eq 1 ]
  [[ $output == *"command: shipit verifyy"* ]]
}
