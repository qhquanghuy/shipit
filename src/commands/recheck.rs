use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::PathBuf;

use crate::commands::check::{base_argument, settle_base};
use crate::commands::setup::load_setup_table;
use crate::edge::base::base_ref;
use crate::edge::git::Git;
use crate::edge::run::run_check;
use crate::edge::write::append_text;
use crate::pure::base::BranchName;
use crate::pure::failure::Failure;
use crate::pure::receipt::Oid;
use crate::pure::recheck::{
    Mode, Report, SetupStatus, ci_paths, mode, plain_checks, setup_change, stopped, summary,
    warnings,
};
use crate::pure::rules::invalid;
use crate::pure::setup::{HEAD, SETUP_PATH, setup_label};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str =
    "usage: shipit recheck --base <name> --branch <name> [--summary <file>]";
pub(crate) const SUMMARY: &str = "What the CI Action runs: on a slice/* branch the full shipit check from the start, on any other branch only the base's check commands; it trusts no receipt and writes the report to the file.";
pub(crate) const EXITS: [&str; 4] = [
    "checks passed",
    "a check failed or the change is over the cap",
    "usage or setup error",
    "uncommitted changes",
];

#[derive(Debug, PartialEq, Eq)]
struct Args {
    base: BranchName,
    branch: String,
    summary: Option<PathBuf>,
}

fn value_of<'a>(
    flag: &str,
    rest: &mut impl Iterator<Item = &'a OsString>,
) -> Result<&'a OsString, Failure> {
    rest.next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Failure::usage(format!("{flag} needs a value; {USAGE}")))
}

fn twice(flag: &str) -> Failure {
    Failure::usage(format!("{flag} given twice; {USAGE}"))
}

fn parse_args(args: &[OsString]) -> Result<Args, Failure> {
    let mut base: Option<BranchName> = None;
    let mut branch: Option<String> = None;
    let mut summary: Option<PathBuf> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--base" {
            if base.replace(base_argument(rest.next(), USAGE)?).is_some() {
                return Err(twice("--base"));
            }
        } else if arg == "--branch" {
            let name = value_of("--branch", &mut rest)?
                .to_string_lossy()
                .into_owned();
            if branch.replace(name).is_some() {
                return Err(twice("--branch"));
            }
        } else if arg == "--summary" {
            let path = PathBuf::from(value_of("--summary", &mut rest)?);
            if summary.replace(path).is_some() {
                return Err(twice("--summary"));
            }
        } else {
            return Err(Failure::usage(format!(
                "unknown argument '{}'; {USAGE}",
                arg.to_string_lossy()
            )));
        }
    }
    Ok(Args {
        base: base.ok_or_else(|| Failure::usage(format!("--base is required; {USAGE}")))?,
        branch: branch.ok_or_else(|| Failure::usage(format!("--branch is required; {USAGE}")))?,
        summary,
    })
}

struct Facts {
    merge_base: Oid,
    adopting: bool,
    at_base_tip: bool,
    setup: SetupStatus,
    ci: Vec<String>,
}

