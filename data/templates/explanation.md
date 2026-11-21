## Form

```toml
kind = "explanation"
outcome = "understood | unclear"
input = "the diff alone, without the goal or the build plan"
```

## Parts
One item per part of the change, each written `path:line: what it does` or `path:first-last: what it does`. Every outcome. Explain only from the diff.

## Unclear
Needed for unclear. One item per part you could not follow, written `path:line: why` or `path:first-last: why`. This is a signal for the informed reviewer, not a gate.
