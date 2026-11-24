use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::edge::hook_process::shipit_project;
use crate::pure::agent_ledger::{
    CommitVerdict, MAKERS_LOG, agent_of_worktree, commit_verdict, missing_commit_line,
};
use crate::pure::failure::Failure;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit pre-commit (git runs it from the pre-commit hook)";
pub(crate) const SUMMARY: &str = "Run by git's pre-commit hook: refuses a commit in a ShipIt maker's isolated worktree that is not on that maker's slice branch, and passes every other commit; where the maker's slice is not known, or its HEAD is detached, it warns and passes; in an agent worktree whose pre-commit hook file has no shipit line, it warns too.";
pub(crate) const EXITS: [&str; 4] = [
    "commit allowed (a warning may be printed)",
    "commit refused: a maker's worktree off its slice branch",
    "usage error: an argument was given",
    "",
];

// The hook file git would run here, as git resolves it (core.hooksPath included): a worktree that
// lacks a generated or uncommitted hooks folder has no such file.
fn hook_text(git: &Git) -> Option<String> {
    let args = [
        "rev-parse",
        "--path-format=absolute",
        "--git-path",
        "hooks/pre-commit",
    ];
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let path = String::from_utf8(git.output(&args).ok()?).ok()?;
    read_text(std::path::Path::new(path.trim())).ok()
}

pub(crate) fn run(
    args: &[OsString],
    _out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    if !args.is_empty() {
        return Err(Failure::usage(USAGE));
    }
    let Ok(git) = Git::find() else {
        return Ok(());
    };
    if !shipit_project(&git) {
        return Ok(());
    }
    let log = git
        .common_dir()
        .ok()
        .and_then(|common| read_text(&common.join(MAKERS_LOG)).ok());
    let verdict = commit_verdict(git.top(), git.current_branch().as_deref(), log.as_deref());
    if let CommitVerdict::Refuse(text) = verdict {
        return Err(Failure::no(text));
    }
    if let CommitVerdict::Warn(text) = verdict {
        writeln!(err, "shipit: {text}").ok();
    }
    let hook = agent_of_worktree(git.top()).and_then(|_agent| hook_text(&git));
    if let Some(text) = missing_commit_line(git.top(), hook.as_deref()) {
        writeln!(err, "shipit: {text}").ok();
    }
    Ok(())
}
