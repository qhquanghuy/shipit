# Formats

Every machine-readable format ShipIt reads is written as a fenced `toml` block (or, for the goal, as a numbered list) in a
Markdown file. This page says each one in plain words. Terms are in [vocab.md](vocab.md); the setup file is in
[setup.md](setup.md). Every problem is one line, `<path>: <message>`, in the same form as the setup's.

The hidden command `shipit format` runs the readers and prints what they parsed as one line of compact JSON. It is how
the formats are tested; other commands call the same readers. `shipit format messages` prints every message the code can print, one per line,
and a test holds this page against it: every message
in a table's `Message` column is one of them, and every one of them is in one of those
columns. The messages marked below as shared come from the setup's rules.

## Reading a text file

A file is read as UTF-8 text. Nothing is created. Exit 2 with one of:

| Message | When |
|---|---|
| `no file at <path>` | the path is missing, is a folder, or cannot be read |
| `<path> is not UTF-8 text` | the bytes are not UTF-8 |

## Fenced block

A section is the lines under a heading line (`## Slices`, written exactly, no trailing space) up to the next line that
starts with `# ` or `## `. Fences are read as in CommonMark: a line of three or more backticks or tildes, up to three
spaces in, opens a fence, and a line of the same mark, at least as long, closes it. A heading, a block or a numbered
line inside a fence is text, not structure. A byte order mark at the start and carriage returns are ignored.

The block is the one fence in the section whose info string is exactly `toml` (so not `TOML`, not `toml x`). A fence
inside another fence is never a block, and a block in another section is never borrowed. If the heading is missing or
the section has no block, there is no block. These are errors, never guesses, in every file the formats read (roadmap, goal, plan):

| Message | When |
|---|---|
| `a fence opened at line <line> is never closed` | a fence anywhere in the file has no closing line |
| `"<heading>" appears twice` | the heading is there twice outside a fence |
| `more than one toml block under "<heading>"` | the section has two `toml` fences |
| `raw HTML opens at line <line>, which hides text from the reader` | a line outside a fence, anywhere in the file, starts (after spaces) with `<` and a letter, `/`, `!` or `?`, or holds `<!--` |

TOML errors give line numbers that count from the first line of the block, not of the file.

## Roadmap

Heading `## Slices`. `shipit format roadmap <file> [<goal file>]` reads it; it needs no setup and no repository.

```toml
goal = "demo"             # the goal's name: lowercase letters, digits and "-", starting with a letter

[[slice]]
name = "alpha"            # lowercase letters, digits and "-", starting with a letter; not "roadmap"
after = []                # names of slices or proof steps; no repeats; not itself
meets = ["DW1"]           # done-when ids of this goal: DW1 or demo:DW1; no repeats
owns = ["src/alpha.rs", "docs/"]   # at least one path, no repeats
test_drive = false

[[proof]]                 # work outside a slice (an owner's live test, another repository)
name = "firectl-proof"
after = ["alpha"]
meets = ["DW15", "DW16"]
how = "a pull request to firectl, run from the firectl session"
```

The top level holds exactly `goal` and `slice` (required, at least one) and `proof` (optional). A slice has exactly the
keys above; a proof step has `name`, `after`, `meets` and `how`. Names are unique across slices and proof steps. Indexes
in paths start at 0 (`.slice[2].owns[0]`). Roadmap order is the slices, then the proof steps.

**Goal names.** A roadmap names its goal. A slice's branch is `slice/<goal>/<slice>`; it is derived, never written in the
file. Inside a roadmap a plain `DW<n>` is an id of its own goal, and `<goal>:DW<n>` is accepted only when it names that
same goal. Views that span goals write the qualified form, and `shipit format roadmap` prints every done-when id
qualified. That a goal name is unique among goals that have not shipped is not checked here: receipts-and-status owns it,
because only it sees more than one goal.

