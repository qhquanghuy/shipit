#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

FIXTURE="$SHIPIT_SRC/tests/fixtures/setup-valid.json"

# stub_run <args>: run `shipit setup <args>` in the current directory.
stub_run() {
  run --separate-stderr "$SHIPIT_BIN" setup "$@"
}

# mutated <jq filter>: the fixture changed by the filter, as compact JSON.
mutated() {
  jq -c "$1" "$FIXTURE"
}

# in_repo <setup json>: a repo whose committed setup file is the argument, as
# the current directory.
in_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo"
  write_setup "$1"
}

@test "the fixture is the helpers' valid setup" {
  [ "$(valid_setup | jq -cS .)" = "$(jq -cS . "$FIXTURE")" ]
}

@test "the fixture is accepted and printed as compact JSON" {
  in_repo "$(cat "$FIXTURE")"
  stub_run
  [ "$status" -eq 0 ]
  [ -z "$stderr" ]
  [ "$output" = "$(jq -c . "$FIXTURE")" ]
}

@test "no setup file dies 2 with what to do" {
  repo=$(make_repo)
  cd "$repo"
  git rm -q .config/shipit.json
  git commit -q -m "drop setup"
  stub_run
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at .config/shipit.json (see docs/setup.md)" ]
}

@test "an empty file is refused with exit 2" {
  in_repo ""
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *"is invalid:"$'\n'".: must hold exactly one JSON document, found 0" ]]
}

@test "malformed JSON is refused with exit 2 and the jq message" {
  in_repo '{"version": 1,'
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: .config/shipit.json is not valid JSON: "* ]]
  [[ $stderr != *"jq: error"* ]]
}

@test "several broken rules give several lines, each with its path" {
  in_repo "$(mutated '.cap = 0 | .base = "" | .roles.maker.effort = "max" | .packs.model = 3')"
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *$'\n'".cap: "* ]]
  [[ $stderr == *$'\n'".base: "* ]]
  [[ $stderr == *$'\n'".roles.maker.effort: "* ]]
  [[ $stderr == *$'\n'".packs.model: "* ]]
  [ "$(printf '%s\n' "$stderr" | wc -l)" -eq 5 ]
}

@test "from a subfolder, the top-level file is used" {
  repo=$(make_repo)
  mkdir -p "$repo/a/b"
  cd "$repo/a/b"
  stub_run
  [ "$status" -eq 0 ]
  [ "$output" = "$(jq -c . "$FIXTURE")" ]
}

@test "from a subfolder, a setup file there is ignored" {
  repo=$(make_repo)
  mkdir -p "$repo/a/.config"
  echo '{}' >"$repo/a/.config/shipit.json"
  cd "$repo/a"
  stub_run
  [ "$status" -eq 0 ]
}

@test "load_setup reads the committed file, never the working copy" {
  repo=$(make_repo)
  cd "$repo"
  echo '{"not": "the setup"}' >.config/shipit.json
  stub_run
  [ "$status" -eq 0 ]
  [ "$output" = "$(jq -c . "$FIXTURE")" ]
  stub_run HEAD
  [ "$status" -eq 0 ]
  git checkout -q -- .config/shipit.json
  git checkout -q -b other
  write_setup '{"not": "the setup"}'
  stub_run main
  [ "$status" -eq 0 ]
  [ "$output" = "$(jq -c . "$FIXTURE")" ]
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: .config/shipit.json is invalid:"* ]]
}

@test "load_setup <ref> names the ref in its messages" {
  repo=$(make_repo)
  cd "$repo"
  git checkout -q -b other
  write_setup '{"version": 2}'
  stub_run other
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: other:.config/shipit.json is invalid:"* ]]
  stub_run nonesuch
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at nonesuch:.config/shipit.json (see docs/setup.md)" ]
}

@test "load_setup <ref> reports malformed JSON with the ref" {
  repo=$(make_repo)
  cd "$repo"
  git checkout -q -b other
  write_setup '{'
  stub_run other
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: other:.config/shipit.json is not valid JSON: "* ]]
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

