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

# The valid setup file: the fixture the unit tests read too.
FIXTURE="$SHIPIT_SRC/tests/fixtures/setup-valid.toml"

valid_setup() {
  cat "$FIXTURE"
}

# fixture_with <path=value>...: the fixture with these values replaced, as TOML.
# A path is the table and the key, dotted: `gate.max_files=100`, `gate.environment.prefix=["env"]`,
# `agents.roles.reader.tier=2`. Each key must already be a one-line `key = value` in the
# fixture; an edit that matches no line fails.
fixture_with() {
  local edits
  edits=$(printf '%s\n' "$@")
  EDITS=$edits awk '
    BEGIN {
      n = split(ENVIRON["EDITS"], edit, "\n")
      for (i = 1; i <= n; i++) {
        if (edit[i] == "") continue
        eq = index(edit[i], "=")
        path = substr(edit[i], 1, eq - 1)
        dot = 0
        for (j = length(path); j > 0; j--) if (substr(path, j, 1) == ".") { dot = j; break }
        table[i] = dot ? substr(path, 1, dot - 1) : ""
        key[i] = substr(path, dot + 1)
        value[i] = substr(edit[i], eq + 1)
        used[i] = 0
      }
    }
    /^\[/ { current = $0; gsub(/^\[+|\]+$/, "", current) }
    {
      for (i = 1; i <= n; i++) {
        if (edit[i] != "" && table[i] == current && index($0, key[i] " = ") == 1) {
          print key[i] " = " value[i]
          used[i] = 1
          next
        }
      }
      print
    }
    END {
      for (i = 1; i <= n; i++) {
        if (edit[i] != "" && !used[i]) {
          print "fixture_with: no line for " edit[i] > "/dev/stderr"
          exit 1
        }
      }
    }
  ' "$FIXTURE"
}

# make_repo: print the path of a new temp git repo on branch main. Its first
# commit holds a valid .config/shipit.toml.
make_repo() {
  local repo
  repo=$(mktemp -d "${BATS_TEST_TMPDIR:?run under bats}/repo.XXXXXX")
  git -C "$repo" init -q -b main
  git -C "$repo" config user.name "ShipIt Test"
  git -C "$repo" config user.email "test@example.invalid"
  git -C "$repo" config commit.gpgsign false
  mkdir -p "$repo/.config"
  valid_setup >"$repo/.config/shipit.toml"
  git -C "$repo" add .config/shipit.toml
  git -C "$repo" commit -q -m "chore: setup file"
  printf '%s\n' "$repo"
}

# write_setup <toml>: write the setup file at the top of the current repo and
# commit it.
write_setup() {
  local top
  top=$(git rev-parse --show-toplevel)
  mkdir -p "$top/.config"
  printf '%s\n' "$1" >"$top/.config/shipit.toml"
  git -C "$top" add .config/shipit.toml
  git -C "$top" commit -q --allow-empty -m "chore: change setup file"
}

# run_shipit <args>: run the binary at SHIPIT_BIN under bats `run`.
run_shipit() {
  run "$SHIPIT_BIN" "$@"
}
