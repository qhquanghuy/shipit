## Form

```toml
kind = "second-opinion"
outcome = "approve | approve-after-fixes | rework"
input = "the change alone, without the goal or the build plan"
```

## Summary
One short paragraph: what you think the change does and how sure you are. Every outcome.

## Security
Items that start `none found: `, `concern: ` or `not relevant: `, then the reason. Every outcome; never leave it out.

## Performance
Items that start `none found: `, `concern: ` or `not relevant: `, then the reason. Structural problems only: chatty I/O, extraneous fetching, N+1 queries, no batching, repeated work, hidden quadratic work, unbounded growth, synchronous I/O in loops, missing timeouts. No micro-tuning unless the owner asks or the goal sets a budget.

## Findings
Needed for approve-after-fixes and rework (at least one must-fix finding); allowed with approve only when none is a must fix. One item per finding, written as a Conventional Comment: `issue (blocking, security): text`, `suggestion (non-blocking): text`, `question: text` or `nitpick: text`. The owner reads blocking as must fix, non-blocking and nitpick as nice to fix, and a question as must answer. No praise.

## Verified
Items that name the command or test that ran, in a code span, then what it showed: `shipit check`, `bats tests/x.bats`, a test name. Write `none` as the only item when nothing was run. Every outcome.

## Inferred
Items reasoned from the code or the docs and not run, each with what it rests on. This is the list the reader of the review must check. Write `none` as the only item when there are none. Every outcome.

## Unverifiable
Items that could not be checked here, each with why: no network, needs the owner's account, a live session. Write `none` as the only item when there are none. Every outcome.
