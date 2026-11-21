use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::edge::github::create_issue;
use crate::edge::records::{
    kept_files, load_goal, metric_records, metrics_path, read_logbook, receipt_doc, remove_goal,
    store_dir, write_text_atomic,
};
use crate::pure::complete::{Findings, metrics_toml, plan, recorded_issues};
use crate::pure::failure::Failure;
use crate::pure::flags::unknown_argument;
use crate::pure::goal::GoalName;
use crate::pure::logbook::{Entry, Logged};
use crate::pure::metric::read_record;
use crate::pure::review::stored_findings;
use crate::pure::setup::{HEAD, toml_table};
use crate::pure::text::comma_list;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit complete <goal> [--confirm]";
pub(crate) const SUMMARY: &str = "For a goal whose every slice and proof step has shipped, prints what it would keep, create and delete; with --confirm (the owner's yes, which the tool cannot check) it keeps the metrics, opens a GitHub issue for each open known issue, then deletes the goal's own files.";
pub(crate) const EXITS: [&str; 4] = [
    "plan printed, or goal completed",
    "the goal is not complete, or a GitHub call failed (nothing deleted)",
    "usage, setup or goal error",
    "",
];

fn parse_args(args: &[OsString]) -> Result<(GoalName, bool), Failure> {
    let mut confirm = false;
    let mut names = Vec::new();
    for arg in args {
        match arg.to_str() {
            Some("--confirm") => confirm = true,
            Some(flag) if flag.starts_with("--") => return Err(unknown_argument(arg, USAGE)),
            Some(word) => names.push(word),
            None => return Err(unknown_argument(arg, USAGE)),
        }
    }
    match names.as_slice() {
        [name] => GoalName::parse(name)
            .map(|goal| (goal, confirm))
            .ok_or_else(|| Failure::usage(format!("'{name}' is not a goal name; {USAGE}"))),
        _ => Err(Failure::usage(USAGE)),
    }
}

// The findings that the known issues name, read from their receipts.
fn findings_of(store: &Path, entries: &[Logged]) -> Findings {
    let mut found = Findings::new();
    for logged in entries {
        if let Entry::KnownIssue { kind, key, .. } = &logged.entry
            && let Some(all) = receipt_doc(store, kind, key)
                .as_ref()
                .and_then(stored_findings)
        {
            found.insert((kind.clone(), key.clone()), all);
        }
    }
    found
}

// The issues a half-done run made. A kept file that is not one is somebody else's.
fn recorded_so_far(store: &Path, goal: &GoalName) -> Result<Option<Vec<String>>, Failure> {
    let Some((_, text)) = kept_files(store).into_iter().find(|(name, _)| name == goal) else {
        return Ok(None);
    };
    let not_ours = || {
        Failure::usage(format!(
            "metrics/{goal}.toml exists and is not a half-done completion of this goal"
        ))
    };
    let table = text
        .and_then(|text| toml_table(text.as_bytes()).ok())
        .ok_or_else(not_ours)?;
    recorded_issues(&table).map(Some).ok_or_else(not_ours)
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (_head, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let (goal, confirm) = parse_args(args)?;
    let common = git.common_dir()?;
    let store = store_dir(&common);
    let roadmap = load_goal(&common, &goal)?;
    let entries = read_logbook(&store, &goal)?;
    let records: Vec<_> = metric_records(&store)
        .iter()
        .filter_map(read_record)
        .collect();
    let planned = plan(
        &goal,
        &roadmap,
        &entries,
        &findings_of(&store, &entries),
        &records,
    )
    .map_err(|names| {
        Failure::no(format!(
            "goal {goal} is not complete: {} not shipped",
            comma_list(names)
        ))
    })?;
    let recorded = recorded_so_far(&store, &goal)?;
    let mut urls = recorded.clone().unwrap_or_default();
    if urls.len() > planned.issues.len() {
        return Err(Failure::usage(format!(
            "metrics/{goal}.toml records more issues than the goal has known issues"
        )));
    }
    let remaining: Vec<_> = planned.issues.iter().skip(urls.len()).collect();

    if !confirm {
        let titles = if remaining.is_empty() {
            "none".to_owned()
        } else {
            remaining
                .iter()
                .map(|issue| issue.title.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        };
        writeln!(
            out,
            "goal {goal} is complete. Keeps: metrics of {} slices (metrics/{goal}.toml). GitHub \
             issues to create ({}): {titles}. Deletes: goals/{goal}/. Owner: confirm, then run it \
             again with --confirm.",
            planned.kept.len(),
            remaining.len()
        )
        .ok();
        return Ok(());
    }

    let kept_file = metrics_path(&store, &goal);
    if recorded.is_none() {
        write_text_atomic(&kept_file, &metrics_toml(&planned.kept, &urls))?;
    }
    for issue in remaining {
        match create_issue(git.top(), &issue.title, &issue.body) {
            Ok(url) => {
                urls.push(url);
                write_text_atomic(&kept_file, &metrics_toml(&planned.kept, &urls))?;
            }
            Err(why) => {
                let made = if urls.is_empty() {
                    "none".to_owned()
                } else {
                    comma_list(&urls)
                };
                return Err(Failure::no(format!(
                    "cannot create the issue \"{}\": {why}; issues created so far: {made}; \
                     nothing was deleted",
                    issue.title
                )));
            }
        }
    }
    remove_goal(&store, &goal)?;
    writeln!(
        out,
        "goal {goal} completed: metrics kept, {} issues created ({}), goals/{goal}/ deleted",
        urls.len(),
        comma_list(&urls)
    )
    .ok();
    Ok(())
}
