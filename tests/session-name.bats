#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# A repository in a folder called shipit, with one commit.
setup() {
  REPO="$BATS_TEST_TMPDIR/shipit"
  mkdir -p "$REPO"
  git -C "$REPO" init -q -b main
  git -C "$REPO" -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false commit -q --allow-empty -m init
}

@test "in a repo folder named shipit, session-name v1 prints shipit·v1" {
  cd "$REPO"
  run_shipit session-name v1
  [ "$status" -eq 0 ]
  [ "$output" = "shipit·v1" ]
}

@test "from a linked worktree, session-name v1 x prints shipit·v1·x" {
  git -C "$REPO" worktree add -q "$BATS_TEST_TMPDIR/wt/x" -b slice/v1/x
  cd "$BATS_TEST_TMPDIR/wt/x"
  run_shipit session-name v1 x
  [ "$status" -eq 0 ]
  [ "$output" = "shipit·v1·x" ]
}

@test "a bad part exits 2 with the message" {
  cd "$REPO"
  run --separate-stderr "$SHIPIT_BIN" session-name "v 1"
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: 'v 1' is not a name part: use letters, digits, '.', '_' or '-'" ]
  run --separate-stderr "$SHIPIT_BIN" session-name v1 "a·b"
  [ "$status" -eq 2 ]
  run --separate-stderr "$SHIPIT_BIN" session-name
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: usage: shipit session-name <goal> [<slice>]" ]
}

@test "a project folder that is not a name part exits 2 naming the folder" {
  mkdir -p "$BATS_TEST_TMPDIR/my project"
  git -C "$BATS_TEST_TMPDIR/my project" init -q
  cd "$BATS_TEST_TMPDIR/my project"
  run --separate-stderr "$SHIPIT_BIN" session-name v1
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: the project folder: 'my project' is not a name part"* ]]
}

@test "outside a repository it exits 2" {
  cd "$BATS_TEST_TMPDIR"
  run --separate-stderr "$SHIPIT_BIN" session-name v1
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: not inside a git repository"* ]]
}
