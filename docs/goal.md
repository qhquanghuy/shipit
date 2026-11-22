# ShipIt: goal

Status: version 4, approved by the owner on 2026-11-22 (v3 approved 2026-11-22; v2 and v1 approved 2026-11-21). Terms are defined in [vocab.md](vocab.md).

## Goal

Make one way of working reusable across projects, domains and LLMs, so that the owner only sets
the goal, answers decision points and ships, and agents do the rest in a way the owner can trust.

ShipIt is a framework, not a how-to. It fixes the flow, the roles, the rules and the receipts.
Packs say how each phase is carried out.

```
goal → roadmap → build plan → make → check → informed review + second opinion + test drive → fix rounds → recheck → ship
```

### Why

- In agentic coding, agents do most of the work, in parallel. The human keeps three jobs:
  critical decisions, final verification (agents prepare the evidence, the human has the last
  word), and code review (wanted, not mandatory). ShipIt keeps those three jobs small and clear.
- The owner's process worked on one project (firectl) but lived in chat and memory. Each new
  session or project had to be told again.
- Rules that live only in instructions get skipped. Rules a tool enforces do not.
- The owner reviews every change, so each change must stay small enough to review.

### The four rules

No pack and no setup can switch these off.

1. Someone else confirms your work, one tier up.
2. Nothing counts without a receipt.
3. Known issues are written down, never hidden.
4. Only the owner ships.

## Design principles: functional thinking

These hold for the tool, the skills and the flow alike. The terms stay plain; the thinking is
functional.

| Principle | In ShipIt |
|---|---|
| Functions with explicit inputs and outputs | Every phase and every role takes named input files and gives one named output. An agent gets its inputs as files, never as inherited chat history. |
| Immutability | An output is never edited in place. A changed roadmap or build plan is a new version. The logbook is append-only. A finding is never deleted; its new status is appended. |
| Derive, don't store | A slice has no status field. Its state is computed from the receipts and logbook that exist, for the exact version of the work. |
| Illegal states can't be written down | The setup file is parsed once into a valid config, or the tool stops. Slice states are a fixed list with fixed allowed moves. |
| Pure core, effects at the edge | Deciding is pure: is this receipt valid, is the slice over the cap, what comes next. Effects are a short named list: run checks, write a receipt, append to the logbook. Ship belongs to the owner alone. |
| Results as values | A failed check, a finding and a decision point are ordinary results that the flow handles. Every phase lists all of its outcomes and what follows each. |
| Composition | A slice's flow is one function applied to each slice. Slices share no changing state, so slices that touch separate work can run in parallel. |
| Memoization | A receipt is a cached result keyed by the content of the work. Recheck computes it again to prove the cache honest. |

## Principles per step

| Step | Principle |
|---|---|
| goal | The owner's words are the source. Done-when is written before any work and is checkable. Ask only what changes the build. |
| roadmap | Slices are complete, never broken pieces; each fits the cap and one session. Plan what runs in parallel; parallel slices only add files. Mark which slices need a test drive. |
| build plan | Say how each part is verified before making. Fix interfaces; leave bodies to the maker. Reviewed by a higher rank first. |
| make | Cheapest capable model, one task at a time, no review mid-way. Code is written for its reviewer. Use the machine's tools; never install globally. |
| check | Tools enforce, not prose. Runs once per exact version and leaves a receipt. A slice cannot change its own checks; only a setup-only change (no other files) may. |
| review | Informed review and second opinion, ranked above the maker; the teach-back reader is a signal. Findings are fixed or written down as known issues. |
| test drive | Use it as its audience would, from the goal only, never the code. Checklist first, then exploration. |
| fix rounds | Limited (2 by default). Past the limit, the lead decides and records it. |
| recheck | Trust no receipt; run everything again where the maker has no control. |
| ship | Only the owner. Always asked. Shows the evidence and known issues, short. |

## Code is written for the person who reviews it

The human reviewer reads the change, so the code must be simple to read:

- Reuse before adding. No dead code and no duplicate code.
- Make the simplest change that meets the done-when.
- Comments only for decisions, not for what the code already says.
- The pull request leads with the changed types and interfaces.

A **reader** (a low-rank model) explains the change without seeing the build plan. The informed
reviewer compares that explanation with the goal, or with what the pull request claims when there is no goal. It is a signal, not a gate.

## Keep a file only if a tool reads it or a test checks it against the code

Keep a file only if a tool reads it or a test checks it against the code. Prose that restates code drifts and misleads agents.

