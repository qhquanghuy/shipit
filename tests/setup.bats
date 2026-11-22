#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# What `shipit setup` prints for the fixture: the setup as compact JSON.
PRINTED='{"version":1,"gate":{"base":"main","max_files":60,"check":["nix flake check --print-build-logs"],"environment":{"prefix":[]}},"agents":{"max_fix_rounds":2,"effort_cap":"high","tiers":[{"tier":1,"model":"haiku"},{"tier":2,"model":"sonnet"},{"tier":3,"model":"opus"},{"tier":4,"model":"fable","stand_in":{"model":"opus","effort":"high"}}],"roles":{"scout":{"tier":1,"retries":1,"escalate_to":{"tier":2}},"planner":{"tier":3,"effort":"high"},"maker":{"tier":2,"effort":"high"},"tester":{"tier":3,"effort":"medium"},"reader":{"tier":1}},"parallel":{"slices":3,"agents":4}},"packs":{"skill":"superpowers","domain":"coding","model":"claude"},"owner":{"max_lines":30,"status_update_max_lines":10,"example_max_lines":15}}'

# stub_run <args>: run `shipit setup <args>` in the current directory.
stub_run() {
  run --separate-stderr "$SHIPIT_BIN" setup "$@"
}

# in_repo <setup toml>: a repo whose committed setup file is the argument, as
# the current directory.
in_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo"
  write_setup "$1"
}

@test "the fixture is accepted and printed as compact JSON" {
  in_repo "$(cat "$FIXTURE")"
  stub_run
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
  [ "$output" = "$PRINTED" ]
}

@test "no setup file dies 2 with what to do" {
  repo=$(make_repo)
  cd "$repo"
  git rm -q .config/shipit.toml
  git commit -q -m "drop setup"
  stub_run
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at .config/shipit.toml (see docs/setup.md)" ]
}

@test "the JSON setup file of earlier versions is not read" {
  repo=$(make_repo)
  cd "$repo"
  git rm -q .config/shipit.toml
  mkdir -p .config
  echo '{"version": 1}' >.config/shipit.json
  git add .config/shipit.json
  git commit -q -m "setup in the old place"
  stub_run
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at .config/shipit.toml (see docs/setup.md)" ]
}

@test "text that is not TOML is refused with exit 2 and the toml message" {
  in_repo 'cap ='
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: .config/shipit.toml is not valid TOML: "* ]]
  [[ $stderr == *"cap ="* ]]
}

@test "several broken rules give several lines, each with its path" {
  in_repo "$(fixture_with 'gate.max_files=0' 'gate.base=""' 'agents.roles.maker.effort="max"' 'packs.model=3')"
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *$'\n'".gate.max_files: "* ]]
  [[ $stderr == *$'\n'".gate.base: "* ]]
  [[ $stderr == *$'\n'".agents.roles.maker.effort: "* ]]
  [[ $stderr == *$'\n'".packs.model: "* ]]
  [ "$(printf '%s\n' "$stderr" | wc -l)" -eq 5 ]
}

@test "from a subfolder, the top-level file is used" {
  repo=$(make_repo)
  mkdir -p "$repo/a/b"
  cd "$repo/a/b"
  stub_run
  [ "$status" -eq 0 ]
  [ "$output" = "$PRINTED" ]
}

@test "from a subfolder, a setup file there is ignored" {
  repo=$(make_repo)
  mkdir -p "$repo/a/.config"
  echo 'not = "the setup"' >"$repo/a/.config/shipit.toml"
  cd "$repo/a"
  stub_run
  [ "$status" -eq 0 ]
}

@test "load_setup reads the committed file, never the working copy" {
  repo=$(make_repo)
  cd "$repo"
  echo 'not = "the setup"' >.config/shipit.toml
  stub_run
  [ "$status" -eq 0 ]
  [ "$output" = "$PRINTED" ]
  stub_run HEAD
  [ "$status" -eq 0 ]
  git checkout -q -- .config/shipit.toml
  git checkout -q -b other
  write_setup 'not = "the setup"'
  stub_run main
  [ "$status" -eq 0 ]
  [ "$output" = "$PRINTED" ]
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: .config/shipit.toml is invalid:"* ]]
}

@test "load_setup <ref> names the ref in its messages" {
  repo=$(make_repo)
  cd "$repo"
  git checkout -q -b other
  write_setup 'version = 2'
  stub_run other
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: other:.config/shipit.toml is invalid:"* ]]
  stub_run nonesuch
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at nonesuch:.config/shipit.toml (see docs/setup.md)" ]
}

@test "load_setup <ref> reports text that is not TOML with the ref" {
  repo=$(make_repo)
  cd "$repo"
  git checkout -q -b other
  write_setup 'cap ='
  stub_run other
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: other:.config/shipit.toml is not valid TOML: "* ]]
}

@test "outside a repository dies 2" {
  cd "$BATS_TEST_TMPDIR"
  GIT_CEILING_DIRECTORIES="$BATS_TEST_TMPDIR" run --separate-stderr "$SHIPIT_BIN" setup
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: not inside a git repository"* ]]
}

@test "an empty ref is a usage error, not HEAD" {
  in_repo "$(cat "$FIXTURE")"
  stub_run ""
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: empty ref; usage: shipit setup [ref]" ]
  [ -z "$output" ]
}
