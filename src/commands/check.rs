use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::setup::load_setup;
use crate::edge::clock::now_utc;
use crate::edge::git::Git;
use crate::edge::run::run_check;
use crate::edge::store::{check_receipt_path, write_json_atomic};
use crate::pure::base::{BaseRef, BranchName, SelfBase, base_verdict, choose_base_ref};
use crate::pure::baseline::{Flow, parse_baseline, setup_changes};
use crate::pure::failure::Failure;
use crate::pure::json::invalid;
use crate::pure::receipt::{CheckReceipt, Oid};
use crate::pure::setup::{CheckCommand, SETUP_PATH, Setup, json_values, setup_label};
use crate::pure::size::size_verdict;
use crate::pure::work::{
    UNCOMMITTED, WorkChange, WorkFacts, change_verdict, dirty_paths, first_five, hidden_paths,
    hidden_refusal,
};

pub(crate) const HIDDEN: bool = false;

const USAGE: &str = "usage: shipit check [--base <name>]";
const HEAD: &str = "HEAD";

// The value after --base is taken whatever it starts with.
fn parse_args(args: &[OsString]) -> Result<Option<BranchName>, Failure> {
    let mut base: Option<BranchName> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--base" {
            let name = rest
                .next()
                .and_then(|value| BranchName::parse(&value.to_string_lossy()))
                .ok_or_else(|| Failure::usage(format!("--base needs a branch name; {USAGE}")))?;
            if base.replace(name).is_some() {
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

fn base_ref(git: &Git, base: &BranchName) -> Result<BaseRef, Failure> {
    let origin_name = BaseRef::Origin(base.clone()).to_string();
    let local_name = base.to_string();
    let origin = git.commit_of(OsStr::new(&origin_name)).is_some();
    let local = git.commit_of(OsStr::new(&local_name)).is_some();
    let local_has_origin =
        origin && local && git.is_ancestor(OsStr::new(&origin_name), OsStr::new(&local_name));
    choose_base_ref(base, origin, local, local_has_origin).ok_or_else(|| {
        Failure::usage(format!(
            "base branch \"{base}\" not found (looked for origin/{base} and {base})"
        ))
    })
}

fn refuse_self_base(
    git: &Git,
    base: &BranchName,
    trusted: bool,
    base_ref: &BaseRef,
) -> Result<(), Failure> {
    let current = git.current_branch();
    let ref_commit = git
        .commit_of(OsStr::new(&base_ref.to_string()))
        .ok_or_else(|| Failure::usage(format!("cannot resolve {base_ref} to a commit")))?;
    let head = head_commit(git)?;
    base_verdict(trusted, base, current.as_deref(), &ref_commit, &head).map_err(
        |reason: SelfBase| {
            Failure::usage(format!(
                "base \"{base}\" {reason}, and the setup is the branch's own word; pass --base \
                 <name> naming the real base branch (CI passes the pull request's base)"
            ))
        },
    )
}

fn load_baseline(
    git: &Git,
    base_ref: &BaseRef,
    err: &mut dyn Write,
) -> Result<Option<Flow>, Failure> {
    let name = base_ref.to_string();
    let rev = OsStr::new(&name);
    let label = setup_label(&name);
    if !git.file_exists(rev, SETUP_PATH) {
        writeln!(err, "shipit: no setup file on {name}; nothing to compare").ok();
        return Ok(None);
    }
    let bytes = git
        .show_file(rev, SETUP_PATH)
        .ok_or_else(|| Failure::usage(format!("no setup file at {label} (see docs/setup.md)")))?;
    let docs =
        json_values(&bytes).map_err(|_why| Failure::usage(format!("{label} is not valid JSON")))?;
    parse_baseline(&docs)
        .map(Some)
        .map_err(|problems| Failure::usage(invalid(&label, &problems)))
}

// The slice must leave base, cap, check and environment as the base branch has them: a branch
// that could change them could vouch for itself.
fn refuse_changed_setup(
    git: &Git,
    setup: &Setup,
    base: &BranchName,
    base_ref: &BaseRef,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let Some(baseline) = load_baseline(git, base_ref, err)? else {
        return Ok(());
    };
    let changes = setup_changes(&baseline, &Flow::of(setup));
    if changes.is_empty() {
        return Ok(());
    }
    let fields = joined(&changes);
    Err(Failure::no(format!(
        "this slice changes the checks ({fields}); the owner merges setup changes on {base} \
         first (or merge/rebase {base} if it moved on)"
    )))
}

fn joined<T: ToString>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
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
    commit: &Oid,
    tree: &Oid,
    out: &mut dyn Write,
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
        let code = run_check(git.top(), &prefix, command);
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
    let base = override_base.unwrap_or_else(|| BranchName::from(&setup.base));
    let base_ref = base_ref(&git, &base)?;
    refuse_self_base(&git, &base, trusted, &base_ref)?;
    refuse_changed_setup(&git, &setup, &base, &base_ref, err)?;
    let merge_base = git
        .merge_base_with_head(OsStr::new(&base_ref.to_string()))
        .ok_or_else(|| Failure::usage(format!("no shared history with {base_ref}")))?;
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
    run_commands(&git, &setup, &commit, &tree, out)?;

    let time = now_utc();
    let path = check_receipt_path(&git.common_dir()?, &tree);
    let commands = setup
        .check
        .iter()
        .map(|command| command.as_str().to_owned())
        .collect();
    let short = git.short(&tree)?;
    let receipt = CheckReceipt::new(
        tree,
        commit,
        (files, setup.cap),
        commands,
        time,
        format!("shipit {}", env!("CARGO_PKG_VERSION")),
    );
    write_json_atomic(&path, &receipt)?;
    writeln!(out, "receipt written for {short}").ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn name(text: &str) -> BranchName {
        BranchName::parse(text).expect("a non-empty name")
    }

    #[test]
    fn no_argument_means_the_setup_names_the_base() {
        assert_eq!(parse_args(&[]), Ok(None));
    }

    #[test]
    fn base_takes_the_next_argument_whatever_it_starts_with() {
        assert_eq!(parse_args(&os(&["--base", "main"])), Ok(Some(name("main"))));
        assert_eq!(parse_args(&os(&["--base", "-x"])), Ok(Some(name("-x"))));
        assert_eq!(
            parse_args(&os(&["--base", "--base"])),
            Ok(Some(name("--base")))
        );
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
        fn base_x_parses_to_x_for_any_non_empty_x(x in ".{1,20}") {
            prop_assert_eq!(parse_args(&os(&["--base", &x])), Ok(Some(name(&x))));
        }
    }
}
