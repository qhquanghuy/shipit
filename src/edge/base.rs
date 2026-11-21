use std::ffi::OsStr;

use crate::edge::git::Git;
use crate::pure::base::{BaseRef, BranchName, choose_base_ref};
use crate::pure::failure::Failure;

pub(crate) fn base_ref(git: &Git, base: &BranchName) -> Result<BaseRef, Failure> {
    let origin = git
        .commit_of(OsStr::new(&BaseRef::Origin(base.clone()).full()))
        .is_some();
    let local = git
        .commit_of(OsStr::new(&BaseRef::Local(base.clone()).full()))
        .is_some();
    choose_base_ref(base, origin, local).ok_or_else(|| {
        let hint = match base.as_str().strip_prefix("origin/") {
            Some(branch) => format!("; name the branch, not the remote: \"{branch}\""),
            None => String::new(),
        };
        Failure::usage(format!(
            "base branch \"{base}\" not found (looked for origin/{base} and {base}){hint}"
        ))
    })
}
