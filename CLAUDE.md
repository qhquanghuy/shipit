# ShipIt

Rust CLI: checks and receipts for agent work. Terms: `docs/vocab.md`. Behaviour: `README.md`, `docs/`.

## Commands (inside `nix develop`, or prefix `nix develop -c`)

- `cargo clippy --all-targets` (every warning is an error), `cargo test`
- `rustfmt $(git ls-files '*.rs')` (not `cargo fmt`: it cannot see the generated `mod` lines)
- `cargo build && bats tests` (or `SHIPIT_BIN=<path> bats tests`)
- `nix flake check`: what CI runs. `git add` new files first; flakes only see tracked files.

## Layout

`src/pure/` types and decisions, no I/O. `src/edge/` effects (`git.rs` is the only module that runs git).
`src/commands/<name>.rs` one command each. `src/main.rs` is the only place that writes errors and exits.
`build.rs` finds the files in those folders and generates the `mod` lines and `COMMANDS`; `data/` is embedded.
It puts `#[forbid(clippy::disallowed_methods, clippy::disallowed_types)]` on `mod pure` and `mod commands`: nothing below can switch the effect lints off.

## Adding without colliding (never edit a shared file)

- A command is one file `src/commands/<name>.rs` (`_` reads as `-`) with `pub(crate) const HIDDEN: bool`
  and `pub(crate) fn run(args, out, err) -> Result<(), Failure>`.
- A module is one file in `src/pure/` or `src/edge/`; folders are flat, no `mod x;` lines.
- Shared (owned by rust-core): `Cargo.toml`, `build.rs`, `clippy.toml`, `flake.nix`, `src/main.rs`, `tests/helpers.bash`.
- Unit-test helpers shared by modules: `src/pure/testing.rs` (`oid`, `os`, `cap`) and `src/edge/testing.rs` (`scratch`); both are `cfg(test)`. Reuse them, do not copy.

## Rules the tooling cannot check

- Messages are copied from the contract, not reworded; `docs/setup.md` and `docs/store.md` are contracts.
- Comments state decisions only: no what, param or return docs.
- Immutable first; newtypes and parse, don't validate.

## Compile and lint on the first try

- No `unwrap`, `expect`, `panic!`, indexing or `&s[a..b]`: use `?`, `.get()`, `ok_or`, `split_once`, `strip_prefix`.
- Suppress with `#[expect(lint, reason = "why")]`, never `#[allow]`; a stale `expect` fails. Edge files start with an `expect` for exactly the effect lints they trip.
- Match every variant; no `_ =>` on enums. `&str` to owned: `.to_owned()`. Appending: `push_str`, not `+`.
- Discard a `Result` with `.ok()`, not `let _ =`; name a dropped error `|_why|`. Use `Self` inside impls.
- Items are `pub(crate)`. No `mod.rs`. Write to `out`/`err` with `write!`, never `println!`.
- Dead code is an error: add a type, variant, field or function only in the change that first uses it.
- Pedantic: avoid lossy `as` casts; `.map(|x| format!(..)).collect::<String>()` is denied.
- Any warning fails the build; finish the edit before checking.

## Tests

- Prefer property tests (`proptest`) over example tests; write an example only for a case a property cannot state (an exact message, one known regression).
- No overlapping cases: each behaviour is proven once, at the lowest level that can prove it. Pure logic: proptest in `src/pure/`. Bats: only what needs the real binary (exit codes, contract messages, git state).
