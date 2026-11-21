# Formats

Every machine-readable format ShipIt reads is written as a fenced `toml` block (or, for the goal, as a numbered list) in a
Markdown file. This page says each one in plain words. Terms are in [vocab.md](vocab.md); the setup file is in
[setup.md](setup.md). Every problem is one line, `<path>: <message>`, in the same form as the setup's.

The hidden command `shipit format` runs the readers and prints what they parsed as one line of compact JSON. It is how
the formats are tested; other commands call the same readers.

## Reading a text file

A file is read as UTF-8 text. Nothing is created. Exit 2 with one of:

| Message | When |
|---|---|
| `no file at <path>` | the path is missing, is a folder, or cannot be read |
| `<path> is not UTF-8 text` | the bytes are not UTF-8 |

## Fenced block

The block is the first one opened by a line that is exactly `` ```toml ``, after a line that is exactly the heading, and
closed by a line that is exactly `` ``` ``. Blocks before the heading, and blocks after the first one, are ignored. A
block that is never closed is no block.

## Roadmap

Heading `## Slices`. `shipit format roadmap <file> [--goal <goal file>]` reads it.

```toml
[[slice]]
name = "alpha"            # lowercase letters, digits and "-", starting with a letter; not "roadmap"
after = []                # names of slices or proof steps; no repeats; not itself
meets = ["DW1"]           # done-when ids, DW then a number from 1; no repeats
owns = ["src/alpha.rs", "docs/"]   # at least one path; a trailing "/" makes a folder
test_drive = false
branch = "slice/alpha"    # a branch name; not the setup's gate.base; unique

[[proof]]                 # work outside a slice (an owner's live test, another repository)
name = "firectl-proof"
after = ["alpha"]
meets = ["DW15", "DW16"]
how = "a pull request to firectl, run from the firectl session"
```

The top level holds exactly `slice` (required, at least one) and `proof` (optional). A slice has exactly the keys
above; a proof step has `name`, `after`, `meets` and `how`, and no `branch`. Names are unique across slices and proof
steps. Indexes in paths start at 0 (`.slice[2].owns[0]`). Roadmap order is the slices, then the proof steps.

A done-when item that no automated test can prove is met by a proof step, never by a slice.

The rules that need the whole roadmap run only when every element is valid on its own, in this order: unknown names,
then a cycle, then overlapping `owns`.

| Path | Message |
|---|---|
| `.<key>` | `unknown key`, `missing` |
| `.slice` | `must be a non-empty array of objects` |
| `.proof` | `must be an array of objects` |
| `.slice[i].name` | `must be lowercase letters, digits and "-", starting with a letter, and not "roadmap"` |
| `.slice[i].name` | `"<name>" is used twice` |
| `.slice[i].after` | `must be an array of slice or proof step names` |
| `.slice[i].after[j]` | `must be a slice or proof step name` |
| `.slice[i].after[j]` | `"<name>" is listed twice` |
| `.slice[i].after[j]` | `"<name>" cannot come after itself` |
| `.slice[i].meets` | `must be an array of done-when ids` |
| `.slice[i].meets[j]` | `must be a done-when id such as "DW4"` |
| `.slice[i].meets[j]` | `"<id>" is listed twice` |
| `.slice[i].owns` | `must be an array of non-empty strings`, `must list at least one path` |
| `.slice[i].owns[j]` | `must be a non-empty string` |
| `.slice[i].test_drive` | `must be true or false` |
| `.slice[i].branch` | `must be a branch name (see git check-ref-format --branch)` |
| `.slice[i].branch` | `must not be the base branch "<base>"` |
| `.slice[i].branch` | `"<branch>" is used twice` |
| `.proof[i].how` | `must be a non-empty string` |
| `.slice[i].after[j]` | `no slice or proof step named "<name>"` |
| `.slice` | `after makes a cycle: a, b, a` |
| `.slice[i].owns[j]` | `"<path>" is also owned by <name>, which runs in parallel` |

`.proof[i]` replaces `.slice[i]` for a proof step. A cycle is named from the first element, in order, that lies on one,
along the shortest way back to itself, following `after`; for a proof step the path is `.proof`.

**Parallel.** Two elements run in parallel when neither reaches the other through `after`, directly or through others.
Two parallel slices may not own the same path. A path that ends in `/` is a folder and holds itself and every path below
it (`src/a/` holds `src/a/x.rs`); `src/a` holds only itself, and `src/a/` does not hold `src/ab.rs`. The message names
the earlier slice, at the later one's path.

**Waves.** An element with no `after` is in wave 1; any other is one wave after the latest of its `after`. The command
prints the names of each wave, in roadmap order.

## Goal

The goal's done-when items are the lines between a line that is exactly `## Done-when` and the next line that starts with
`## `, skipping every line inside a fenced block. An item is a line that starts `<n>. ` (a number from 1, no sign, no
leading zero, no leading space); it is the id `DW<n>`.

