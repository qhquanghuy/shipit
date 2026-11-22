#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load claude-helpers

SKILLS=$SHIPIT_SRC/skills

# shell_commands: every code span that starts `git ` or `gh `, and every line starting git or gh
# inside a fence, in skills/. The opening of an example fence written in a span is cut first.
shell_commands() {
  local file
  for file in "$SKILLS"/*/SKILL.md; do
    [[ -f $file ]] || continue
    awk '
      /^```/ { fence = !fence; next }
      fence { if ($0 ~ /^(git|gh) /) print; next }
      {
        gsub(/```` ```[a-z]* ````/, "")
        line = $0
        while (match(line, /`[^`]+`/)) {
          span = substr(line, RSTART + 1, RLENGTH - 2)
          if (span ~ /^(git|gh) /) print span
          line = substr(line, RSTART + RLENGTH)
        }
      }' "$file"
  done
}

# fill <text>: the placeholders of a skill replaced by sample values.
fill() {
  sed -e 's/<goal>/v1/g' -e 's/<slice>/demo/g' -e 's/<n>/1/g' -e 's/<base>/main/g' \
    -e 's|<branch>|slice/v1/demo|g' -e 's|<file>|notes/a.md|g' -e 's|<path>|src/a.rs|g' \
    -e 's/<name>/sample/g' -e 's/<[^>]*>/x/g' <<<"$1"
}

@test "claude skills: every git and gh command a skill shows passes the ship gate" {
  local repo command filled total=0
  repo=$(shipit_repo)
  cd "$repo"
  git checkout -q -b slice/v1/demo
  while IFS= read -r command; do
    [[ -n $command ]] || continue
    total=$((total + 1))
    if [[ $command == "git push"* && $command != "git push -u origin slice/<goal>/<slice>" ]]; then
      echo "a push other than the slice push: $command"
      return 1
    fi
    filled=$(fill "$command")
    run_hook claude-ship-gate "$(bash_input "$repo" "$filled" bypassPermissions)"
    [ -z "$output" ] || { echo "the gate answers for: $filled"; return 1; }
  done < <(shell_commands)
  [ "$total" -ge 1 ]
}

@test "claude skills: the store copy as the skills write it passes the ship gate" {
  local repo command filled total=0
  repo=$(shipit_repo)
  cd "$repo"
  git checkout -q -b slice/v1/demo
  while IFS= read -r command; do
    [[ -n $command ]] || continue
    total=$((total + 1))
    filled=${command//<git common dir>/$repo/.git}
    filled=$(fill "$filled")
    run_hook claude-ship-gate "$(bash_input "$repo" "$filled" bypassPermissions)"
    [ -z "$output" ] || { echo "the gate answers for: $filled"; return 1; }
  done < <(grep -ohE '`mkdir -p [^`]*`' "$SKILLS"/*/SKILL.md | tr -d '`')
  [ "$total" -ge 1 ]
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


# allow_rules <doc>: the Bash(...) rules of the allow list under "Permissions the skills need".
allow_rules() {
  awk '/^#+ Permissions the skills need$/ { on = 1; next } /^#+ / { on = 0 } on' "$1" |
    grep -oE '"Bash\([^"]*\)"' | sed -e 's/^"Bash(//' -e 's/)"$//'
}

