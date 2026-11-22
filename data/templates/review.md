## Form

```toml
kind = "review"
outcome = "approve | approve-after-fixes | rework"
input = "the change, its claim and the reader's explanation; the goal and the build plan when there are any"
```

## Summary
One short paragraph: what the change does and why you reached this outcome. Every outcome.

## Claim
What the change says it does: the title and body of the pull request, or the commit messages, in a few lines. Every outcome.

## Teach-back
One item per part the reader explained, each starting `[matches] `, `[wrong] ` or `[unclear] `, then why. Say where the reader's reading differs from the claim, and where the claim differs from the change. Every outcome. A signal for you, not a gate.

## Reading path
The way through the change, one flow for each entry point it adds or alters (a hook event, an API handler, a command), each under a `### Flow: <entry point>` heading. One item per step, in call order from the entry's input and output inward: the function or type in a code span, what it takes, what it returns, what it calls next, a risk word (`risk: high`, `risk: medium` or `risk: low`) with why on a step that needs care, and where it is: a diff link (`https://github.com/<owner>/<repo>/pull/<n>/files#diff-<sha256 of the path>R<line>`) or `<file>:<line>`. After the steps of each flow, its Mermaid `sequenceDiagram` in a `mermaid` fence, drawn from the same steps; every flow has one. Write `none` as the only item when the change adds or alters no entry point (then there are no flows). Every outcome.

## Other files
The changed files that are on no path: tests, docs, configuration. One item per group, naming the files and a risk word (`risk: high`, `risk: medium` or `risk: low`) with why. Write `none` as the only item when every changed file is on a path. Every outcome.

## Look here
First every changed test: one item each, saying whether the change edited, removed or weakened it and whether it still proves what it proved (a weakening is a must-fix finding unless the build plan says why). Then two or three spots you are least sure of. Each item says where it is, as `<file>:<line>` or a diff link, then why it needs a look. Every outcome.

## Findings
Needed for approve-after-fixes and rework (at least one must-fix finding); allowed with approve only when none is a must fix. One item per finding, written as a Conventional Comment: `issue (blocking, security): text`, `suggestion (non-blocking): text`, `question: text` or `nitpick: text`. Two reviews that found the same problem make one item that says both found it. The owner reads blocking as must fix, non-blocking and nitpick as nice to fix, and a question as must answer. No praise.

## Security
Items that start `none found: `, `concern: ` or `not relevant: `, then the reason. Every outcome; never leave it out. An approve with any `concern: ` item is refused, even when a non-blocking finding names it: a concern means approve-after-fixes or rework.

## Performance
Items that start `none found: `, `concern: ` or `not relevant: `, then the reason. Structural problems only, from the coding pack's list. No micro-tuning unless the owner asks or the goal sets a budget. An approve with any `concern: ` item is refused, even when a non-blocking finding names it: a concern means approve-after-fixes or rework.

## Skipped
What this review did not do, each with why: a check that was not run (rank, receipt, coverage, validation) or a file that was not read. Write `none` as the only item when nothing was skipped. Every outcome.

## Verified
Items that name the command or test that ran, in a code span that is not empty, then what it showed: `shipit check`, `bats tests/x.bats`, a test name. Write `none` as the only item when nothing was run. Every outcome.

## Inferred
Items reasoned from the code or the docs and not run, each with what it rests on. This is the list the reader of the review must check. Write `none` as the only item when there are none. Every outcome.

## Unverifiable
Items that could not be checked here, each with why: no network, needs the owner's account, a live session. Write `none` as the only item when there are none. Every outcome.
