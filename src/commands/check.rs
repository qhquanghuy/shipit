use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::setup::load_setup;
use crate::edge::clock::now_utc;
use crate::edge::git::Git;
use crate::edge::run::run_check;
use crate::edge::store::{check_receipt_path, write_json_atomic};
use crate::pure::base::{BaseRef, BranchName, SelfBase, base_verdict, choose_base_ref};
use crate::pure::baseline::{
    ALL_FIELDS, Flow, FlowField, SetupVerdict, parse_baseline, setup_only, setup_verdict,
};
use crate::pure::failure::Failure;
use crate::pure::json::invalid;
use crate::pure::receipt::{BaseUsed, CheckReceipt, Oid};
use crate::pure::setup::{CheckCommand, HEAD, SETUP_PATH, Setup, json_values, setup_label};
use crate::pure::size::size_verdict;
use crate::pure::text::comma_list;
use crate::pure::work::{
    UNCOMMITTED, WorkChange, WorkFacts, change_verdict, dirty_paths, first_five, hidden_paths,
    hidden_refusal,
};

pub(crate) const HIDDEN: bool = false;

const USAGE: &str = "usage: shipit check [--base <name>]";

// The value after --base is taken whatever it starts with, then it must be a branch name.
fn base_argument(value: Option<&OsString>) -> Result<BranchName, Failure> {
    let value = value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Failure::usage(format!("--base needs a branch name; {USAGE}")))?;
    value.to_str().and_then(BranchName::parse).ok_or_else(|| {
        Failure::usage(format!(
            "--base '{}' is not a branch name (see git check-ref-format --branch); {USAGE}",
            value.to_string_lossy()
        ))
    })
}

fn parse_args(args: &[OsString]) -> Result<Option<BranchName>, Failure> {
    let mut base: Option<BranchName> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--base" {
            if base.replace(base_argument(rest.next())?).is_some() {
                return Err(Failure::usage(format!("--base given twice; {USAGE}")));
            }
        } else {
            return Err(Failure::usage(format!(
                "unknown argument '{}'; {USAGE}",
                arg.to_string_lossy()
            )));
        }
    }
    Ok(base)
}

fn head_commit(git: &Git) -> Result<Oid, Failure> {
    git.commit_of(OsStr::new(HEAD))
        .ok_or_else(|| Failure::usage("cannot resolve HEAD to a commit"))
}

const MAX_HOPS: usize = 5;

fn base_ref(git: &Git, base: &BranchName) -> Result<BaseRef, Failure> {
    let origin_ref = BaseRef::Origin(base.clone());
    let local_ref = BaseRef::Local(base.clone());
    let origin_full = origin_ref.full();
    let local_full = local_ref.full();
    let origin = git.commit_of(OsStr::new(&origin_full)).is_some();
    let local = git.commit_of(OsStr::new(&local_full)).is_some();
    let local_has_origin =
        origin && local && git.is_ancestor(OsStr::new(&origin_full), OsStr::new(&local_full));
    choose_base_ref(base, origin, local, local_has_origin).ok_or_else(|| {
        let hint = match base.as_str().strip_prefix("origin/") {
            Some(branch) => format!("; name the branch, not the remote: \"{branch}\""),
            None => String::new(),
        };
        Failure::usage(format!(
            "base branch \"{base}\" not found (looked for origin/{base} and {base}){hint}"
        ))
    })
}

fn refuse_self_base(
    git: &Git,
    base: &BranchName,
    trusted: bool,
    ref_commit: &Oid,
) -> Result<(), Failure> {
    let current = git.current_branch();
    let head = head_commit(git)?;
    base_verdict(trusted, base, current.as_deref(), ref_commit, &head).map_err(
        |reason: SelfBase| {
            Failure::usage(format!(
                "base \"{base}\" {reason}, and the setup is the branch's own word; pass --base \
                 <name> naming the real base branch (CI passes the pull request's base)"
            ))
        },
    )
}

