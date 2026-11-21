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

# refused <jq filter> <path> [lines]: the mutated fixture is refused with exit
# 2 and an error line starting with <path>; by default that is the only error.
refused() {
  in_repo "$(mutated "$1")"
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: .config/shipit.json is invalid:"$'\n'* ]]
  [[ $stderr == *$'\n'"$2: "* ]]
  [[ $stderr != *"jq: error"* ]]
  local count
  count=$(printf '%s\n' "$stderr" | wc -l)
  [ "$count" -eq "${3:-2}" ] || {
    printf 'expected %s lines, got:\n%s\n' "${3:-2}" "$stderr" >&2
    return 1
  }
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

@test "an empty check list is accepted" {
  in_repo "$(mutated '.check = []')"
  stub_run
  [ "$status" -eq 0 ]
}

@test "a stand_in is optional and effort is optional on roles" {
  in_repo "$(mutated 'del(.tiers[3].stand_in) | del(.roles.maker.effort)')"
  stub_run
  [ "$status" -eq 0 ]
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

@test "two JSON documents are refused with exit 2" {
  in_repo "$(jq -c . "$FIXTURE") $(jq -c . "$FIXTURE")"
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *"is invalid:"$'\n'".: must hold exactly one JSON document, found 2" ]]
}

@test "a top level that is an array is refused with exit 2" {
  in_repo "[$(jq -c . "$FIXTURE")]"
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *"is invalid:"$'\n'".: must be a JSON object" ]]
}

@test "a top level that is a string or null is refused with exit 2" {
  in_repo '"x"'
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *".: must be a JSON object" ]]
  in_repo 'null'
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *".: must be a JSON object" ]]
}

@test "malformed JSON is refused with exit 2 and the jq message" {
  in_repo '{"version": 1,'
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: .config/shipit.json is not valid JSON: "* ]]
  [[ $stderr != *"jq: error"* ]]
}

@test "unknown key at the top level" {
  refused '.extra = 1' '.extra'
}

@test "unknown key inside roles" {
  refused '.roles.boss = {"tier": 1}' '.roles.boss'
}

@test "unknown key inside roles.maker" {
  refused '.roles.maker.retries = 1' '.roles.maker.retries'
}

@test "unknown key inside roles.scout" {
  refused '.roles.scout.speed = 1' '.roles.scout.speed'
}

@test "unknown key inside roles.scout.then" {
  refused '.roles.scout.then.effort = "low"' '.roles.scout.then.effort'
}

@test "unknown key inside a tier" {
  refused '.tiers[0].color = "red"' '.tiers[0].color'
}

@test "unknown key inside a stand_in" {
  refused '.tiers[3].stand_in.extra = 1' '.tiers[3].stand_in.extra'
}

@test "unknown key inside packs" {
  refused '.packs.extra = "x"' '.packs.extra'
}

@test "a missing top-level key" {
  refused 'del(.cap)' '.cap'
}

@test "version must be 1" {
  refused '.version = 2' '.version'
}

@test "base must not be empty" {
  refused '.base = ""' '.base'
}

@test "cap must be at least 1" {
  refused '.cap = 0' '.cap'
}

@test "cap must be an integer" {
  refused '.cap = 1.5' '.cap'
}

@test "check entries must not be empty" {
  refused '.check = ["ok", ""]' '.check[1]'
}

@test "fix_rounds must be at least 0" {
  refused '.fix_rounds = -1' '.fix_rounds'
}

@test "effort_cap must be on the scale" {
  refused '.effort_cap = "extreme"' '.effort_cap'
}

@test "tiers must not be empty" {
  refused '.tiers = []' '.tiers' 7
}

@test "tiers numbered 1,3 are refused" {
  refused '.tiers = [{"tier": 1, "model": "haiku"}, {"tier": 3, "model": "sonnet"}] | .roles.tester.tier = 1' '.tiers[1].tier'
}

@test "a tier model must not be empty" {
  refused '.tiers[1].model = ""' '.tiers[1].model'
}

@test "a tier needs its model" {
  refused 'del(.tiers[0].model)' '.tiers[0].model'
}

@test "a stand_in needs an effort" {
  refused 'del(.tiers[3].stand_in.effort)' '.tiers[3].stand_in.effort'
}

@test "a stand_in needs a model" {
  refused 'del(.tiers[3].stand_in.model)' '.tiers[3].stand_in.model'
}

@test "a stand_in model must not be empty" {
  refused '.tiers[3].stand_in.model = ""' '.tiers[3].stand_in.model'
}

@test "a stand_in effort must be on the scale" {
  refused '.tiers[3].stand_in.effort = "ultra"' '.tiers[3].stand_in.effort'
}

@test "a stand_in effort must not be above effort_cap" {
  refused '.tiers[3].stand_in.effort = "max"' '.tiers[3].stand_in.effort'
}

