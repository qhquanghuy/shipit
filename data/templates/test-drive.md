## Form

```toml
kind = "test-drive"
outcome = "pass | findings"
input = "the built result, its docs and sample data, with the goal and the done-when"
```

## Done-when
One item per done-when item you tried, each starting `[pass] ` or `[fail] `, then what you did and saw. Every outcome.

## Exploration
What else you tried as a user of the result, and what happened. Every outcome.

## Findings
Needed for findings (at least one must-fix finding); allowed with pass only when none is a must fix. One item per finding, written as a Conventional Comment: `issue (blocking): text`, `suggestion (non-blocking): text`, `question: text` or `nitpick: text`.
