---
name: shipit-roadmap
description: Use when a goal needs a roadmap, or its roadmap changes. Turns a goal into reviewed, owner-approved slices and settles how they run.
argument-hint: [goal]
---
# shipit-roadmap
Turns one goal into a reviewed roadmap that the owner approves, and settles how the slices run. The lead runs it. Terms: [vocabulary](../../docs/vocab.md).
## Inputs
- The goal (the argument): `goals/<goal>/goal.vN.md` in the store, newest version, with numbered done-when items (Store files). With no goal, write it with the owner first.
- The setup (`.config/shipit.toml`): `gate.max_files` (the cap), `agents.tiers`, `agents.roles`, `agents.effort_cap`, `agents.max_fix_rounds` and `agents.parallel`.
- The domain pack ([coding pack](../../data/packs/domain/coding.md)) for what a slice is and how its size is measured, and the skill pack (`packs.skill`) for the goal and roadmap phase.
## Output
One `roadmap`: `shipit template roadmap`, checked with `shipit validate roadmap <file> --goal <goal>`.
## Effects
Drafts the goal and roadmap versions in a drafts folder that it makes itself with `mkdir -p .shipit/td/<goal>` and the one line `*` in its `.gitignore`, so that git ignores it (it never asks the owner for one) and, once the owner approves the roadmap in chat, stores it with `shipit store put roadmap <draft> --goal <goal>`; a goal version is stored the same way, `shipit store put goal <draft> --goal <goal>`, after the owner approves it (Store files). Other commands: `shipit template`, `shipit validate`, `git rev-parse` and `git log`. Starts `shipit:scout` and `shipit:plan-reviewer`. Runs `shipit receipt plan-review roadmap`. Asks the owner with decision points.
Questions to the owner use the words of the [guide](../../docs/guide.md) and its glossary, and ask only what changes what is built.
Never: write code, commit, push, or begin a slice (`shipit-slice` does that, after the build plan is approved).
## Outcomes
- **ready-for-review**: the roadmap is drafted and validates against its goal; it is not in the store yet (step 8 stores it after the owner approves it). Next: `shipit:plan-reviewer` at the top tier reviews it.
- **no goal**: the store has no goal of that name. Next: the lead writes the goal with the owner, as the guide shows (a short name, what it is for, numbered done-when items; approval as a decision point; the lead stores the approved goal with `shipit store put goal <draft> --goal <goal>` once the owner approves it (Store files)), then runs this skill again.
- **reviewed**: the reviewer approved, or approved after fixes that are made. Next: the lead asks the owner to approve the roadmap, as a decision point.
- **approved**: the owner approved the roadmap. Next: the lead asks the owner how the slices run, as a decision point; then the lead hands each slice of wave 1 to `shipit-slice`.
- **changes asked**: the owner or the reviewer wants a change. Next: the lead writes a new roadmap version as a delta of the newest (step 7), and its diff is reviewed again.
- **no reviewer above the planner**: the rank rule leaves no reviewer, or the stand-in has the same model and effort as the planner. Next: the lead asks the owner, as a decision point: wait for the model, or a ruling.
- **rework past the cap**: the reviewer still says rework after `agents.max_fix_rounds` rounds. Next: the lead rules; no slice exists yet to log to, so the ruling goes in the Owner decisions part of the roadmap, in a new version; the owner decides whether the goal must change.
## Steps
1. Find the store: run `git rev-parse --git-common-dir` alone, and type the printed path into the next command. Read the newest goal with `shipit store show goal --goal <goal> --copy`. Check that its done-when items are numbered DW1, DW2 and so on. Without a goal, stop as **no goal**.
2. First decision point: write it as a `decision-point` (`shipit template decision-point`) with a ```` ```example ```` block where it helps the owner decide, and check it with `shipit validate decision-point <file>`. It offers model advice for this goal, with the date `agents.tiers` last changed (`git log -1 --format=%cs -G tiers -- .config/shipit.toml`); the owner may skip it. If the owner wants it, the lead runs the model advice route of the `shipit` skill. Offer it once for the goal, never once for each slice: one goal keeps one set of models.
3. Make a drafts folder with `mkdir -p .shipit/td/<goal>`; never ask the owner for one. Draft the roadmap from `shipit template roadmap` there. The goal name is short and unused (the start of a slice refuses a used name, and one with `origin/slice/<goal>/…` branches). Every slice is complete end to end, under the cap, and fills one session. Order only with `after`. Slices that run in parallel own different paths. Small setup-only changes that wait together go in one setup-batch slice. Mark `test_drive` as the domain pack says. Write a proof step for each done-when item that no test can show.
4. Check the draft with `shipit validate roadmap <draft> --goal <goal>` and fix what it lists.
5. For background facts start `shipit:scout` (`shipit template scout-report`). On `partial` or `not-found` ask once more with a narrower question and say where it looked (`agents.roles.scout.retries`); then use a scout at `agents.roles.scout.escalate_to`; then read the source yourself. No slice exists yet, so `shipit log` cannot record these steps: a known gap.
6. Start `shipit:plan-reviewer` with the model and effort of its rank: the top tier, at the planner's effort. Author at the top tier: same tier at a higher effort, up to `agents.effort_cap`; at the cap too, a fresh agent at the same rank, recorded with `--fresh`. A model that is unavailable: its `stand_in`, written `<model>:<effort>@<tier>` in `--by`; `shipit receipt` refuses a stand-in that equals the author, and that is the outcome **no reviewer above the planner**. It fills `plan-review` (`shipit template plan-review`) and ends its report with Verified, Inferred and Unverifiable.
7. Fix rounds, at most `agents.max_fix_rounds`. A roadmap already in the store is never edited: each change is a new version file, drafted and checked as in steps 3 and 4. When the store holds one, a new version starts as a copy of the newest stored one: `shipit store show roadmap --goal <goal> --copy` writes it to `.shipit/td/<goal>/store/roadmap.vN.md` (docs/store.md, Store show), and the planner writes the draft from that copy's text with the file tools; the copy in `store/` stays as it is, as the base of the diff. A first roadmap, and fix rounds before the first put, start from the template (step 3). The planner changes only what was decided (a ruling, an owner answer, a review finding) and leaves every other line as it is. `shipit:plan-reviewer` gets the diff, `git diff --no-index <copy> <draft>` (it exits 1 when the files differ, the normal result), with the draft, and checks the changed slices and what they change in the order (`after`) and the owns. The reviewed draft stays in the drafts folder until the owner approves it (step 8).
8. Second decision point: write it as a `decision-point` to approve the roadmap, with a ```` ```example ```` block of the waves, and check it with `shipit validate decision-point <file>`. Its What names the roadmap version to approve (`roadmap.vN.md`) and what changed for the owner: the slices, their order and the waves. It does not list the reviewer's findings. Its Recommendation names the slices of wave 1. Once the owner approves, store it with `shipit store put roadmap <draft> --goal <goal>` (it prints the version and records it in the logbook; status and every pull request show it), then record `shipit receipt plan-review roadmap --goal <goal> --author <author> --by <model>:<effort> --result <result>`. Write `--by` as `<model>:<effort>@<tier>` only when a stand-in reviewed (step 6); otherwise plain `<model>:<effort>`.
9. Third decision point: write it as a `decision-point` on how the slices run, with a ```` ```example ```` block like the one under How slices run, and check it with `shipit validate decision-point <file>`. It lists the ways, says which the lead recommends and why. After the answer the lead hands each slice of wave 1 to `shipit-slice`, in the way the owner chose; `shipit-slice` writes the build plan first and begins the slice only after it is approved.
## How slices run
Slices run one way, and the decision point says so and names the waves.

The way is one session per slice, opened in the slice's worktree. `shipit:maker` works only in a working copy on a slice branch, and the Agent tool cannot start an agent in another folder, so no session in the main working copy can make a slice (`docs/claude-code.md`, The author rule). The slice's worktree comes from `shipit-slice` step 2, after the build plan is approved: the owner opens a new session in the worktree then.

| Way | Who runs `shipit-slice` | Recommended when |
|---|---|---|
| one session per slice | a slice session the owner opens in the slice's worktree | every slice: the owner watches and answers each one |

Example for the decision point, with the real waves:

```example
Wave 1: alpha, beta (parallel), then gamma.
1. one session per slice: you open 2 sessions now, 1 later
```
## Store files
- The store is `<git common dir>/shipit`. Find it with `git rev-parse --git-common-dir`, run alone. A skill never spells the git folder itself.
- Write: draft with the file tools in a drafts folder made with `mkdir -p .shipit/td/<goal>` and validate the draft there. A goal or roadmap version is stored only after the owner approves it in chat: `shipit store put goal <draft> --goal <goal>` or `shipit store put roadmap <draft> --goal <goal>`; each put is recorded (docs/store.md). The hand copy, `mkdir -p <git common dir>/shipit/goals/<goal> && cp <draft> <git common dir>/shipit/goals/<goal>/goal.v<N>.md` (`roadmap.v<N>.md` for a roadmap), stays a fallback only; status shows a version stored that way as not recorded. A store file is never edited in place: a changed goal is `goal.vN.md`, a changed roadmap `roadmap.vN.md`.
- Read: `shipit store show <kind> --goal <goal> [--slice <slice>] [--round <k>] --copy`, from any working copy, then the Read tool on the path it prints.
- If the copy asks or is refused (a headless run refuses), stop and give the owner the exact `mkdir -p` and `cp` commands to run.
