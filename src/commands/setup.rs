use std::ffi::{OsStr, OsString};
use std::io::Write;

use toml::Table;

use crate::edge::git::Git;
use crate::pure::failure::Failure;
use crate::pure::rules::invalid;
use crate::pure::setup::{HEAD, SETUP_PATH, Setup, parse_setup, setup_label, toml_table};

pub(crate) const HIDDEN: bool = true;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit setup [ref]";
pub(crate) const SUMMARY: &str =
    "Reads the setup file at a ref (default HEAD) and prints it as compact JSON.";
pub(crate) const EXITS: [&str; 4] = [
    "the setup, printed",
    "",
    "usage error, or a missing or invalid setup file",
    "",
];

// The table is returned with the setup so that a caller needing both reads the file once.
pub(crate) fn load_setup_table(git: &Git, rev: &OsStr) -> Result<(Setup, Table), Failure> {
    let label = setup_label(&rev.to_string_lossy());
    let bytes = git
        .show_file(rev, SETUP_PATH)
        .ok_or_else(|| Failure::usage(format!("no setup file at {label} (see docs/setup.md)")))?;
    let table = toml_table(&bytes)
        .map_err(|why| Failure::usage(format!("{label} is not valid TOML: {why}")))?;
    let setup =
        parse_setup(&table).map_err(|problems| Failure::usage(invalid(&label, &problems)))?;
    Ok((setup, table))
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    let rev = match args {
        [] => OsString::from(HEAD),
        [rev] if rev.is_empty() => return Err(Failure::usage(format!("empty ref; {USAGE}"))),
        [rev] => rev.clone(),
        [_, _, ..] => return Err(Failure::usage(USAGE)),
    };
    let git = Git::find()?;
    let (setup, _table) = load_setup_table(&git, &rev)?;
    let text = serde_json::to_string(&setup)
        .map_err(|why| Failure::usage(format!("cannot print the setup: {why}")))?;
    writeln!(out, "{text}").ok();
    Ok(())
}
