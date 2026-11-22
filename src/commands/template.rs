use std::ffi::OsString;
use std::io::Write;

use crate::pure::failure::Failure;
use crate::pure::kinds::{form, kind_names};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit template [kind]";
pub(crate) const SUMMARY: &str =
    "Prints the blank form of an output kind, or the kinds when none is given.";
pub(crate) const EXITS: [&str; 4] = ["printed", "", "usage error or unknown kind", ""];

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    match args {
        [] => {
            writeln!(out, "kinds: {}", kind_names()).ok();
            Ok(())
        }
        [name] => {
            let name = name.to_string_lossy();
            let text = form(&name).ok_or_else(|| {
                Failure::usage(format!("unknown kind \"{name}\"; kinds: {}", kind_names()))
            })?;
            write!(out, "{text}").ok();
            Ok(())
        }
        [_, _, ..] => Err(Failure::usage(USAGE)),
    }
}
