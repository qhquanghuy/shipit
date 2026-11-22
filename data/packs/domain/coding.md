# Coding pack

The domain pack for software projects: what a slice, its size, the checks, the review perspectives, the test drive and ship mean when the work is code in a git repository.

## Slice

A slice is one pull request that leaves the base branch working. It is made on its own branch in its own worktree, and it changes only the files the roadmap says it owns.

## Size

The size of a slice is the number of files it changes against the base branch, counted from the point where the branch left the base; a rename counts once. The limit is `gate.max_files` in the setup.

## Check commands

The check is the list `gate.check` of the setup, run by `shipit check`. For code it holds:

- the formatter, in check mode, so that it reports and never rewrites;
- the linter, with every warning an error;
- the tests, wrapped as `shipit coverage -- <command>` so that the tests the build plan names are known to have run.

The measures and scans of this pack are these check commands, added after the tests:

- `shipit measure clones`, which fails when a clone touches the slice;
- `shipit metric clone-ratio "$(shipit measure clone-ratio)"`, which records the share of the changed lines that are cloned;
- `shipit metric cognitive-complexity "$(shipit measure cognitive-complexity)"`, which records the highest cognitive complexity of a function in the changed files, and never fails on it;
- `cargo deny check`, for a Rust project: advisories, licences, bans and sources of the dependencies, per `deny.toml`; the advisories need the network, so this command runs outside the Nix build;
- `base="$(shipit measure merge-base)" && gitleaks git --redact --no-banner --log-opts="--text $base..HEAD" .`, which scans the commits of the slice for secrets and prints no secret; `--text` is there because a `binary` line in a `.gitattributes` file would otherwise hide a secret from the scan.

The limits of the clone and complexity measures are in `shipit help measure`.

## Setup files

A slice must not weaken the checks it is judged by. List in `gate.setup_files` every file the check commands read, so that a change to one is a setup change that the owner approves by merging and never comes with code:

- `deny.toml`, for `cargo deny`;
- `.gitleaks.toml` and `.gitleaksignore`, for the secret scan, and `.gitattributes`, which changes what the scan sees (list the root file; a nested one is not covered by the list, which `--text` above makes harmless);
- the lint configuration: `clippy.toml`, `rustfmt.toml` and `.cargo/config.toml` for a Rust project;
- the tool environment: `flake.nix` and `flake.lock` for a Nix project, or the lock and tool-version files of another environment manager;
- any script that a check command runs.

`Cargo.toml` holds the lint levels (`[lints]`), and they can be protected only by listing `Cargo.toml`. That makes every dependency change a setup change, so it is the owner's call for each project.

List real files, not symlinks to them, and for a path inside a submodule list the submodule's path. The base branch's list is the one that applies to a slice, so add or change the list in a slice of its own.

## Recheck

The recheck is the continuous integration run on the pull request. It runs the same commands where the maker has no control and trusts no receipt.

## Review perspectives

Every review answers two perspectives, each with a reason.

- Security: input that crosses a boundary, secrets, permissions, and what a hostile input can reach.
- Performance, as structural problems only, the antipatterns Microsoft lists: chatty input and output, extraneous fetching, N+1 queries, no batching, repeated work that is not cached, hidden quadratic work, unbounded growth, synchronous input and output inside loops, and missing timeouts.

Do not suggest micro-tuning unless the owner asks for it or the goal sets a budget.

## Test drive

The test drive runs in an empty folder, with the built result, its documents and its sample data, and never the source. The tester knows only the goal and the done-when items it was given.

## Ship

Ship is the owner merging the pull request, or the owner saying yes to a named tag. No agent merges, tags or pushes to the base branch.
