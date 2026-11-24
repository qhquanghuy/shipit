use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::commands::setup::load_setup_table;
use crate::edge::clock::now_utc;
use crate::edge::git::Git;
use crate::edge::records::{
    WriteNew, append_line, build_plan_path, goal_names, load_goal, logbook_path, newest_roadmap,
    receipt_path, store_dir, write_new,
};
use crate::edge::slice_git::{branch_tip, file_hash, load_policy, tip_of};
use crate::edge::store::read_json;
use crate::pure::branch::slice_branch;
use crate::pure::failure::Failure;
use crate::pure::finding::{Finding, Outcome, outcome_problems, parse_finding};
use crate::pure::flags::{goal_flag, once, take_value, unknown_argument, worker_flag};
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::{element_named, which_goal};
use crate::pure::logbook::{Entry, line};
use crate::pure::messages::{NOT_A_RESULT, say};
use crate::pure::rank::{Basis, Worker, rank_verdict, role_notice, role_text};
use crate::pure::receipt::Oid;
use crate::pure::review::{Receipt, ReceiptKind, Subject, receipt_time, tool};
use crate::pure::roadmap::{Element, Roadmap, SliceName};
use crate::pure::setup::{HEAD, Role};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit receipt <plan-review|informed-review|second-opinion|test-drive> <slice|roadmap> --author <w> --by <w> --result <r> [--finding <t>]… [--fresh] [--commit <c>] [--goal <name>]";
pub(crate) const SUMMARY: &str = "Records a review or test-drive receipt for the current version of a plan or of the work, after checking that the reviewer ranks above the author under the base branch's setup. For the work the author must be the setup's maker.";
pub(crate) const EXITS: [&str; 4] = [
    "receipt written",
    "refused: rank rule, a receipt for this version exists, no check receipt for this content, no changes against the base, or the author is not the setup's maker",
    "usage, setup, goal, roadmap or finding error",
    "",
];

#[derive(Debug, Default)]
struct Args {
    positional: Vec<String>,
    author: Option<String>,
    by: Option<String>,
    result: Option<String>,
    commit: Option<String>,
    findings: Vec<String>,
    fresh: bool,
    goal: Option<GoalName>,
}

