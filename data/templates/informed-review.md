## Form

```toml
kind = "informed-review"
outcome = "approve | approve-after-fixes | rework"
input = "the change, the goal, the build plan and the reader's explanation"
```

## Summary
One short paragraph: what the change does and why you reached this outcome. Every outcome.

## Teach-back
One item per part the reader explained, each starting `[matches] `, `[wrong] ` or `[unclear] `, then why. Compare the reader's explanation with the goal and the build plan, part by part. Every outcome. A signal for you, not a gate.

## Simple code
One item: how easy the change is to review. Look at reuse before adding, dead or duplicate code, and comments that state decisions only. Every outcome.

## Security
Items that start `none found: `, `concern: ` or `not relevant: `, then the reason. Every outcome; never leave it out.

## Performance
Items that start `none found: `, `concern: ` or `not relevant: `, then the reason. Structural problems only, from the coding pack's list. No micro-tuning unless the owner asks or the goal sets a budget.

## Findings
Needed for approve-after-fixes and rework (at least one must-fix finding); allowed with approve only when none is a must fix. One item per finding, written as a Conventional Comment: `issue (blocking, security): text`, `suggestion (non-blocking): text`, `question: text` or `nitpick: text`. The owner reads blocking as must fix, non-blocking and nitpick as nice to fix, and a question as must answer. No praise.
