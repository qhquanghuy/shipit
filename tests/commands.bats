#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers

DOC="$SHIPIT_SRC/docs/commands.md"

# documented <status>: the command names the table of docs/commands.md gives that status.
documented() {
  sed -nE "s/^\| \`shipit ([a-z-]+)[^|]*\| $1 \|.*/\1/p" "$DOC" | sort
}

@test "the table in docs/commands.md has only built and planned rows" {
  local rows listed
  rows=$(grep -cE '^\| `shipit [^|]*\| [a-z]+ \| ' "$DOC")
  listed=$(($(documented built | wc -l) + $(documented planned | wc -l)))
  [ "$rows" -eq "$listed" ]
}

@test "the built rows of docs/commands.md are exactly the commands shipit help lists" {
  run_shipit help
  [ "$status" -eq 0 ]
  local helped
  helped=$(sed -nE 's/^  ([a-z-]+)$/\1/p' <<<"$output" | sort)
  [ -n "$helped" ]
  [ "$(documented built)" = "$helped" ]
}

@test "no planned row of docs/commands.md is a command shipit help lists" {
  run_shipit help
  [ "$status" -eq 0 ]
  local name
  for name in $(documented planned); do
    if grep -qx "  $name" <<<"$output"; then return 1; fi
  done
}
