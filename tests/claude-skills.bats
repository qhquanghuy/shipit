#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load claude-helpers

SKILLS=$SHIPIT_SRC/skills

# fill <text>: the placeholders of a skill replaced by sample values.
fill() {
  sed -e 's/<goal>/v1/g' -e 's/<slice>/demo/g' -e 's/<n>/1/g' -e 's/<base>/main/g' \
    -e 's|<branch>|slice/v1/demo|g' -e 's|<file>|notes/a.md|g' -e 's|<path>|src/a.rs|g' \
    -e 's/<name>/sample/g' -e 's/<[^>]*>/x/g' <<<"$1"
}

@test "claude skills: the plugin lists the five skills" {
  command -v claude >/dev/null || skip "claude is not on the PATH"
  local name
  run claude plugin validate "$SHIPIT_SRC"
  [ "$status" -eq 0 ]
  for name in shipit shipit-roadmap shipit-slice shipit-test-drive shipit-review; do
    grep -qF "skills: shipit:$name" <<<"$output" || { echo "not listed: $name"; return 1; }
  done
}


# forbidden_text <skills dir>: one line for each thing a skill must never say, in a code span or out
# of one, with any whitespace between the words: a merge, a skipped hook, or a push that is not
# exactly the slice push (the form, then the end of the span, a comma, a stop or the end of the line; no option between git and push).
forbidden_text() {
  local dir=$1 pattern pushes slice_pushes file
  for pattern in 'git[[:space:]]+merge' '--no-verify' 'gh[[:space:]]+pr[[:space:]]+merge'; do
    grep -rnE -- "$pattern" "$dir" | sed "s/^/forbidden: /" || true
  done
  for file in "$dir"/*/SKILL.md; do
    pushes=$(grep -oE 'git[[:space:]]+([^[:space:]`]+[[:space:]]+)*push([^[:alnum:]]|$)' "$file" | wc -l)
    slice_pushes=$(grep -oE 'git push -u origin slice/<goal>/<slice>([`,;)]|\.?$|\.[[:space:]`)])' "$file" | wc -l)
    [[ $pushes -eq $slice_pushes ]] || echo "a push other than the slice push in $file"
  done
}

@test "claude skills: no skill merges, skips a hook, or pushes anything but the slice branch" {
  run forbidden_text "$SKILLS"
  [ "$output" = "" ]
  local text broken n=0
  for text in 'git merge main' 'git   merge main' 'a --no-verify push' 'gh pr merge 1' 'gh  pr  merge 1' \
    'git push origin main' 'git push -u origin main' 'git push -u origin slice/<goal>/<slice> --force' \
    'git push -u origin slice/<goal>/<slice> main' 'git  push -u origin slice/<goal>/<slice>' \
    'git push -u origin slice/<goal>/<slice>: now' 'then git push' 'git push -u origin slice/<goal>/<slice>.main' 'git -C . push origin main' 'git -C . push -u origin slice/<goal>/<slice>'; do
    n=$((n + 1))
    broken=$BATS_TEST_TMPDIR/broken-$n
    mkdir -p "$broken"
    cp -r "$SKILLS"/* "$broken"/
    printf '\nOne more line: %s.\n' "$text" >>"$broken/shipit-review/SKILL.md"
    run forbidden_text "$broken"
    [ -n "$output" ] || { echo "not found: $text"; return 1; }
  done
}

# allow_rules <doc>: the Bash(...) rules of the allow list under "Permissions the skills need".
allow_rules() {
  awk '/^#+ Permissions the skills need$/ { on = 1; next } /^#+ / { on = 0 } on' "$1" |
    grep -oE '"Bash\([^"]*\)"' | sed -e 's/^"Bash(//' -e 's/)"$//'
}

# ask_prefixes <doc>: the commands the same part says still ask: the code spans of each "- Asks:" line.
ask_prefixes() {
  awk '/^#+ Permissions the skills need$/ { on = 1; next } /^#+ / { on = 0 } on && /^- Asks:/' "$1" |
    grep -oE '`[^`]*`' | tr -d '`'
}

# needed_commands <skills dir>: every command a skill names in a code span that the harness asks about:
# shipit, git, gh, mkdir, cp, mktemp and printf, one per line, a compound split at && and at a pipe,
# placeholders filled.
needed_commands() {
  local file span part
  for file in "$1"/*/SKILL.md; do
    grep -oE '`(shipit [a-z]|git |gh |mkdir -p |cp |mktemp |printf )[^`]*`' "$file" | tr -d '`' | while IFS= read -r span; do
      while IFS= read -r part; do
        printf '%s\n' "$(fill "$part")"
      done < <(sed -e 's/ && /\n/g' -e 's/ | /\n/g' <<<"$span")
    done
  done
}