## Talking to the owner

Everything the owner reads must be simple and easy to understand: plain words, short sentences,
the glossary's terms only, no jargon. The owner may not be a native English speaker.

Every question to the owner, and every decision point, has the same three parts:

1. **What**: the decision, in one sentence.
2. **Context**: only the facts needed to decide, briefly.
3. **Recommendation**: the options when there are several, each with its trade-off, and which one
   the lead recommends and why.

**Every question stands on its own.** The owner may be working on several things at once. A
question or decision point starts with a short label of where it belongs (project, slice, phase),
for example `[firectl · slice A · review]`, and its context gives what the owner needs to decide
without remembering earlier messages.

**Always say what is next.** Every message to the owner ends with a short "Next" part: what
happens next, who does it, and what the owner must do, or "nothing until …". `shipit status` names
the next step for each slice, and every skill's outcomes say what follows.

The decision-point template enforces this form: label, What, Context, Recommendation, Next.

A concrete example helps the owner, so a fenced block whose info string is `example` is counted
separately: its lines count against `[owner] example_max_lines`, not `max_lines`.

## What gets built (version 1)

### 1. The `shipit` tool

One command, a Rust program, kept in this repo. It runs git as a process and needs only git, and
bash to run the check commands, at run time. Hooks and CI call the same tool.

The commands, their usage and their exit codes are printed by `shipit help` and `shipit help <command>`.

- **A receipt is keyed by the exact input of what it records.**
  - A check and a test drive judge the whole work, so their key is the git tree hash. An amended
    commit with the same content keeps the receipt; any change to the content loses it.
  - A review of the work judges the change, so its key is the change itself (git's patch id). A
    rebase that leaves the change identical keeps the review; the check must still run again.
  - A review of a roadmap or a build plan judges a file, so its key is the hash of that file.
    Editing the file after its review loses the receipt.
- Receipts and the logbook are JSON. Deciding is a pure core with no input or output; running git,
  the check commands and writing files are the effects at the edge.
- A check receipt is written only by `shipit check`. Review and test-drive receipts are records
  made by the lead: they say who claims what about which input. They are honest notes, not
  proof, which is why recheck and the owner come after them.
- The tool checks rule 1 on every review and test-drive receipt, for plans as well as for the work: the
  reviewer must rank above the author. Ranks come from the base branch's setup, so a slice cannot change the ranks it is judged by: first by model tier, then, for
  the same model, by effort level. So a reviewer on the same model meets rule 1 by working at a
  higher effort than the author, up to the setup's effort cap. A stand-in counts as the tier it
  stands in for, and the receipt records that a stand-in was used.
- The roadmap has a machine-readable part: each slice's and proof step's name, `after` (what it follows), `meets` (the done-when items it
  proves), `owns` (what it owns), test-drive mark and branch. `shipit status` reads it, so a slice with no receipts yet still shows.
- All working copies of a project see the same roadmap, logbook and receipts.
- `.shipit/coverage/<goal>/<slice>.toml` is committed evidence of which done-when items a slice covers.

**Slice states.** The list is fixed, and each state is computed, never set:

| State | True when |
|---|---|
| planned | the slice is in the roadmap |
| build plan ready | a build plan exists and has a valid review receipt |
| made | the slice's branch has commits beyond the base |
| checked | a valid check receipt exists for the current version |
| ready to ship | checked, and valid receipts exist for the informed review, the second opinion and, if marked, the test drive; every finding is fixed or is a known issue |
| shipped | the logbook holds a "shipped" entry naming the owner's merge |

A slice only moves forward by gaining a receipt or a logbook entry. It moves back by itself when
the work changes: the receipts no longer match, so the computed state drops.

### 2. The setup file

One committed file per project, `.config/shipit.toml`. It holds the cap (`gate.max_files`), the check commands, the
fix-round limit (`agents.max_fix_rounds`), the effort cap (`agents.effort_cap`), the tiers with their models and stand-ins, the roles
(scout, `planner`, maker, tester, reader), the packs in use and the limits on messages to the owner. `[agents.parallel]` names
`slices` and `agents`: how many slices and how many agents may run at once.

- There is no environment override. Changing a value means changing the committed file, so the
  change shows up in the diff the owner reviews.
- Recheck reads the cap from the base branch, so a slice cannot raise its own cap. The one
  exception is the pull request that adopts ShipIt: the base has no setup file yet, so recheck
  uses the pull request's own and says so.
- The setup file is the authority for tiers. A tier is a model plus its effort level. The model
  pack only supplies the starting values that `shipit init` writes.
- The setup sets an **effort cap**: the highest effort any role may use, for example `high`. It
  bounds cost. No role, including a reviewer raising its effort to rank above the author, goes
  past the cap.

### 3. Five skills

| Skill | Started by | Input | Output |
|---|---|---|---|
| `shipit` | owner or lead | none | Explains the flow, rules, roles and terms. Routes to the others. |
| `shipit-roadmap` | lead | goal with done-when | A reviewed roadmap: slices under the cap, their order (`after`), which need a test drive. Stops at a decision point for the owner. |
| `shipit-slice` | lead, once per slice | roadmap, one slice | A slice ready to ship: build plan, its review, the work, receipts, known issues. Stops before ship. |
| `shipit-test-drive` | tester | goal, done-when, the finished result | Findings and a test-drive result. The tester never sees how the work was made. |
| `shipit-review` | owner, lead, or `shipit-slice` | a pull request or branch; the goal, done-when and build plan when there are any | A review: the reader's teach-back and where it differs from what the change claims, a reading order with the spots to look at first, findings, the security and performance answers, and the outcome. `shipit-slice` uses it for the informed review and the second opinion. Outside a ShipIt project it still runs and says which checks it skipped (rank, receipt, coverage). |

Each skill is written as a function: its inputs, its one output, the effects it may cause, and
every outcome with what follows it.

**Every output has a template**, so each skill and each role is a function from named inputs to
one output of a known shape. A template fixes:

- the **outcomes**: a closed list, for example a scout returns `found`, `partial` or `not found`;
- the **required parts** for each outcome, for example every fact a scout reports has a source;
- a small machine-readable header that names the outcome and the input it answered.

Templates exist for: a decision point, the roadmap, the build plan, a scout's report, model advice, a review (informed or second
opinion), a test drive, and a known issue. `shipit validate` checks a filled output against its
template and refuses one with an unlisted outcome or a missing part. The lead hands an output to
the next phase only after it validates.