| Message | When |
|---|---|
| `goal: DW<n> is numbered twice` | a number is used by two items |
| `goal: no numbered items under "## Done-when"` | there is no section or no item |

With `--goal`, the roadmap is checked against the items. First, in path order, `<path>.meets[j]: "DW99" is not a
done-when item of the goal`; then, in number order, `DW3: no slice` for each item that no slice or proof step meets.

## `shipit format roadmap`

Reads the setup (for `gate.base`) from `HEAD`, then the files given, relative to the current folder. Prints
`{"roadmap": …, "waves": [[…], …], "goal": ["DW1", …] | null}`. Every failure is exit 2:

| Message | When |
|---|---|
| `usage: shipit format <roadmap\|map\|findings> …` | no form, or an unknown one |
| `usage: shipit format roadmap <file> [--goal <goal file>]` | other arguments |
| `<file> has no toml block under "## Slices"` | no block |
| `<file> is not valid TOML: <why>` | the block does not parse |
| `<file> is invalid:` and one problem per line | any problem above, including the goal checks |

## Done-when map

A committed file `.shipit/coverage/<slice>.toml`, and the same table in the build plan under `## Done-when coverage`. It
says which tests prove each done-when item the change meets.

```toml
DW4 = ["status: a slice walks from planned to shipped", "the_state_is_the_longest_chain_that_holds"]
```

Each key is a done-when id; each value is a non-empty array of distinct, non-empty test names. A test name must not
carry a done-when id (`dw` in any case, then a digit): ids live in the plan, not in the code. An empty table is valid: it
says the change meets no done-when item. Problems are in file order.

| Path | Message |
|---|---|
| `.<key>` | `must be a done-when id such as "DW4"` |
| `.DW4` | `must be an array of non-empty strings`, `must list at least one test name` |
| `.DW4[j]` | `must be a non-empty string` |
| `.DW4[j]` | `a test name must not carry a done-when id` |
| `.DW4[j]` | `"<name>" is listed twice` |

`shipit format map <file>` reads a TOML file or, for a name ending `.md`, the block under `## Done-when coverage`. It
prints the map as compact JSON in number order (`{"DW2": […], "DW10": […]}`).

| Message | When |
|---|---|
| `usage: shipit format map <file>` | other arguments |
| `<file> has no toml block under "## Done-when coverage"` | a `.md` file without the block |
| `<file> is not valid TOML: <why>` | the file or block does not parse |
| `<file> is invalid:` and one problem per line | any problem above |

## Findings

A finding is one line, `<label> [(<decorations>)]: <text>` (Conventional Comments). Labels: `issue`, `suggestion`,
`question`, `nitpick`. Decorations are comma-separated: `blocking`, `non-blocking`, or a perspective (lowercase letters
and `-`, such as `security`).

| Label | Decoration | Class |
|---|---|---|
| `issue`, `suggestion` | exactly one `blocking` | must fix |
| `issue`, `suggestion` | exactly one `non-blocking` | nice to fix |
| `nitpick` | no `blocking` | nice to fix |
| `question` | neither | must answer |

Ids are `F1…Fn` in the order given. Every refusal is `finding "<finding>": <why>`:

| Why | When |
|---|---|
| `must start with issue, suggestion, question or nitpick, then ": "` | no known label, or no `: ` after it and its decorations |
| `"<word>" is not a decoration` | a decoration that is not `blocking`, `non-blocking` or a perspective |
| `issue and suggestion need (blocking) or (non-blocking)` | neither, or both |
| `a nitpick is never blocking` | `nitpick (blocking)` |
| `a question is neither blocking nor non-blocking` | `question (blocking)` or `question (non-blocking)` |
| `the text after ": " is empty` | nothing after `: ` |

## Results

Review results are `approve`, `approve-after-fixes`, `rework`; test-drive results are `pass`, `findings`. A review or a
test drive is accepted only with a result of its own kind. The blocking rule: `approve` and `pass` allow no must-fix
finding; `approve-after-fixes`, `rework` and `findings` need at least one. Must-answer findings go with any result, and
nice-to-fix findings never decide one.

| Message | When |
|---|---|
| `<result> is a test-drive result` | a review with `pass` or `findings` |
| `<result> is a review result` | a test drive with `approve`, `approve-after-fixes` or `rework` |
| `<result> needs a must-fix finding` | `approve-after-fixes`, `rework` or `findings` without one |
| `<result> allows no must-fix finding` | `approve` or `pass` with one |

`shipit format findings <review|test-drive> <result> [<finding>…]` prints `{"result": …, "findings": […]}`. Findings
that cannot be read are all named, one per line, before the result is judged.

| Message | When |
|---|---|
| `usage: shipit format findings <review\|test-drive> <result> [<finding>…]` | missing arguments, or a kind that is not `review` or `test-drive` |
| `"<word>" is not a result: approve, approve-after-fixes, rework, pass or findings` | an unknown result |
