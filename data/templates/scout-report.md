## Form

```toml
kind = "scout-report"
outcome = "found | partial | not-found"
input = "one question about the project"
```

## Question
The question you were asked, in your own words. Every outcome.

## Facts
Needed for found and partial. One item per fact, each with its source in the form `(source: path:line or link)`.

## Missing
Needed for partial and not-found. What you could not find out.

## Where I looked
Needed for partial and not-found. The places you searched, so nobody repeats the search.

## Verified
Items that name the command or test that ran, in a code span that is not empty, then what it showed: `shipit check`, a test name. Write `none` as the only item when nothing was run. Every outcome.

## Inferred
Items reasoned from what you read and not run, each with what it rests on. Write `none` as the only item when there are none. Every outcome.

## Unverifiable
Items that could not be checked here, each with why: no network, needs the owner's account, a live session. Write `none` as the only item when there are none. Every outcome.
