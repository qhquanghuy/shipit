use std::ffi::OsString;
use std::io::Write;

use serde_json::Value;

use crate::edge::home::machine_notes;
use crate::edge::hook_process::{gate_folder, input_folder, run_gate};
use crate::pure::claude_hook::{Answer, HookInput};
use crate::pure::failure::Failure;
use crate::pure::install_gate::install_refusal;
use crate::pure::machine_notes::parse_notes;

pub(crate) const HIDDEN: bool = true;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str =
    "usage: shipit claude-install-gate (the hook input is read from stdin)";
pub(crate) const SUMMARY: &str = "A Claude Code hook; reads one JSON event from stdin and prints nothing or one deny decision; it allows everything outside a ShipIt project.";
pub(crate) const EXITS: [&str; 4] = [
    "no decision, or one decision printed",
    "",
    "internal failure: the hook blocks",
    "",
];

fn unreadable(why: &str) -> Answer {
    Answer::Deny(format!("ShipIt cannot read the hook input ({why})."))
}

// The refusal is looked for first: a command that installs nothing is passed without asking
// git whether the folder is a ShipIt project.
fn judge(input: &HookInput) -> Answer {
    let command = input
        .tool_input
        .as_ref()
        .and_then(|tool_input| tool_input.get("command"))
        .and_then(Value::as_str);
    let Some(command) = command else {
        return if input_folder(input).is_some() {
            unreadable("the hook input has no tool_input.command")
        } else {
            Answer::Pass
        };
    };
    let notes = parse_notes(machine_notes().as_deref());
    match install_refusal(command, &notes) {
        Some(refusal) if input_folder(input).is_some() => Answer::Deny(refusal),
        Some(_) | None => Answer::Pass,
    }
}

fn decide(input: &Result<HookInput, String>) -> Answer {
    match input {
        Ok(input) => judge(input),
        Err(why) if gate_folder(input).is_some() => unreadable(why),
        Err(_why) => Answer::Pass,
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    run_gate(args, out, USAGE, decide)
}