| Role | Output | Outcomes |
|---|---|---|
| scout | report | found, partial, not found |
| planner | roadmap, build plan | ready for review |
| reviewer | review | approve, approve after fixes, rework |
| tester | test drive | pass, findings |
| researcher | model advice | ready for the owner |
| reader | explanation of the change, written without the build plan | understood, unclear |

A scout that returns `partial` or `not found` is asked once more with a narrower question and
where it already looked. If that is still not enough, the question goes to a scout one tier up,
and after that the requester reads the source itself. Each step is a logbook entry.

The test drive has two passes: first every done-when item as a checklist, then free exploration.

**Model advice at setup.** When a project adopts ShipIt, and again whenever the owner asks (for
example after new models come out), a researcher looks up current models, prices and published
results on the web and recommends a model and effort for each role, with sources, expected cost
and trade-offs. Its output follows the model-advice template. It is a decision point: the owner
picks, and only then are the values written into the setup file, where the rank rule checks them.
The tool itself never goes online; research is done by an agent through the skill.

### 4. Packs

| Kind | First pack | Says |
|---|---|---|
| Skill pack | Superpowers | Which Superpowers skill carries out each phase, and where ShipIt's rules override its defaults (for example, no review after every task). |
| Domain pack | Coding | A slice is a pull request. Size is the number of changed files against the base branch. Recheck is CI. The test drive runs in an empty directory with only the built result, its documentation and sample data. |
| Model pack | Claude | Tier 1 to 4 and the model for each, with the stand-in for each tier. |

Model names appear only in a model pack and in a project's setup file.

In version 1 a pack is text that the skills read. The tool does not load packs: it works on git
and measures size as changed files. A pack interface for the tool waits for a second domain.

### 5. Gates

| Gate | When | Done by |
|---|---|---|
| Review needs a receipt | Before a reviewer of the work or a tester starts. Reviewers of a roadmap or a build plan are not gated, because no work exists yet. | A Claude Code hook that runs `shipit verify` for the slice's working copy. It never runs the checks itself. |
| Push needs a receipt | Before `git push` of a slice branch. Other pushes pass. | A git `pre-push` hook that runs `shipit verify` on the newest commit of each pushed branch. A slice branch is a branch named `slice/<goal>/<slice>`; pushes of other branches pass, with a notice. |
| Recheck | On every pull request | CI runs `shipit check` from the start, at a pinned ShipIt commit, and trusts no receipt. |
| Ship needs the owner | Before any ship command: merging a pull request, pushing to the base branch, pushing a tag, creating a release | A Claude Code hook that turns the command into a question the owner must answer, in every permission mode. The owner may say yes and let the agent carry it out. |

