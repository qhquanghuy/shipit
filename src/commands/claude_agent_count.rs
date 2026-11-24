use std::ffi::OsString;
use std::io::Write;

use serde_json::Value;

use crate::edge::agent_store::{Ledger, remove_stale};
use crate::edge::clock::now_utc;
use crate::edge::files::read_text;
use crate::edge::hook_process::{input_folder, read_hook_input, settle_project};
use crate::edge::records::append_line;
use crate::pure::agent_ledger::{
    Change, Entry, MAKERS_LOG, MakerRecord, on_failure, on_start, on_stop, reconcile, record_line,
    resumed_away, seconds, subagent_context,
};
use crate::pure::claude_hook::{HookInput, MAKER, agent_call, parse_input, safe_field};
use crate::pure::failure::Failure;

pub(crate) const HIDDEN: bool = true;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str =
    "usage: shipit claude-agent-count (the hook input is read from stdin)";
pub(crate) const SUMMARY: &str = "A Claude Code hook; reads one JSON event from stdin and keeps the count of the agents a session runs; it prints nothing but a warning to a resumed isolated maker (SubagentStart additionalContext), and it never stops the work.";
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

fn resume_warning(agent: &str) -> String {
    format!(
        "ShipIt: not checked: agent {agent} is an isolated shipit:maker started again outside its worktree; stop and tell the lead to start a new maker (docs/claude-code.md, Parallel makers)"
    )
}

// The lines the commit hook reads, written whether or not the session has a ledger. A start of a
// maker that is already known and is not in its own worktree is a resume: it works outside the
// check's sight, and the warning is the text returned.
fn record_maker(input: &HookInput) -> Result<Option<String>, String> {
    let Some(git) = input_folder(input) else {
        return Ok(None);
    };
    let session = safe_field(input.session_id.as_deref(), "session_id")?;
    let log = git
        .common_dir()
        .map_err(|failure| failure.message)?
        .join(MAKERS_LOG);
    let append = |record: &MakerRecord| {
        append_line(&log, &record_line(record)).map_err(|failure| failure.message)
    };
    match input.event.as_str() {
        "PostToolUse" => {
            let Some(tool_input) = input.tool_input.as_ref().filter(|_| agent_call(input)) else {
                return Ok(None);
            };
            let text = |key: &str| tool_input.get(key).and_then(Value::as_str);
            let isolated_maker =
                text("subagent_type") == Some(MAKER) && text("isolation") == Some("worktree");
            let Some(agent) = input.launched_agent.as_deref().filter(|_| isolated_maker) else {
                return Ok(None);
            };
            append(&MakerRecord::Launched {
                session: session.to_owned(),
                call: safe_field(input.tool_use_id.as_deref(), "tool_use_id")?.to_owned(),
                agent: safe_field(Some(agent), "agentId")?.to_owned(),
            })?;
            Ok(None)
        }
        "SubagentStart" if input.agent_type.as_deref() == Some(MAKER) => {
            let agent = safe_field(input.agent_id.as_deref(), "agent_id")?;
            let warning = read_text(&log)
                .ok()
                .filter(|text| resumed_away(text, agent, &input.cwd))
                .map(|_text| resume_warning(agent));
            append(&MakerRecord::Maker {
                session: session.to_owned(),
                agent: agent.to_owned(),
            })?;
            Ok(warning)
        }
        _ => Ok(None),
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    if !args.is_empty() {
        return Err(Failure::usage(USAGE));
    }
    let done = read_hook_input(INPUT_LIMIT)
        .and_then(|bytes| parse_input(&bytes))
        .and_then(|input| {
            let settled = settle(&input);
            let recorded = record_maker(&input).map(|warning| {
                if let Some(text) = warning {
                    writeln!(out, "{}", subagent_context(&text)).ok();
                }
            });
            bookkeep(&input).and(settled).and(recorded)
        });
    if let Err(why) = done {
        writeln!(err, "shipit: agent count: {why}").ok();
    }
    Ok(())
}
