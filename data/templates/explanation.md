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

## Verified
Items that name the command or test that ran, in a code span that is not empty, then what it showed: `shipit check`, a test name. Write `none` as the only item when nothing was run. Every outcome.

## Inferred
Items reasoned from what you read and not run, each with what it rests on. Write `none` as the only item when there are none. Every outcome.

## Unverifiable
Items that could not be checked here, each with why: no network, needs the owner's account, a live session. Write `none` as the only item when there are none. Every outcome.
