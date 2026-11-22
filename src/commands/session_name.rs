use std::ffi::OsString;
use std::io::Write;

use crate::edge::git::Git;
use crate::pure::failure::Failure;
use crate::pure::session::{NamePart, parse_part, project_of, session_name};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit session-name <goal> [<slice>]";
pub(crate) const SUMMARY: &str = "Prints the name for a session, the project folder, the goal and for a slice session its slice, joined by the middle dot; a session started with that name can be written to by that name.";
pub(crate) const EXITS: [&str; 4] = ["the name", "", "usage error, or not in a repository", ""];

fn part(text: &OsString) -> Result<NamePart, Failure> {
    let text = text
        .to_str()
        .ok_or_else(|| Failure::usage(format!("a name part is not text; {USAGE}")))?;
    parse_part(text).map_err(Failure::usage)
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    let (goal, slice) = match args {
        [goal] => (part(goal)?, None),
        [goal, slice] => (part(goal)?, Some(part(slice)?)),
        [] | [_, _, _, ..] => return Err(Failure::usage(USAGE)),
    };
    let common = Git::find()?.common_dir()?;
    let folder = project_of(&common)
        .ok_or_else(|| Failure::usage("cannot tell the project folder of this repository"))?;
    let project =
        parse_part(&folder).map_err(|why| Failure::usage(format!("the project folder: {why}")))?;
    writeln!(out, "{}", session_name(&project, &goal, slice.as_ref())).ok();
    Ok(())
}
