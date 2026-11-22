# Coding pack

The domain pack for software projects: what a slice, its size, the checks, the review perspectives, the test drive and ship mean when the work is code in a git repository.

## Slice

A slice is one pull request that leaves the base branch working. It is made on its own branch in its own worktree, and it changes only the files the roadmap says it owns.

## Size

The size of a slice is the number of files it changes against the base branch, counted from the point where the branch left the base; a rename counts once. The limit is `gate.max_files` in the setup.

## Bug fix

A bug fix starts with a test that reproduces the bug, committed failing before the fix; the build plan names that test.

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
- `base="$(shipit measure merge-base)" && gitleaks git --redact --no-banner --log-opts="$base..HEAD" .`, which scans the commits of the slice for secrets and prints no secret.

The limits of the clone and complexity measures are in `shipit help measure`.

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
