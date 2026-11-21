use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::setup::load_setup_table;
use crate::commands::verify::verify_at;
use crate::edge::git::Git;
use crate::edge::input::read_stdin;
use crate::pure::branch::is_slice_branch;
use crate::pure::failure::{Exit, Failure};
use crate::pure::push::{PushTarget, branch_pushes, is_base, parse_push};
use crate::pure::quote::json_string;
use crate::pure::setup::SETUP_PATH;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit pre-push <remote> <url>";
pub(crate) const SUMMARY: &str = "Run by git's pre-push hook: blocks the push of a slice/* branch that has no valid check receipt, and passes every other push.";
pub(crate) const EXITS: [&str; 4] = [
    "push allowed",
    "push blocked: no check receipt",
    "usage error, unreadable input from git, or an invalid setup at the pushed commit",
    "",
];

const INPUT_LIMIT: usize = 16 * 1024 * 1024;

fn parse_args(args: &[OsString]) -> Result<(), Failure> {
    match args {
        [_remote, _url] => Ok(()),
        [] | [_] | [_, _, _, ..] => Err(Failure::usage(format!(
            "{USAGE} (git runs it from the pre-push hook)"
        ))),
    }
}

fn check_target(git: &Git, target: &PushTarget, err: &mut dyn Write) -> Result<(), Failure> {
    let branch = &target.branch;
    if !is_slice_branch(branch) {
        writeln!(err, "shipit: {branch} is not a slice/* branch; not checked").ok();
        return Ok(());
    }
    let rev = OsStr::new(target.commit.as_str());
    let short = git.short(&target.commit)?;
    if !git.file_exists(rev, SETUP_PATH) {
        writeln!(
            err,
            "shipit: {branch} has no setup file at {short}; not checked (ShipIt is not adopted there)"
        )
        .ok();
        return Ok(());
    }
    let (setup, _table) = load_setup_table(git, rev)?;
    if is_base(target, &setup.gate.base) {
        return Ok(());
    }
    match verify_at(git, Some(rev)) {
        Ok(line) => {
            if line.contains(" (--base ") {
                writeln!(err, "shipit: {branch}: {line}").ok();
            }
            Ok(())
        }
        Err(failure) => match failure.exit {
            Exit::No => Err(Failure::no(format!(
                "push blocked: no check receipt for {branch} ({short}); run shipit check on it, then push again"
            ))),
            Exit::Usage | Exit::Unclean => Err(failure),
        },
    }
}

pub(crate) fn run(
    args: &[OsString],
    _out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    parse_args(args)?;
    let bytes = read_stdin(INPUT_LIMIT)
        .map_err(|why| Failure::usage(format!("cannot read the input from git: {why}")))?;
    let lines = parse_push(&String::from_utf8_lossy(&bytes)).map_err(|line| {
        Failure::usage(format!("unexpected input from git: {}", json_string(&line)))
    })?;
    let targets = branch_pushes(&lines);
    if targets.is_empty() {
        return Ok(());
    }
    let git = Git::find()?;
    for target in &targets {
        check_target(&git, target, err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn pre_push_args(words in proptest::collection::vec(".{0,6}", 0..5)) {
            let args: Vec<OsString> = words.iter().map(OsString::from).collect();
            let parsed = parse_args(&args);
            if words.len() == 2 {
                prop_assert_eq!(parsed, Ok(()));
            } else {
                prop_assert_eq!(
                    parsed,
                    Err(Failure::usage(
                        "usage: shipit pre-push <remote> <url> (git runs it from the pre-push hook)"
                    ))
                );
            }
        }
    }
}
