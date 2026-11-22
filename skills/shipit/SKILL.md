---
name: shipit
description: Use when the owner starts or resumes work with ShipIt, asks what to do next, or asks for model advice. Shows where the project stands and names the skill to run next.
---
# shipit
Shows the owner where the project stands and routes to the skill to run next. The owner or the lead runs it. It explains by pointing to the [guide](../../docs/guide.md) and the [vocabulary](../../docs/vocab.md), and restates neither.
## Inputs
- Where each goal and slice stands: `shipit status --all`.
- The setup: `.config/shipit.toml` at HEAD.
- The guide and the vocabulary, read only as far as the owner's question needs.
## Output
One `decision-point`: `shipit template decision-point`. What: what can happen next. Context: the status lines. Recommendation: the skill to run and its input.
## Effects
Reads only: `shipit status`, `shipit help`, `shipit doctor`, and git. Starts `shipit:model-scout` for model advice. Drafts its own decision point, or the model advice, in a temporary folder outside the repository, to validate it there.
Never: change a project file.
## Outcomes
- **waiting-for-the-owner**: the decision point is written and valid. Next: the owner picks; the skill it names runs.
- **not a ShipIt project**: HEAD has no `.config/shipit.toml`. Next: the lead asks for adoption as a decision point, as the README says, then offers model advice.
- **model advice ready**: the model advice is written and valid. Next: the owner picks; the lead writes the values into the setup as a setup-only pull request.
- **tool missing**: `shipit` is not on the PATH. Next: the lead follows the install lines of [Claude Code](../../docs/claude-code.md).
- **a slice was merged**: the owner says a slice is merged, or its branch is in the base. Next: the lead runs `shipit log <slice> shipped --merge <commit>`.
- **goal done**: `shipit status` shows every slice and proof step of the goal shipped. Next: the owner runs `shipit complete <goal>` (it lists what it keeps and deletes; `--confirm` is the owner's yes).
## Steps
1. Run `shipit status --all` alone in its Bash call. If missing: stop as **tool missing**.
2. Check the setup is there: run `git show HEAD:.config/shipit.toml`. If it is not, stop as **not a ShipIt project**: write a `decision-point` for adoption (the README, "Adopt ShipIt in a project") with a ```` ```example ```` block of the first command, then offer model advice (step 4).
3. Route by the status lines. Each names the next step. No goal, or a roadmap not approved: `shipit-roadmap`. A slice to make, from its build plan to its pull request: `shipit-slice`. A change to review, a pull request or a branch: `shipit-review`. The test drive runs inside `shipit-slice`; `shipit-test-drive` is the tester's skill. A merged slice or a done goal: the outcomes above.
4. Model advice, when the owner asks and at adoption: start `shipit:model-scout` at the planner's tier and effort, with the setup's tiers and roles and what the owner cares about. It fills `model-advice` (`shipit template model-advice`) with a source link for each choice and ends its report with Verified, Inferred and Unverifiable. Check it with `shipit validate model-advice <file>`. If missing: check it by hand against the form that `shipit template model-advice` prints. The owner picks; the lead writes the values into the setup as a setup-only pull request, where `shipit receipt` then checks the ranks. The tool never goes online.
5. Write the answer as a `decision-point` with the label `[project · slice · phase]` and a ```` ```example ```` block where it helps the owner decide (a command, a sample status line); check it with `shipit validate decision-point <file>`. Keep it within `owner.max_lines`, the example within `owner.example_max_lines`. For a status message to the owner, stay within `owner.status_update_max_lines`.
6. Use what the machine has. If a tool is missing, tell the owner the recipe of the machine notes (`shipit doctor` shows it); never install globally.
