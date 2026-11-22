# ShipIt guide for the owner

You are the owner. You have three jobs: set the goal, answer the decision points, and ship. An agent does the rest.
Words are in [vocab.md](vocab.md). How to install and adopt is in [README.md](../README.md). How it runs in Claude Code is in [claude-code.md](claude-code.md). The setup file is in [setup.md](setup.md).

## The flow and the skills

goal → roadmap → build plan → make → check → reviews (and a test drive for some slices) → fix rounds → ship.

| Phase | Skill | Who runs it |
|---|---|---|
| where are we, what next, model advice | `shipit` | you or the lead |
| goal into a roadmap you approve | `shipit-roadmap` | the lead |
| one slice, from build plan to pull request | `shipit-slice` | the lead, once for each slice |
| using the result as its audience would | `shipit-test-drive` | the tester |
| review of any pull request or branch | `shipit-review` | you, the lead, or `shipit-slice` |

In Claude Code you type `/shipit:shipit`. Start there; it reads `shipit status` and names the next skill.
In v1 every slice gets the full review: a reader, an informed review and a second opinion.

## Who does the work

Each agent has a role. On the screen you see the role: `shipit:maker`, `shipit:scout`, `shipit:reader`, `shipit:work-reviewer`, `shipit:plan-reviewer`, `shipit:tester`, `shipit:model-scout` and `shipit:slice`.
The goal text calls the model advice role the "researcher"; it is `shipit:model-scout`.
A reviewer is always one tier above the one who made the work. The tool checks this when a receipt is recorded.

## Decision points

Every question to you is a decision point. It starts with a label, then What, Context, Recommendation and Next. It stays short. An `example` block shows a real command or output and does not count toward the short limit.

```
[shop · pay · review]

**What:** ship the pay slice now, or fix one finding first?
**Context:** the review found one small bug in the refund path. Tests pass.
**Recommendation:** fix it first: one more round, about an hour.
**Next:** if you say fix, I start a fix round; if you say ship, you merge the pull request.
```

## What you ship

Each slice ends as a pull request. Its body leads with the changed types and interfaces, and a reading path to follow. It ends with three lists:

- **Verified**: checked by running something. Each item names the command.
- **Inferred**: reasoned, not run. This is your list to check. If it is long, look hard.
- **Unverifiable**: could not be checked here, and why.

You read the pull request and merge it. After the merge the lead runs `shipit log <slice> shipped --merge <commit>`. When every slice has shipped, you run `shipit complete <goal>`.

## Where things live

The goal, the roadmap, the build plans, the logbook and the receipts live in the store, in the git common directory. Git never tracks them. In git only these are kept: the setup, the CI job and the coverage maps (`.shipit/coverage/`).

## Model advice

At adoption, at the start of each goal, and whenever you ask, a `shipit:model-scout` reads current models and prices on the web and gives advice with sources. You pick. The lead writes your pick into the setup as a setup-only pull request.

## Sessions and how slices run

The lead session is named by `shipit session-name <goal>`; a slice session by `shipit session-name <goal> <slice>`. At the end of the roadmap you pick one of four ways:

1. one session per slice: you open a session for each;
2. lead fan-out: the lead runs the slices one after the other;
3. one agent per slice: `shipit:slice` runs a whole slice; it cannot ask you, so a question ends it as blocked and the lead asks you;
4. the `Workflow` tool: a script runs the slices; only if you ask for it.

## Mixed teams

ShipIt's rules apply only to `slice/*` branches; every other branch runs only the base's checks, so a teammate without ShipIt is not blocked.
Coverage shows that a mapped test exists and ran, not what it asserts. See [README.md](../README.md).
