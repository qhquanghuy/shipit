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
Drafts the goal and roadmap versions in a temporary folder that it makes itself with `mktemp -d /tmp/shipit.XXXXXX` (it never asks the owner for one) and copies an approved roadmap version into the store with `mkdir -p` and `cp` once the owner approves it; a goal version is the owner's copy (Store files). Other commands: `shipit template`, `shipit validate`, `git rev-parse` and `git log`. Starts `shipit:scout` and `shipit:plan-reviewer`. Runs `shipit receipt plan-review roadmap`. Asks the owner with decision points.
Questions to the owner use the words of the [guide](../../docs/guide.md) and its glossary, and ask only what changes what is built.
Never: write code, commit, push, or begin a slice (`shipit-slice` does that, after the build plan is approved).
## Outcomes
- **ready-for-review**: the roadmap is drafted and validates against its goal; it is not in the store yet (step 8 copies it after the owner approves it). Next: `shipit:plan-reviewer` at the top tier reviews it.
- **no goal**: the store has no goal of that name. Next: the lead writes the goal with the owner, as the guide shows (a short name, what it is for, numbered done-when items; approval as a decision point; the owner copies the approved goal into the store (Store files)), then runs this skill again.
- **reviewed**: the reviewer approved, or approved after fixes that are made. Next: the lead asks the owner to approve the roadmap, as a decision point.
- **approved**: the owner approved the roadmap. Next: the lead asks the owner how the slices run, as a decision point; then the lead hands each slice of wave 1 to `shipit-slice`.
- **changes asked**: the owner or the reviewer wants a change. Next: the lead writes a new roadmap version, and it is reviewed again.
- **no reviewer above the planner**: the rank rule leaves no reviewer, or the stand-in has the same model and effort as the planner. Next: the lead asks the owner, as a decision point: wait for the model, or a ruling.
- **rework past the cap**: the reviewer still says rework after `agents.max_fix_rounds` rounds. Next: the lead rules; no slice exists yet to log to, so the ruling goes in the Owner decisions part of the roadmap, in a new version; the owner decides whether the goal must change.
## Steps
1. Find the store: run `git rev-parse --git-common-dir` alone, and type the printed path into the next command. Read the newest `goals/<goal>/goal.vN.md` with the file tools. Check that its done-when items are numbered DW1, DW2 and so on. Without a goal, stop as **no goal**.
2. First decision point: write it as a `decision-point` (`shipit template decision-point`) with a ```` ```example ```` block where it helps the owner decide, and check it with `shipit validate decision-point <file>`. It offers model advice for this goal, with the date `agents.tiers` last changed (`git log -1 --format=%cs -G tiers -- .config/shipit.toml`); the owner may skip it. If the owner wants it, the lead runs the model advice route of the `shipit` skill. Offer it once for the goal, never once for each slice: one goal keeps one set of models.
3. Make a temporary folder with `mktemp -d /tmp/shipit.XXXXXX`; never ask the owner for one. Draft the roadmap from `shipit template roadmap` there. The goal name is short and unused (the start of a slice refuses a used name, and one with `origin/slice/<goal>/…` branches). Every slice is complete end to end, under the cap, and fills one session. Order only with `after`. Slices that run in parallel own different paths. Small setup-only changes that wait together go in one setup-batch slice. Mark `test_drive` as the domain pack says. Write a proof step for each done-when item that no test can show.
4. Check the draft with `shipit validate roadmap <draft> --goal <goal>` and fix what it lists.
5. For background facts start `shipit:scout` (`shipit template scout-report`). On `partial` or `not-found` ask once more with a narrower question and say where it looked (`agents.roles.scout.retries`); then use a scout at `agents.roles.scout.escalate_to`; then read the source yourself. No slice exists yet, so `shipit log` cannot record these steps: a known gap.
6. Start `shipit:plan-reviewer` with the model and effort of its rank: the top tier, at the planner's effort. Author at the top tier: same tier at a higher effort, up to `agents.effort_cap`; at the cap too, a fresh agent at the same rank, recorded with `--fresh`. A model that is unavailable: its `stand_in`, written `<model>:<effort>@<tier>` in `--by`; `shipit receipt` refuses a stand-in that equals the author, and that is the outcome **no reviewer above the planner**. It fills `plan-review` (`shipit template plan-review`) and ends its report with Verified, Inferred and Unverifiable.
7. Fix rounds, at most `agents.max_fix_rounds`. A roadmap already in the store is never edited: each change is a new version file, drafted and checked as in steps 3 and 4. The reviewed draft stays in the temporary folder until the owner approves it (step 8).
8. Second decision point: write it as a `decision-point` to approve the roadmap, with a ```` ```example ```` block of the waves, and check it with `shipit validate decision-point <file>`. Its What names the roadmap version to approve (`roadmap.vN.md`) and what changed for the owner: the slices, their order and the waves. It does not list the reviewer's findings. Its Recommendation names the slices of wave 1. Once the owner approves, copy the draft in with one plain command, `mkdir -p <git common dir>/shipit/goals/<goal> && cp <draft> <git common dir>/shipit/goals/<goal>/roadmap.v<N>.md` (it asks; in a headless run hand the owner that command), then record `shipit receipt plan-review roadmap --goal <goal> --author <author> --by <model>:<effort> --result <result>`. Write `--by` as `<model>:<effort>@<tier>` only when a stand-in reviewed (step 6); otherwise plain `<model>:<effort>`.
9. Third decision point: write it as a `decision-point` on how the slices run, with a ```` ```example ```` block like the one under How slices run, and check it with `shipit validate decision-point <file>`. It lists the four ways, says which the lead recommends and why. After the answer the lead hands each slice of wave 1 to `shipit-slice`, in the way the owner chose; `shipit-slice` writes the build plan first and begins the slice only after it is approved.
## How slices run
The lead recommends one way from the number and size of the slices.

| Way | Who runs `shipit-slice` | Recommended when |
|---|---|---|
| one session per slice | a slice session the owner opens | the slices are large, or the owner wants to watch and answer each one |
| lead fan-out | the lead, slice after slice, in each slice's worktree | two or three small slices, so the lead keeps them in its own context |
| one agent per slice | `shipit:slice`, started by the lead for each slice | several mid-size slices at once, few owner questions; the cap must hold the slice agent and its two reviewers: 3 agents each (`agents.parallel.agents`) |
| the `Workflow` tool | a script | many slices of one fixed shape; only on the owner's explicit opt-in |

Example for the decision point, with the real waves:

```example
Wave 1: alpha, beta (parallel), then gamma.
1. one session per slice: you open 2 sessions now, 1 later
2. lead fan-out: I run them one by one here
3. one agent per slice (recommended: 3 mid-size slices; raise agents.parallel.agents to 6, or run them in turn)
4. the `Workflow` tool: your opt-in
```
## Store files
- The store is `<git common dir>/shipit`. Find it with `git rev-parse --git-common-dir`, run alone. A skill never spells the git folder itself.
- Write: draft with the file tools in a temporary folder made with `mktemp -d /tmp/shipit.XXXXXX` and validate the draft there. A goal or roadmap version is the owner's decision, and `shipit store put` refuses both: a roadmap version is copied in by step 8; for a goal version give the owner the exact commands, `mkdir -p <git common dir>/shipit/goals/<goal> && cp <draft> <git common dir>/shipit/goals/<goal>/goal.v<N>.md`. A store file is never edited in place: a changed goal is `goal.vN.md`, a changed roadmap `roadmap.vN.md`.
- Read: with the file tools.
- If the copy asks or is refused (a headless run refuses), stop and give the owner the exact `mkdir -p` and `cp` commands to run.
