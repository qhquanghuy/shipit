#!/usr/bin/env bats

load helpers

@test "--version prints the version from VERSION" {
  run_shipit --version
  [ "$status" -eq 0 ]
  [ "$output" = "shipit 0.1.0" ]
}

@test "an unknown command exits 2 and names shipit help" {
  run_shipit nosuchcommand
  [ "$status" -eq 2 ]
  [[ $output == shipit:* ]]
  [[ $output == *"shipit help"* ]]
}

@test "command names with a slash or .. exit 2" {
  local name
  for name in ../bin/shipit "a/b" ".." "../x" "/etc/passwd" "UPPER" "1abc" "-x" ""; do
    run_shipit "$name"
    [ "$status" -eq 2 ] || { echo "name '$name' gave status $status"; return 1; }
    [[ $output == *"shipit help"* ]]
  done
}

@test "help lists every libexec/shipit-* file and no arguments does the same" {
  local tool="$BATS_TEST_TMPDIR/tool"
  cp -r "$SHIPIT_SRC" "$tool"
  printf 'echo a\n' >"$tool/libexec/shipit-alpha"
  printf 'echo b\n' >"$tool/libexec/shipit-beta-two"
  run "$tool/bin/shipit" help
  [ "$status" -eq 0 ]
  [[ $output == *alpha* ]]
  [[ $output == *beta-two* ]]
  local with_help=$output
  run "$tool/bin/shipit"
  [ "$status" -eq 0 ]
  [ "$output" = "$with_help" ]
}

@test "help with no commands installed still succeeds" {
  local tool="$BATS_TEST_TMPDIR/tool"
  cp -r "$SHIPIT_SRC" "$tool"
  rm -f "$tool"/libexec/shipit-*
  run "$tool/bin/shipit" help
  [ "$status" -eq 0 ]
  [[ $output == *"shipit <command>"* ]]
}

@test "adding libexec/shipit-hello (no shebang) makes hello listed and runnable" {
  local tool="$BATS_TEST_TMPDIR/tool"
  cp -r "$SHIPIT_SRC" "$tool"
  printf 'echo "hello root=$SHIPIT_ROOT args=$*"\n' >"$tool/libexec/shipit-hello"
  chmod -x "$tool/libexec/shipit-hello"
  run "$tool/bin/shipit" help
  [[ $output == *hello* ]]
  run "$tool/bin/shipit" hello one two
  [ "$status" -eq 0 ]
  [ "$output" = "hello root=$tool args=one two" ]
}

@test "the install root is found through a symlink to bin/shipit" {
  local tool="$BATS_TEST_TMPDIR/tool"
  cp -r "$SHIPIT_SRC" "$tool"
  printf 'echo "root=$SHIPIT_ROOT"\n' >"$tool/libexec/shipit-where"
  mkdir -p "$BATS_TEST_TMPDIR/elsewhere"
  ln -s "$tool/bin/shipit" "$BATS_TEST_TMPDIR/elsewhere/shipit"
  run "$BATS_TEST_TMPDIR/elsewhere/shipit" where
  [ "$status" -eq 0 ]
  [ "$output" = "root=$tool" ]
}

@test "a command's exit status passes through" {
  local tool="$BATS_TEST_TMPDIR/tool"
  cp -r "$SHIPIT_SRC" "$tool"
  printf 'exit 3\n' >"$tool/libexec/shipit-three"
  run "$tool/bin/shipit" three
  [ "$status" -eq 3 ]
}
