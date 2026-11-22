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
