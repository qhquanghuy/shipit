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
