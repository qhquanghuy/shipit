#![expect(
    clippy::disallowed_methods,
    reason = "this module reads the environment that shipit check and the CI entry point set"
)]

use crate::edge::git::Git;
use crate::pure::failure::Failure;
use crate::pure::measure::given_merge_base as given;
use crate::pure::receipt::Oid;

// What `shipit check` hands to each check command.
#[derive(Debug)]
pub(crate) struct CheckEnv {
    pub(crate) merge_base: Oid,
    pub(crate) command: String,
}

pub(crate) fn check_env() -> Option<CheckEnv> {
    let merge_base = std::env::var("SHIPIT_MERGE_BASE").ok()?;
    Some(CheckEnv {
        merge_base: Oid::parse(&merge_base)?,
        command: std::env::var("SHIPIT_CHECK").ok()?,
    })
}

// CI checks a detached commit and names the pull request's branch in SHIPIT_BRANCH; either can
// be set by hand, which only forges a local receipt on purpose: CI repeats the run.
pub(crate) fn branch_name(git: &Git) -> Option<String> {
    std::env::var("SHIPIT_BRANCH")
        .ok()
        .filter(|name| !name.is_empty())
        .or_else(|| git.current_branch())
}

// The merge base the check was given, when this runs inside `shipit check`; a check variable
// with no usable merge base is a failure, never a quiet fallback to another base.
pub(crate) fn given_merge_base() -> Result<Option<Oid>, Failure> {
    let check = std::env::var("SHIPIT_CHECK").ok();
    let merge_base = std::env::var("SHIPIT_MERGE_BASE").ok();
    given(check.as_deref(), merge_base.as_deref()).map_err(Failure::usage)
}
