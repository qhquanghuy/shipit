# ShipIt vocabulary (locked 2026-11-21)

Framework name: **ShipIt**. Skills: `shipit`, `shipit-roadmap`, `shipit-slice`, `shipit-test-drive`.

Flow: goal → roadmap → build plan → make → check → informed review + second opinion + test drive* → fix rounds → recheck → ship
(* only slices the roadmap marks)

## Terms
| Term | Meaning | Coding-pack example |
|---|---|---|
| flow | the fixed order of phases, goal to ship | issue to merged PR |
| goal | what the owner wants and why, with its done-when | a GitHub issue, or a milestone for a version |
| done-when | what must be true for the goal to count as met; written before work | "every `--help` example runs" |
| roadmap | the ordered list of slices: contents, dependencies, parallel groups, which need a test drive | 0.2.0: A+D in parallel, then B, C |
| slice | one piece complete end to end, under the cap, shippable alone; never a broken piece | one PR that leaves main releasable |
| cap | size limit per slice; the domain pack says how size is measured | ≤ 60 changed files |
| build plan | how one slice is made and how each part is checked | Superpowers plan naming tests and commands |
| make | doing the slice | implementing on a branch |
| check | a script, pass/fail, no judgement; also the phase | `scripts/check`: size, fmt, clippy, tests |
| receipt | record of a check, review or test drive: who, on exactly which version, result | stamp keyed by git tree hash |
| informed review | review that knows the goal and build plan | code review with context |
| second opinion | review that sees only the work | fresh code review |
| test drive | using the result as its audience would, knowing only goal and done-when; never sees the code | fresh agent in a temp dir with binary, docs, seed data |
| finding | one problem reported by a review or test drive | |
| fix round | fix findings, then check and review / test drive again; limited | at most 2 per slice |
| known issue | a finding left open on purpose, written down with a reason, passed to the owner | listed in the PR body |
| recheck | every check rerun where the maker has no control; trusts no receipt | CI on the PR |
| ship | the owner's yes that lets work out; only the owner | owner merges; owner says go for tags |
| decision point | where the flow stops and asks the owner | roadmap approval, disagreements, merge |
| rule | holds in every domain; no pack or setup can switch it off | the four rules |
| setup | the project's one config file: cap, fix-round limit, tiers → models, packs | repo config file |
| tier | a level of model strength | 1 Haiku, 2 Sonnet, 3 Opus, 4 Fable |
| stand-in | model used when a tier's model is unavailable | Opus high for Fable |
| pack | plug-in: skill pack (how phases are done), domain pack (what slice/cap/check/test drive mean), model pack (other LLMs). Rules beat pack defaults | Superpowers; coding; Claude |
| logbook | append-only record per slice of what happened: events, known issues, stand-ins used. A slice's state is computed from it and from the receipts; it is never written down as a status | shared file outside git history, removed after merge |

## Roles
| Role | Job | Coding-pack example |
|---|---|---|
| owner | sets the goal, answers decision points, ships; always a person | the user |
| lead | runs the flow, hands out roles | main session |
| scout | collects background facts | tier 1 subagent |
| planner | writes the roadmap and build plans | usually the lead |
| maker | does make | tier 2, high effort |
| reviewer | informed review or second opinion; one tier above whoever made the thing | Opus reviewing Sonnet |
| tester | does the test drive | fresh isolated agent |
| reader | teach-back: explains the diff without the plan, as a signal for the informed reviewer | tier 1 agent, ranked below the maker |

## Four rules
1. Someone else confirms your work, one tier up.
2. Nothing counts without a receipt.
3. Known issues are written down, never hidden.
4. Only the owner ships.

## Writing rules
- Never bare "plan": say "roadmap" or "build plan".
- Never bare "test" for the test drive.
- Not framework terms: task, unit, artifact, workflow, pipeline, milestone (GitHub's), release.