// None when the file is missing.
fn read_flow(git: &Git, rev: &OsStr, label: &str) -> Result<Option<Flow>, Failure> {
    if !git.file_exists(rev, SETUP_PATH) {
        return Ok(None);
    }
    let bytes = git
        .show_file(rev, SETUP_PATH)
        .ok_or_else(|| Failure::usage(format!("no setup file at {label} (see docs/setup.md)")))?;
    let docs =
        json_values(&bytes).map_err(|_why| Failure::usage(format!("{label} is not valid JSON")))?;
    parse_baseline(&docs)
        .map(Some)
        .map_err(|problems| Failure::usage(invalid(label, &problems)))
}

fn load_baseline(
    git: &Git,
    base_ref: &BaseRef,
    err: &mut dyn Write,
) -> Result<Option<Flow>, Failure> {
    let name = base_ref.to_string();
    let flow = read_flow(git, OsStr::new(&base_ref.full()), &setup_label(&name))?;
    if flow.is_none() {
        writeln!(err, "shipit: no setup file on {name}; nothing to compare").ok();
    }
    Ok(flow)
}

// The base branch's own setup says which branch is the base. A slice that names another branch
// (one that holds its own code) is followed to the base that branch's setup names, until a
// branch names itself: the slice's new value never judges the slice. --base is taken as given.
fn settle_base(
    git: &Git,
    start: BranchName,
    trusted: bool,
    err: &mut dyn Write,
) -> Result<(BranchName, BaseRef, Option<Flow>), Failure> {
    let mut base = start;
    for _hop in 0..MAX_HOPS {
        let base_ref = base_ref(git, &base)?;
        let tip = load_baseline(git, &base_ref, err)?;
        let named = match &tip {
            Some(flow) if !trusted && flow.base() != base.as_str() => flow.base().to_owned(),
            Some(_) | None => return Ok((base, base_ref, tip)),
        };
        base = BranchName::parse(&named).ok_or_else(|| {
            Failure::usage(format!(
                "{} names the base \"{named}\", which is not a branch name",
                setup_label(&base_ref.to_string())
            ))
        })?;
    }
    Err(Failure::usage(format!(
        "the base setups lead from branch to branch and no branch names itself as the base \
         (stopped at \"{base}\")"
    )))
}

fn changed_setup(fields: &[FlowField], base: &BranchName) -> Failure {
    Failure::no(format!(
        "this slice changes the checks ({}); the owner merges setup changes on {base} first \
         (or merge/rebase {base} if it moved on)",
        comma_list(fields)
    ))
}

// A branch that could change the checks along with its code could vouch for itself, so the
// owner merges those first. A slice that edits the setup file alone only asks the owner: it is
// marked in the receipt and approved by merging.
fn approve_setup_change(
    git: &Git,
    changes: &[FlowField],
    since: &Oid,
    base: &BranchName,
    err: &mut dyn Write,
) -> Result<Vec<String>, Failure> {
    if !setup_only(&git.changed_names(since)?) {
        return Err(changed_setup(changes, base));
    }
    writeln!(
        err,
        "shipit: this slice changes the checks ({}); setup change: the owner approves it by \
         merging",
        comma_list(changes)
    )
    .ok();
    Ok(changes.iter().map(ToString::to_string).collect())
}

