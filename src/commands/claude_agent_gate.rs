use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::claude_ship_gate::{base_names, gate_folder};
use crate::commands::verify::verify_at;
use crate::edge::git::Git;
use crate::edge::input::read_stdin;
use crate::pure::base::BranchName;
use crate::pure::claude_hook::{HookInput, Target, answer_json, parse_input, review_target};
use crate::pure::failure::{Exit, Failure};
use crate::pure::ship_gate::Answer;

pub(crate) const HIDDEN: bool = true;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str =
    "usage: shipit claude-agent-gate (the hook input is read from stdin)";
pub(crate) const SUMMARY: &str = "A Claude Code hook; reads one JSON event from stdin and prints nothing or one ask or deny decision; it allows everything outside a ShipIt project.";
pub(crate) const EXITS: [&str; 4] = [
    "no decision, or one decision printed",
    "",
    "internal failure: the hook blocks",
    "",
];

const INPUT_LIMIT: usize = 8 * 1024 * 1024;

fn deny(text: String) -> Answer {
    Answer::Deny(text)
}

// The branch the prompt names must be a local branch that is not a base: a revision such as
// HEAD, a tag or a remote branch would be judged as some other work.
fn named_branch(git: &Git, branch: &str, bases: &[String]) -> Option<String> {
    let name = BranchName::parse(branch)?;
    if bases.iter().any(|base| base == name.as_str()) {
        return None;
    }
    let full = format!("refs/heads/{name}");
    git.commit_of(OsStr::new(&full)).map(|_commit| full)
}

fn receipt_answer(
    role: &str,
    branch: Option<&str>,
    result: Result<String, Failure>,
) -> Option<Answer> {
    let failure = result.err()?;
    Some(deny(match failure.exit {
        Exit::No => format!(
            "ShipIt: {role} needs a check receipt: {}. Run \"shipit check\" on {}, then start it again.",
            failure.message,
            branch.unwrap_or("this working copy")
        ),
        Exit::Usage | Exit::Unclean => {
            format!("ShipIt: {role} cannot start: {}.", failure.message)
        }
    }))
}

fn review_gate(git: &Git, input: &HookInput) -> Option<Answer> {
    let (role, branch) = match review_target(input) {
        Target::NotGated => return None,
        Target::Bad(why) => {
            return Some(deny(format!("ShipIt: the agent cannot start: {why}.")));
        }
        Target::Gated { role, branch } => (role, branch),
    };
    let bases = base_names(git);
    if let Some(branch) = branch {
        let Some(full) = named_branch(git, &branch, &bases) else {
            return Some(deny(format!(
                "ShipIt: \"shipit-branch: {branch}\" must name a local slice branch, not a base branch or another revision."
            )));
        };
        let result = verify_at(git, Some(OsStr::new(&full)));
        receipt_answer(&role, Some(&branch), result)
    } else {
        if git
            .current_branch()
            .is_some_and(|current| bases.contains(&current))
        {
            return Some(deny(format!(
                "ShipIt: name the slice: add a line \"shipit-branch: <branch>\" to the prompt of {role}."
            )));
        }
        receipt_answer(&role, None, verify_at(git, None))
    }
}

fn decide(input: &Result<HookInput, String>) -> Answer {
    let Some(git) = gate_folder(input) else {
        return Answer::Pass;
    };
    match input {
        Ok(input) => review_gate(&git, input).unwrap_or(Answer::Pass),
        Err(why) => deny(format!("ShipIt: the agent cannot start: {why}.")),
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
    let input = read_stdin(INPUT_LIMIT).and_then(|bytes| parse_input(&bytes));
    if let Some(json) = answer_json(&decide(&input)) {
        writeln!(out, "{json}").ok();
    }
    Ok(())
}