@test "roles must hold scout, maker and tester" {
  refused 'del(.roles.tester)' '.roles.tester'
}

@test "a role needs its tier" {
  refused 'del(.roles.maker.tier)' '.roles.maker.tier'
}

@test "a role tier must exist" {
  refused '.roles.maker.tier = 5' '.roles.maker.tier'
}

@test "roles.maker.effort above effort_cap is refused" {
  refused '.roles.maker.effort = "xhigh"' '.roles.maker.effort'
}

@test "a role effort must be on the scale" {
  refused '.roles.tester.effort = "huge"' '.roles.tester.effort'
}

@test "a role effort equal to effort_cap is accepted" {
  in_repo "$(mutated '.roles.maker.effort = "high"')"
  stub_run
  [ "$status" -eq 0 ]
}

@test "roles.reader is required" {
  refused 'del(.roles.reader)' '.roles.reader'
}

@test "unknown key inside roles.reader" {
  refused '.roles.reader.extra = 1' '.roles.reader.extra'
}

@test "roles.reader.tier must exist" {
  refused '.roles.reader.tier = 5' '.roles.reader.tier'
}

@test "roles.reader.effort is optional, on the scale and not above effort_cap" {
  in_repo "$(mutated '.roles.reader.effort = "medium"')"
  stub_run
  [ "$status" -eq 0 ]
  refused '.roles.reader.effort = "huge"' '.roles.reader.effort'
  refused '.roles.reader.effort = "xhigh"' '.roles.reader.effort'
}

@test "a wrong-typed roles.reader" {
  refused '.roles.reader = "x"' '.roles.reader'
  refused '.roles.reader = null' '.roles.reader'
}

@test "scout.then.tier equal to scout.tier is refused" {
  refused '.roles.scout.then.tier = 1' '.roles.scout.then.tier'
}

@test "scout.then.tier below scout.tier is refused" {
  refused '.roles.scout.tier = 3 | .roles.scout.then.tier = 2' '.roles.scout.then.tier'
}

@test "scout.then.tier must exist" {
  refused '.roles.scout.then.tier = 9' '.roles.scout.then.tier'
}

@test "scout.then needs its tier" {
  refused '.roles.scout.then = {}' '.roles.scout.then.tier'
}

@test "scout needs retries" {
  refused 'del(.roles.scout.retries)' '.roles.scout.retries'
}

@test "scout needs then" {
  refused 'del(.roles.scout.then)' '.roles.scout.then'
}

@test "scout retries must be at least 0" {
  refused '.roles.scout.retries = -1' '.roles.scout.retries'
}

@test "packs needs model" {
  refused 'del(.packs.model)' '.packs.model'
}

@test "a packs value must not be empty" {
  refused '.packs.skill = ""' '.packs.skill'
}

@test "a wrong-typed version" {
  refused '.version = "1"' '.version'
}

@test "a wrong-typed base" {
  refused '.base = 5' '.base'
}

@test "a wrong-typed cap" {
  refused '.cap = "60"' '.cap'
}

@test "a wrong-typed check" {
  refused '.check = "x"' '.check'
}

@test "a wrong-typed check entry" {
  refused '.check = [5]' '.check[0]'
}

@test "a wrong-typed fix_rounds" {
  refused '.fix_rounds = "2"' '.fix_rounds'
}

@test "a wrong-typed effort_cap" {
  refused '.effort_cap = 5' '.effort_cap'
}

@test "a wrong-typed tiers" {
  refused '.tiers = "x"' '.tiers'
}

@test "a wrong-typed tiers entry" {
  refused '.tiers[1] = "x"' '.tiers[1]'
}

@test "a wrong-typed tier model" {
  refused '.tiers[0].model = 5' '.tiers[0].model'
}

@test "a wrong-typed tier number" {
  refused '.tiers[0].tier = "1"' '.tiers[0].tier'
}

@test "a wrong-typed stand_in" {
  refused '.tiers[3].stand_in = "x"' '.tiers[3].stand_in'
}

@test "a wrong-typed roles" {
  refused '.roles = "x"' '.roles'
}

@test "a wrong-typed role" {
  refused '.roles.maker = "x"' '.roles.maker'
}

@test "a wrong-typed role tier" {
  refused '.roles.maker.tier = "2"' '.roles.maker.tier'
}

@test "a wrong-typed role effort" {
  refused '.roles.maker.effort = 5' '.roles.maker.effort'
}

@test "a wrong-typed scout retries" {
  refused '.roles.scout.retries = "1"' '.roles.scout.retries'
}

@test "a wrong-typed scout then" {
  refused '.roles.scout.then = "x"' '.roles.scout.then'
}

@test "a wrong-typed scout then tier" {
  refused '.roles.scout.then.tier = "2"' '.roles.scout.then.tier'
}

@test "a wrong-typed packs" {
  refused '.packs = "x"' '.packs'
}