- The local hooks are guardrails against forgetting, not security against an agent that wants to
  cheat. A review done by the lead itself, without starting a reviewer, is not caught by the
  hook; the skill forbids it and the missing receipt shows in `shipit status`.
- A pull request can edit its own CI job, the ShipIt pin and the check commands. Recheck
  therefore compares the setup file and the CI job with the base branch and says plainly, in the
  pull request, when a slice changes them. The owner sees that before shipping. A project that
  wants more can add branch rules on its host; ShipIt documents how but does not require it.
- Local copies of the tool update with the plugin. CI updates when the project moves its pin.
  The two may differ; that is safe, because recheck trusts no local receipt.

### 6. Harness

Version 1 runs on Claude Code and ships as a plugin. The files that only Claude Code needs are
few and listed in one place. The tool, the skills' content and the packs do not depend on them,
so a second harness is an addition, not a rewrite.

## Done-when

The goal is met when all of these are true.

**Tool**
1. `shipit check` refuses a dirty tree, fails a slice over the cap, runs the check commands in
   order, and writes a receipt only when every one passes.
2. `shipit verify` passes only for the exact version that was checked. A new commit with changed
   content fails it; an amended commit with the same content passes.
3. A malformed or missing setup file stops every command that needs a setup (`shipit help` shows
   which) with a clear message. Nothing runs on a guessed default.
4. `shipit status` shows each slice's state and next step without any stored status field.
   Deleting a receipt changes the answer.
5. A review receipt from a reviewer who does not rank above the author is refused: same model and
   same effort fails, same model and higher effort passes, and any effort above the setup's cap
   is refused.
6. The tool's own tests run in temporary git repositories and pass. `cargo clippy` reports nothing;
   every warning is an error.

**Gates**

7. With no valid check receipt, starting a reviewer or tester in Claude Code is blocked with a
   message that says what to run. With a valid receipt it is allowed. The hook finishes in under
   one second.
8. `git push` of a slice with no valid check receipt is blocked.
9. In Claude Code, merging a pull request, pushing to the base branch, pushing a tag and
   creating a release each stop for the owner's answer, also in auto mode. Pushing a slice
   branch does not.
10. CI on a pull request runs every check again and fails a pull request that is over the base
   branch's cap, whatever receipts exist locally.

**Skills and packs**

11. The five skills exist. Each states its inputs, output, allowed effects and outcomes. Every
    output in the table of templates has a template, and `shipit validate` refuses a filled output
    whose outcome is not listed or that misses a required part.
12. A script fails the build when a skill or pack uses a rejected term, a model name outside the
    model pack, or a harness name outside the harness files.
13. The Superpowers pack states every place where a ShipIt rule overrides a Superpowers default.

**Proof on real work**

14. ShipIt's own repository uses ShipIt: its setup file exists and its pull requests are
    rechecked by its own tool.
15. firectl adopts ShipIt by adding only a setup file and a CI job.
16. One real firectl slice goes through the whole flow, from roadmap to a pull request the owner
    ships. The pull request shows its receipts and its known issues.
17. Test drive of ShipIt itself: a fresh agent that has only the README and the docs sets ShipIt
    up in a sample project and takes one small slice to "ready to ship".
