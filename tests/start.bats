#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# cap_setup <n>: the setup with [agents.parallel] slices = <n>, committed on main.
cap_setup() {
  write_setup "$(fixture_with 'gate.check=[]'; printf '\n[agents.parallel]\nslices = %s\n' "$1")"
}

# an_open_gamma: gamma comes after nothing in goal demo, so that two slices can run at once.
an_open_gamma() {
  sed -i '/^name = "gamma"/{n;s/after = \["alpha"\]/after = []/}' "$(store)/goals/demo/roadmap.v1.md"
}

sh_start() {
  run --separate-stderr "$SHIPIT_BIN" start "$@"
}

@test "start: creates the branch from the base and logs it" {
  make_slice_repo
  plan_ready alpha
  sh_start alpha
  [ "$status" -eq 0 ]
  [ "$output" = "started alpha on slice/demo/alpha; for one session per slice make its working copy: git worktree add $(git rev-parse --show-toplevel)/.shipit/wt/alpha slice/demo/alpha; an isolated maker needs none (it switches to slice/demo/alpha)" ]
  [ "$(git rev-parse slice/demo/alpha)" = "$(git rev-parse main)" ]
  [ -z "$(git config --get branch.slice/demo/alpha.remote || true)" ]
  [ "$(jq -r 'select(.event == "started") | [.slice, .branch] | join(" ")' "$(store)/goals/demo/logbook.jsonl")" = "alpha slice/demo/alpha" ]
  st
  [[ $(the_line alpha) == "demo/alpha: build plan ready. Next: maker: make the slice on slice/demo/alpha" ]]
  [[ $output == *"started: 1 of 3"* ]]
}

@test "start: refused at the parallel cap" {
  make_slice_repo
  cap_setup 1
  an_open_gamma
  plan_ready alpha
  plan_ready gamma
  sh_start alpha
  [ "$status" -eq 0 ]
  sh_start gamma
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: 1 slices are started and not shipped, in demo ([agents.parallel] slices = 1): ship one first" ]
  [ -z "$(git branch --list slice/demo/gamma)" ]
  [ "$(jq -c 'select(.event == "started") | .slice' "$(store)/goals/demo/logbook.jsonl")" = '"alpha"' ]
}

@test "start: the cap counts across goals" {
  make_slice_repo
  cap_setup 1
  add_goal v1
  add_goal w2
  plan_ready alpha v1
  plan_ready alpha w2
  sh_start alpha --goal v1
  [ "$status" -eq 0 ]
  sh_start alpha --goal w2
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: 1 slices are started and not shipped, in v1 ([agents.parallel] slices = 1): ship one first" ]
  [ -z "$(git branch --list slice/w2/alpha)" ]
}

@test "start: the branch is slice/<goal>/<slice>" {
  make_slice_repo
  add_goal w2
  plan_ready alpha w2
  sh_start alpha --goal w2
  [ "$status" -eq 0 ]
  [ -n "$(git branch --list slice/w2/alpha)" ]
  [ -z "$(git branch --list 'slice/demo/*')" ]
  [ -f "$(store)/goals/w2/logbook.jsonl" ]
}

@test "start: --goal, else the current branch, else the only goal" {
  make_slice_repo
  plan_ready alpha
  add_goal w2
  plan_ready alpha w2
  sh_start alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: more than one goal in the store (demo, w2): pass --goal <name>" ]
  git checkout -q -b slice/w2/other
  sh_start alpha
  [ "$status" -eq 0 ]
  [[ $output == "started alpha on slice/w2/alpha;"* ]]
  sh_start alpha --goal nope
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no goal named "nope" in the store' ]
}

@test "start: refused before the build plan counts or while an after slice is not shipped" {
  make_slice_repo
  sh_start beta
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: beta is planned: its build plan needs a counting review first" ]
  plan_ready beta
  sh_start beta
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: wait for alpha to ship" ]
  [ -z "$(git branch --list slice/demo/beta)" ]
  ship_entry alpha
  sh_start beta
  [ "$status" -eq 0 ]
}