@test "a wrong-typed packs value" {
  refused '.packs.domain = 5' '.packs.domain'
}


@test "messages_to_owner is accepted when valid, with the update limit equal to max_lines" {
  in_repo "$(mutated '.messages_to_owner = {"max_lines": 5, "status_update_max_lines": 5}')"
  stub_run
  [ "$status" -eq 0 ]
}

@test "messages_to_owner is required" {
  refused 'del(.messages_to_owner)' '.messages_to_owner'
}

@test "messages_to_owner needs max_lines" {
  refused 'del(.messages_to_owner.max_lines)' '.messages_to_owner.max_lines'
}

@test "messages_to_owner needs status_update_max_lines" {
  refused 'del(.messages_to_owner.status_update_max_lines)' '.messages_to_owner.status_update_max_lines'
}

@test "messages_to_owner max_lines must be at least 1" {
  refused '.messages_to_owner.max_lines = 0 | .messages_to_owner.status_update_max_lines = 1' '.messages_to_owner.max_lines'
}

@test "messages_to_owner status_update_max_lines must be at least 1" {
  refused '.messages_to_owner.status_update_max_lines = 0' '.messages_to_owner.status_update_max_lines'
}

@test "messages_to_owner values must be integers" {
  refused '.messages_to_owner.max_lines = "30"' '.messages_to_owner.max_lines'
  refused '.messages_to_owner.status_update_max_lines = 2.5' '.messages_to_owner.status_update_max_lines'
}

@test "messages_to_owner status_update_max_lines must not be above max_lines" {
  refused '.messages_to_owner.status_update_max_lines = 31' '.messages_to_owner.status_update_max_lines'
}

@test "messages_to_owner refuses an unknown key" {
  refused '.messages_to_owner.extra = 1' '.messages_to_owner.extra'
}

@test "a wrong-typed messages_to_owner" {
  refused '.messages_to_owner = "x"' '.messages_to_owner'
  refused '.messages_to_owner = null' '.messages_to_owner'
}

@test "null in place of every value is reported, never a crash" {
  in_repo "$(mutated 'with_entries(.value = null)')"
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr != *"jq: error"* ]]
  [[ $stderr == *$'\n'".version: "* ]]
  [[ $stderr == *$'\n'".roles: "* ]]
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

@test "environment.run accepts a prefix and an empty list" {
  in_repo "$(mutated '.environment.run = ["nix", "develop", "--command"]')"
  stub_run
  [ "$status" -eq 0 ]
  in_repo "$(mutated '.environment.run = []')"
  stub_run
  [ "$status" -eq 0 ]
}

@test "environment is required" {
  refused 'del(.environment)' '.environment'
}

@test "environment needs run" {
  refused '.environment = {}' '.environment.run'
}

@test "environment refuses an unknown key" {
  refused '.environment.extra = 1' '.environment.extra'
}

@test "environment.run must be an array" {
  refused '.environment.run = "nix develop"' '.environment.run'
  refused '.environment.run = null' '.environment.run'
}

@test "environment.run holds only non-empty strings" {
  refused '.environment.run = ["nix", ""]' '.environment.run[1]'
  refused '.environment.run = ["nix", 3]' '.environment.run[1]'
}

@test "a wrong-typed environment" {
  refused '.environment = "x"' '.environment'
  refused '.environment = null' '.environment'
  refused '.environment = ["nix"]' '.environment'
}

@test "base must be a branch name, never a revision" {
  local bad
  for bad in 'HEAD~1' 'HEAD' '-x' 'a..b' 'v1^' 'a b'; do
    refused ".base = \"$bad\"" '.base'
  done
  [[ $stderr == *".base: must be a branch name (see git check-ref-format --branch)" ]]
}

@test "a base like feature/x is a branch name" {
  in_repo "$(mutated '.base = "feature/x"')"
  stub_run
  [ "$status" -eq 0 ]
}

@test "an empty ref is a usage error, not HEAD" {
  in_repo "$(cat "$FIXTURE")"
  stub_run ""
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: empty ref; usage: shipit setup [ref]" ]
  [ -z "$output" ]
}

@test "the largest u64 is a cap and 2^64 is not" {
  in_repo "$(sed 's/"cap": 60/"cap": 18446744073709551615/' "$FIXTURE")"
  stub_run
  [ "$status" -eq 0 ]
  [[ $output == *'"cap":18446744073709551615,'* ]]
  in_repo "$(sed 's/"cap": 60/"cap": 18446744073709551616/' "$FIXTURE")"
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *$'\n'".cap: must be an integer of at least 1" ]]
}

@test "a cap written with a fraction part at 2^64 is refused too" {
  in_repo "$(sed 's/"cap": 60/"cap": 18446744073709551616.0/' "$FIXTURE")"
  stub_run
  [ "$status" -eq 2 ]
  [[ $stderr == *$'\n'".cap: must be an integer of at least 1" ]]
}