fn parse_args(args: &[OsString]) -> Result<Args, Failure> {
    let mut parsed = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.to_str() {
            Some("--author") => once(&mut parsed.author, "--author", rest.next(), USAGE)?,
            Some("--by") => once(&mut parsed.by, "--by", rest.next(), USAGE)?,
            Some("--result") => once(&mut parsed.result, "--result", rest.next(), USAGE)?,
            Some("--commit") => once(&mut parsed.commit, "--commit", rest.next(), USAGE)?,
            Some("--finding") => parsed
                .findings
                .push(take_value("--finding", rest.next(), USAGE)?.to_owned()),
            Some("--fresh") => parsed.fresh = true,
            Some("--goal") => goal_flag(&mut parsed.goal, rest.next(), USAGE)?,
            Some(flag) if flag.starts_with("--") => return Err(unknown_argument(arg, USAGE)),
            Some(word) => parsed.positional.push(word.to_owned()),
            None => return Err(unknown_argument(arg, USAGE)),
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
    commit: Option<String>,
    goal: Option<GoalName>,
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
    if args.commit.is_some() && kind == ReceiptKind::PlanReview {
        return Err(Failure::usage(format!(
            "--commit is for work, not a plan; {USAGE}"
        )));
    }
    Ok(Request {
        kind,
        target: target.clone(),
        author: worker_flag("--author", args.author.as_deref(), USAGE)?,
        by: worker_flag("--by", args.by.as_deref(), USAGE)?,
        outcome,
        findings,
        fresh: args.fresh,
        commit: args.commit,
        goal: args.goal,
    })
}

// What a receipt is about: the slice it names (none for a roadmap), where it is kept, the key
// that makes it this version's, and what it says it is about.
struct Target {
    slice: Option<SliceName>,
    key: Oid,
    subject: Subject,
    tree: Option<Oid>,
    commit: Option<Oid>,
}

fn unknown_slice(target: &str, goal: &GoalName) -> Failure {
    Failure::usage(format!(
        "no slice named \"{target}\" in the roadmap of goal \"{goal}\""
    ))
}

fn plan_target(
    git: &Git,
    store: &Path,
    goal: &GoalName,
    roadmap: &Roadmap,
    target: &str,
) -> Result<Target, Failure> {
    let (slice, file) = match SliceName::parse(target) {
        None => (None, newest_roadmap(store, goal)?),
        Some(slice) => match element_named(roadmap, &slice) {
            None => return Err(unknown_slice(target, goal)),
            Some(Element::Proof(_)) => {
                return Err(Failure::usage(format!(
                    "\"{target}\" is a proof step: it has no build plan"
                )));
            }
            Some(Element::Slice(_)) => {
                let file = build_plan_path(store, goal, &slice);
                (Some(slice), file)
            }
        },
    };
    let key = file_hash(git, &file)?;
    let relative = file.strip_prefix(store).map_or_else(
        |_why| file.display().to_string(),
        |rest| rest.display().to_string(),
    );
    Ok(Target {
        slice,
        key,
        subject: Subject::File { file: relative },
        tree: None,
        commit: None,
    })
}

fn work_target(
    git: &Git,
    common: &Path,
    (goal, roadmap): (&GoalName, &Roadmap),
    request: &Request,
) -> Result<Target, Failure> {
    let slice =
        SliceName::parse(&request.target).ok_or_else(|| unknown_slice(&request.target, goal))?;
    match element_named(roadmap, &slice).ok_or_else(|| unknown_slice(&request.target, goal))? {
        Element::Proof(_) => {
            return Err(Failure::usage(format!(
                "\"{}\" is a proof step: it has no work to review",
                request.target
            )));
        }
        Element::Slice(found) => {
            if request.kind == ReceiptKind::TestDrive && !found.test_drive {
                return Err(Failure::usage(format!(
                    "slice \"{}\" has no test drive in the roadmap",
                    request.target
                )));
            }
        }
    }
    let branch = slice_branch(goal, &slice);
    let branch_head = branch_tip(git, &branch)
        .ok_or_else(|| Failure::usage(format!("branch {branch} not found")))?;
    // A review of another slice's commit must not stand for this slice's work.
    let commit = match &request.commit {
        Some(rev) => {
            let commit = git
                .commit_of(OsStr::new(rev))
                .ok_or_else(|| Failure::usage(format!("'{rev}' is not a commit")))?;
            if !git.is_ancestor(&commit, &branch_head) {
                return Err(Failure::no(format!("{rev} is not on {branch}")));
            }
            commit
        }
        None => branch_head,
    };
    let tip = tip_of(git, common, commit)?;
    let checked = tip.checked.ok_or_else(|| {
        Failure::no(format!(
            "no check receipt for this content: run shipit check on {branch}"
        ))
    })?;
    let key = match request.kind {
        ReceiptKind::TestDrive => tip.tree.clone(),
        ReceiptKind::PlanReview | ReceiptKind::InformedReview | ReceiptKind::SecondOpinion => {
            checked.patch_id.ok_or_else(|| {
                Failure::no(format!("the slice has no changes against {}", checked.base))
            })?
        }
    };
    Ok(Target {
        slice: Some(slice),
        key,
        subject: Subject::Work {
            branch,
            commit: tip.commit.to_string(),
            tree: tip.tree.to_string(),
            merge_base: checked.merge_base.to_string(),
        },
        tree: Some(tip.tree),
        commit: Some(tip.commit),
    })
}

fn log_receipt(
    store: &Path,
    goal: &GoalName,
    (request, target): (&Request, &Target),
    (basis, time): (Basis, &str),
) -> Result<(), Failure> {
    let Some(slice) = &target.slice else {
        return Ok(());
    };
    let entry = Entry::Receipt {
        kind: request.kind.name().to_owned(),
        key: target.key.to_string(),
        tree: target.tree.as_ref().map(ToString::to_string),
        commit: target.commit.as_ref().map(ToString::to_string),
        result: request.outcome.to_string(),
        findings: u64::try_from(request.findings.len()).unwrap_or(u64::MAX),
        author: request.author.to_string(),
        by: request.by.to_string(),
        rank: basis.to_string(),
    };
    let text = line(&entry, slice, time, &tool()).map_err(Failure::usage)?;
    append_line(&logbook_path(store, goal), &text)
}

// A plan's author may be anyone, with a notice; the author of the work is the setup's maker.
fn check_author(
    request: &Request,
    (role_of, notice): (&Role, Option<&String>),
    err: &mut dyn Write,
) -> Result<(), Failure> {
    if request.kind == ReceiptKind::PlanReview {
        if let Some(notice) = notice {
            writeln!(err, "shipit: {notice}").ok();
        }
        return Ok(());
    }
    if notice.is_some() {
        return Err(Failure::no(format!(
            "--author {} is not the setup's maker ({}): the work is the maker's, so its review names the setup's maker",
            request.author,
            role_text(role_of)
        )));
    }
    Ok(())
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (head, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let request = request(parse_args(args)?)?;
    let (policy, _base_ref) = load_policy(&git, head, err)?;
    let common = git.common_dir()?;
    let store = store_dir(&common);
    let goal = which_goal(
        request.goal.as_ref(),
        git.current_branch().as_deref(),
        &goal_names(&store),
    )?;
    let roadmap = load_goal(&common, &goal)?;

    let (target, role, role_of) = match request.kind {
        ReceiptKind::PlanReview => (
            plan_target(&git, &store, &goal, &roadmap, &request.target)?,
            "planner",
            &policy.agents.roles.planner,
        ),
        ReceiptKind::InformedReview | ReceiptKind::SecondOpinion | ReceiptKind::TestDrive => (
            work_target(&git, &common, (&goal, &roadmap), &request)?,
            "maker",
            &policy.agents.roles.maker,
        ),
    };
    let (author_rank, by_rank, basis) = rank_verdict(
        &policy,
        &request.author,
        &request.by,
        request.fresh,
        (role, role_of),
    )?;
    let notice = role_notice(role, role_of, &request.author, &author_rank);
    check_author(&request, (role_of, notice.as_ref()), err)?;
    let is_role = Some(notice.is_none());
    let is_planner = is_role.filter(|_| request.kind == ReceiptKind::PlanReview);
    let is_maker = is_role.filter(|_| request.kind != ReceiptKind::PlanReview);
    let time = now_utc();
    let receipt = Receipt {
        kind: request.kind.name(),
        version: 1,
        key: target.key.to_string(),
        slice: target.slice.as_ref(),
        subject: target.subject.clone(),
        author: &author_rank,
        author_is_maker: is_maker,
        author_is_planner: is_planner,
        by: &by_rank,
        rank: basis,
        result: request.outcome,
        findings: &request.findings,
        time: time.clone(),
        tool: tool(),
    };
    let path = receipt_path(&store, request.kind, &target.key);
    match write_new(&path, &receipt) {
        Ok(()) => {}
        Err(WriteNew::Exists) => {
            let when = receipt_time(read_json(&path).as_deref())
                .unwrap_or_else(|| "unreadable".to_owned());
            return Err(Failure::no(format!(
                "a {} receipt for this version already exists ({when})",
                request.kind.name()
            )));
        }
        Err(WriteNew::Cannot(failure)) => return Err(failure),
    }
    log_receipt(&store, &goal, (&request, &target), (basis, &time))?;
    let short = target.key.as_str().get(..12).unwrap_or(target.key.as_str());
    writeln!(
        out,
        "receipt written: {} for {} ({short}), {} above {} by {basis}",
        request.kind.name(),
        target
            .slice
            .as_ref()
            .map_or_else(|| "roadmap".to_owned(), ToString::to_string),
        request.by,
        request.author,
    )
    .ok();
    Ok(())
}
