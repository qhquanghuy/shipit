---
name: shipit-slice
description: Use when one slice of a roadmap is to be made, from build plan to a pull request that is ready to ship.
argument-hint: [slice]
---
# shipit-slice
Takes one slice from its build plan to a pull request that is ready to ship. The lead runs it once for each slice; so does the owner in a slice session, or the slice agent (`shipit:slice`) that the lead starts. Terms: [vocabulary](../../docs/vocab.md).
## Inputs
- The slice (the argument) and its goal: from `shipit status`.
- The roadmap, the slice's entry in it, the goal and the build plan if it exists: from the store (Store files).
- The setup (`.config/shipit.toml`) and the domain pack, for ranks, caps and what a check is.
- The machine facts and notes: `shipit doctor`.
## Output
The pull request body, which has no form: `shipit status --summary <slice>`, then the merged review's Reading path with its diagrams, Other files and Look here, then the slice's Verified, Inferred and Unverifiable lists.
## Effects
Writes the build plan into the store with `mkdir -p` and `cp`, drafting in a temporary folder that it makes itself with `mktemp -d /tmp/shipit.XXXXXX`; runs `shipit validate`, `shipit receipt`, `shipit start`, `shipit check`, `shipit log` and `shipit doctor`; starts `shipit:scout`, `shipit:plan-reviewer`, `shipit:maker` and `shipit:tester`, and the reviewers through `shipit-review`; works in a worktree (`git worktree add`) and commits on the slice branch (`git add`, `git commit`); pushes the slice branch (`git push -u origin slice/<goal>/<slice>`); opens the pull request (`gh pr create`); sends one message to the lead.
Questions to the owner use the words of the [guide](../../docs/guide.md) and its glossary, and ask only what changes what is built.
Never: merge, tag, push another branch, change the setup, or review its own work as the lead. As a slice agent: ask the owner directly.
## Outcomes
- **ready to ship**: `shipit status` says so, the branch is pushed and the pull request is open. Next: one message to the lead (as a slice agent, its final message); the owner reads the pull request and ships; after the merge the lead runs `shipit log <slice> shipped --merge <commit>`.
- **blocked**: only the owner can settle a question. Next: a decision point for the owner; message the lead "blocked" (as a slice agent, end with the decision point in the final message); the lead asks the owner and starts a new slice agent.
- **fix rounds used up**: past `agents.max_fix_rounds` a finding is still open. Next: the lead rules and records it with `shipit log <slice> note`; each open must-fix becomes `shipit log <slice> known-issue`; then ready to ship, with known issues.
- **no pull request host**: `gh` is missing or has no host for this repository. Next: the branch is pushed; tell the owner where it is; the owner opens the pull request.
## Steps
1. Build plan. The planner fills `build-plan` as a draft in a temporary folder that you make yourself with `mktemp -d /tmp/shipit.XXXXXX`, never asking the owner for one (`shipit template build-plan`); check it with `shipit validate build-plan <draft>`. Start `shipit:plan-reviewer` at the top tier, at the planner's effort, to fill `plan-review` (`shipit template plan-review`); run fix rounds; copy the approved version into the store (Store files); record `shipit receipt plan-review <slice>`.
   - Background facts: start `shipit:scout` (`shipit template scout-report`). On `partial` or `not-found` ask once more with a narrower question and say where it looked (`agents.roles.scout.retries`); then use a scout at `agents.roles.scout.escalate_to`; then read the source yourself. Log each step with `shipit log <slice> note`.
   - Owner questions: each is a `decision-point` (`shipit template decision-point`) with a ```` ```example ```` block where it helps the owner decide (a command, a sample output, two lines of the change); check it with `shipit validate decision-point <file>`.
2. Only now, with the approved build plan in the store, run `shipit start <slice>`; it refuses at the cap or before the slices it comes `after` have shipped: report and stop. Make the worktree with `git worktree add ../wt/<slice> slice/<goal>/<slice>`. The slice session is named by `shipit session-name <goal> <slice>`.
3. Make: start `shipit:maker` one build-plan step at a time, following the skill pack's make phase (`packs.skill`), each with the model and effort of `agents.roles.maker`; no review between steps. Use what the machine has; for a missing tool follow the recipe of the machine notes (`shipit doctor` shows it); never install globally. Commit the coverage map. Each maker report ends with Verified, Inferred and Unverifiable.
   - A bug fix: the build plan names a test that reproduces the bug as its first step, and `shipit:maker` commits that test alone, failing, before any fix; the fix comes in later commits.
   - The lead checks it before the reviews: in a scratch worktree of the test's commit (`git worktree add --detach <dir> <commit>`) the named test fails, and at the slice's last commit it passes; both runs go under Verified, and a missing test, or one that passes before the fix, is a must-fix finding.
4. Run `shipit check` until it passes. This is not a fix round. Never remove, skip or loosen a test to make a check pass; a changed test goes to the reviewers with its reason.
5. Run `shipit-review` in slice mode: the full review (reader, informed review, second opinion) for every slice in v1. If the roadmap marks a test drive: prepare the folder as the domain pack says, start `shipit:tester` with `shipit-test-drive` and the line `shipit-branch: slice/<goal>/<slice>`, check the report with `shipit validate test-drive <file>`, and record `shipit receipt test-drive <slice>`. The test drive runs after the reviews approve, because its receipt is keyed by the tree.
6. Fix rounds, at most `agents.max_fix_rounds`: fix, run `shipit check`, review again (receipts are keyed by the change). If the roadmap marks a test drive, run it again after the reviews approve: its receipt is keyed by the tree, so each fix round needs a new one, at most `agents.max_fix_rounds` times. Log each owner answer with `shipit log <slice> answer`. Past the cap the lead rules and writes known issues.
7. When `shipit status` says "ready to ship": push the branch with `git push -u origin slice/<goal>/<slice>`, alone in its call, the branch typed out in full: no variable, no `&&`, no command substitution. Write the body to a file in a temporary folder made with `mktemp -d /tmp/shipit.XXXXXX`, outside the repository: `shipit status --summary <slice>`, then the merged review's Reading path, diagrams, Other files and Look here, then Verified (each done-when item with the test the coverage record shows ran, and the check receipt), Inferred (what the owner must still check) and Unverifiable. Then run `gh pr create --title <title> --body-file <file>`. If gh is missing: stop as **no pull request host**.
8. One message to the lead, within `owner.status_update_max_lines`: ready to ship or blocked, the pull request, the number of known issues and how many claims are inferred. Message the lead session by its name (`shipit session-name <goal>`); the lead confirms with `shipit status`.
## As a slice agent
- The lead starts `shipit:slice` in the main working copy, at the planner's rank, and passes the goal, the slice and the store path. The agent runs steps 1 to 7 itself: it writes the build plan in step 1 and makes the worktree itself in step 2.
- Its makers, reader, reviewers and tester run one layer deeper, each with the model and effort of its rank. None of them can start an agent: their `tools` leave `Agent` out.
- The review gate and the agent cap count agents started by an agent (measured). One slice agent needs room for itself and its widest moment, two reviewers: 3 agents. `docs/claude-code.md` says what that means for `agents.parallel.agents`.
- It never asks the owner. A decision point ends it as **blocked**, and its final message carries the decision point; the lead asks the owner and starts a new slice agent, which continues from `shipit status`.
- Step 8 becomes its final message: the same line, ending with Verified, Inferred and Unverifiable. The lead confirms with `shipit status`.
## Store files
- The store is `<git common dir>/shipit`. Find it with `git rev-parse --git-common-dir`, run alone, and type the printed path into the next command. A skill never spells the git folder itself.
- Write: draft with the file tools in a temporary folder made with `mktemp -d /tmp/shipit.XXXXXX`, validate the draft there, then copy it in with one plain command: `mkdir -p <git common dir>/shipit/goals/<goal>/slices/<slice> && cp <draft> <git common dir>/shipit/goals/<goal>/slices/<slice>/build-plan.md`. A store file is never edited in place.
- Read: with the file tools.
- If the copy asks or is refused (a headless run refuses), stop and give the owner the exact `mkdir -p` and `cp` commands to run.
