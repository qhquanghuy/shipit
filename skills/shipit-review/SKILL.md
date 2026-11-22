---
name: shipit-review
description: Use when any change needs a full review: a pull request, a branch or a slice. Runs a reader and two reviewers and merges them into one review.
argument-hint: [pr number, PR address or branch] [base] [focus: what to look at]
---
# shipit-review
Reviews one change in full and merges the result into one `review`. The owner, the lead or `shipit-slice` starts it; it works in a ShipIt project or not. Terms: [vocabulary](../../docs/vocab.md).
## Inputs
- The change: a pull request number or address, or a branch and its base (the setup's base when none is given). Source: `gh pr view` and `gh pr diff`, or `git diff`.
- Its claim: the pull request's title and body, or the commit messages.
- The owner's focus note: an argument that begins with `focus:`. Only the informed reviewer gets it.
- When there are any: the goal, the done-when and the build plan, from the store (Store files).
- The ranks: the setup (`agents.roles.reader`, `agents.tiers`, `agents.effort_cap`), or the model pack's starting setup (Ranks).
## Output
One merged `review`: `shipit template review`, checked with `shipit validate review <file>`. Its agents fill `explanation`, `informed-review` and `second-opinion`.
## Effects
Reads git and gh: `git diff`, `git log`, `git fetch`, `gh pr view`, `gh pr diff`, `grep` and `sha256sum`. Starts `shipit:reader`, then two reviewers at once. Writes every output in a temporary folder that it makes itself with `mktemp -d` (it never asks the owner for one); in a slice only, copies them into the store and records the receipts. In v1 every review is full.
Questions to the owner use the words of the [guide](../../docs/guide.md) and its glossary, and ask only what changes what is built.
Never: edit code, push, merge, or review the work itself as the lead. Post to the pull request only when the owner says yes: no gate stops a gh write, so this is a rule of prose.
## Outcomes
- **approve**: both reviews approve (the worse of the two decides the outcome; from worst to best: rework, approve-after-fixes, approve). Next: the owner decides whether to ship; in a slice `shipit-slice` goes on to the test drive if the roadmap marks one, then the pull request.
- **approve-after-fixes**: a must-fix finding is left. Next: a fix round, then review again; in a slice at most `agents.max_fix_rounds`, then the lead rules.
- **rework**: the change cannot ship as it is. Next: the owner decides between a new build plan and a new slice; the lead starts what the owner picks.
- **cannot review**: there is no diff, or the review gate denies a reviewer in a slice. Next: the lead runs `shipit check` on the slice branch, then asks for the review again.
## Steps
1. Find the change, its claim and the owner's focus note: `gh pr view <n>` and `gh pr diff <n>` for a pull request; `git diff <base>...<branch>` and `git log <base>..<branch>` for a branch. If gh is missing: `git fetch origin pull/<n>/head` and git alone. An argument that begins with `focus:` is the owner's note on what to look at; it goes to the informed reviewer only. With no goal or build plan (cases 2 and 3: a teammate's pull request, any other repository) the informed reviewer gathers its own context in step 4a, so the two reviews do not get the same input. Run each command alone in its own Bash call: one command per call keeps the harness from asking.
2. Pick the case (Cases) and the ranks (Ranks).
2a. Changed tests: list every test file the change edits or removes (`git diff --name-status <base>...<branch>`, filtered by where the domain pack says tests live) and every test or assertion line the diff removes or loosens (a removed `#[test]` or `@test`, an assert line, an expected value widened, a check turned into a skip). The list goes to the informed reviewer.
3. Start `shipit:reader` on the diff only, with the model and effort of its rank → `explanation` (`shipit template explanation`; If missing: the form is in `../../data/templates/explanation.md`). Tell it to name lines of the changed files (`path:line`), not lines of the diff, and to return the whole form, its `## Form` part included.
4. Start two `shipit:work-reviewer` agents in parallel, each with the model and effort of its rank (4a and 4b; in case 2 start two plain agents, each given the text of `../../agents/work-reviewer.md`, per Cases). Each ends with Verified, Inferred and Unverifiable (Claims) and returns its whole form as text, the `## Form` part included; write each into the temporary folder.
4a. Informed reviewer (`shipit:work-reviewer`; in case 2 a plain agent given its text): the diff, the claim, the goal and build plan when any, the reader's explanation, the changed-tests list of step 2a and the owner's focus note → `informed-review` (`shipit template informed-review`; If missing: the form is in `../../data/templates/informed-review.md`) with Reading path, Other files and Look here. It lists the key claims the build plan (or, without one, the pull request's claim) makes about how the code behaves and must test each by running something, not only compare the work against them. Outside a slice it first gathers context: the pull request's comments and the issues it closes (`gh pr view <n> --comments`, `gh pr view <n> --json closingIssuesReferences`), the repository's guides (the top-level guide file, `README`, `docs/`), the recent history of each changed file (`git log -5 -- <path>`), and the callers of each changed function (`grep -rn <name>`). Look here opens with each changed test, and the reviewer judges for each whether the change weakens what is proven (a weakening is a must-fix finding unless the build plan says why).
4b. Second opinion (`shipit:work-reviewer`; in case 2 a plain agent given its text): the diff and the spec only, that is the done-when, or the pull request's claim when there is no goal; never the explanation, the goal, the build plan, the focus note or any gathered context → `second-opinion` (`shipit template second-opinion`; If missing: the form is in `../../data/templates/second-opinion.md`).
5. `shipit validate <kind> <file>` on each output; in case 3 there is no setup, so the tool exits with an error: go straight to the hand check. If missing: check by hand against the form (`../../data/templates/`, one file for each kind) and list "validation" as skipped.
6. In a slice only: `shipit receipt informed-review <slice>` and `shipit receipt second-opinion <slice>`, each with `--author`, `--by` (`<model>:<effort>`), `--result` and each `--finding`. If missing: a slice has no review without the tool (the plugin builds it); stop as **cannot review**.
7. Merge into one `review`: Claim; Teach-back and its gaps against the claim; Reading path, diagrams, Other files and Look here from the informed review, checked (each link's path is a changed file and its line is in a hunk of `git diff -U0 <base>...<branch> -- <path>`); Findings in Conventional Comments (duplicates become one line that says both found it); Security and Performance (both answers); Verified, Inferred and Unverifiable (both lists joined; a claim one reviewer verified and the other only inferred counts as verified, naming the command); Skipped; the outcome is the worse result (rework is worse than approve-after-fixes, which is worse than approve). Then `shipit validate review <file>`. If missing: by hand, as step 5.
8. Hand over: to `shipit-slice` as a file; to the owner as the file and one decision point written with `shipit template decision-point` and a ```` ```example ```` block where it helps the owner decide, checked with `shipit validate decision-point <file>`. If missing: by hand, as step 5.
## Cases
| Case | How it is told | Agents | Skipped, listed in the review with why | Outputs go to |
|---|---|---|---|---|
| 1. in a slice (called by `shipit-slice`) | a slice branch with a check receipt | `shipit:reader`; `shipit:work-reviewer` twice, each prompt with the line `shipit-branch: slice/<goal>/<slice>` | nothing | drafted in a temporary folder, then copied into the store (Store files) at `goals/<goal>/slices/<slice>/reviews/round-<k>/` |
| 2. a ShipIt project, not a slice | `.config/shipit.toml` at HEAD, and the change is not a slice branch under review | `shipit:reader`; two plain agents given the text of `../../agents/work-reviewer.md`, no `shipit-branch:` line | receipt, rank, coverage | a temporary folder outside the repository |
| 3. outside a ShipIt project | no `.config/shipit.toml` at HEAD | `shipit:reader`; `shipit:work-reviewer` twice, no `shipit-branch:` line | receipt, rank, coverage, and validation (check by hand) | a temporary folder |
Case 2 is the one place with plain agents: in a ShipIt project the review gate denies `shipit:work-reviewer` without a `shipit-branch:` line that names a slice branch, and a change that is not a slice has none. They show as general agents (`docs/claude-code.md`). Case 3 has no setup, so the gate lets the named role start.
## Ranks
The setup has no reviewer role, so this rule is the skill's, and `shipit receipt` refuses a reviewer who does not rank above the author.

| Agent | Rank in a ShipIt project |
|---|---|
| reader (`shipit:reader`) | `agents.roles.reader`, below the maker |
| reviewer of the work (`shipit:work-reviewer`) | one tier above the author, at medium effort |
| plain agents of case 2 | the same as `shipit:work-reviewer` |

- Author at the top tier: same tier, higher effort, up to `agents.effort_cap`. At the cap too: a fresh agent at the same rank, recorded with `--fresh`.
- A tier's model is unavailable: its `stand_in`, written `<model>:<effort>@<tier>` in `--by`. The tool refuses a stand-in whose real model and effort equal the author's: then stop and ask the owner as a decision point (wait for the model, or a ruling), never a silent same-rank review.
- Every agent start passes the model and effort of its rank. Role files hold neither.
- Outside a ShipIt project: the model pack's starting setup (`../../data/packs/model/`): the reader at its `agents.roles.reader`, both reviewers at tier 3, medium.
## Reading path
Written by the informed reviewer, who knows the build plan; checked in step 7.
- One flow for each entry point the change adds or alters (a hook event, an API handler, a command), headed `### Flow: <entry point>`.
- Each step of a flow is one function or type, in call order from the entry contract inward (input and output first, then service, domain, storage): its name in a code span, what it takes, what it returns, what it calls next, a risk word (`risk: high`, `risk: medium` or `risk: low`) with why on a step that needs care, and a link to its line: `https://github.com/<owner>/<repo>/pull/<n>/files#diff-<sha256 of the path>R<line>`. The hash comes from `printf '%s' '<path>' | sha256sum` (If missing: `shasum -a 256`). A branch with no pull request: the compare view, the same anchor. Off GitHub: `<path>:<line>`.
- One Mermaid `sequenceDiagram` for each flow, in a `mermaid` fence, drawn from the same steps.
- Other files: the files on no path (tests, docs, configuration), grouped, each group with a risk word.
- Look here: first every changed test from step 2a (path and line; edited, removed or weakened, and does it still prove what it proved), then two or three spots the informed reviewer is least sure of, each a step on a path.
## Store files
- The store is `<git common dir>/shipit`. Find it with `git rev-parse --git-common-dir`, run alone, and type the printed path into the next command. A skill never spells the git folder itself.
- Write: draft with the file tools in a temporary folder made with `mktemp -d`, validate the draft there, then copy it in with one plain command: `mkdir -p <git common dir>/shipit/goals/<goal>/slices/<slice>/reviews/round-<k> && cp <draft> <git common dir>/shipit/goals/<goal>/slices/<slice>/reviews/round-<k>/<name>`. A store file is never edited in place.
- Read: with the file tools, or `cat` and `ls`.
## Claims
Every agent report and every review ends with three lists, each of which may be the single item `none`. **Verified:** checked by running something; each item names the command or test that ran. **Inferred:** reasoned from the code or the docs, not run; this is the list the owner must check. **Unverifiable:** could not be checked here, and why.