**Paths.** An `owns` path is written as git lists it: relative, no `.` or `..` segment, no empty segment, no segment that starts or ends with a space, no `\`, `*`, `?` or
`[`, and only printable ASCII characters (so no control character and no non-ASCII one). A trailing `/` is dropped, so `docs/` and `docs` are one path. A path holds itself and every
path below it (`docs` holds `docs/x.md`; `src/a` does not hold `src/ab.rs`). Case and Unicode spellings are not compared: `SRC/a` is another path (a known issue).

A done-when item that no automated test can prove is met by a proof step, never by a slice.

The rules that need the whole roadmap run only when every element is valid on its own, in this order: unknown names,
then a cycle, then overlapping `owns`.

| Path | Message |
|---|---|
| `.<key>` | `unknown key`, `missing` (shared) |
| `.slice[i]` | `must be an object` (shared) |
| `.goal` | `must be lowercase letters, digits and "-", starting with a letter` |
| `.slice` | `must be a non-empty array of objects` |
| `.proof` | `must be an array of objects` |
| `.slice[i].name` | `must be lowercase letters, digits and "-", starting with a letter, and not "roadmap"` |
| `.slice[i].name` | `"<name>" is used twice` |
| `.slice[i].after` | `must be an array of slice or proof step names` |
| `.slice[i].after[j]` | `must be a slice or proof step name` |
| `.slice[i].after[j]` | `"<item>" is listed twice` |
| `.slice[i].after[j]` | `"<name>" cannot come after itself` |
| `.slice[i].meets` | `must be an array of done-when ids` |
| `.slice[i].meets[j]` | `must be a done-when id of this goal, written DW4 or <goal>:DW4` |
| `.slice[i].owns` | `must be an array of paths`, `must list at least one path` |
| `.slice[i].owns[j]` | `must be a path inside the repository, written like src/a.rs or docs/` |
| `.slice[i].test_drive` | `must be true or false` |
| `.proof[i].how` | `must be a non-empty string` |
| `.slice[i].after[j]` | `no slice or proof step named "<name>"` |
| `.slice` | `after makes a cycle: <names>` |
| `.slice[i].owns[j]` | `"<path>" is also owned by <name>, which runs in parallel` |

`.meets[j]` and `.owns[j]` also say `"<item>" is listed twice` for a repeat (after the path is normalized). `.proof[i]`
replaces `.slice[i]` for a proof step. A cycle is named from the first element, in order, that lies on one, along the
shortest way back to itself, following `after`; for a proof step the path is `.proof`.

**Parallel.** Two elements run in parallel when neither reaches the other through `after`, directly or through others.
Two parallel slices may not own paths that overlap. The message names the earlier slice, at the later one's path.

**Waves.** An element with no `after` is in wave 1; any other is one wave after the latest of its `after`. The command
prints the names of each wave, in roadmap order.

## Goal

The goal's done-when items are the lines under a line that is exactly `## Done-when`, up to the next heading, outside
fences, as above. An item is a line that starts `<n>. ` (a number from 1, no sign, no leading zero, no leading space); it
is the id `DW<n>`.

| Message | When |
|---|---|
| `goal: DW<n> is numbered twice` | a number is used by two items |
| `goal: no numbered items under "## Done-when"` | there is no section or no item |
| `goal: <why>` | the file cannot be read with certainty: any message of the fenced block table that applies |

With a goal file, the roadmap is checked against the items:

| Path | Message |
|---|---|
| `<path>.meets[j]` | `"<id>" is not a done-when item of the goal` |
| `DW3` | `no slice` |

The first kind comes first, in path order; the second, in number order, names each item that no slice or proof step meets.

## `shipit format roadmap`

Prints `{"roadmap": …, "waves": [[…], …], "goal": ["demo:DW1", …] | null}`; done-when ids are printed qualified, and
paths are printed as normalized. Every failure is exit 2:

| Message | When |
|---|---|
| `usage: shipit format <roadmap\|map\|findings\|messages> …` | no form, or an unknown one |
| `usage: shipit format roadmap <file> [<goal file>]` | other arguments |
| `<file> has no toml block under "<heading>"` | no block under `## Slices` (or `## Done-when coverage` for `map`) |
| `<file>: <why>` | the file cannot be read with certainty: any message of the fenced block table |
| `<file> is not valid TOML: <why>` | the block does not parse |
| `<file> is invalid:` | followed by one problem per line, any problem above, including the goal checks (shared) |

## Done-when map

A committed file `.shipit/coverage/<goal>/<slice>.toml`, and the same table in the build plan under `## Done-when
coverage`. It says which tests prove each done-when item the change meets. Keys are plain `DW<n>`: the goal is in the path.

```toml
DW4 = ["status: a slice walks from planned to shipped", "the_state_is_the_longest_chain_that_holds"]
```

Each key is a done-when id; each value is a non-empty array of distinct, non-empty test names. A test name must not
carry a done-when id: `dw` in any case followed at once by a digit (`dw4`, `DW12`; not `dw 4` or `DW-4`). Ids live in the
plan, not in the code. An empty table is valid: it says the change meets no done-when item. Problems are in file order.

| Path | Message |
|---|---|
| `.<key>` | `must be a done-when id such as "DW4"` |
| `.DW4` | `must be an array of non-empty strings`, `must list at least one test name` |
| `.DW4[j]` | `must be a non-empty string` |
| `.DW4[j]` | `a test name must not carry a done-when id` |

`shipit format map <file>` reads a TOML file or, for a name ending `.md` (in any case), the block under `## Done-when
coverage`. It prints the map as compact JSON in number order (`{"DW2": […], "DW10": […]}`).

| Message | When |
|---|---|
| `usage: shipit format map <file>` | other arguments |

## Findings

A finding is one line, `<label> [(<decorations>)]: <text>` (Conventional Comments), with no control character in it.
Labels: `issue`, `suggestion`, `question`, `nitpick`. Decorations are comma-separated: `blocking`, `non-blocking`, or a
perspective (lowercase letters and `-`, such as `security`).

| Label | Decoration | Class |
|---|---|---|
| `issue`, `suggestion` | exactly one `blocking` | must fix |
| `issue`, `suggestion` | exactly one `non-blocking` | nice to fix |
| `nitpick` | no `blocking` | nice to fix |
| `question` | neither | must answer |

Ids are `F1…Fn` in the order given. Every refusal is `finding "<finding>": <why>` (a control character in the finding is
shown escaped, so a refusal is one line); the `<why>` is one of:

| Message | When |
|---|---|
| `finding "<finding>": <why>` | the form of every refusal below |
| `a finding is one line, without control characters` | a newline, tab or other control character |
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