# needed_commands <skills dir>: every command a skill names in a code span that the harness asks about:
# shipit, git, gh, mkdir, cp and mktemp, one per line, a compound split at &&, placeholders filled.
needed_commands() {
  local file span part
  for file in "$1"/*/SKILL.md; do
    grep -oE '`(shipit [a-z]|git |gh |mkdir -p |cp |mktemp )[^`]*`' "$file" | tr -d '`' | while IFS= read -r span; do
      while IFS= read -r part; do
        printf '%s\n' "$(fill "$part")"
      done < <(sed 's/ && /\n/g' <<<"$span")
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

# uncovered_commands <skills dir> <doc>: one line for each command a skill names that no rule allows.
uncovered_commands() {
  local command rules=$BATS_TEST_TMPDIR/rules.$$
  allow_rules "$2" >"$rules"
  [[ -s $rules ]] || { echo "no allow rules in $2"; return 0; }
  while IFS= read -r command; do
    covered "$command" "$rules" || echo "no allow rule for: $command"
  done < <(needed_commands "$1" | sort -u)
}

@test "claude skills: every command a skill runs is covered by the allow list in the docs, and both lists are the same" {
  run uncovered_commands "$SKILLS" "$SHIPIT_SRC/docs/claude-code.md"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  diff <(allow_rules "$SHIPIT_SRC/README.md") <(allow_rules "$SHIPIT_SRC/docs/claude-code.md")
  [ -n "$(allow_rules "$SHIPIT_SRC/README.md")" ]
  mkdir -p "$BATS_TEST_TMPDIR/docs"
  grep -v '"Bash(gh pr create \*)"' "$SHIPIT_SRC/docs/claude-code.md" >"$BATS_TEST_TMPDIR/docs/claude-code.md"
  run uncovered_commands "$SKILLS" "$BATS_TEST_TMPDIR/docs/claude-code.md"
  [[ $output == *"no allow rule for: gh pr create"* ]]
  grep -v '"Bash(shipit \*)"' "$SHIPIT_SRC/docs/claude-code.md" >"$BATS_TEST_TMPDIR/docs/claude-code.md"
  run uncovered_commands "$SKILLS" "$BATS_TEST_TMPDIR/docs/claude-code.md"
  [[ $output == *"no allow rule for: shipit "* ]]
  grep -v '"Bash(git push' "$SHIPIT_SRC/docs/claude-code.md" >"$BATS_TEST_TMPDIR/docs/claude-code.md"
  run uncovered_commands "$SKILLS" "$BATS_TEST_TMPDIR/docs/claude-code.md"
  [[ $output == *"no allow rule for: git push -u origin slice/v1/demo"* ]]
  sed 's/"Bash(cp \*)"/"Bash(cp)"/' "$SHIPIT_SRC/docs/claude-code.md" >"$BATS_TEST_TMPDIR/docs/claude-code.md"
  run uncovered_commands "$SKILLS" "$BATS_TEST_TMPDIR/docs/claude-code.md"
  [[ $output == *"no allow rule for: cp "* ]]
}

# forbidden_text <skills dir>: one line for each thing a skill must never say, in a code span or out
# of one, with any whitespace between the words: a merge, a skipped hook, or a push that is not
# exactly the slice push (the form, then the end of the span, a stop or the end of the line).
forbidden_text() {
  local dir=$1 pattern pushes slice_pushes file
  for pattern in 'git[[:space:]]+merge' '--no-verify' 'gh[[:space:]]+pr[[:space:]]+merge'; do
    grep -rnE -- "$pattern" "$dir" | sed "s/^/forbidden: /" || true
  done
  for file in "$dir"/*/SKILL.md; do
    pushes=$(grep -oE 'git[[:space:]]+push' "$file" | wc -l)
    slice_pushes=$(grep -oE 'git push -u origin slice/<goal>/<slice>([`,.;:)]|$)' "$file" | wc -l)
    [[ $pushes -eq $slice_pushes ]] || echo "a push other than the slice push in $file"
  done
}

@test "claude skills: no skill merges, skips a hook, or pushes anything but the slice branch" {
  run forbidden_text "$SKILLS"
  [ "$output" = "" ]
  local text broken n=0
  for text in 'git merge main' 'git   merge main' 'a --no-verify push' 'gh pr merge 1' 'gh  pr  merge 1' \
    'git push origin main' 'git push -u origin main' 'git push -u origin slice/<goal>/<slice> --force' \
    'git push -u origin slice/<goal>/<slice> main' 'git  push -u origin slice/<goal>/<slice>'; do
    n=$((n + 1))
    broken=$BATS_TEST_TMPDIR/broken-$n
    mkdir -p "$broken"
    cp -r "$SKILLS"/* "$broken"/
    printf '\nOne more line: %s.\n' "$text" >>"$broken/shipit-review/SKILL.md"
    run forbidden_text "$broken"
    [ -n "$output" ] || { echo "not found: $text"; return 1; }
  done
}
