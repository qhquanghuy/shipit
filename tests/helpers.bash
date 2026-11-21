# shellcheck shell=bash
# Shared helpers for the bats tests. Load with: load helpers

SHIPIT_SRC=$(cd "$BATS_TEST_DIRNAME/.." && pwd)
export SHIPIT_SRC

# The binary under test: cargo build, or set SHIPIT_BIN.
SHIPIT_BIN="${SHIPIT_BIN:-$SHIPIT_SRC/target/debug/shipit}"
export SHIPIT_BIN
if [[ ! -x $SHIPIT_BIN ]]; then
  printf 'shipit binary not found at %s: run cargo build, or set SHIPIT_BIN\n' "$SHIPIT_BIN" >&2
  return 1
fi

# A folder standing in for HOME, so no developer git config leaks into tests.
test_home() {
  local home="${BATS_TEST_TMPDIR:?run under bats}/home"
  mkdir -p "$home"
  printf '%s\n' "$home"
}

# BATS_TEST_TMPDIR is unset while bats gathers test names.
if [[ -n ${BATS_TEST_TMPDIR:-} ]]; then
  HOME=$(test_home)
  export HOME
fi
export GIT_CONFIG_NOSYSTEM=1

# The schema example from the build plan: a valid setup file.
valid_setup() {
  cat <<'JSON'
{
  "version": 1,
  "base": "main",
  "cap": 60,
  "check": ["nix flake check --print-build-logs"],
  "environment": {"run": []},
  "fix_rounds": 2,
  "effort_cap": "high",
  "tiers": [
    {"tier": 1, "model": "haiku"},
    {"tier": 2, "model": "sonnet"},
    {"tier": 3, "model": "opus"},
    {"tier": 4, "model": "fable", "stand_in": {"model": "opus", "effort": "high"}}
  ],
  "roles": {
    "scout":  {"tier": 1, "retries": 1, "then": {"tier": 2}},
    "maker":  {"tier": 2, "effort": "high"},
    "tester": {"tier": 3, "effort": "medium"},
    "reader": {"tier": 1}
  },
  "packs": {"skill": "superpowers", "domain": "coding", "model": "claude"},
  "messages_to_owner": {"max_lines": 30, "status_update_max_lines": 10}
}
JSON
}

# make_repo: print the path of a new temp git repo on branch main. Its first
# commit holds a valid .config/shipit.json.
make_repo() {
  local repo
  repo=$(mktemp -d "${BATS_TEST_TMPDIR:?run under bats}/repo.XXXXXX")
  git -C "$repo" init -q -b main
  git -C "$repo" config user.name "ShipIt Test"
  git -C "$repo" config user.email "test@example.invalid"
  git -C "$repo" config commit.gpgsign false
  mkdir -p "$repo/.config"
  valid_setup >"$repo/.config/shipit.json"
  git -C "$repo" add .config/shipit.json
  git -C "$repo" commit -q -m "chore: setup file"
  printf '%s\n' "$repo"
}

# write_setup <json>: write the setup file at the top of the current repo and
# commit it.
write_setup() {
  local top
  top=$(git rev-parse --show-toplevel)
  mkdir -p "$top/.config"
  printf '%s\n' "$1" >"$top/.config/shipit.json"
  git -C "$top" add .config/shipit.json
  git -C "$top" commit -q --allow-empty -m "chore: change setup file"
}

# run_shipit <args>: run the binary at SHIPIT_BIN under bats `run`.
run_shipit() {
  run "$SHIPIT_BIN" "$@"
}
