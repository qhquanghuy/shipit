## Form

```toml
kind = "plan-review"
outcome = "approve | approve-after-fixes | rework"
input = "a roadmap or a build plan, and the goal it answers"
```

## Summary
One short paragraph: what you reviewed and why you reached this outcome. Every outcome.

## Done-when
One item per done-when item the roadmap or build plan has to meet (a roadmap: every item of the goal; a build plan: the items of its slice), each starting `[covered] ` or `[gap] `, then the done-when id (`- [covered] DW3: slice alpha`, `- [gap] DW4: no slice meets it`), then the slice or test that covers it, or what is missing. An approve with any `[gap] ` item is refused, wherever a line starts with it in the file, headings included, except inside a code fence. Every outcome.

## Findings
Needed for approve-after-fixes and rework (at least one must-fix finding); allowed with approve only when none is a must fix. One item per finding, written as a Conventional Comment: `issue (blocking): text`, `suggestion (non-blocking): text`, `question: text` or `nitpick: text`. The owner reads blocking as must fix, non-blocking and nitpick as nice to fix, and a question as must answer. No praise.

## Verified
Items that name the command or test that ran, in a code span that is not empty, then what it showed: `shipit validate roadmap`, a test name. Write `none` as the only item when nothing was run. Every outcome.

## Inferred
Items reasoned from the text and not run, each with what it rests on. This is the list the reader of the review must check. Write `none` as the only item when there are none. Every outcome.

## Unverifiable
Items that could not be checked here, each with why: no network, needs the owner's account, a live session. Write `none` as the only item when there are none. Every outcome.
