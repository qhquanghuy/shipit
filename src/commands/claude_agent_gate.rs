use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::claude_ship_gate::{base_names, gate_folder};
use crate::commands::setup::load_setup_table;
use crate::commands::verify::verify_at;
use crate::edge::agent_store::Ledger;
use crate::edge::clock::now_utc;
use crate::edge::git::Git;
use crate::edge::hook_process::{exists, run_gate};
use crate::edge::slice_git::load_policy;
use crate::pure::agent_ledger::{Change, Entry, admit, seconds};
use crate::pure::base::BranchName;
use crate::pure::branch::is_slice_branch;
use crate::pure::claude_hook::{HookInput, Target, agent_call, review_target, safe_field};
use crate::pure::failure::{Exit, Failure};
use crate::pure::setup::{HEAD, SETUP_PATH};
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

fn deny(text: String) -> Answer {
    Answer::Deny(text)
}

// The branch the prompt names must be a local branch that is not a base: a revision such as
// HEAD, a tag or a remote branch would be judged as some other work.
fn named_branch(git: &Git, branch: &str, bases: &[String]) -> Option<String> {
    let name = BranchName::parse(branch).filter(|name| is_slice_branch(name.as_str()))?;
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
            return Some(deny(cannot_start(why)));
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

// The cap is the base branch's word, as every other limit is: a slice cannot raise its own.
fn agent_cap(git: &Git) -> Result<u64, Failure> {
    let (head, _table) = load_setup_table(git, OsStr::new(HEAD))?;
    let mut ignored = Vec::new();
    let (policy, _base) = load_policy(git, head, &mut ignored)?;
    Ok(u64::from(&policy.agents.parallel.agents))
}

fn cannot_start(why: impl std::fmt::Display) -> String {
    format!("ShipIt: the agent cannot start: {why}.")
}

// The text of the denial when the start is not admitted.
fn admit_start(git: &Git, input: &HookInput) -> Result<(), String> {
    let session = safe_field(input.session_id.as_deref(), "session_id").map_err(cannot_start)?;
    let call = safe_field(input.tool_use_id.as_deref(), "tool_use_id").map_err(cannot_start)?;
    // The cap is read as committed: a setup that is only in the working tree says nothing yet.
    if !git.file_exists(OsStr::new(HEAD), SETUP_PATH) && exists(&git.top().join(SETUP_PATH)) {
        return Err(cannot_start(
            "the setup file .config/shipit.toml is not committed, and the setup is read as committed: commit it first",
        ));
    }
    let cap = agent_cap(git).map_err(|failure| cannot_start(failure.message))?;
    let now = seconds(&now_utc()).ok_or_else(|| cannot_start("cannot read the clock"))?;
    let ledger = Ledger::open(session, true)
        .map_err(|why| format!("ShipIt cannot count the running agents: {why}."))?
        .ok_or_else(|| cannot_start("the ledger is missing"))?;
    let held = ledger.locked().map_err(cannot_start)?;
    admit(&held.entries(), now, cap)?;
    held.apply(&[Change::Write(Entry::Reserved {
        tool_use_id: call.to_owned(),
        at: now,
    })])
    .map_err(cannot_start)
}

fn decide(input: &Result<HookInput, String>) -> Answer {
    let Some(git) = gate_folder(input) else {
        return Answer::Pass;
    };
    let input = match input {
        Ok(input) => input,
        Err(why) => return deny(cannot_start(why)),
    };
    if let Some(answer) = review_gate(&git, input) {
        return answer;
    }
    if !agent_call(input) {
        return Answer::Pass;
    }
    match admit_start(&git, input) {
        Ok(()) => Answer::Pass,
        Err(text) => deny(text),
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    run_gate(args, out, USAGE, decide)
}