# covered <command> <rules file>: succeeds when an allow rule matches the command.
covered() {
  local command=$1 rule prefix
  while IFS= read -r rule; do
    if [[ $rule == *' *' ]]; then
      prefix=${rule% \*}
      [[ $command == "$prefix" || $command == "$prefix "* ]] && return 0
    else
      # shellcheck disable=SC2053
      [[ $command == $rule ]] && return 0
    fi
  done <"$2"
  return 1
}

# uncovered_commands <skills dir> <doc>: one line for each command a skill names that no rule allows
# and the doc does not list as asking, and one for each asking entry that no skill names any more.
uncovered_commands() {
  local command prefix commands rules=$BATS_TEST_TMPDIR/rules.$$ asks=$BATS_TEST_TMPDIR/asks.$$
  allow_rules "$2" >"$rules"
  ask_prefixes "$2" >"$asks"
  [[ -s $rules ]] || { echo "no allow rules in $2"; return 0; }
  commands=$(needed_commands "$1" | sort -u)
  while IFS= read -r command; do
    covered "$command" "$rules" && continue
    while IFS= read -r prefix; do
      [[ $command == "$prefix" || $command == "$prefix "* ]] && continue 2
    done <"$asks"
    echo "no allow rule for: $command"
  done <<<"$commands"
  while IFS= read -r prefix; do
    [[ -n $prefix ]] || continue
    grep -qxE "$prefix( .*)?" <<<"$commands" || echo "stale asks entry: $prefix"
  done <"$asks"
}

@test "claude skills: every command a skill runs is allowed or listed as asking in the docs, and both lists are the same" {
  local doc=$SHIPIT_SRC/docs/claude-code.md broken=$BATS_TEST_TMPDIR/docs/claude-code.md
  run uncovered_commands "$SKILLS" "$doc"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  diff <(allow_rules "$SHIPIT_SRC/README.md") <(allow_rules "$doc")
  diff <(ask_prefixes "$SHIPIT_SRC/README.md") <(ask_prefixes "$doc")
  [ -n "$(allow_rules "$SHIPIT_SRC/README.md")" ]
  [ -n "$(ask_prefixes "$SHIPIT_SRC/README.md")" ]
  mkdir -p "$BATS_TEST_TMPDIR/docs"
  grep -v '"Bash(gh pr create \*)"' "$doc" >"$broken"
  run uncovered_commands "$SKILLS" "$broken"
  [[ $output == *"no allow rule for: gh pr create"* ]]
  grep -v '"Bash(shipit \*)"' "$doc" >"$broken"
  run uncovered_commands "$SKILLS" "$broken"
  [[ $output == *"no allow rule for: shipit "* ]]
  grep -v '"Bash(git push' "$doc" >"$broken"
  run uncovered_commands "$SKILLS" "$broken"
  [[ $output == *"no allow rule for: git push -u origin slice/v1/demo"* ]]
  grep -v '^- Asks: the review' "$doc" >"$broken"
  run uncovered_commands "$SKILLS" "$broken"
  [[ $output == *"no allow rule for: printf"* ]]
  sed 's/its `printf`/its `printf` and `xyzzy`/' "$doc" >"$broken"
  run uncovered_commands "$SKILLS" "$broken"
  [[ $output == *"stale asks entry: xyzzy"* ]]
  sed 's/"Bash(sha256sum \*)",//' "$doc" >"$broken"
  run uncovered_commands "$SKILLS" "$broken"
  [[ $output == *"no allow rule for: sha256sum"* ]]
}
