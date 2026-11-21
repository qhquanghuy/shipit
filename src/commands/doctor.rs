use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::edge::home::{home, machine_notes};
use crate::edge::machine::{probe, signals};
use crate::edge::write::writability;
use crate::pure::failure::Failure;
use crate::pure::machine::{
    Doctor, SetupHealth, TOOLS, Writability, first_problem, headless, report,
};
use crate::pure::machine_notes::parse_notes;
use crate::pure::setup::{HEAD, SETUP_PATH};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit doctor";
pub(crate) const SUMMARY: &str = "Shows the machine facts (system, headless or not, common agent tools with versions, whether agent config is writable, the machine notes) and the setup file's health; the notes' format is the one docs/machine-notes.md states.";
pub(crate) const EXITS: [&str; 4] = ["facts printed", "", "usage error", ""];

// A missing or invalid setup is reported, never a reason to stop: the machine facts are what
// a person needs most when the project is not set up yet.
fn setup_health() -> SetupHealth {
    let Ok(git) = Git::find() else {
        return SetupHealth::NotARepository;
    };
    if !git.file_exists(OsStr::new(HEAD), SETUP_PATH) {
        return SetupHealth::Missing;
    }
    match load_setup_table(&git, OsStr::new(HEAD)) {
        Ok(_loaded) => SetupHealth::Ok,
        Err(failure) => SetupHealth::Invalid(first_problem(&failure.message)),
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    if !args.is_empty() {
        return Err(Failure::usage(USAGE));
    }
    let notes_text = machine_notes();
    let agent_config = home().map_or(Writability::CannotCreate, |home| {
        writability(&home.join(".claude").join("CLAUDE.md"))
    });
    let os = std::env::consts::OS;
    let doctor = Doctor {
        os: os.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        headless: headless(os, &signals()),
        agent_config,
        notes_file: notes_text.is_some(),
        notes: parse_notes(notes_text.as_deref()),
        tools: probe(&TOOLS),
        setup: setup_health(),
    };
    for line in report(&doctor) {
        writeln!(out, "{line}").ok();
    }
    Ok(())
}