fn text_at(git: &Git, rev: &OsStr) -> Option<String> {
    git.show_file(rev, SETUP_PATH)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn facts(git: &Git, base: &BranchName) -> Result<Facts, Failure> {
    let base_ref = base_ref(git, base)?;
    let full = base_ref.full();
    let merge_base = git
        .merge_base_with_head(OsStr::new(&full))
        .ok_or_else(|| Failure::usage(format!("no shared history with {base_ref}")))?;
    let setup = setup_change(
        text_at(git, OsStr::new(merge_base.as_str())).as_deref(),
        text_at(git, OsStr::new(HEAD)).as_deref(),
    );
    let ci = ci_paths(&git.changed_names(&merge_base)?);
    let head = git.commit_of(OsStr::new(HEAD));
    Ok(Facts {
        adopting: !git.file_exists(OsStr::new(&full), SETUP_PATH),
        at_base_tip: head.is_some() && head == git.commit_of(OsStr::new(&full)),
        merge_base,
        setup,
        ci,
    })
}

// The base's own check commands and environment: the pull request's setup judges nothing,
// unless the base has none yet (adoption).
fn plain(
    git: &Git,
    base: &BranchName,
    merge_base: &Oid,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    // The pull request's setup judges nothing here, but a command that needs a setup stops
    // when there is none or it is broken: deleting the file is not a way to skip the checks.
    let (_head_setup, head_table) = load_setup_table(git, OsStr::new(HEAD))?;
    let (_base, base_ref, tip) = settle_base(git, base.clone(), true, err)?;
    let (table, label) = match tip {
        Some(flow) => (flow.table().clone(), setup_label(&base_ref.to_string())),
        None => (head_table, setup_label(HEAD)),
    };
    let checks =
        plain_checks(&table).map_err(|problems| Failure::usage(invalid(&label, &problems)))?;
    for command in &checks.commands {
        writeln!(out, "check: {command}").ok();
        out.flush().ok();
        let vars = [
            ("SHIPIT_MERGE_BASE", OsStr::new(merge_base.as_str())),
            ("SHIPIT_CHECK", OsStr::new(command)),
        ];
        let code =
            run_check(git.top(), &checks.prefix, command, &vars).unwrap_or_else(|not_started| {
                writeln!(err, "shipit: {}", not_started.message).ok();
                not_started.code
            });
        if code != 0 {
            return Err(Failure::no(format!(
                "check failed (exit {code}): {command}"
            )));
        }
    }
    Ok(())
}

fn leave(summary: Option<&PathBuf>, out: &mut dyn Write, text: &str) -> Result<(), Failure> {
    write!(out, "{text}").ok();
    summary.map_or(Ok(()), |path| append_text(path, text))
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let args = match parse_args(args) {
        Ok(args) => args,
        Err(usage) => {
            // NEEDS_SETUP says a missing setup file stops this command, so it outranks a
            // complaint about the arguments (tests/dispatch.bats holds every command to it).
            if let Ok(git) = Git::find() {
                load_setup_table(&git, OsStr::new(HEAD))?;
            }
            return Err(usage);
        }
    };
    let prepared = Git::find().and_then(|git| facts(&git, &args.base).map(|facts| (git, facts)));
    let (git, facts) = match prepared {
        Ok(found) => found,
        Err(failure) => {
            leave(args.summary.as_ref(), out, &stopped(&failure.message)).ok();
            return Err(failure);
        }
    };
    let report = Report {
        base: args.base.to_string(),
        mode: mode(&args.branch),
        branch: args.branch.clone(),
        adopting: facts.adopting,
        at_base_tip: facts.at_base_tip,
        setup: facts.setup,
        ci: facts.ci,
    };
    for line in warnings(&report) {
        writeln!(out, "{line}").ok();
    }
    let result = match report.mode {
        Mode::Slice => crate::commands::check::run(
            &[OsString::from("--base"), OsString::from(args.base.as_str())],
            out,
            err,
        ),
        Mode::Plain => plain(&git, &args.base, &facts.merge_base, out, err),
    };
    let verdict = match &result {
        Ok(()) => Ok(()),
        Err(failure) => Err(failure.exit.code()),
    };
    let written = leave(args.summary.as_ref(), out, &summary(&report, verdict));
    result.and(written)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::{name, os};

    #[test]
    fn recheck_args() {
        let parsed = parse_args(&os(&["--base", "main", "--branch", "slice/x"]));
        assert_eq!(
            parsed,
            Ok(Args {
                base: name("main"),
                branch: "slice/x".to_owned(),
                summary: None,
            })
        );
        for (words, message) in [
            (vec![], "--base is required"),
            (vec!["--base", "main"], "--branch is required"),
            (vec!["--branch", "x"], "--base is required"),
            (vec!["--base"], "--base needs a branch name"),
            (
                vec!["--base", "main", "--base", "main"],
                "--base given twice",
            ),
            (vec!["--branch"], "--branch needs a value"),
            (vec!["--branch", ""], "--branch needs a value"),
            (
                vec!["--branch", "a", "--branch", "b"],
                "--branch given twice",
            ),
            (
                vec!["--base", "main", "--branch", "a", "--summary"],
                "--summary needs a value",
            ),
            (
                vec![
                    "--base",
                    "main",
                    "--branch",
                    "a",
                    "--summary",
                    "f",
                    "--summary",
                    "g",
                ],
                "--summary given twice",
            ),
            (
                vec!["--base", "main", "--branch", "a", "--nope"],
                "unknown argument '--nope'",
            ),
        ] {
            assert_eq!(
                parse_args(&os(&words)),
                Err(Failure::usage(format!("{message}; {USAGE}"))),
                "{words:?}"
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn the_flags_come_in_any_order(
            order in Just(vec![0_usize, 1, 2]).prop_shuffle(),
            branch in "[a-z/]{1,8}",
            file in "[a-z.]{1,8}",
        ) {
            let pairs = [
                ["--base".to_owned(), "main".to_owned()],
                ["--branch".to_owned(), branch.clone()],
                ["--summary".to_owned(), file.clone()],
            ];
            let words: Vec<String> = order
                .iter()
                .flat_map(|at| pairs.get(*at).cloned().unwrap_or_default())
                .collect();
            let refs: Vec<&str> = words.iter().map(String::as_str).collect();
            prop_assert_eq!(
                parse_args(&os(&refs)),
                Ok(Args { base: name("main"), branch, summary: Some(PathBuf::from(file)) })
            );
        }
    }
}
