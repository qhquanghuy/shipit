use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::edge::records::{
    build_plan_path, load_goals, newest_roadmap, read_logbook, receipt_path, store_dir,
};
use crate::edge::slice_git::{
    branch_tip, commits_beyond, file_hash, load_policy, slice_branches, tip_of,
};
use crate::edge::store::read_json;
use crate::pure::base::BaseRef;
use crate::pure::branch::{parse_slice_branch, slice_branch};
use crate::pure::failure::Failure;
use crate::pure::finding::Outcome;
use crate::pure::flags::{goal_flag, unknown_argument};
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::{element_name, element_named};
use crate::pure::logbook::Logged;
use crate::pure::receipt::Oid;
use crate::pure::review::{ReceiptKind, counts, read_receipt};
use crate::pure::roadmap::{Element, Roadmap};
use crate::pure::setup::{HEAD, Setup};
use crate::pure::state::{
    Counts, Facts, GoalView, Kind, Plan, Policy, Unknown, Work, is_shipped, render, started_count,
    view,
};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit status [--goal <name>] [--all] [--json]";
pub(crate) const SUMMARY: &str = "Shows each slice's state and next step, computed from receipts and the logbook, and writes nothing; a stale branch of an abandoned slice counts as started until it is deleted.";
pub(crate) const EXITS: [&str; 4] = [
    "shown",
    "",
    "usage, setup, goal, roadmap or logbook error",
    "",
];

#[derive(Debug, Default)]
struct Args {
    goal: Option<GoalName>,
    all: bool,
    json: bool,
}

fn parse_args(args: &[OsString]) -> Result<Args, Failure> {
    let mut parsed = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.to_str() {
            Some("--goal") => goal_flag(&mut parsed.goal, rest.next(), USAGE)?,
            Some("--all") => parsed.all = true,
            Some("--json") => parsed.json = true,
            Some(_) | None => return Err(unknown_argument(arg, USAGE)),
        }
    }
    Ok(parsed)
}

fn receipt_of(
    store: &Path,
    kind: ReceiptKind,
    key: &Oid,
) -> Option<crate::pure::review::ReceiptView> {
    read_receipt(read_json(&receipt_path(store, kind, key)).as_deref())
}

fn work_of(
    git: &Git,
    common: &Path,
    base_ref: &BaseRef,
    branch: &str,
) -> Result<Option<Work>, Failure> {
    let store = store_dir(common);
    let Some(commit) = branch_tip(git, branch) else {
        return Ok(None);
    };
    let commits = commits_beyond(git, &commit, base_ref)?;
    let tip = tip_of(git, common, commit)?;
    let patch = tip
        .checked
        .as_ref()
        .and_then(|checked| checked.patch_id.clone());
    Ok(Some(Work {
        commits,
        checked: tip.checked.is_some(),
        informed: patch
            .as_ref()
            .and_then(|key| receipt_of(&store, ReceiptKind::InformedReview, key)),
        second: patch
            .as_ref()
            .and_then(|key| receipt_of(&store, ReceiptKind::SecondOpinion, key)),
        drive: receipt_of(&store, ReceiptKind::TestDrive, &tip.tree),
        patch_id: patch.map(|key| key.to_string()),
        tree: tip.tree.to_string(),
    }))
}

// The facts of every element of a goal, read and not judged.
pub(crate) fn collect(
    git: &Git,
    common: &Path,
    base_ref: &BaseRef,
    (goal, roadmap): (&GoalName, &Roadmap),
) -> Result<Vec<Facts>, Failure> {
    let store = store_dir(common);
    let logbook = read_logbook(&store, goal)?;
    let mut all = Vec::new();
    for element in &roadmap.elements {
        let name = element_name(element).clone();
        let entries: Vec<Logged> = logbook
            .iter()
            .filter(|logged| logged.slice == name)
            .cloned()
            .collect();
        let branch = slice_branch(goal, &name);
        let plan_file = build_plan_path(&store, goal, &name);
        let (kind, after, meets, plan, work) = match element {
            Element::Proof(proof) => (
                Kind::Proof {
                    how: proof.how.clone(),
                },
                proof.after.clone(),
                proof.meets.clone(),
                None,
                None,
            ),
            Element::Slice(slice) => {
                let plan = file_hash(git, &plan_file).ok().map(|key| Plan {
                    review: receipt_of(&store, ReceiptKind::PlanReview, &key),
                    key: key.to_string(),
                });
                (
                    Kind::Slice {
                        test_drive: slice.test_drive,
                    },
                    slice.after.clone(),
                    slice.meets.clone(),
                    plan,
                    work_of(git, common, base_ref, &branch)?,
                )
            }
        };
        all.push(Facts {
            goal: goal.clone(),
            name,
            kind,
            after,
            meets,
            branch,
            plan_path: plan_file.display().to_string(),
            plan,
            work,
            entries,
        });
    }
    Ok(all)
}

