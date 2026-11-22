# The setup file

ShipIt reads one file per project: `.config/shipit.toml` at the top of the repository, written in
[TOML](https://toml.io). Comments are allowed in it. No
environment variable changes any value in it. The schema is checked in full
whenever a command reads the file. `shipit init` writes the starting setup, `agents.parallel` included.

## Example

```toml
version = 1

[gate]
base = "main"
max_files = 60
check = ["nix flake check --print-build-logs"]
setup_files = ["deny.toml", ".gitleaks.toml", ".gitleaksignore", "flake.nix", "flake.lock"]

[gate.environment]
prefix = []

[agents]
max_fix_rounds = 2
effort_cap = "high"

[[agents.tiers]]
tier = 1
model = "haiku"

[[agents.tiers]]
tier = 2
model = "sonnet"

[[agents.tiers]]
tier = 3
model = "opus"

[[agents.tiers]]
tier = 4
model = "fable"
stand_in = { model = "opus", effort = "high" }

[agents.roles.scout]
tier = 1
retries = 1
escalate_to = { tier = 2 }

[agents.roles.planner]
tier = 3
effort = "high"

[agents.roles.maker]
tier = 2
effort = "high"

[agents.roles.tester]
tier = 3
effort = "medium"

[agents.roles.reader]
tier = 1

[agents.parallel]
slices = 3
agents = 4

[packs]
skill = "superpowers"
domain = "coding"
model = "claude"

[owner]
max_lines = 30
status_update_max_lines = 10
example_max_lines = 15
```

A tier is a model. Effort belongs to a role or a stand-in. A worker's rank is its tier,
then its effort.

## Rules

The file is one TOML document, which is a table of keys (the messages say "object" for a table). Every table in it refuses keys that this page does
not list. Every problem is reported at once, each with its path (for example
`.agents.roles.maker.effort`). A value of the wrong type is a reported problem. TOML tells integers from
floats: a float, even `60.0`, is the wrong type where an integer is needed. A TOML integer is at most
9223372036854775807 (2^63 - 1); a larger one, or a key written twice, makes the file not valid TOML.
An effort above the cap is reported as `must not be above agents.effort_cap`.

| Path | Rule |
|---|---|
| top level | A table with exactly the keys `version`, `gate`, `agents`, `packs`, `owner`; all are required. |
| `version` | The integer `1`. |
| `gate` | Exactly `base`, `max_files`, `check`, `environment`, and optionally `setup_files`: what `shipit check` reads. |
| `gate.base` | A branch name, as `git check-ref-format --branch` accepts it: the base branch. Not a revision (`HEAD~1`), not a tag. |
| `gate.max_files` | An integer, 1 or more: the size cap for one change, in changed files. |
| `gate.check` | An array of non-empty strings: the check commands. It may be empty (a new project's first day). Each command runs with `SHIPIT_MERGE_BASE` (the commit where the slice left the base) and `SHIPIT_CHECK` (the command's own text, as written here) in its environment. A check command that calls `shipit` needs it on PATH: the Claude Code plugin's `bin/` puts it there in Claude sessions, the action does in CI, and in a terminal you install it or use the plugin's `bin/shipit`. |
| `gate.environment` | Exactly `prefix`: an array of non-empty strings, the command prefix that enters the project's tool environment (for example `["nix", "develop", "--command"]`, `["mise", "exec", "--"]`, `["devbox", "run", "--"]`); an empty array means none. Each check runs as `<prefix…> bash -c "<command>"`. |
| `gate.setup_files` | Optional; default an empty array. An array of paths inside the repository, written like `deny.toml` or `conf/` (relative, normal, no glob; a path holds itself and everything below it, so `conf/` and `conf` are one path). They are the files the checks' own tools read: a change to one counts as a change to the setup file, because a slice could otherwise weaken its own checks through them (an advisory added to the ignore list of `deny.toml`, a fingerprint in `.gitleaksignore`, a fake tool in the dev shell of `flake.nix`). The list that applies to a slice is the base branch's, never the slice's own, so a slice cannot drop a file from the list to escape it; adding or changing `gate.setup_files` is itself a setup change. |
| `agents` | Exactly `max_fix_rounds`, `effort_cap`, `tiers`, `roles`, and optionally `parallel`. |
| `agents.max_fix_rounds` | An integer, 0 or more. |
| `agents.effort_cap` | On the effort scale: `low`, `medium`, `high`, `xhigh`, `max`, in that order. |
| `agents.tiers` | A non-empty array of tables. The `tier` values are exactly 1 to n, in order. `model` is a non-empty string. `stand_in` is optional, a table in any TOML form (the example writes it inline); if present it has both `model` (a non-empty string) and `effort` (on the scale, not above `agents.effort_cap`). |
| `agents.roles` | Exactly `scout`, `planner`, `maker`, `tester`, `reader`. Each has `tier`, the number of an existing tier. `effort` is optional: on the scale, not above `agents.effort_cap`. `scout` also requires `retries` (an integer, 0 or more) and `escalate_to` (a table with only `tier`: an existing tier above `agents.roles.scout.tier`). |
| `agents.parallel` | Optional; when it is missing the defaults below hold. A table with only `slices` and `agents`, both optional. An empty `[agents.parallel]` and a missing one mean the same caps, but `check` counts adding or removing the table as a change of `agents.parallel`. |
| `agents.parallel.slices` | An integer, 1 or more; default 3: the cap on slices running in parallel. |
| `agents.parallel.agents` | An integer, 1 or more; default 4: the cap on agents running in parallel. |
| `packs` | Exactly `skill`, `domain`, `model`; each a non-empty string. |
| `owner` | Exactly `max_lines` and `status_update_max_lines`, and optionally `example_max_lines`; each an integer, 1 or more. `status_update_max_lines` is not above `max_lines`. `max_lines` is the most lines any message to the owner may have, not counting example blocks. `status_update_max_lines` is the most lines a progress update to the owner may have. |
| `owner.example_max_lines` | An integer, 1 or more; default 15: the most lines all the example blocks of one message to the owner may have together. An example block is a fenced block whose info string is exactly `example` (a fence of three or more backticks or tildes, indented by at most 3 spaces). Its lines count here and not toward `max_lines`; its two fence lines count here too, so an empty block is not free. It may be above `max_lines`. Adding or removing the key counts as a change of `owner.example_max_lines` for `shipit check`, as for `agents.parallel`, although a missing key means the default. A setup without the key means 15, so a setup written before the key existed stays valid. |

## Who reads what

`shipit check` reads `gate.base`, `gate.max_files`, `gate.check`, `gate.environment` and `gate.setup_files` to judge and run the checks, and compares the whole setup with the base branch's, field by field. `shipit verify` reads no setup at all: a receipt is
valid for a tree and nothing else. `agents` (`max_fix_rounds`, `effort_cap`, `tiers`, `roles`, `planner` included, `parallel`), `packs` and `owner` are
validated now but read by later parts of ShipIt (the roles that make and review work, the packs they use, the
limits on messages to the owner). `agents.parallel.slices` is read by the commands that start slices and report on them, and `agents.parallel.agents` by the plugin (its agent cap), both from the base branch's setup.

## Loading

Every command that reads the setup reads it as committed at a ref (default `HEAD`), never from the working tree. A missing file, a file that is not TOML and a file
that breaks a rule above each stop the command with exit 2 and a message that says what to fix. The message for
a file that is not TOML is `shipit: .config/shipit.toml is not valid TOML: <the parser's message>` (the path takes a `<ref>:` prefix for a ref other than HEAD; for the base branch's file, there is no
parser message). A setup file named `.config/shipit.json` is not read: there is no fallback.

`check` also reads the base branch's setup, which must only be TOML with a `gate.base` string (nothing else in it is validated, so the schema can grow). The whole setup, field by field, is compared with the setup of the slice's merge base with that branch (a field is a key inside a top-level table, such as `gate.max_files` or `agents.tiers`; `version` is one field): the base branch's tip must
not differ from it (else: merge or rebase; a slice whose fields equal the tip's own is never refused for that), and the slice's fields must be identical to it, with one
exception: when the slice's diff against the base touches only `.config/shipit.toml` and files that the base's `gate.setup_files` lists, a change is allowed. `check` then
runs the slice's new checks with its new cap (`gate.max_files`) and environment, prints `shipit: this slice changes the checks (<fields>);
setup change: the owner approves it by merging` on stderr, and marks the receipt (`"setup_change": true`,
`"setup_fields"`); `shipit verify` prints `valid receipt for <tree> (setup change)`, with `; --base <ref>` before the closing parenthesis when `--base` named the base. The owner's merge is the approval.
With any other file in the diff, any difference is refused, because a branch must not choose its own checks along with
its own code; the owner merges setup changes on the base first.
A file that the base lists in `gate.setup_files` counts as the setup file here: a diff that changes a listed file
is a setup change even when no field of the setup changed. `<fields>` then names the changed listed files by path, after any changed fields, and the receipt's `"setup_fields"` holds them too.
A diff that changes a listed file together with any other file is refused with the refusal for any setup change, `shipit: this slice changes the checks (<fields>); the owner merges setup changes on <base> first (or merge/rebase <base> if it moved on)`, with the file's path in `<fields>`. `shipit coverage` asks no done-when map of a diff of setup files alone. A base without a setup file prints a notice and nothing is compared, but the slice's diff must then be the setup file alone; with any other file in it the check is refused as for any setup change. `shipit check --base <name>` names the
base instead of the setup's `gate.base` (a branch name too).

An old file gets one `unknown key` line for each old top-level key (`base`, `cap`, `check`, `fix_rounds`, `effort_cap`, `environment`, `tiers`, `roles`, `messages_to_owner`) and `missing` for `gate`, `agents` and `owner`. There is no migration.

## What a local check cannot stop

The setup is the branch's own word, so `--base` is trusted: CI passes the pull request's real base there. A local check
cannot stop `--base <your own branch>`, which compares the slice with itself. The recheck in CI is the gate, and local
receipts can be gamed by design: they say that this machine ran the checks. A slice can also name a base branch whose own setup names itself (it holds its own code): it is then judged by that branch, and `base_ref` in the receipt shows it. Stacked slices are counted against the root base their base branch's setup names.