@test "start: refused when the branch exists, and a proof step or an unknown slice exits 2" {
  make_slice_repo
  plan_ready alpha
  git branch slice/demo/alpha main
  sh_start alpha
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: slice/demo/alpha already exists: the slice is started" ]
  sh_start proof
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: "proof" is a proof step: it has no branch to start' ]
  sh_start nowhere
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no slice named "nowhere" in the roadmap of goal "demo"' ]
}

@test "start: a goal name used on the remote is refused for its first slice, not for later ones" {
  make_slice_repo
  cap_setup 2
  an_open_gamma
  plan_ready alpha
  plan_ready gamma
  git init -q --bare "$BATS_TEST_TMPDIR/origin.git"
  git remote add origin "$BATS_TEST_TMPDIR/origin.git"
  git push -q origin main
  git push -q origin main:refs/heads/slice/demo/zzz
  git fetch -q origin
  sh_start alpha
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: goal "demo" is already used (origin/slice/demo/zzz): pick another goal name; if the remote branches are new, fetch first' ]
  [ -z "$(git branch --list slice/demo/alpha)" ]
  git push -q origin --delete slice/demo/zzz
  git fetch -q --prune origin
  sh_start alpha
  [ "$status" -eq 0 ]
  git push -q origin slice/demo/alpha
  git fetch -q origin
  sh_start gamma
  [ "$status" -eq 0 ]
}

@test "start: keeps the worktree folder out of git once, in the common dir, and never the coverage maps" {
  make_slice_repo
  an_open_gamma
  plan_ready alpha
  plan_ready gamma
  sh_start alpha
  [ "$status" -eq 0 ]
  common=$(git rev-parse --path-format=absolute --git-common-dir)
  [ "$(grep -cxF '/.shipit/wt/' "$common/info/exclude")" -eq 1 ]
  [ "$(grep -cxF '/.claude/worktrees/' "$common/info/exclude")" -eq 1 ]
  git worktree add -q .shipit/wt/alpha slice/demo/alpha
  [ -z "$(git status --porcelain --ignore-submodules=none)" ]
  run bash -c 'cd .shipit/wt/alpha && "$SHIPIT_BIN" start gamma'
  [ "$status" -eq 0 ]
  [ "$(grep -cxF '/.shipit/wt/' "$common/info/exclude")" -eq 1 ]
  [ "$(grep -cxF '/.claude/worktrees/' "$common/info/exclude")" -eq 1 ]
  [ "$(grep -cE '^/\.(shipit/wt|claude/worktrees)/$' "$common/info/exclude")" -eq 2 ]
  mkdir -p .shipit/coverage/demo
  touch .shipit/coverage/demo/alpha.toml
  run git check-ignore -q .shipit/coverage/demo/alpha.toml
  [ "$status" -eq 1 ]
}

@test "start: names the worktree under the top of the main working copy, from a subfolder or another worktree" {
  make_slice_repo
  an_open_gamma
  plan_ready alpha
  plan_ready gamma
  top=$(git rev-parse --show-toplevel)
  mkdir -p sub/deeper
  run --separate-stderr bash -c 'cd sub/deeper && "$SHIPIT_BIN" start alpha'
  [ "$status" -eq 0 ]
  [ "$output" = "started alpha on slice/demo/alpha; for one session per slice make its working copy: git worktree add $top/.shipit/wt/alpha slice/demo/alpha; an isolated maker needs none (it switches to slice/demo/alpha)" ]
  git worktree add -q "$top/.shipit/wt/alpha" slice/demo/alpha
  run --separate-stderr bash -c 'cd .shipit/wt/alpha && "$SHIPIT_BIN" start gamma'
  [ "$status" -eq 0 ]
  [ "$output" = "started gamma on slice/demo/gamma; for one session per slice make its working copy: git worktree add $top/.shipit/wt/gamma slice/demo/gamma; an isolated maker needs none (it switches to slice/demo/gamma)" ]
}