// Reviewed only by an approval: a roadmap has no slice to log a known issue for.
fn roadmap_reviewed(git: &Git, common: &Path, policy: &Setup, goal: &GoalName) -> bool {
    let store = store_dir(common);
    let Ok(file) = newest_roadmap(&store, goal) else {
        return false;
    };
    let Ok(key) = file_hash(git, &file) else {
        return false;
    };
    receipt_of(&store, ReceiptKind::PlanReview, &key).is_some_and(|receipt| {
        receipt.result == Outcome::Approve && counts(&receipt, policy, key.as_str(), &[])
    })
}

fn unknown_branches(git: &Git, known: &[(GoalName, Roadmap)]) -> Result<Vec<Unknown>, Failure> {
    let mut unknown = Vec::new();
    for branch in slice_branches(git)? {
        let Some((goal, slice)) = parse_slice_branch(&branch) else {
            continue;
        };
        match known.iter().find(|(name, _)| *name == goal) {
            None => unknown.push(Unknown {
                goal,
                branch,
                slice: None,
            }),
            Some((_, roadmap)) if element_named(roadmap, &slice).is_none() => {
                unknown.push(Unknown {
                    goal,
                    branch,
                    slice: Some(slice),
                });
            }
            Some(_) => {}
        }
    }
    Ok(unknown)
}

#[derive(Serialize)]
struct StartedJson {
    count: u64,
    cap: u64,
    over: bool,
}

#[derive(Serialize)]
struct RoadmapJson {
    reviewed: bool,
}

#[derive(Serialize)]
struct GoalJson<'a> {
    goal: &'a GoalName,
    roadmap: RoadmapJson,
    open: Counts,
    slices: &'a [crate::pure::state::View],
}

#[derive(Serialize)]
struct Json<'a> {
    open: Counts,
    started: StartedJson,
    unknown: &'a [Unknown],
    goals: Vec<GoalJson<'a>>,
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (head, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let args = parse_args(args)?;
    let (policy, base_ref) = load_policy(&git, head, err)?;
    let common = git.common_dir()?;
    let loaded = load_goals(&common)?;
    if let Some(goal) = &args.goal
        && !loaded.iter().any(|(name, _)| name == goal)
    {
        return Err(Failure::usage(format!(
            "no goal named \"{goal}\" in the store"
        )));
    }
    let mut goals = Vec::new();
    for (goal, roadmap) in &loaded {
        let facts = collect(&git, &common, &base_ref, (goal, roadmap))?;
        let shipped: Vec<_> = facts
            .iter()
            .filter(|fact| is_shipped(&fact.entries))
            .map(|fact| fact.name.clone())
            .collect();
        let context = Policy {
            setup: &policy,
            shipped: &shipped,
        };
        let views = facts.iter().map(|fact| view(fact, &context)).collect();
        goals.push(GoalView {
            goal: goal.clone(),
            reviewed: roadmap_reviewed(&git, &common, &policy, goal),
            views,
        });
    }
    let unknown = unknown_branches(&git, &loaded)?;
    let all_views: Vec<_> = goals.iter().flat_map(|goal| goal.views.iter()).collect();
    let started = started_count(&all_views, unknown.len());
    let cap = u64::from(&policy.agents.parallel.slices);
    let shown: Vec<&GoalView> = goals
        .iter()
        .filter(|goal| args.goal.as_ref().is_none_or(|wanted| *wanted == goal.goal))
        .collect();
    let shown_unknown: Vec<&Unknown> = unknown
        .iter()
        .filter(|item| args.goal.as_ref().is_none_or(|wanted| *wanted == item.goal))
        .collect();
    if args.json {
        let open = shown
            .iter()
            .fold(Counts::default(), |all, goal| all.sum(goal.open()));
        let json = Json {
            open,
            started: StartedJson {
                count: started,
                cap,
                over: started > cap,
            },
            unknown: &unknown,
            goals: shown
                .iter()
                .map(|goal| GoalJson {
                    goal: &goal.goal,
                    roadmap: RoadmapJson {
                        reviewed: goal.reviewed,
                    },
                    open: goal.open(),
                    slices: &goal.views,
                })
                .collect(),
        };
        let text = serde_json::to_string(&json)
            .map_err(|why| Failure::usage(format!("cannot print the status: {why}")))?;
        writeln!(out, "{text}").ok();
        return Ok(());
    }
    let limit = (!args.all).then(|| u64::from(&policy.owner.status_update_max_lines));
    for line in render(&shown, &shown_unknown, (started, cap), limit) {
        writeln!(out, "{line}").ok();
    }
    Ok(())
}
