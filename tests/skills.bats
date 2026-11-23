#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

# These tests read skills/ and what a skill says about itself. A skill is prose, so they check its
# shape (the function parts), what each part must name, and that what it names exists. Whether an
# agent follows a skill well is not tested here. Each check prints one line for each thing wrong and
# returns 1; the tests also break a copy of the plugin and expect the named line.

SKILL_NAMES=(shipit shipit-roadmap shipit-slice shipit-test-drive shipit-review)

# copy_plugin <dest>: the parts of the plugin a skill reads or links to, to break.
copy_plugin() {
  local part
  mkdir -p "$1"
  for part in skills docs agents data README.md; do
    [[ -e $SHIPIT_SRC/$part ]] && cp -r "$SHIPIT_SRC/$part" "$1/$part"
  done
  return 0
}

# five_skills <root>: fails unless exactly the five skill folders exist, each with a SKILL.md.
five_skills() {
  local root=$1 name entry bad=0
  for name in "${SKILL_NAMES[@]}"; do
    [[ -f $root/skills/$name/SKILL.md ]] || { echo "missing skill: $name"; bad=1; }
  done
  if [[ -d $root/skills ]]; then
    for entry in "$root"/skills/*; do
      [[ -e $entry ]] || continue
      name=${entry##*/}
      [[ " ${SKILL_NAMES[*]} " == *" $name "* ]] || { echo "unexpected entry in skills: $name"; bad=1; }
    done
  fi
  return "$bad"
}

# frontmatter <file>: the lines between the opening and closing ---; fails when it does not open
# or close.
frontmatter() {
  awk 'NR == 1 { if ($0 != "---") exit 1; next } $0 == "---" { closed = 1; exit } { print } END { exit closed ? 0 : 1 }' "$1"
}

# section <file> <name>: the lines under "## <name>", up to the next "## " heading.
section() {
  awk -v heading="## $2" '$0 == heading { on = 1; next } /^## / { on = 0 } on' "$1"
}

# step_line <file> <number>: the numbered Steps line that starts "<number>. ".
step_line() {
  section "$1" Steps | grep -E "^$2\\. " || true
}

# skill_file <root> <name>
skill_file() {
  printf '%s\n' "$1/skills/$2/SKILL.md"
}

check_frontmatter() {
  local root=$1 name file front desc bad=0
  five_skills "$root" || return 1
  for name in "${SKILL_NAMES[@]}"; do
    file=$(skill_file "$root" "$name")
    front=$(frontmatter "$file") || { echo "$name: frontmatter does not open and close with ---"; bad=1; continue; }
    [[ $(grep '^name: ' <<<"$front" || true) == "name: $name" ]] || { echo "$name: name is not the folder name"; bad=1; }
    desc=$(grep '^description: ' <<<"$front" | sed 's/^description: //' || true)
    [[ -n $desc ]] || { echo "$name: no description"; bad=1; }
    if [[ $desc == *'<'* || $desc == *'>'* ]]; then echo "$name: angle bracket in the description"; bad=1; fi
    if grep -qE '^(model|effort|allowed-tools|context|disable-model-invocation):' <<<"$front"; then
      echo "$name: a field a skill must not set"
      bad=1
    fi
  done
  return "$bad"
}

check_function_parts() {
  local root=$1 name file head60 heading count at prev bad=0
  five_skills "$root" || return 1
  for name in "${SKILL_NAMES[@]}"; do
    file=$(skill_file "$root" "$name")
    head60=$(head -60 "$file")
    prev=0
    for heading in Inputs Output Effects Outcomes Steps; do
      count=$(grep -c "^## $heading\$" <<<"$head60" || true)
      if [[ $count -ne 1 ]]; then
        echo "$name: ## $heading appears $count times in the first 60 lines"
        bad=1
        continue
      fi
      at=$(grep -n "^## $heading\$" <<<"$head60" | cut -d: -f1)
      [[ $at -gt $prev ]] || { echo "$name: ## $heading is out of order"; bad=1; }
      prev=$at
    done
    section "$file" Effects | grep -q '^Never:' || { echo "$name: Effects has no line starting Never:"; bad=1; }
  done
  return "$bad"
}

