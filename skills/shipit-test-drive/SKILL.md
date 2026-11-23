---
name: shipit-test-drive
description: Use when a slice needs its test drive: using the built result as its audience would, knowing only the goal and the done-when.
---
# shipit-test-drive
Does the test drive of one slice. The tester (`shipit:tester`) runs it, started by the lead or by `shipit-slice` once the reviews approve. Terms: [vocabulary](../../docs/vocab.md).
## Inputs
- The goal and its done-when: in the prompt, from the lead.
- A folder with only the built result, its docs and sample data: the lead prepares it as the domain pack says, at `.shipit/drive/<slice>` inside the project, apart from the drafts folder of the slice (made with `mkdir -p .shipit/drive/<slice>` and the one line `*` in its `.gitignore`), so that the tester may run the result there.
- The path of the slice's worktree, from the lead: step 6 runs `shipit validate` from there.
- The machine facts and notes: `shipit doctor`.
## Output
One `test-drive`: `shipit template test-drive`, checked with `shipit validate test-drive <file>`.
## Effects
Runs the result inside the folder, with the commands its docs give, and `shipit validate` for the report. Writes files only inside the folder. For a missing tool it follows the recipe of the machine notes.
Never: read the source, the build plan or the repository; edit anything outside the folder; install a tool globally (the plugin's install gate refuses it); start agents.
## Outcomes
- **pass**: every done-when item is `[pass]` and no finding is a must fix. Next: the lead records it with `shipit receipt test-drive <slice>`, then the slice goes on to the pull request.
- **findings**: at least one must-fix finding. Next: a fix round; the lead records the receipt with each finding as `--finding`, then the slice is checked and reviewed again.
## Steps
1. Read the goal, the done-when and the docs in the folder. Stay in the folder. A file that is missing is a finding, never a reason to read the source.
2. Use what the machine has. For a missing tool follow the recipe of the machine notes (`shipit doctor` shows them); never install globally.
3. First pass: go through every done-when item as a checklist. Do each one as a user of the result, then write `[pass] ` or `[fail] `, what you did and what you saw.
4. Second pass: explore as the audience would, with no input, wrong input and only what the docs say. Write what you tried and what happened.
5. Write each problem as a Conventional Comment: `issue (blocking): text`, `suggestion (non-blocking): text`, `question: text` or `nitpick: text`. Each says what you did, what you expected and what happened.
6. Write the report to `test-drive.md` in the folder (Claude Code refuses an agent's file named `report…`, `summary…`, `findings…` or `analysis…`). Then run `shipit validate test-drive <file>` from the slice's worktree, which is a ShipIt project, with the file's full path (the folder has no setup of its own). If missing: check it by hand against the form `shipit template test-drive` prints, and say so in Unverifiable. Fix what the tool lists, then return the text.
7. End the report with Verified, Inferred and Unverifiable. Verified names the command that ran and what it showed; Inferred is what you reasoned from the docs and did not run; Unverifiable is what you could not check here, and why. Each list may be the single item `none`.
