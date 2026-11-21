# Commands

The current reference for the `shipit` commands. The goal and its done-when are in [goal.md](goal.md); the behaviour of
`check` and `verify` is in the [README](../README.md); the setup file is in [setup.md](setup.md).

## The commands

`built` is a command `shipit help` lists. `planned` is a command no slice has built yet. A test compares this table
with the compiled command list in both directions: a built row that `shipit help` does not list fails, and so does a
planned row that it does list. The slice that builds a command changes its row from `planned` to `built`.

| Command | Status | What it does |
|---|---|---|
| `shipit check [--base <name>]` | built | Refuses a dirty tree. Measures the size against the cap. Runs the setup's check commands in order. On a full pass, writes a check receipt for this exact version of the work. |
| `shipit verify [commit]` | built | Says whether a valid check receipt exists for the current version, or for the given commit. Fast: it only reads files and asks git. |
| `shipit init` | planned | Writes a starting setup file in a project, prints the CI job to add (`--ci-job`), and installs the `pre-push` hook in this clone. |
| `shipit pre-push` | planned | Runs as the git `pre-push` hook: verifies the newest commit of each pushed slice branch. |
| `shipit recheck` | planned | Runs the checks again from the start, where the maker has no control, and trusts no receipt. |
| `shipit doctor` | planned | Reports whether this machine is ready to run ShipIt. |
| `shipit receipt` | planned | Records a review or test-drive receipt: who, which model and tier, which exact input, the result. |
| `shipit status` | planned | Computes each slice's state from the roadmap, the receipts and the logbook, and names the next step. It can also print a short summary of a slice (receipts and known issues) for the pull request. |
| `shipit start` | planned | Starts a slice; at most `agents.parallel.slices` run at once. |
| `shipit log` | planned | Appends one entry to a slice's logbook. |
| `shipit coverage` | planned | Records which done-when items a slice covers (`.shipit/coverage/<slice>.toml`). |
| `shipit metric` | planned | Records a measured number for the work (`receipts/metric/`). |
| `shipit validate` | planned | Checks a filled output against its template and refuses one with an unlisted outcome or a missing part. |
| `shipit template` | planned | Prints the template of an output. |
| `shipit measure` | planned | Measures code duplication and complexity. |
| `shipit session-name` | planned | Names a session for the Claude Code plugin. |

## Exit codes

| Command | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| `shipit check [--base <name>]` | receipt written (marked `setup_change` when the slice only changes the setup file) | a check failed, a check changed the work, the change is over the cap, or the slice changes the checks together with other files | usage or setup error (bad arguments, a base that is not a branch name, missing or invalid setup file, base branch not found, no shared history with it, a base that is the branch itself, sparse checkout, git failed, the receipt could not be written) | uncommitted changes, or a tracked file hidden from git |
| `shipit verify [commit]` | valid receipt | no valid receipt, or (no argument only) uncommitted changes or a tracked file hidden from git | usage error (bad arguments, not a commit) | not used |

## Commands that run without a setup

`shipit verify` (a receipt is valid for a tree and nothing else), `shipit help` and `shipit --version` need no setup file.
`shipit init` is planned to write it. Every other command stops with exit 2 when the setup file is missing or invalid.
