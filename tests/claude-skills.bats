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
  local repo command filled total=0 pushes=0
  repo=$(shipit_repo)
  cd "$repo"
  git checkout -q -b slice/v1/demo
  while IFS= read -r command; do
    [[ -n $command ]] || continue
    total=$((total + 1))
    [[ $command == "git push"* ]] && pushes=$((pushes + 1))
    if [[ $command == "git push"* && $command != "git push -u origin slice/<goal>/<slice>" ]]; then
      echo "a push other than the slice push: $command"
      return 1
    fi
    filled=$(fill "$command")
    run_hook claude-ship-gate "$(bash_input "$repo" "$filled" bypassPermissions)"
    [ -z "$output" ] || { echo "the gate answers for: $filled"; return 1; }
  done < <(shell_commands)
  [ "$total" -ge 1 ]
  [ "$pushes" -ge 1 ]
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
