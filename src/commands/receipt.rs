use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::commands::setup::load_setup_table;
use crate::edge::clock::now_utc;
use crate::edge::git::Git;
use crate::edge::records::{
    WriteNew, append_line, build_plan_path, goal_names, load_goal, logbook_path, newest_roadmap,
    plan_review_path, store_dir, write_new,
};
use crate::edge::slice_git::{file_hash, load_policy};
use crate::edge::store::read_json;
use crate::pure::failure::Failure;
use crate::pure::finding::{Finding, Outcome, outcome_problems, parse_finding};
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::{element_named, which_goal};
use crate::pure::logbook::{Entry, line};
use crate::pure::messages::{NOT_A_RESULT, say};
use crate::pure::rank::{Worker, parse_worker, rank_verdict, role_notice};
use crate::pure::review::{Receipt, ReceiptKind, Subject, receipt_time, tool};
use crate::pure::roadmap::{Element, Roadmap, SliceName};
use crate::pure::setup::HEAD;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit receipt <plan-review> <slice|roadmap> --author <w> --by <w> --result <r> [--finding <t>]… [--fresh] [--goal <name>]";
pub(crate) const SUMMARY: &str = "Records a review receipt for the current version of a plan, after checking that the reviewer ranks above the author under the base branch's setup.";
pub(crate) const EXITS: [&str; 4] = [
    "receipt written",
    "refused: rank rule, or a receipt for this version exists",
    "usage, setup, goal, roadmap or finding error",
    "",
];

#[derive(Debug, Default)]
struct Args {
    positional: Vec<String>,
    author: Option<String>,
    by: Option<String>,
    result: Option<String>,
    findings: Vec<String>,
    fresh: bool,
    goal: Option<GoalName>,
}

fn take_value<'a>(flag: &str, next: Option<&'a OsString>) -> Result<&'a str, Failure> {
    next.and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Failure::usage(format!("{flag} needs a value; {USAGE}")))
}

fn once(slot: &mut Option<String>, flag: &str, value: &str) -> Result<(), Failure> {
    if slot.replace(value.to_owned()).is_some() {
        return Err(Failure::usage(format!("{flag} given twice; {USAGE}")));
    }
    Ok(())
}

fn parse_args(args: &[OsString]) -> Result<Args, Failure> {
    let mut parsed = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.to_str() {
            Some("--author") => once(
                &mut parsed.author,
                "--author",
                take_value("--author", rest.next())?,
            )?,
            Some("--by") => once(&mut parsed.by, "--by", take_value("--by", rest.next())?)?,
            Some("--result") => once(
                &mut parsed.result,
                "--result",
                take_value("--result", rest.next())?,
            )?,
            Some("--finding") => parsed
                .findings
                .push(take_value("--finding", rest.next())?.to_owned()),
            Some("--fresh") => parsed.fresh = true,
            Some("--goal") => {
                let name = take_value("--goal", rest.next())?;
                let goal = GoalName::parse(name).ok_or_else(|| {
                    Failure::usage(format!("--goal '{name}' is not a goal name; {USAGE}"))
                })?;
                if parsed.goal.replace(goal).is_some() {
                    return Err(Failure::usage(format!("--goal given twice; {USAGE}")));
                }
            }
            Some(flag) if flag.starts_with("--") => {
                return Err(Failure::usage(format!(
                    "unknown argument '{flag}'; {USAGE}"
                )));
            }
            Some(word) => parsed.positional.push(word.to_owned()),
            None => {
                return Err(Failure::usage(format!(
                    "unknown argument '{}'; {USAGE}",
                    arg.to_string_lossy()
                )));
            }
        }
    }
    Ok(parsed)
}

struct Request {
    kind: ReceiptKind,
    target: String,
    author: Worker,
    by: Worker,
    outcome: Outcome,
    findings: Vec<Finding>,
    fresh: bool,
    goal: Option<GoalName>,
}

fn worker(flag: &str, text: Option<&str>) -> Result<Worker, Failure> {
    let text = text.ok_or_else(|| Failure::usage(format!("{flag} is required; {USAGE}")))?;
    parse_worker(text).ok_or_else(|| {
        Failure::usage(format!(
            "{flag} '{text}' is not <model>:<effort> or <model>:<effort>@<tier>; {USAGE}"
        ))
    })
}

