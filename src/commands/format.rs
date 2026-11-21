use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::commands::setup::load_setup;
use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::pure::block::toml_block;
use crate::pure::failure::Failure;
use crate::pure::goal::{DoneWhen, goal_items};
use crate::pure::roadmap::{Roadmap, SliceName, meets_problems, parse_roadmap, waves};
use crate::pure::rules::invalid;
use crate::pure::setup::{HEAD, toml_table};

pub(crate) const HIDDEN: bool = true;

const USAGE: &str = "usage: shipit format <roadmap|map|findings> …";
const ROADMAP_USAGE: &str = "usage: shipit format roadmap <file> [--goal <goal file>]";
const SLICES: &str = "## Slices";

#[derive(Serialize)]
struct Printed<'a> {
    roadmap: &'a Roadmap,
    waves: Vec<Vec<&'a SliceName>>,
    goal: Option<&'a [DoneWhen]>,
}

fn print(out: &mut dyn Write, value: &impl Serialize) -> Result<(), Failure> {
    let text = serde_json::to_string(value)
        .map_err(|why| Failure::usage(format!("cannot print the result: {why}")))?;
    writeln!(out, "{text}").ok();
    Ok(())
}

fn roadmap_command(args: &[OsString], out: &mut dyn Write) -> Result<(), Failure> {
    let (file, goal_file) = match args {
        [file] => (file, None),
        [file, flag, goal] if flag == OsStr::new("--goal") => (file, Some(goal)),
        _ => return Err(Failure::usage(ROADMAP_USAGE)),
    };
    let git = Git::find()?;
    let setup = load_setup(&git, OsStr::new(HEAD))?;
    let label = file.to_string_lossy();
    let text = read_text(Path::new(file))?;
    let block = toml_block(&text, SLICES)
        .ok_or_else(|| Failure::usage(format!("{label} has no toml block under \"{SLICES}\"")))?;
    let table = toml_table(block.as_bytes())
        .map_err(|why| Failure::usage(format!("{label} is not valid TOML: {why}")))?;
    let roadmap = parse_roadmap(&table, &setup.gate.base)
        .map_err(|problems| Failure::usage(invalid(&label, &problems)))?;
    let goal = match goal_file {
        None => None,
        Some(goal_file) => {
            let items = goal_items(&read_text(Path::new(goal_file))?).map_err(Failure::usage)?;
            let problems = meets_problems(&roadmap, &items);
            if !problems.is_empty() {
                return Err(Failure::usage(invalid(&label, &problems)));
            }
            Some(items)
        }
    };
    print(
        out,
        &Printed {
            roadmap: &roadmap,
            waves: waves(&roadmap),
            goal: goal.as_deref(),
        },
    )
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    match args.split_first() {
        Some((form, rest)) if form == OsStr::new("roadmap") => roadmap_command(rest, out),
        Some(_) | None => Err(Failure::usage(USAGE)),
    }
}