fn judge_setup(
    git: &Git,
    setup: &Setup,
    (tip, base): (Option<&Flow>, &BranchName),
    (base_ref, merge_base): (&BaseRef, &Oid),
    err: &mut dyn Write,
) -> Result<Option<Vec<String>>, Failure> {
    let Some(tip) = tip else {
        // A base with no setup file is the adoption: it vouches for nothing, so only the setup
        // file alone may come with it, or a slice could name an old branch to skip every check.
        if setup_only(&git.changed_names(merge_base)?) {
            return Ok(None);
        }
        return Err(changed_setup(&ALL_FIELDS, base));
    };
    // A broken setup at the merge base reads as none: every difference from the tip is then
    // refused, which is the safe side.
    let forked = read_flow(git, OsStr::new(merge_base.as_str()), &base_ref.to_string())
        .ok()
        .flatten();
    match setup_verdict(tip, forked.as_ref(), &Flow::of(setup)) {
        SetupVerdict::Same => Ok(None),
        SetupVerdict::Moved(fields) => Err(changed_setup(&fields, base)),
        SetupVerdict::Changed(fields) => {
            approve_setup_change(git, &fields, merge_base, base, err).map(Some)
        }
    }
}

fn refuse_changed_work(git: &Git, command: &str, commit: &Oid, tree: &Oid) -> Result<(), Failure> {
    let commit_after = head_commit(git)?;
    let tree_after = git.tree_of(OsStr::new(HEAD))?;
    let status = git.status_porcelain()?;
    let hidden = hidden_paths(&git.ls_files_v_z()?);
    let verdict = change_verdict(&WorkFacts {
        dirty: !status.is_empty(),
        hidden: !hidden.is_empty(),
        tree_before: tree,
        tree_after: &tree_after,
        commit_before: commit,
        commit_after: &commit_after,
    });
    match verdict {
        WorkChange::None => Ok(()),
        WorkChange::Dirty => Err(Failure::no(format!(
            "a check changed the work: {} (check: {command})",
            dirty_paths(&status)
        ))),
        WorkChange::Hidden => Err(Failure::no(format!(
            "a check changed the work: it marked tracked files skip-worktree or assume-unchanged \
             (check: {command})"
        ))),
        WorkChange::Committed => {
            let paths = git.changed_paths(tree, &tree_after)?;
            let listed = first_five(paths.lines());
            let shown = if listed.is_empty() {
                "the commit content"
            } else {
                &listed
            };
            Err(Failure::no(format!(
                "a check changed the work: {shown} (it committed; check: {command})"
            )))
        }
        WorkChange::MovedHead => Err(Failure::no(format!(
            "a check changed the work: it moved HEAD to a new commit (check: {command})"
        ))),
    }
}

