## Form

```toml
kind = "build-plan"
outcome = "ready-for-review"
input = "one slice of the roadmap, its goal and the done-when items it meets"
```

## Goal
What the slice makes true, in one or two sentences, and which done-when items it meets.

## Architecture
How the change is built: the types, the modules and where the effects sit. Name each file the slice adds or changes.

## Global constraints
Rules that hold for every step: the project's own rules, the files the slice may not touch, the exit codes and messages that are fixed by a contract.

## Done-when coverage
Which tests prove each done-when item the slice meets, as one toml block under this heading. A key is a done-when id such as DW3; its value lists test names that carry no done-when id. An empty block says the slice meets none.

```toml
DW1 = ["a test name", "another test name"]
```

## Steps
The steps in order. Each step names its files and how it is verified: the command that must pass, and the test that is written first.

## Review focus
The few places a reviewer should look first, each with the test that holds it.

## Needs from other slices
What this slice takes from the others, and what it gives them. Write none when nothing is shared.

## Decisions for the owner
Choices still open that only the owner can make. Write none when there are none.
