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
  [ "$stderr" = "shipit: 1 clone touches the slice:
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
  [ "$output" = "17" ]
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


# unreadable_source: a valid file that PMD's Rust reader stops at (a string that ends in a
# backslash, followed by another string: it skips the file and exits 5).
unreadable_source() {
  printf 'fn a() -> Vec<&'"'"'static str> {\n    vec!["~", "\\\\", ".."]\n}\n' >src/bad.rs
  printf 'src/bad.rs\n' >"$BATS_TEST_TMPDIR/bad-list"
  run pmd cpd --language rust --minimum-tokens 100 --format csv --file-list "$BATS_TEST_TMPDIR/bad-list"
  [ "$status" -eq 5 ]
}

@test "measure: a tracked file that pmd cannot read exits 2 and names it, changed or not" {
  clone_repo
  git checkout -q main
  unreadable_source
  commit_all "add a file pmd cannot read"
  git checkout -q slice/demo/alpha
  git merge -q main
  printf '// touched\n' >>src/three.rs
  commit_all "touch three"
  measure clones
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: pmd cannot read src/bad.rs; its clones cannot be checked (rewrite what it stops at)" ]
  printf '// touched\n' >>src/bad.rs
  commit_all "touch the unreadable file"
  measure clone-ratio
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: pmd cannot read src/bad.rs; its clones cannot be checked (rewrite what it stops at)" ]
}

@test "measure: a marker word anywhere but alone on its line exits 1" {
  clone_repo
  local text
  for text in 'pub fn z() -> i64 { 1 } // CPD-OFF' '// note: CPD-OFF here' $'/*\nCPD-OFF\n*/' '//CPD-OFF: why' '// CPD-ON again'; do
    git reset -q --hard
    git checkout -q main -- src/one.rs
    printf '%s\n' "$text" >>src/one.rs
    commit_all "marker form"
    measure clones
    [ "$status" -eq 1 ]
    [[ $stderr == "shipit: CPD-OFF markers are broken:
  src/one.rs:5"[0-9]*": CPD-OFF and CPD-ON must stand alone on their line: // CPD-OFF: <why> or // CPD-ON" ]]
    measure clone-ratio
    [ "$status" -eq 1 ]
  done
}

@test "measure: a copy that was reformatted prints its own lines and counts them" {
  clone_repo
  sed -i 's/^    let mut total = 0;$/    let mut total = 0;\n/' src/two.rs
  printf '// touched\n' >>src/one.rs
  commit_all "reformat one copy"
  measure clones
  [ "$status" -eq 1 ]
  [ "$stderr" = "shipit: 1 clone touches the slice:
  src/one.rs:1-50, src/two.rs:1-51" ]
}

@test "measure: inside shipit check the merge base is the one the check was given" {
  clone_repo
  printf '// touched\n' >>src/one.rs
  commit_all "touch one"
  printf '// touched\n' >>src/three.rs
  commit_all "touch three"
  measure clones
  [ "$status" -eq 1 ]
  SHIPIT_MERGE_BASE=$(git rev-parse HEAD~1) SHIPIT_CHECK=x measure clones
  [ "$status" -eq 0 ]
  [ "$output" = "clones: none touch the slice" ]
}

@test "measure: a file name that cannot be handed to pmd exits 2 and names it" {
  clone_repo
  cp src/one.rs "src/a,b.rs"
  commit_all "a comma"
  measure clones
  [ "$status" -eq 2 ]
  [ "$stderr" = "shipit: cannot measure src/a,b.rs: a .rs file name with a comma, a line break or bytes that are not UTF-8" ]
  git rm -q "src/a,b.rs"
  cp src/one.rs "$(printf 'src/o\377ne.rs')"
  commit_all "not utf-8"
  measure clones
  [ "$status" -eq 2 ]
  [[ $stderr == "shipit: cannot measure src/o"*"ne.rs: a .rs file name with a comma, a line break or bytes that are not UTF-8" ]]
}

# The clone tool never reads the markers, so a marker it would read differently from shipit (in
# a block comment, in a string) hides nothing.
@test "measure: markers inside a block comment or a string hide nothing" {
  clone_repo
  { printf '/*\n// CPD-OFF: x\n// CPD-ON\n*/\n'; clone_function; } >src/one.rs
  commit_all "markers in a block comment"
  measure clones
  [ "$status" -eq 1 ]
  [[ $stderr == *"src/one.rs:5-54, src/two.rs:1-50" ]]
  git reset -q --hard main
  { printf '// CPD-OFF: x\nconst S: &str = r"\n// CPD-ON\n";\n'; clone_function; } >src/one.rs
  commit_all "markers around a string"
  measure clones
  [ "$status" -eq 1 ]
  [[ $stderr == *"src/one.rs:5-54, src/two.rs:1-50" ]]
}

@test "measure: a new copy of a block that an unchanged file marks off is found" {
  clone_repo
  git checkout -q main
  { printf '// CPD-OFF: legacy\n'; clone_function; printf '// CPD-ON\n'; } >src/two.rs
  commit_all "mark the old copy"
  git checkout -q slice/demo/alpha
  git merge -q main
  clone_function >src/four.rs
  commit_all "copy it again"
  measure clones
  [ "$status" -eq 1 ]
  [[ $stderr == *"src/four.rs:1-50"* ]]
  [[ $stderr == *"src/two.rs:2-51"* ]]
}

@test "measure: a check variable with no merge base is an error" {
  clone_repo
  printf '// touched\n' >>src/one.rs
  commit_all "touch one"
  local value
  for value in "" nonsense; do
    SHIPIT_CHECK=x SHIPIT_MERGE_BASE=$value measure clones
    [ "$status" -eq 2 ]
    [ "$stderr" = "shipit: SHIPIT_CHECK is set but SHIPIT_MERGE_BASE is missing or not a commit id" ]
  done
  SHIPIT_CHECK=x measure clones
  [ "$status" -eq 2 ]
}
