#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

# lint_repo: a repo as the current directory, holding the vocabulary and a clean skill.
lint_repo() {
  local repo
  repo=$(make_repo)
  cd "$repo"
  mkdir -p docs skills/a
  printf '# Vocabulary\n- Not framework terms: task, unit, release (GitHub'"'"'s).\n' >docs/vocab.md
  printf 'A clean line.\n' >skills/a/SKILL.md
  git add -A
  git commit -q -m "add skills"
}

@test "lint: a finding exits 1 with path and line" {
  lint_repo
  printf 'One.\nTwo tasks are here.\n' >skills/a/SKILL.md
  mkdir -p data
  printf 'Fine.\n\nThe plan is here, and a `task` is not.\nA release.\n' >data/b.md
  git add -A
  git commit -q -m "add findings"
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 1 ]
  [ -z "$output" ]
  [ "$stderr" = 'data/b.md:3: bare plan: plan
data/b.md:4: rejected term: release
skills/a/SKILL.md:2: rejected term: tasks
shipit: lint found 3 problems; put names of tools in a code span or reword' ]
}

@test "lint: a clean tree says how many files it read" {
  lint_repo
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 0 ]
  [ "$output" = 'lint: 1 files clean' ]
}

@test "lint: only what is committed is read" {
  lint_repo
  printf 'A task.\n' >skills/a/SKILL.md
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 0 ]
  printf 'A task.\n' >skills/a/NEW.md
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 0 ]
}

@test "lint: a harness name is allowed where harness/claude-code.txt says so" {
  lint_repo
  printf 'Run Claude Code.\n' >skills/a/SKILL.md
  git commit -q -am "name the harness"
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 1 ]
  [[ $stderr == "skills/a/SKILL.md:1: harness name: claude code"* ]]
  mkdir harness
  printf '# globs\nskills/*/SKILL.md\n' >harness/claude-code.txt
  git add -A
  git commit -q -m "list the harness files"
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 0 ]
}

@test "lint: a vocabulary without the line, or none, exits 2" {
  lint_repo
  printf '# Vocabulary\n' >docs/vocab.md
  git commit -q -am "drop the line"
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: docs/vocab.md has no "Not framework terms" line' ]
  git rm -q docs/vocab.md
  git commit -q -m "drop the vocabulary"
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no docs/vocab.md at HEAD' ]
}

@test "lint: a missing setup stops it" {
  lint_repo
  git rm -q .config/shipit.toml
  git commit -q -m "drop setup"
  run --separate-stderr "$SHIPIT_BIN" lint
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: no setup file at .config/shipit.toml (see docs/setup.md)' ]
}

@test "lint: an argument exits 2" {
  lint_repo
  run --separate-stderr "$SHIPIT_BIN" lint now
  [ "$status" -eq 2 ]
  [ "$stderr" = 'shipit: usage: shipit lint' ]
}
