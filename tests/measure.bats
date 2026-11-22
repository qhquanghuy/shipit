#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# clone_function: a function of 50 lines, over 200 tokens (twice the clone tool's minimum).
clone_function() {
  cat <<'RUST'
pub fn tally(values: &[i64], limit: i64) -> i64 {
    let mut total = 0;
    let mut count = 0;
    let mut high = 0;
    let mut low = 0;
    for value in values {
        if *value > limit {
            total += value * 2;
            high += 1;
        } else if *value < 0 {
            total -= value;
            low += 1;
        } else if *value == 0 {
            count += 1;
            continue;
        } else {
            total += value;
        }
        count += 1;
        if count > 1000 {
            break;
        }
    }
    if count > 0 {
        total += high * 3;
        total -= low * 2;
    }
    let mut result = total;
    if result > 1000 {
        result = 1000;
    }
    if result < -1000 {
        result = -1000;
    }
    if high > low {
        result += 1;
    }
    let mut spread = 0;
    for value in values.iter().rev() {
        if *value > limit {
            spread += 1;
        } else if *value < 0 {
            spread -= 1;
        }
    }
    if spread != 0 {
        result += spread;
    }
    result
}
RUST
}

# clone_repo: a repo on main with the same function in src/one.rs and src/two.rs and an unrelated
# src/three.rs; the current directory, on branch slice/demo/alpha.
clone_repo() {
  make_slice_repo
  mkdir src
  clone_function >src/one.rs
  clone_function >src/two.rs
  printf 'pub fn other() -> i64 {\n    1\n}\n' >src/three.rs
  git add -A
  git commit -q -m "add the sources"
  git checkout -q -b slice/demo/alpha
}

# commit_all <message>
commit_all() {
  git add -A
  git commit -q -m "$1"
}

measure() {
  run --separate-stderr "$SHIPIT_BIN" measure "$@"
}

@test "measure: a clone touching the slice exits 1 with its places" {
  clone_repo
  printf 'src/one.rs\nsrc/two.rs\n' >"$BATS_TEST_TMPDIR/list"
  run pmd cpd --language rust --minimum-tokens 100 --format csv --file-list "$BATS_TEST_TMPDIR/list"
  [ "$status" -eq 4 ]
  [ "$(printf '%s\n' "$output" | wc -l)" -eq 2 ]
  [ "$(printf '%s\n' "$output" | sed -n 2p | cut -d, -f2)" -ge 200 ]
  printf '// touched\n' >>src/one.rs
  commit_all "touch one"
  measure clones
  [ "$status" -eq 1 ]
  [ -z "$output" ]
  [ "$stderr" = "shipit: 1 clones touch the slice:
  src/one.rs:1-50, src/two.rs:1-50" ]
}

@test "measure: a branch changing a third file has no clone touching it" {
  clone_repo
  printf '// touched\n' >>src/three.rs
  commit_all "touch three"
  measure clones
  [ "$status" -eq 0 ]
  [ "$output" = "clones: none touch the slice" ]
}

@test "measure: clone-ratio and cognitive-complexity print one number each" {
  clone_repo
  printf '// touched\n' >>src/one.rs
  commit_all "touch one"
  measure clone-ratio
  [ "$status" -eq 0 ]
  [ "$output" = "98.04" ]
  measure cognitive-complexity
  [ "$status" -eq 0 ]
  [[ $output =~ ^[0-9]+$ ]]
  [ "$output" -gt 0 ]
  git checkout -q main
  measure clone-ratio
  [ "$output" = "0.00" ]
  measure cognitive-complexity
  [ "$output" = "0" ]
}

@test "measure: a marked block is printed with its reason" {
  clone_repo
  printf '// touched\n' >>src/one.rs
  commit_all "touch one"
  measure clone-ratio
  local before=$output
  { printf '// CPD-OFF: shared on purpose\n'; clone_function; printf '// CPD-ON\n'; } >src/one.rs
  commit_all "mark the block"
  measure clones
  [ "$status" -eq 0 ]
  [ "$output" = "hidden from the clone check: src/one.rs:1-52: shared on purpose
clones: none touch the slice" ]
  measure clone-ratio
  [ "$status" -eq 0 ]
  awk -v a="$output" -v b="$before" 'BEGIN { exit !(a >= b) }'
}

@test "measure: a CPD-OFF without a reason exits 1" {
  clone_repo
  { printf '// CPD-OFF\n'; clone_function; printf '// CPD-ON\n'; } >src/one.rs
  commit_all "mark the block"
  measure clones
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: CPD-OFF markers are broken:
  src/one.rs:1: CPD-OFF needs a reason: // CPD-OFF: <why>" ]
  measure clone-ratio
  [ "$status" -eq 1 ]
}

@test "measure: a CPD-OFF without CPD-ON exits 1" {
  clone_repo
  { printf '// CPD-OFF: forever\n'; clone_function; } >src/one.rs
  commit_all "mark the rest of the file"
  measure clones
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: CPD-OFF markers are broken:
  src/one.rs:1: CPD-OFF has no matching CPD-ON" ]
}

@test "measure: PATH without pmd exits 2" {
  clone_repo
  printf '// touched\n' >>src/one.rs
  commit_all "touch one"
  mkdir "$BATS_TEST_TMPDIR/bin"
  ln -s "$(command -v git)" "$BATS_TEST_TMPDIR/bin/git"
  PATH="$BATS_TEST_TMPDIR/bin" measure clones
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: cannot run pmd: "* ]]
  PATH="$BATS_TEST_TMPDIR/bin" measure cognitive-complexity
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: cannot run lizard: "* ]]
}

@test "measure: merge-base prints the commit the branch left the base at" {
  clone_repo
  printf '// touched\n' >>src/one.rs
  commit_all "touch one"
  measure merge-base
  [ "$status" -eq 0 ]
  [ "$output" = "$(git rev-parse main)" ]
}

@test "measure: uncommitted work exits 3" {
  clone_repo
  printf '// touched\n' >>src/one.rs
  measure clones
  [ "$status" -eq 3 ]
  [ "$stderr" = "shipit: uncommitted changes: commit first, a receipt is per commit content" ]
}

@test "measure: a missing setup stops it" {
  clone_repo
  git rm -q .config/shipit.toml
  commit_all "drop setup"
  measure clones
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: no setup file at .config/shipit.toml (see docs/setup.md)" ]
}

@test "measure: an unknown word is a usage error" {
  clone_repo
  measure everything
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: usage: shipit measure <clones|clone-ratio|cognitive-complexity|merge-base>" ]
}
