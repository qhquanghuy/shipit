use std::ffi::OsString;
use std::io::Write;

use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::edge::hook_process::shipit_project;
use crate::pure::agent_ledger::{CommitVerdict, MAKERS_LOG, commit_verdict};
use crate::pure::failure::Failure;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit pre-commit (git runs it from the pre-commit hook)";
pub(crate) const SUMMARY: &str = "Run by git's pre-commit hook: refuses a commit in a ShipIt maker's isolated worktree that is not on that maker's slice branch, and passes every other commit; where the maker's slice is not known, it warns and passes.";
pub(crate) const EXITS: [&str; 4] = [
    "commit allowed (a warning may be printed)",
    "commit refused: a maker's worktree off its slice branch",
    "usage error: an argument was given",
    "",
];

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
    match commit_verdict(git.top(), git.current_branch().as_deref(), log.as_deref()) {
        CommitVerdict::Pass => Ok(()),
        CommitVerdict::Warn(text) => {
            writeln!(err, "shipit: {text}").ok();
            Ok(())
        }
        CommitVerdict::Refuse(text) => Err(Failure::no(text)),
    }
}