18. Several goals run at the same time. Each has a short name; its slices use branches `slice/<goal>/<slice>`, its done-when items are written `<goal>:DW<n>` where goals meet, and the slice cap counts across all goals.
19. When every slice and proof step of a goal has shipped, `shipit complete <goal>` keeps only the data a tool reads (each slice's metrics), turns the goal's open known issues into GitHub issues after the owner confirms, and deletes the goal's own files (goal, roadmap, build plans, logbook).

## Non-goals (version 1)

- A second harness, a second skill pack, or a domain pack other than coding.
- Work that does not live in git.
- Security against an agent that deliberately forges receipts.
- Merging or releasing automatically.
- Teaching how to plan, brainstorm or implement. That belongs to the skill pack.

## Anti-goals

ShipIt has failed, even if it works, when any of these is true.

- It grows into a long instruction file that agents skim.
- A rule that a tool could enforce lives only in prose.
- A slice's state is a flag someone sets by hand.
- An agent can ship.
- A hook is slow enough that people switch it off.
- A project needs more than a setup file and a CI job to adopt it.

## Approaches considered

**Where the machinery lives**

| Approach | Trade-off | Decision |
|---|---|---|
| One tool kept in ShipIt; a project adds a setup file and a CI job | Most work here. A fix is made once; projects get it by updating the plugin and moving their CI pin. Projects depend on ShipIt being installed and fetched in CI. | **Chosen by the owner.** |
| Copy scripts and hooks into each project | Nothing needed at run time. Copies drift; every update is manual in every project. | Rejected. |
| Contract only; each project builds its own gate | Smallest ShipIt. Every project rebuilds the same thing. | Rejected: defeats reuse. |

**Which harness first**

| Approach | Trade-off | Decision |
|---|---|---|
| Claude Code first, open to more later | Rules are enforced by real hooks. Other harnesses wait. | **Chosen by the owner.** |
| Many harnesses from day one | Wider, but rules become instructions where hooks are missing. | Rejected for version 1. |

**Language of the tool**

| Approach | Trade-off | Decision |
|---|---|---|
| Rust | Types and the compiler enforce more: newtypes, exhaustive matches, no panics, every warning an error. One binary, also as a static Linux build; no jq and no bash version at run time. Needs a build, which Nix does for CI and installs. | **Chosen by the owner** (2026-11-21, after slice check-and-verify was built in shell). |
| Shell, written in a functional style, with `jq` for the pure logic | No build step. Needs jq and bash 4.4 at run time; macOS ships bash 3.2. No types: discipline comes only from style and tests. | Rejected. It was chosen first, then replaced by Rust: one binary, no runtime dependencies, and types enforce the style. |

## Decided by the owner

- The name, the vocabulary and the flow.
- The four rules, including "reviewer one tier above the maker" and "only the owner ships".
- A reviewer on the same model as the author ranks higher by using a higher effort. The highest
  effort is capped in the setup.
- The cap is set only in a committed file. The first project's cap is 60 changed files.
- Checks run once per version and leave a receipt. Hooks only verify, because checks are slow.
- A slice is complete on its own. Work is never split into broken pieces. Parallel slices run in
  separate working copies.
- The test drive is a separate role from review and works without seeing how the work was made.
- Planning files stay out of git and are removed after the slice ships.
- Functional thinking guides the tool, the skills and the flow.
- Rule 4 is enforced by always asking: every ship command stops for the owner's answer. This is
  a guardrail. A separate login for agents, which cannot merge or tag, is documented as optional
  hardening and is not required.

## The lead's calls (the owner may overrule)

- The setup file is TOML, not JSON (the owner's call, 2026-11-21): it takes comments; receipts and the
  logbook stay JSON.
- Receipts and the logbook are JSON files; the logbook is one JSON entry per line.
- Roadmaps, build plans and logbooks stay out of git history, in one place that every working
  copy of the project shares. Receipts sit beside them.
- A receipt's key is the exact input it judged: tree hash for a check or a test drive, patch id
  for a review of the work, file hash for a review of a plan.
- "Shipped" is a logbook entry, because a squash merge changes hashes and the planning files are
  removed afterwards.
- In version 1, packs are text for the skills; the tool is wired to git and changed files.
- Recheck reports any change a slice makes to the setup file or the CI job.
- When the author already uses the top model, the reviewer uses the same model at a higher effort.
  Only when the author is at the top model and at the effort cap is rule 1 met by a fresh agent at
  the same rank. The receipt records which of these applied.
- Review and test-drive receipts are recorded by the lead through the tool and are treated as
  notes, not proof.
- The tool is offered as a Nix flake package, so it is on the path for git hooks and people; a
  static Linux binary is built as well.
- firectl's planned check gate is built here as part of the tool. firectl then only adopts it.

## Context for the builder

- ShipIt's repository is public, so CI in any project can fetch it without a secret.
- firectl is a Rust command-line tool. Its CI is GitHub Actions and already has Nix. Its checks
  are `cargo fmt --check`, `cargo clippy --all-targets` and `cargo nextest run`. Its cap is 60.
- The first skill pack targets Superpowers 7.0.2.
- Tagging and releasing count as shipping, as merging does.

## Left to the builder

- The exact fields of a receipt and a logbook entry, beyond: kind, version of the work, who,
  model and tier, result, time.
- The file layout inside `.shipit/` and where receipts are kept inside the git directory.
- How the Claude Code hook recognises a reviewer or tester.
- The form of the CI entry point and how it is pinned.
- The test runner for the tool.
- The roadmap: which slices build this, and in what order.
