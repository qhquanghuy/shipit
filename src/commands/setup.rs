use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::edge::git::Git;
use crate::pure::failure::Failure;
use crate::pure::json::invalid;
use crate::pure::setup::{HEAD, SETUP_PATH, Setup, json_values, parse_setup, setup_label};

pub(crate) const HIDDEN: bool = true;

const USAGE: &str = "usage: shipit setup [ref]";

pub(crate) fn load_setup(git: &Git, rev: &OsStr) -> Result<Setup, Failure> {
    let label = setup_label(&rev.to_string_lossy());
    let bytes = git
        .show_file(rev, SETUP_PATH)
        .ok_or_else(|| Failure::usage(format!("no setup file at {label} (see docs/setup.md)")))?;
    let docs = json_values(&bytes)
        .map_err(|why| Failure::usage(format!("{label} is not valid JSON: {why}")))?;
    parse_setup(&docs).map_err(|problems| Failure::usage(invalid(&label, &problems)))
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
    let setup = load_setup(&git, &rev)?;
    let text = serde_json::to_string(&setup)
        .map_err(|why| Failure::usage(format!("cannot print the setup: {why}")))?;
    writeln!(out, "{text}").ok();
    Ok(())
}
