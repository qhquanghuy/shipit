use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;

use crate::commands::check::settle_base;
use crate::edge::git::Git;
use crate::pure::failure::Failure;
use crate::pure::messages::{NO_FILE, say};
use crate::pure::receipt::Oid;
use crate::pure::rules::invalid;
use crate::pure::setup::{Setup, parse_setup, setup_label};

// The hash git gives the bytes of the file as they are, so a plan or a roadmap is keyed by what
// the reviewer read and not by any filter of the project.
pub(crate) fn file_hash(git: &Git, path: &Path) -> Result<Oid, Failure> {
    git.output(&[
        OsStr::new("hash-object"),
        OsStr::new("--no-filters"),
        OsStr::new("--"),
        path.as_os_str(),
    ])
    .ok()
    .and_then(|bytes| Oid::parse(String::from_utf8_lossy(&bytes).trim()))
    .ok_or_else(|| Failure::usage(say(NO_FILE, &[("path", &path.display())])))
}

// The ranks and limits are the base branch's word: a branch never judges itself with its own.
// A base with no setup file is the adoption, and the branch's own setup is the policy.
pub(crate) fn load_policy(git: &Git, head: Setup, err: &mut dyn Write) -> Result<Setup, Failure> {
    let (_base, base_ref, flow) = settle_base(git, head.gate.base.clone(), false, err)?;
    let Some(flow) = flow else {
        writeln!(
            err,
            "shipit: the policy is this branch's own setup (adoption)"
        )
        .ok();
        return Ok(head);
    };
    parse_setup(flow.table())
        .map_err(|problems| Failure::usage(invalid(&setup_label(&base_ref.to_string()), &problems)))
}

// The local branch, else origin's: a slice another clone pushed can be reviewed here.
pub(crate) fn branch_tip(git: &Git, branch: &str) -> Option<Oid> {
    ["refs/heads/", "refs/remotes/origin/"]
        .iter()
        .find_map(|prefix| git.commit_of(OsStr::new(&format!("{prefix}{branch}"))))
}

pub(crate) fn merge_base(git: &Git, a: &Oid, b: &Oid) -> Option<Oid> {
    let bytes = git
        .output(&[
            OsStr::new("merge-base"),
            OsStr::new("--end-of-options"),
            OsStr::new(a.as_str()),
            OsStr::new(b.as_str()),
        ])
        .ok()?;
    Oid::parse(String::from_utf8_lossy(&bytes).trim())
}

// Every option is given, so that no setting of the user's (prefixes, renames, an external
// diff program, colour, a relative path) changes the bytes the id is made from. None for no
// change at all.
pub(crate) fn patch_id(git: &Git, from: &Oid, to: &Oid) -> Result<Option<Oid>, Failure> {
    let cannot = || Failure::usage(format!("cannot read the change from {from} to {to}"));
    let diff = git
        .output(&[
            OsStr::new("-c"),
            OsStr::new("core.quotePath=true"),
            OsStr::new("diff"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--no-textconv"),
            OsStr::new("--no-color"),
            OsStr::new("--no-relative"),
            OsStr::new("--no-renames"),
            OsStr::new("--ignore-submodules=none"),
            OsStr::new("--submodule=short"),
            OsStr::new("--binary"),
            OsStr::new("--full-index"),
            OsStr::new("-U3"),
            OsStr::new("--inter-hunk-context=0"),
            OsStr::new("--diff-algorithm=myers"),
            OsStr::new("--indent-heuristic"),
            OsStr::new("--src-prefix=a/"),
            OsStr::new("--dst-prefix=b/"),
            OsStr::new(from.as_str()),
            OsStr::new(to.as_str()),
        ])
        .map_err(|_why| cannot())?;
    if diff.is_empty() {
        return Ok(None);
    }
    let ids = git
        .output_fed(
            &[OsStr::new("patch-id"), OsStr::new("--stable")],
            Some(&diff),
        )
        .map_err(|_why| cannot())?;
    let text = String::from_utf8_lossy(&ids);
    let first = text.split_whitespace().next();
    Ok(first.and_then(Oid::parse))
}
