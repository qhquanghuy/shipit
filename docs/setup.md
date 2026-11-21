# The setup file

ShipIt reads one file per project: `.config/shipit.json` at the top of the repository. No
environment variable changes any value in it. The schema is checked in full, by
`lib/setup.jq`, whenever a command loads the file.

## Example

```json
{
  "version": 1,
  "base": "main",
  "cap": 60,
  "check": ["nix flake check --print-build-logs"],
  "environment": {"run": []},
  "fix_rounds": 2,
  "effort_cap": "high",
  "tiers": [
    {"tier": 1, "model": "haiku"},
    {"tier": 2, "model": "sonnet"},
    {"tier": 3, "model": "opus"},
    {"tier": 4, "model": "fable", "stand_in": {"model": "opus", "effort": "high"}}
  ],
  "roles": {
    "scout":  {"tier": 1, "retries": 1, "then": {"tier": 2}},
    "maker":  {"tier": 2, "effort": "high"},
    "tester": {"tier": 3, "effort": "medium"}
  },
  "packs": {"skill": "superpowers", "domain": "coding", "model": "claude"},
  "messages_to_owner": {"max_lines": 30, "status_update_max_lines": 10}
}
```

A tier is a model. Effort belongs to a role or a stand-in. A worker's rank is its tier,
then its effort.

## Rules

The file holds exactly one JSON object. Every object in it refuses keys that this page does
not list. Every problem is reported at once, each with its path (for example
`.roles.maker.effort`). A value of the wrong type is a reported problem.

| Path | Rule |
|---|---|
| top level | An object with exactly the keys above; all are required. |
| `version` | The integer `1`. |
| `base` | A non-empty string: the base branch. |
| `cap` | An integer, 1 or more: the size cap for one change. |
| `check` | An array of non-empty strings: the check commands. It may be empty (a new project's first day). |
| `environment` | Exactly `run`: an array of non-empty strings, the command prefix that enters the project's tool environment (for example `["nix", "develop", "--command"]`, `["mise", "exec", "--"]`, `["devbox", "run", "--"]`); an empty array means none. Each check runs as `<run…> bash -c "<command>"`. |
| `fix_rounds` | An integer, 0 or more. |
| `effort_cap` | On the effort scale: `low`, `medium`, `high`, `xhigh`, `max`, in that order. |
| `tiers` | A non-empty array of objects. The `tier` values are exactly 1 to n, in order. `model` is a non-empty string. `stand_in` is optional; if present it has both `model` (a non-empty string) and `effort` (on the scale, not above `effort_cap`). |
| `roles` | Exactly `scout`, `maker`, `tester`. Each has `tier`, the number of an existing tier. `effort` is optional: on the scale, not above `effort_cap`. `scout` also requires `retries` (an integer, 0 or more) and `then` (an object with only `tier`: an existing tier above `scout.tier`). |
| `packs` | Exactly `skill`, `domain`, `model`; each a non-empty string. |
| `messages_to_owner` | Exactly `max_lines` and `status_update_max_lines`, each an integer, 1 or more; `status_update_max_lines` is not above `max_lines`. `max_lines` is the most lines any message to the owner may have. `status_update_max_lines` is the most lines a progress update to the owner may have. |

## Who reads what

`shipit check` reads `base`, `cap`, `check` and `environment`. `shipit verify` reads no setup at all: a receipt is
valid for a tree and nothing else. `tiers`, `roles`, `packs`, `fix_rounds`, `effort_cap` and `messages_to_owner` are
validated now but read by later parts of ShipIt (the roles that make and review work, the packs they use, the
limits on messages to the owner).

## Loading

`load_setup [ref]` in `lib/setup.sh` prints the setup as compact JSON, as committed at `ref`
(default `HEAD`); it never reads the working tree. A missing file, a file that is not JSON and a file
that breaks a rule above each stop the command with exit 2 and a message that says what to fix.

`check` also reads the base branch's setup (`load_baseline` in `lib/setup.sh`), but only `base`, `cap`,
`check` and `environment` (validated for their type only). Those four fields must be identical to the slice's: any
difference is refused, because a branch must not choose its own checks; the owner merges setup changes on the base
first. Nothing else in the base file is looked at, so the schema can grow. A base without a setup file prints a
notice and nothing is compared. `shipit check --base <name>` names the base instead of the setup's `base`.