// After every command the work must be as it was.
fn run_commands(
    git: &Git,
    setup: &Setup,
    (commit, tree): (&Oid, &Oid),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let prefix: Vec<String> = setup
        .environment
        .run
        .iter()
        .map(|word| word.as_str().to_owned())
        .collect();
    for command in setup.check.iter().map(CheckCommand::as_str) {
        writeln!(out, "check: {command}").ok();
        out.flush().ok();
        let code = run_check(git.top(), &prefix, command).unwrap_or_else(|not_started| {
            writeln!(err, "shipit: {}", not_started.message).ok();
            not_started.code
        });
        if code != 0 {
            return Err(Failure::no(format!(
                "check failed (exit {code}): {command}; fix it and run shipit check again"
            )));
        }
        refuse_changed_work(git, command, commit, tree)?;
    }
    Ok(())
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let override_base = parse_args(args)?;
    let git = Git::find()?;
    let setup = load_setup(&git, OsStr::new(HEAD))?;

    if git.sparse_checkout() {
        return Err(Failure::usage(
            "sparse checkout is not supported: checks would not see every file",
        ));
    }
    let hidden = hidden_paths(&git.ls_files_v_z()?);
    if !hidden.is_empty() {
        return Err(Failure::unclean(hidden_refusal(&hidden)));
    }
    if !git.status_porcelain()?.is_empty() {
        return Err(Failure::unclean(UNCOMMITTED));
    }

    let trusted = override_base.is_some();
    let start = override_base.unwrap_or_else(|| setup.base.clone());
    let (base, base_ref, tip) = settle_base(&git, start, trusted, err)?;
    let base_commit = git
        .commit_of(OsStr::new(&base_ref.full()))
        .ok_or_else(|| Failure::usage(format!("cannot resolve {base_ref} to a commit")))?;
    refuse_self_base(&git, &base, trusted, &base_commit)?;
    let merge_base = git
        .merge_base_with_head(OsStr::new(&base_ref.full()))
        .ok_or_else(|| Failure::usage(format!("no shared history with {base_ref}")))?;
    let setup_fields = judge_setup(
        &git,
        &setup,
        (tip.as_ref(), &base),
        (&base_ref, &merge_base),
        err,
    )?;
    let files = git.changed_files_count(&merge_base)?;
    let (fits, message) = size_verdict(files, setup.cap);
    writeln!(out, "{message}").ok();
    if !fits {
        return Err(Failure::no(format!(
            "too big: {files} files (cap {}) against {base_ref}; split the work into smaller \
             changes",
            setup.cap.get()
        )));
    }

    let commit = head_commit(&git)?;
    let tree = git.tree_of(OsStr::new(HEAD))?;
    run_commands(&git, &setup, (&commit, &tree), out, err)?;

    let time = now_utc();
    let path = check_receipt_path(&git.common_dir()?, &tree);
    let commands = setup
        .check
        .iter()
        .map(|command| command.as_str().to_owned())
        .collect();
    let short = git.short(&tree)?;
    let receipt = CheckReceipt::new(
        (tree, commit),
        BaseUsed {
            name: base_ref.full(),
            commit: base_commit,
        },
        (files, setup.cap),
        commands,
        setup_fields,
        time,
    );
    write_json_atomic(&path, &receipt)?;
    writeln!(out, "receipt written for {short}").ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::{name, os};

    #[test]
    fn no_argument_means_the_setup_names_the_base() {
        assert_eq!(parse_args(&[]), Ok(None));
    }

    #[test]
    fn a_base_that_is_not_a_branch_name_is_refused_whatever_git_would_make_of_it() {
        for text in ["-x", "--base", "HEAD~1", "HEAD", "v1^", "a..b", "a b"] {
            assert_eq!(
                parse_args(&os(&["--base", text])),
                Err(Failure::usage(format!(
                    "--base '{text}' is not a branch name (see git check-ref-format --branch); \
                     usage: shipit check [--base <name>]"
                ))),
                "{text}"
            );
        }
    }

    #[test]
    fn the_refusals_say_the_usage() {
        let usage = "usage: shipit check [--base <name>]";
        assert_eq!(
            parse_args(&os(&["--nope"])),
            Err(Failure::usage(format!(
                "unknown argument '--nope'; {usage}"
            )))
        );
        assert_eq!(
            parse_args(&os(&["--cap-from", "main"])),
            Err(Failure::usage(format!(
                "unknown argument '--cap-from'; {usage}"
            )))
        );
        for args in [&["--base"][..], &["--base", ""]] {
            assert_eq!(
                parse_args(&os(args)),
                Err(Failure::usage(format!(
                    "--base needs a branch name; {usage}"
                )))
            );
        }
        assert_eq!(
            parse_args(&os(&["--base", "main", "--base", "main"])),
            Err(Failure::usage(format!("--base given twice; {usage}")))
        );
    }

    #[test]
    fn the_arguments_are_read_in_order() {
        let usage = "usage: shipit check [--base <name>]";
        assert_eq!(
            parse_args(&os(&["--base", "a", "--nope"])),
            Err(Failure::usage(format!(
                "unknown argument '--nope'; {usage}"
            )))
        );
        assert_eq!(
            parse_args(&os(&["--base", "a", "--base", ""])),
            Err(Failure::usage(format!(
                "--base needs a branch name; {usage}"
            )))
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn base_x_parses_to_x_for_any_branch_name_x(x in "[a-z][a-z0-9]{0,9}(/[a-z0-9]{1,5}){0,2}") {
            prop_assert_eq!(parse_args(&os(&["--base", &x])), Ok(Some(name(&x))));
        }
    }
}
