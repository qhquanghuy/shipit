use std::ffi::OsString;
use std::io::Write;

use crate::commands::claude_ship_gate::settle_project;
use crate::edge::agent_store::{Ledger, remove_stale};
use crate::edge::clock::now_utc;
use crate::edge::hook_process::read_hook_input;
use crate::pure::agent_ledger::{Change, Entry, on_failure, on_start, on_stop, reconcile, seconds};
use crate::pure::claude_hook::{HookInput, parse_input, safe_field};
use crate::pure::failure::Failure;

pub(crate) const HIDDEN: bool = true;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str =
    "usage: shipit claude-agent-count (the hook input is read from stdin)";
pub(crate) const SUMMARY: &str = "A Claude Code hook; reads one JSON event from stdin and keeps the count of the agents a session runs; it prints nothing, and it never stops the work.";
pub(crate) const EXITS: [&str; 4] = [
    "done, or nothing to count (a failure is told on stderr)",
    "",
    "usage error: an argument was given",
    "",
];

const INPUT_LIMIT: usize = 8 * 1024 * 1024;

// What an event does to the ledger of its session; a session with no ledger has nothing to count.
fn bookkeep(input: &HookInput) -> Result<(), String> {
    let session = safe_field(input.session_id.as_deref(), "session_id")?;
    let now = seconds(&now_utc()).ok_or("cannot read the clock")?;
    let changes = |entries: &[Entry]| -> Result<Vec<Change>, String> {
        Ok(match input.event.as_str() {
            "SubagentStart" => on_start(
                entries,
                safe_field(input.agent_id.as_deref(), "agent_id")?,
                now,
            ),
            "SubagentStop" => on_stop(entries, safe_field(input.agent_id.as_deref(), "agent_id")?),
            "PostToolUseFailure" => on_failure(
                entries,
                safe_field(input.tool_use_id.as_deref(), "tool_use_id")?,
            ),
            // Without a list of background tasks the agents are left as they are.
            "Stop" => reconcile(entries, input.background_subagents.unwrap_or(usize::MAX)),
            _ => Vec::new(),
        })
    };
    if input.event == "SessionStart" {
        // A start and a resume are new processes: no agent of an earlier run is alive, so its
        // entries are leaks. A compact, a clear and a fork go on in the same process.
        let source = input.source.as_deref();
        if matches!(source, Some("startup" | "resume"))
            && let Some(ledger) = Ledger::open(session, false)?
        {
            ledger.clear();
        }
        if source == Some("startup") {
            remove_stale(session)?;
        }
        return Ok(());
    }
    let Some(ledger) = Ledger::open(session, false)? else {
        return Ok(());
    };
    let held = ledger.locked()?;
    let changes = changes(&held.entries())?;
    held.apply(&changes)
}

// A start and a resume in a project pin it: see `settle_project`.
fn settle(input: &HookInput) -> Result<(), String> {
    if input.event == "SessionStart"
        && matches!(input.source.as_deref(), Some("startup" | "resume"))
    {
        settle_project(&input.cwd)
    } else {
        Ok(())
    }
}

pub(crate) fn run(
    args: &[OsString],
    _out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    if !args.is_empty() {
        return Err(Failure::usage(USAGE));
    }
    let done = read_hook_input(INPUT_LIMIT)
        .and_then(|bytes| parse_input(&bytes))
        .and_then(|input| {
            let settled = settle(&input);
            bookkeep(&input).and(settled)
        });
    if let Err(why) = done {
        writeln!(err, "shipit: agent count: {why}").ok();
    }
    Ok(())
}