fn request(args: Args) -> Result<Request, Failure> {
    let [kind, target] = args.positional.as_slice() else {
        return Err(Failure::usage(USAGE));
    };
    let kind = ReceiptKind::parse(kind)
        .ok_or_else(|| Failure::usage(format!("unknown kind \"{kind}\"; {USAGE}")))?;
    let result = args
        .result
        .as_deref()
        .ok_or_else(|| Failure::usage(format!("--result is required; {USAGE}")))?;
    let outcome = Outcome::parse(result)
        .ok_or_else(|| Failure::usage(say(NOT_A_RESULT, &[("word", &result)])))?;
    let findings = args
        .findings
        .iter()
        .enumerate()
        .map(|(index, text)| parse_finding(index, text))
        .collect::<Result<Vec<_>, _>>()
        .map_err(Failure::usage)?;
    let problems = outcome_problems(kind.result_kind(), outcome, &findings);
    if !problems.is_empty() {
        return Err(Failure::usage(problems.join("\n")));
    }
    Ok(Request {
        kind,
        target: target.clone(),
        author: worker("--author", args.author.as_deref())?,
        by: worker("--by", args.by.as_deref())?,
        outcome,
        findings,
        fresh: args.fresh,
        goal: args.goal,
    })
}

// The file a plan review is about, and the slice it names (none for the roadmap).
fn plan_target(
    store: &Path,
    goal: &GoalName,
    roadmap: &Roadmap,
    target: &str,
) -> Result<(Option<SliceName>, PathBuf), Failure> {
    let Some(slice) = SliceName::parse(target) else {
        return Ok((None, newest_roadmap(store, goal)?));
    };
    let unknown = || {
        Failure::usage(format!(
            "no slice named \"{target}\" in the roadmap of goal \"{goal}\""
        ))
    };
    match element_named(roadmap, &slice).ok_or_else(unknown)? {
        Element::Slice(_) => Ok((Some(slice.clone()), build_plan_path(store, goal, &slice))),
        Element::Proof(_) => Err(Failure::usage(format!(
            "\"{target}\" is a proof step: it has no build plan"
        ))),
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (head, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let request = request(parse_args(args)?)?;
    let policy = load_policy(&git, head, err)?;
    let common = git.common_dir()?;
    let store = store_dir(&common);
    let goal = which_goal(
        request.goal.as_ref(),
        git.current_branch().as_deref(),
        &goal_names(&store),
    )?;
    let roadmap = load_goal(&common, &goal)?;

    let ReceiptKind::PlanReview = request.kind;
    let (slice, path) = plan_target(&store, &goal, &roadmap, &request.target)?;
    let key = file_hash(&git, &path)?;
    let (author_rank, by_rank, basis) =
        rank_verdict(&policy, &request.author, &request.by, request.fresh)?;
    let notice = role_notice(
        "planner",
        &policy.agents.roles.planner,
        &request.author,
        &author_rank,
    );
    if let Some(notice) = &notice {
        writeln!(err, "shipit: {notice}").ok();
    }
    let relative = path.strip_prefix(&store).map_or_else(
        |_why| path.display().to_string(),
        |rest| rest.display().to_string(),
    );
    let time = now_utc();
    let receipt = Receipt {
        kind: request.kind.name(),
        version: 1,
        key: key.to_string(),
        slice: slice.as_ref(),
        subject: Subject::File { file: relative },
        author: &author_rank,
        author_is_planner: Some(notice.is_none()),
        by: &by_rank,
        rank: basis,
        result: request.outcome,
        findings: &request.findings,
        time: time.clone(),
        tool: tool(),
    };
    let receipt_path = plan_review_path(&store, &key);
    match write_new(&receipt_path, &receipt) {
        Ok(()) => {}
        Err(WriteNew::Exists) => {
            let when = receipt_time(read_json(&receipt_path).as_deref())
                .unwrap_or_else(|| "unreadable".to_owned());
            return Err(Failure::no(format!(
                "a {} receipt for this version already exists ({when})",
                request.kind.name()
            )));
        }
        Err(WriteNew::Cannot(failure)) => return Err(failure),
    }
    if let Some(slice) = &slice {
        let entry = Entry::Receipt {
            kind: request.kind.name().to_owned(),
            key: key.to_string(),
            result: request.outcome.to_string(),
            findings: u64::try_from(request.findings.len()).unwrap_or(u64::MAX),
            author: request.author.to_string(),
            by: request.by.to_string(),
            rank: basis.to_string(),
        };
        let text = line(&entry, slice, &time, &tool()).map_err(Failure::usage)?;
        append_line(&logbook_path(&store, &goal), &text)?;
    }
    let short = key.as_str().get(..12).unwrap_or(key.as_str());
    writeln!(
        out,
        "receipt written: {} for {} ({short}), {} above {} by {basis}",
        request.kind.name(),
        slice
            .as_ref()
            .map_or_else(|| "roadmap".to_owned(), ToString::to_string),
        request.by,
        request.author,
    )
    .ok();
    Ok(())
}