check_outputs() {
  local root=$1 name file out kinds kind outcomes outcome outcome_section bad=0
  five_skills "$root" || return 1
  for name in "${SKILL_NAMES[@]}"; do
    file=$(skill_file "$root" "$name")
    out=$(section "$file" Output)
    if [[ $name == shipit-slice ]]; then
      grep -q 'shipit status --summary' <<<"$out" || { echo "$name: Output does not name shipit status --summary"; bad=1; }
      continue
    fi
    kinds=$(grep -oE 'shipit template [a-z-]+' <<<"$out" | sort -u || true)
    if [[ -z $kinds || $(wc -l <<<"$kinds") -ne 1 ]]; then
      echo "$name: Output does not name exactly one shipit template <kind>"
      bad=1
      continue
    fi
    kind=${kinds#shipit template }
    outcomes=$("$SHIPIT_BIN" template "$kind" 2>/dev/null | sed -n 's/^outcome = "\(.*\)"$/\1/p') || true
    [[ -n $outcomes ]] || { echo "$name: shipit template $kind prints no form"; bad=1; continue; }
    outcome_section=$(section "$file" Outcomes)
    while IFS= read -r outcome; do
      outcome=$(sed 's/^ *//; s/ *$//' <<<"$outcome")
      grep -qF -- "- **$outcome**:" <<<"$outcome_section" || { echo "$name: no outcome item for $outcome"; bad=1; }
    done < <(tr '|' '\n' <<<"$outcomes")
  done
  return "$bad"
}

check_outcomes_say_next() {
  local root=$1 name file line items bad=0
  five_skills "$root" || return 1
  for name in "${SKILL_NAMES[@]}"; do
    file=$(skill_file "$root" "$name")
    items=0
    while IFS= read -r line; do
      [[ -n $line ]] || continue
      items=$((items + 1))
      [[ $line =~ ^-\ \*\*[^*]+\*\*:\ .*Next:\  ]] || { echo "$name: outcome without Next: $line"; bad=1; }
    done < <(section "$file" Outcomes)
    [[ $items -ge 1 ]] || { echo "$name: no outcome"; bad=1; }
  done
  return "$bad"
}

check_if_missing() {
  local root=$1 file line count=0 bad=0
  five_skills "$root" || return 1
  file=$(skill_file "$root" shipit-review)
  while IFS= read -r line; do
    [[ $line == *'`shipit '* ]] || continue
    count=$((count + 1))
    [[ $line == *'If missing:'* ]] || { echo "shipit-review: a tool step without If missing: ${line:0:60}"; bad=1; }
  done < <(section "$file" Steps | grep -E '^[0-9]+[a-z]?\. ' || true)
  [[ $count -ge 3 ]] || { echo "shipit-review: fewer than three tool steps"; bad=1; }
  return "$bad"
}

check_advice_and_machine() {
  local root=$1 name bad=0
  five_skills "$root" || return 1
  grep -qF 'shipit template model-advice' "$(skill_file "$root" shipit)" || { echo "shipit: no shipit template model-advice"; bad=1; }
  for name in shipit-slice shipit-test-drive; do
    grep -qF 'shipit doctor' "$(skill_file "$root" "$name")" || { echo "$name: no shipit doctor"; bad=1; }
  done
  return "$bad"
}

check_example_blocks() {
  local root=$1 name file line count bad=0
  five_skills "$root" || return 1
  for name in shipit shipit-roadmap shipit-slice shipit-review; do
    file=$(skill_file "$root" "$name")
    count=0
    while IFS= read -r line; do
      [[ $line == *'decision-point'* ]] || continue
      count=$((count + 1))
      [[ $line == *'```example'* ]] || { echo "$name: a step names decision-point without the example fence: ${line:0:60}"; bad=1; }
    done < <(section "$file" Steps)
    [[ $count -ge 1 ]] || { echo "$name: no step names decision-point"; bad=1; }
  done
  return "$bad"
}

check_links() {
  local root=$1 name file link total=0 bad=0
  five_skills "$root" || return 1
  for name in "${SKILL_NAMES[@]}"; do
    file=$(skill_file "$root" "$name")
    while IFS= read -r link; do
      [[ -n $link ]] || continue
      total=$((total + 1))
      link=${link%.}
      [[ -e $(dirname "$file")/$link ]] || { echo "$name: link to a missing file: $link"; bad=1; }
    done < <(grep -oE '\.\./\.\./[A-Za-z0-9_./-]*' "$file" || true)
  done
  [[ $total -ge 1 ]] || { echo "no relative link in any skill"; bad=1; }
  return "$bad"
}

check_roles_have_files() {
  local root=$1 name file span role total=0 bad=0
  five_skills "$root" || return 1
  for name in "${SKILL_NAMES[@]}"; do
    file=$(skill_file "$root" "$name")
    while IFS= read -r span; do
      [[ -n $span ]] || continue
      role=${span#\`shipit:}
      role=${role%\`}
      [[ $role == shipit* ]] && continue
      total=$((total + 1))
      [[ -f $root/agents/$role.md ]] || { echo "$name: role without an agent file: $role"; bad=1; }
    done < <(grep -oE '`shipit:[a-z-]+`' "$file" || true)
  done
  [[ $total -ge 1 ]] || { echo "no role in any skill"; bad=1; }
  return "$bad"
}

check_named_roles() {
  local root=$1 name file line plain spans total=0 bad=0
  five_skills "$root" || return 1
  for name in "${SKILL_NAMES[@]}"; do
    file=$(skill_file "$root" "$name")
    while IFS= read -r line; do
      plain=$(sed 's/`[^`]*`//g' <<<"$line")
      grep -qiE '(^|[^a-z])start' <<<"$plain" || continue
      grep -qiE 'agent|maker|reader|reviewer|tester|scout' <<<"$plain" || continue
      total=$((total + 1))
      if [[ $name == shipit-review && $line == *'plain agents'* && $line == *'../../agents/work-reviewer.md'* ]]; then
        continue
      fi
      spans=$(grep -oE '`shipit:[a-z-]+`' <<<"$line" | grep -v '^`shipit:shipit' || true)
      [[ -n $spans ]] || { echo "$name: a start line without a named role: ${line:0:70}"; bad=1; }
    done < <(section "$file" Steps)
  done
  [[ $total -ge 1 ]] || { echo "no start line in any skill"; bad=1; }
  return "$bad"
}

check_no_store_path() {
  local root=$1 bad=0
  five_skills "$root" || return 1
  if grep -rnF '.git/' "$root/skills"; then bad=1; fi
  if grep -rnF '$(' "$root/skills"; then bad=1; fi
  return "$bad"
}

check_docs_name_skills() {
  local root=$1 word bad=0
  five_skills "$root" || return 1
  # shellcheck disable=SC2046
  for word in $(grep -ohE 'shipit-[a-z]+(-[a-z]+)*' "$root/README.md" "$root"/docs/*.md | sort -u); do
    [[ $word == shipit-branch || -d $root/skills/$word ]] || { echo "a doc names a skill that does not exist: $word"; bad=1; }
  done
  return "$bad"
}

check_claim_lists() {
  local root=$1 name file line found bad=0
  five_skills "$root" || return 1
  for name in shipit-review shipit-slice shipit-roadmap shipit-test-drive; do
    file=$(skill_file "$root" "$name")
    found=$(grep -E 'Verified' "$file" | grep -E 'Inferred' | grep -cE 'Unverifiable' || true)
    [[ $found -ge 1 ]] || { echo "$name: no line names Verified, Inferred and Unverifiable"; bad=1; }
  done
  file=$(skill_file "$root" shipit-slice)
  line=$(section "$file" Steps | grep -F -- '--body-file' || true)
  if [[ $line != *Verified* || $line != *Inferred* || $line != *Unverifiable* ]]; then
    echo "shipit-slice: the step that writes the PR body does not name all three lists"
    bad=1
  fi
  return "$bad"
}

check_changed_tests_first() {
  local root=$1 file line word bad=0
  five_skills "$root" || return 1
  file=$(skill_file "$root" shipit-review)
  section "$file" Steps | grep -E '^[0-9]+[a-z]?\. ' | grep -F -- '--name-status' | grep -qF 'removes or loosens' ||
    { echo "shipit-review: no step lists the changed tests (--name-status, removes or loosens)"; bad=1; }
  line=$(step_line "$file" 4a)
  for word in 'Look here' 'changed test' 'weakens'; do
    [[ $line == *"$word"* ]] || { echo "shipit-review: line 4a does not name $word"; bad=1; }
  done
  return "$bad"
}

check_key_claims() {
  local root=$1 file line word bad=0
  five_skills "$root" || return 1
  file=$(skill_file "$root" shipit-review)
  line=$(step_line "$file" 4a)
  for word in 'key claims' 'test each'; do
    [[ $line == *"$word"* ]] || { echo "shipit-review: line 4a does not name $word"; bad=1; }
  done
  return "$bad"
}

check_context_and_blind() {
  local root=$1 file a b word bad=0
  five_skills "$root" || return 1
  file=$(skill_file "$root" shipit-review)
  a=$(step_line "$file" 4a)
  b=$(step_line "$file" 4b)
  [[ -n $a && -n $b ]] || { echo "shipit-review: a line 4a. or 4b. is missing"; return 1; }
  for word in '--comments' 'git log -5' 'callers'; do
    [[ $a == *"$word"* ]] || { echo "shipit-review: line 4a does not name $word"; bad=1; }
    [[ $b != *"$word"* ]] || { echo "shipit-review: line 4b names $word"; bad=1; }
  done
  frontmatter "$file" | grep '^argument-hint:' | grep -qF 'focus' || { echo "shipit-review: argument-hint does not name focus"; bad=1; }
  return "$bad"
}

check_bug_fix() {
  local root=$1 file n next bad=0
  five_skills "$root" || return 1
  file=$(skill_file "$root" shipit-slice)
  n=$(grep -n 'bug fix' "$file" | grep -F 'failing' | grep -F 'before any fix' | head -1 | cut -d: -f1 || true)
  [[ -n $n ]] || { echo "shipit-slice: no line names bug fix, failing and before any fix"; return 1; }
  next=$(sed -n "$((n + 1))p" "$file")
  [[ $next == *'git worktree add'* ]] || { echo "shipit-slice: the line after the bug fix line does not name git worktree add"; bad=1; }
  return "$bad"
}

check_short() {
  local root=$1 name lines bad=0
  five_skills "$root" || return 1
  for name in "${SKILL_NAMES[@]}"; do
    lines=$(wc -l <"$(skill_file "$root" "$name")")
    [[ $lines -le 200 ]] || { echo "$name: $lines lines, over 200"; bad=1; }
  done
  return "$bad"
}

@test "skills: the five skills exist, each named by its folder" {
  run check_frontmatter "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i 's/^name: shipit-slice$/name: shipit-slices/' "$BATS_TEST_TMPDIR/copy/skills/shipit-slice/SKILL.md"
  run check_frontmatter "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-slice: name is not the folder name"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy2"
  sed -i '0,/^description: /s/^description: /description: <b> /' "$BATS_TEST_TMPDIR/copy2/skills/shipit/SKILL.md"
  run check_frontmatter "$BATS_TEST_TMPDIR/copy2"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit: angle bracket in the description"* ]]
}

@test "skills: each skill states its inputs, output, effects and outcomes" {
  run check_function_parts "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i 's/^## Effects$/## Allowed/' "$BATS_TEST_TMPDIR/copy/skills/shipit-roadmap/SKILL.md"
  run check_function_parts "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-roadmap: ## Effects appears 0 times"* ]]
}

@test "skills: each output is a form the tool prints, and each of its outcomes has a line" {
  run check_outputs "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i 's/^- \*\*pass\*\*:/- **passed**:/' "$BATS_TEST_TMPDIR/copy/skills/shipit-test-drive/SKILL.md"
  run check_outputs "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-test-drive: no outcome item for pass"* ]]
}

@test "skills: every outcome says what follows" {
  run check_outcomes_say_next "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i '/^## Outcomes$/,/^## Steps$/s/ Next: / Then: /' "$BATS_TEST_TMPDIR/copy/skills/shipit/SKILL.md"
  run check_outcomes_say_next "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit: outcome without Next:"* ]]
}

@test "skills: every tool step of shipit-review says what to do without the tool" {
  run check_if_missing "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i '/^5\. /s/If missing:/If absent:/' "$BATS_TEST_TMPDIR/copy/skills/shipit-review/SKILL.md"
  run check_if_missing "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-review: a tool step without If missing:"* ]]
}

@test "skills: model advice and machine facts have their steps" {
  run check_advice_and_machine "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i 's/shipit doctor/shipit status/g' "$BATS_TEST_TMPDIR/copy/skills/shipit-test-drive/SKILL.md"
  run check_advice_and_machine "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-test-drive: no shipit doctor"* ]]
}

@test "skills: every step that writes an owner message asks for an example block" {
  run check_example_blocks "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i '/^## Steps$/,$s/```example//' "$BATS_TEST_TMPDIR/copy/skills/shipit-slice/SKILL.md"
  run check_example_blocks "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-slice: a step names decision-point without the example fence"* ]]
}

@test "skills: every relative link names a file of the plugin" {
  run check_links "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  printf '\nSee ../../docs/nothing.md.\n' >>"$BATS_TEST_TMPDIR/copy/skills/shipit/SKILL.md"
  run check_links "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit: link to a missing file: ../../docs/nothing.md"* ]]
}

@test "skills: every role a skill starts has an agent file" {
  run check_roles_have_files "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  printf '\nStart `shipit:nobody`.\n' >>"$BATS_TEST_TMPDIR/copy/skills/shipit/SKILL.md"
  run check_roles_have_files "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit: role without an agent file: nobody"* ]]
}

@test "skills: every agent a skill starts is a named role" {
  run check_named_roles "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i '/^## Steps$/,$s/Start `shipit:reader`/Start a reader/' "$BATS_TEST_TMPDIR/copy/skills/shipit-review/SKILL.md"
  run check_named_roles "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-review: a start line without a named role"* ]]
}

@test "skills: no skill names a path under .git/ or a command substitution" {
  run check_no_store_path "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  printf '\ncp x .git/shipit/y\n' >>"$BATS_TEST_TMPDIR/copy/skills/shipit/SKILL.md"
  printf '\necho $(date)\n' >>"$BATS_TEST_TMPDIR/copy/skills/shipit-slice/SKILL.md"
  run check_no_store_path "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *".git/shipit/y"* ]]
  [[ $output == *'$(date)'* ]]
}

@test "skills: the docs name only skills that exist" {
  run check_docs_name_skills "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  printf '\nRun shipit-nothing.\n' >>"$BATS_TEST_TMPDIR/copy/README.md"
  run check_docs_name_skills "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"a doc names a skill that does not exist: shipit-nothing"* ]]
}

@test "skills: every report and review splits verified, inferred and unverifiable claims" {
  run check_claim_lists "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i 's/Unverifiable/Unknown/g' "$BATS_TEST_TMPDIR/copy/skills/shipit-test-drive/SKILL.md"
  run check_claim_lists "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-test-drive: no line names Verified, Inferred and Unverifiable"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy2"
  sed -i '/--body-file/s/Inferred/Guessed/g' "$BATS_TEST_TMPDIR/copy2/skills/shipit-slice/SKILL.md"
  run check_claim_lists "$BATS_TEST_TMPDIR/copy2"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-slice: the step that writes the PR body does not name all three lists"* ]]
}

@test "skills: the review puts changed tests first in Look here" {
  run check_changed_tests_first "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i '/^4a\. /s/weakens/changes/' "$BATS_TEST_TMPDIR/copy/skills/shipit-review/SKILL.md"
  run check_changed_tests_first "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-review: line 4a does not name weakens"* ]]
}

@test "skills: the informed reviewer tests the build plan's key claims" {
  run check_key_claims "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i '/^4a\. /s/key claims/main claims/' "$BATS_TEST_TMPDIR/copy/skills/shipit-review/SKILL.md"
  run check_key_claims "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-review: line 4a does not name key claims"* ]]
}

@test "skills: outside a slice the informed reviewer gathers context and the second opinion stays blind" {
  run check_context_and_blind "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i '/^4b\. /s/$/ Read --comments too./' "$BATS_TEST_TMPDIR/copy/skills/shipit-review/SKILL.md"
  run check_context_and_blind "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-review: line 4b names --comments"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy2"
  sed -i 's/^argument-hint: .*$/argument-hint: [pr number]/' "$BATS_TEST_TMPDIR/copy2/skills/shipit-review/SKILL.md"
  run check_context_and_blind "$BATS_TEST_TMPDIR/copy2"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-review: argument-hint does not name focus"* ]]
}

@test "skills: a bug fix starts with a failing test committed before the fix" {
  run check_bug_fix "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i 's/before any fix/before the fix/' "$BATS_TEST_TMPDIR/copy/skills/shipit-slice/SKILL.md"
  run check_bug_fix "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-slice: no line names bug fix, failing and before any fix"* ]]
}

@test "skills: each skill stays short" {
  run check_short "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  seq 1 201 >>"$BATS_TEST_TMPDIR/copy/skills/shipit/SKILL.md"
  run check_short "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit: "*" lines, over 200"* ]]
}


# check_start_order: in shipit-slice, on every line of the file, only a numbered step names
# `shipit start`, and it comes after the line that records the build plan's review. The roadmap
# skill never names it, on any line.
check_start_order() {
  local root=$1 file body receipt line n bad=0
  five_skills "$root" || return 1
  file=$(skill_file "$root" shipit-slice)
  body=$(cat "$file")
  receipt=$(grep -n '`shipit receipt plan-review' <<<"$body" | head -1 | cut -d: -f1 || true)
  [[ -n $receipt ]] || { echo "shipit-slice: no line records the build plan's review"; return 1; }
  while IFS=: read -r n line; do
    [[ $line =~ ^[0-9]+\.\  ]] || { echo "shipit-slice: a line that is not a numbered step names shipit start: ${line:0:70}"; bad=1; continue; }
    [[ $n -gt $receipt ]] || { echo "shipit-slice: shipit start comes before the build plan review"; bad=1; }
  done < <(grep -nE 'shipit[[:space:]]+start' <<<"$body" || true)
  grep -qE 'shipit[[:space:]]+start' <<<"$body" || { echo "shipit-slice: no step runs shipit start"; bad=1; }
  if grep -nE 'shipit[[:space:]]+start' "$(skill_file "$root" shipit-roadmap)"; then
    echo "shipit-roadmap: a line names shipit start"
    bad=1
  fi
  return "$bad"
}

@test "skills: the build plan is reviewed before shipit start, and the roadmap skill never names it" {
  run check_start_order "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i '/^1\. /s/^1\. /1. Run `shipit start <slice>`. /' "$BATS_TEST_TMPDIR/copy/skills/shipit-slice/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-slice: shipit start comes before the build plan review"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy2"
  sed -i 's/^- Its makers, reader/- The lead runs `shipit start <slice>` first. Its makers, reader/' "$BATS_TEST_TMPDIR/copy2/skills/shipit-slice/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy2"
  [ "$status" -eq 1 ]
  [[ $output == *"not a numbered step names shipit start"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy3"
  sed -i 's/^Never: write code/Never: run `shipit start`, write code/' "$BATS_TEST_TMPDIR/copy3/skills/shipit-roadmap/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy3"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-roadmap: a line names shipit start"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy4"
  sed -i 's/^| one agent per slice | `shipit:slice`, started by the lead for each slice |/| one agent per slice | `shipit:slice`, started by the lead, which runs `shipit start` |/' "$BATS_TEST_TMPDIR/copy4/skills/shipit-roadmap/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy4"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-roadmap: a line names shipit start"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy5"
  sed -i '/^- \*\*blocked\*\*/s/^- \*\*blocked\*\*: /- **blocked**: run `shipit start <slice>` first. /' "$BATS_TEST_TMPDIR/copy5/skills/shipit-slice/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy5"
  [ "$status" -eq 1 ]
  [[ $output == *"not a numbered step names shipit start"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy6"
  sed -i '/^Writes the build plan/s/runs `shipit validate`/runs `shipit start`, `shipit validate`/' "$BATS_TEST_TMPDIR/copy6/skills/shipit-slice/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy6"
  [ "$status" -eq 1 ]
  [[ $output == *"not a numbered step names shipit start"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy7"
  sed -i '/^- \*\*blocked\*\*/s/^- \*\*blocked\*\*: /- **blocked**: run shipit start first. /' "$BATS_TEST_TMPDIR/copy7/skills/shipit-slice/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy7"
  [ "$status" -eq 1 ]
  [[ $output == *"not a numbered step names shipit start"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy8"
  sed -i '/^- \*\*blocked\*\*/s/^- \*\*blocked\*\*: /- **blocked**: run `shipit  start` first. /' "$BATS_TEST_TMPDIR/copy8/skills/shipit-slice/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy8"
  [ "$status" -eq 1 ]
  [[ $output == *"not a numbered step names shipit start"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy9"
  sed -i 's/^Never: write code/Never: write code, run shipit start/' "$BATS_TEST_TMPDIR/copy9/skills/shipit-roadmap/SKILL.md"
  run check_start_order "$BATS_TEST_TMPDIR/copy9"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-roadmap: a line names shipit start"* ]]
}

# check_store_writes: a build plan, a plan review and a review reach the store through
# `shipit store put`; only a goal or roadmap version, the owner's, is copied with `cp`.
check_store_writes() {
  local root=$1 slice review file span target bad=0
  five_skills "$root" || return 1
  slice=$(skill_file "$root" shipit-slice)
  review=$(skill_file "$root" shipit-review)
  grep -qF '`shipit store put build-plan' "$slice" || { echo "shipit-slice: no shipit store put of the build plan"; bad=1; }
  grep -qF '`shipit store put plan-review' "$slice" || { echo "shipit-slice: no shipit store put of the plan review"; bad=1; }
  grep -F '`shipit store put <kind>' "$review" | grep -qF -- '--round' || { echo "shipit-review: no shipit store put of a review with --round"; bad=1; }
  for file in "$root"/skills/*/SKILL.md; do
    if grep -nE 'shipit store put (roadmap|goal)' "$file"; then
      echo "${file#"$root"/}: a goal or roadmap version is put with shipit store put"
      bad=1
    fi
    while IFS= read -r span; do
      [[ $span == *'<git common dir>/shipit/goals/'* ]] || continue
      target=${span##*/}
      target=${target%'`'}
      [[ $target =~ ^(goal|roadmap)\.v\<N\>\.md$ ]] || { echo "${file#"$root"/}: cp into the store of $target, which is not a goal or roadmap version"; bad=1; }
    done < <(grep -oE '`[^`]*cp [^`]*`' "$file" || true)
  done
  return "$bad"
}

@test "skills: every store write but a goal or roadmap version is a shipit store put" {
  run check_store_writes "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  printf '\n`cp x <git common dir>/shipit/goals/<goal>/slices/<slice>/build-plan.md`\n' >>"$BATS_TEST_TMPDIR/copy/skills/shipit-slice/SKILL.md"
  run check_store_writes "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"build-plan.md"* ]]
}

# check_lead_leaves_claims: step 7 of shipit-review says the review lead never edits a reviewer's
# claims and sends the report back.
check_lead_leaves_claims() {
  local line bad=0
  line=$(step_line "$(skill_file "$1" shipit-review)" 7)
  [[ $line == *"never edits a reviewer's claims"* ]] || { echo "shipit-review: step 7 does not say the lead never edits a reviewer's claims"; bad=1; }
  [[ $line == *"sends the report back"* ]] || { echo "shipit-review: step 7 does not say the lead sends the report back"; bad=1; }
  return "$bad"
}

@test "skills: the review lead never edits a reviewer's claims" {
  run check_lead_leaves_claims "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  sed -i "/^7\. /s/ The review lead never edits a reviewer's claims: it sends the report back to that reviewer, or records its own disagreement beside it\.//" "$BATS_TEST_TMPDIR/copy/skills/shipit-review/SKILL.md"
  run check_lead_leaves_claims "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"never edits a reviewer's claims"* ]]
}

# check_drafts_in_project <root>: no skill or doc names a /tmp/shipit folder, and each skill that
# drafts, or runs a test drive, names its folder inside the project, `.shipit/td/`.
check_drafts_in_project() {
  local root=$1 skill folder bad=0
  five_skills "$root" || return 1
  if grep -rnE 'tmp/shipit' "$root/README.md" "$root/docs" "$root/skills"; then
    echo "a skill or doc names a folder under /tmp/shipit"
    bad=1
  fi
  for skill in "${SKILL_NAMES[@]}"; do
    folder='.shipit/td/'
    [[ $skill == shipit-test-drive ]] && folder='.shipit/drive/'
    grep -qF "$folder" "$(skill_file "$root" "$skill")" || { echo "$skill: no folder under $folder"; bad=1; }
  done
  return "$bad"
}

@test "skills: every drafts and test-drive folder is inside the project, under .shipit/td" {
  run check_drafts_in_project "$SHIPIT_SRC"
  [ "$output" = "" ]
  [ "$status" -eq 0 ]
  copy_plugin "$BATS_TEST_TMPDIR/copy"
  printf '\nMake it with `mktemp -d /tmp/shipit.XXXXXX`.\n' >>"$BATS_TEST_TMPDIR/copy/skills/shipit-slice/SKILL.md"
  run check_drafts_in_project "$BATS_TEST_TMPDIR/copy"
  [ "$status" -eq 1 ]
  [[ $output == *"under /tmp/shipit"* ]]
  copy_plugin "$BATS_TEST_TMPDIR/copy2"
  sed -i 's|\.shipit/drive/|.shipit/x/|g' "$BATS_TEST_TMPDIR/copy2/skills/shipit-test-drive/SKILL.md"
  run check_drafts_in_project "$BATS_TEST_TMPDIR/copy2"
  [ "$status" -eq 1 ]
  [[ $output == *"shipit-test-drive: no folder under .shipit/drive/"* ]]
}

@test "skills: an owner's answer is logged with the lead's model and effort, and names the owner in --text" {
  local line
  line=$(step_line "$SHIPIT_SRC/skills/shipit-slice/SKILL.md" 6)
  [[ $line == *'--by <model>:<effort>'* ]]
  [[ $line == *'--text'* ]]
  [[ $line == *'`--by owner` is refused'* ]]
}

@test "skills: the test drive has its own folder, apart from the drafts folder of the slice" {
  local slice drive drafts
  slice=$SHIPIT_SRC/skills/shipit-slice/SKILL.md
  drive=$(grep -oE '\.shipit/(td|drive)/<slice>' "$SHIPIT_SRC/skills/shipit-test-drive/SKILL.md" | sort -u)
  drafts=$(grep -oE '\.shipit/(td|drive)/<slice>' "$slice" | sort -u)
  [ "$drive" = ".shipit/drive/<slice>" ]
  [ "$drafts" = ".shipit/td/<slice>" ]
  [ "$drive" != "$drafts" ]
}

@test "skills: the owner-answer command of shipit-slice runs against the binary" {
  local span cmd
  make_slice_repo
  make_work "slice/demo/beta" 1
  check_here
  rec second-opinion beta --author sonnet:high --by opus:high --result approve \
    --finding "question: F1 text"
  [ "$status" -eq 0 ]
  span=$(step_line "$SHIPIT_SRC/skills/shipit-slice/SKILL.md" 6 | grep -oE '`shipit log <slice> answer[^`]*`' | tr -d '`')
  [ -n "$span" ]
  cmd=$(sed -e 's/<slice>/beta/' -e 's/<Fn>/F1/' -e 's/<model>:<effort>/sonnet:high/' \
    -e 's/<kind>/second-opinion/' -e 's/<answer>/because/' <<<"$span")
  [[ $cmd != *"<"* ]]
  read -ra words <<<"$cmd"
  run --separate-stderr "$SHIPIT_BIN" "${words[@]:1}"
  [ "$status" -eq 0 ]
  [ "$output" = "logged answer for beta" ]
}

@test "skills: the store is read with shipit store show" {
  local file skill
  for file in "$SHIPIT_SRC"/skills/*/SKILL.md "$SHIPIT_SRC"/agents/*.md; do
    run grep -nF -e 'Read: with the file tools' -e '`goal.vN.md` with the file tools' "$file"
    [ "$output" = "" ]
    [ "$status" -eq 1 ]
  done
  for skill in shipit-slice shipit-roadmap shipit-review; do
    grep -qF 'shipit store show' "$SHIPIT_SRC/skills/$skill/SKILL.md"
  done
}

@test "skills: the maker runs each command alone" {
  local maker="$SHIPIT_SRC/agents/maker.md" drive="$SHIPIT_SRC/skills/shipit-test-drive/SKILL.md" name
  grep -qF '`./test.sh`' "$maker"
  grep -qF 'No `;`, `&&`, `|`, `echo` or `$`' "$maker"
  grep -qF 'shipit store show build-plan' "$maker"
  grep -qF "split a step's \`&&\` chain into separate calls" "$maker"
  grep -qF 'test-drive.md' "$drive"
  # Claude Code refuses an agent's Write of a file named report…, summary…, findings… or analysis….
  while read -r name; do
    [[ ${name##*/} != [Rr][Ee][Pp][Oo][Rr][Tt]* ]]
    [[ ${name##*/} != [Ss][Uu][Mm][Mm][Aa][Rr][Yy]* ]]
    [[ ${name##*/} != [Ff][Ii][Nn][Dd][Ii][Nn][Gg][Ss]* ]]
    [[ ${name##*/} != [Aa][Nn][Aa][Ll][Yy][Ss][Ii][Ss]* ]]
  done < <(grep -oE '[A-Za-z0-9_./-]+\.md' "$drive")
}
