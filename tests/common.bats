#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# Run a snippet with lib/common.sh loaded.
with_common() {
  SHIPIT_ROOT="$SHIPIT_SRC" bash -c 'set -euo pipefail; source "$SHIPIT_ROOT/lib/common.sh"; '"$1"
}

@test "die prints 'shipit: <message>' to stderr and exits with the code" {
  run --separate-stderr with_common 'die 3 "something broke"'
  [ "$status" -eq 3 ]
  [ -z "$output" ]
  [ "$stderr" = "shipit: something broke" ]
}

@test "repo_top prints the top folder from a subfolder" {
  local repo
  repo=$(make_repo)
  mkdir -p "$repo/a/b"
  cd "$repo/a/b"
  run with_common 'repo_top'
  [ "$status" -eq 0 ]
  [ "$output" = "$(cd "$repo" && pwd -P)" ]
}

@test "repo_top outside a repository dies 2" {
  cd "$BATS_TEST_TMPDIR"
  run --separate-stderr env GIT_CEILING_DIRECTORIES="$BATS_TEST_TMPDIR" \
    bash -c 'set -e; SHIPIT_ROOT="$1"; source "$1/lib/common.sh"; x=$(repo_top); echo "unreachable $x"' _ "$SHIPIT_SRC"
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: not inside a git repository"* ]]
}

@test "jq_lib runs jq with the lib folder on the module path" {
  local tool="$BATS_TEST_TMPDIR/tool"
  cp -r "$SHIPIT_SRC" "$tool"
  printf 'def twice: . * 2;\n' >"$tool/lib/probe.jq"
  run env SHIPIT_ROOT="$tool" bash -c \
    'source "$SHIPIT_ROOT/lib/common.sh"; echo 21 | jq_lib "include \"probe\"; twice"'
  [ "$status" -eq 0 ]
  [ "$output" = "42" ]
}

@test "a die inside a nested command substitution stops the caller after one message" {
  run --separate-stderr with_common '
    inner() { die 2 "boom"; }
    outer() { local x; x=$(inner); echo "outer went on"; }
    y=$(outer)
    echo "main went on"'
  [ "$status" -eq 2 ]
  [ -z "$output" ]
  [ "$stderr" = "shipit: boom" ]
}

@test "require_bash refuses a bash older than 4.4 with the version found" {
  run --separate-stderr with_common 'require_bash 3 2 57'
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: shipit needs bash 4.4 or newer (found 3.2.57)" ]
  run --separate-stderr with_common 'require_bash 4 3 48'
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: shipit needs bash 4.4 or newer (found 4.3.48)" ]
}

@test "require_bash accepts 4.4 and newer, comparing numbers (4.10 is newer than 4.4)" {
  run with_common 'require_bash 4 4 0; require_bash 4 10 0; require_bash 5 0 17; require_bash 5 3 9'
  [ "$status" -eq 0 ]
}

@test "common.sh checks the bash version before it asks for inherit_errexit" {
  local check_line shopt_line
  check_line=$(grep -n '^require_bash "\${BASH_VERSINFO\[0\]}"' "$SHIPIT_SRC/lib/common.sh" | cut -d: -f1)
  shopt_line=$(grep -n '^shopt -s inherit_errexit' "$SHIPIT_SRC/lib/common.sh" | cut -d: -f1)
  [ -n "$check_line" ]
  [ -n "$shopt_line" ]
  [ "$check_line" -lt "$shopt_line" ]
}
