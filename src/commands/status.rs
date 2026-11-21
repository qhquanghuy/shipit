use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::edge::records::{
    build_plan_path, coverage_record_path, kept_metrics, load_goals, metric_records,
    newest_roadmap, read_logbook, receipt_path, store_dir,
};
use crate::edge::slice_git::{
    branch_tip, commits_beyond, file_hash, load_policy, slice_branches, tip_of,
};
use crate::edge::store::{check_receipt_path, read_json};
use crate::pure::base::BaseRef;
use crate::pure::branch::{parse_slice_branch, slice_branch};
use crate::pure::coverage_map::parse_map;
use crate::pure::failure::Failure;
use crate::pure::finding::{Class, Outcome};
use crate::pure::flags::{goal_flag, once, unknown_argument};
use crate::pure::goal::{GoalName, QualifiedDoneWhen};
use crate::pure::goal_pick::{element_name, element_named, which_goal};
use crate::pure::logbook::{Entry, Logged, closed};
use crate::pure::metric::{history, outlier, read_record};
use crate::pure::receipt::Oid;
use crate::pure::review::{
    ReceiptKind, Stored, check_base, counts, open_findings, read_receipt, stored_findings,
};
use crate::pure::roadmap::{Element, Roadmap, SliceName};
use crate::pure::setup::{CheckCommand, HEAD, Setup, toml_table};
use crate::pure::state::{
    Counts, Facts, GoalView, Kind, Plan, Policy, Unknown, View, Work, is_shipped, render,
    started_count, view,
};
use crate::pure::summary::{
    Answered, CheckRow, DoneWhen, Known, MetricLine, Row, SummaryInput, Unfixed, summary_md, tick,
};
use crate::pure::tests_ran::map_path;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit status [--goal <name>] [--all] [--json] | shipit status --summary <slice> [--goal <name>]";
pub(crate) const SUMMARY: &str = "Shows each slice's state and next step, computed from receipts and the logbook, and writes nothing; with --summary it prints the pull-request summary of one slice; a stale branch of an abandoned slice counts as started until it is deleted.";
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
    summary: Option<String>,
}

fn parse_args(args: &[OsString]) -> Result<Args, Failure> {
    let mut parsed = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.to_str() {
            Some("--goal") => goal_flag(&mut parsed.goal, rest.next(), USAGE)?,
            Some("--all") => parsed.all = true,
            Some("--json") => parsed.json = true,
            Some("--summary") => once(&mut parsed.summary, "--summary", rest.next(), USAGE)?,
            Some(_) | None => return Err(unknown_argument(arg, USAGE)),
        }
    }
    if parsed.summary.is_some() && (parsed.all || parsed.json) {
        return Err(Failure::usage(format!(
            "--summary does not go with --all or --json; {USAGE}"
        )));
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
        commit: tip.commit.to_string(),
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

// Every goal of the store, judged; the cap counts across all of them.
pub(crate) fn judge_goals(
    git: &Git,
    common: &Path,
    (policy, base_ref): (&Setup, &BaseRef),
    loaded: &[(GoalName, Roadmap)],
) -> Result<Vec<GoalView>, Failure> {
    let mut goals = Vec::new();
    for (goal, roadmap) in loaded {
        let facts = collect(git, common, base_ref, (goal, roadmap))?;
        let shipped: Vec<_> = facts
            .iter()
            .filter(|fact| is_shipped(&fact.entries))
            .map(|fact| fact.name.clone())
            .collect();
        let context = Policy {
            setup: policy,
            shipped: &shipped,
        };
        let views = facts.iter().map(|fact| view(fact, &context)).collect();
        goals.push(GoalView {
            goal: goal.clone(),
            reviewed: roadmap_reviewed(git, common, policy, goal),
            views,
        });
    }
    Ok(goals)
}

pub(crate) fn unknown_branches(
    git: &Git,
    known: &[(GoalName, Roadmap)],
) -> Result<Vec<Unknown>, Failure> {
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

// Everything the pull-request summary is made from, read and not judged.
struct Reading<'a> {
    git: &'a Git,
    common: &'a Path,
    policy: &'a Setup,
    fact: &'a Facts,
}

fn short(text: &str) -> String {
    text.get(..12).unwrap_or(text).to_owned()
}

fn doc_of(store: &Path, kind: &str, key: &str) -> Option<Value> {
    let key = Oid::parse(key)?;
    let docs = read_json(&receipt_path(store, ReceiptKind::parse(kind)?, &key))?;
    <[Value; 1]>::try_from(docs).ok().map(|[doc]| doc)
}

fn check_row(reading: &Reading<'_>) -> Option<CheckRow> {
    let work = reading.fact.work.as_ref().filter(|work| work.checked)?;
    let tree = Oid::parse(&work.tree)?;
    let docs = read_json(&check_receipt_path(reading.common, &tree))?;
    let doc = docs.first()?;
    let size = doc.get("size")?;
    Some(CheckRow {
        tree: short(&work.tree),
        files: size.get("files")?.as_u64()?,
        cap: size.get("cap")?.as_u64()?,
        base: check_base(doc)?.name,
    })
}

// An item is confirmed by the coverage record of this version, when the command that wrote it is
// one of the setup's own check lines.
fn done_when(reading: &Reading<'_>, goal: &GoalName) -> DoneWhen {
    let fact = reading.fact;
    let committed = fact.work.as_ref().and_then(|work| {
        let bytes = reading
            .git
            .show_file(OsStr::new(&work.commit), &map_path(goal, &fact.name))?;
        parse_map(&toml_table(&bytes).ok()?).ok()
    });
    let Some(map) = committed else {
        return DoneWhen::NoMap;
    };
    let record = fact
        .work
        .as_ref()
        .and_then(|work| Oid::parse(&work.tree))
        .and_then(|tree| read_json(&coverage_record_path(&store_dir(reading.common), &tree)))
        .and_then(|docs| docs.into_iter().next());
    let checks: Vec<&str> = reading
        .policy
        .gate
        .check
        .iter()
        .map(CheckCommand::as_str)
        .collect();
    let mine = format!("{goal}/{}", fact.name);
    let items = map
        .items
        .iter()
        .map(|(id, names)| {
            let recorded = record
                .as_ref()
                .and_then(|doc| doc.get("maps")?.get(&mine)?.get(id.to_string()))
                .and_then(Value::as_array)
                .map_or(names.len(), Vec::len);
            let check = record.as_ref().and_then(|doc| doc.get("check")?.as_str());
            (
                QualifiedDoneWhen::of(goal, *id),
                tick(check, &checks, recorded),
            )
        })
        .collect();
    DoneWhen::Items(items)
}

fn current_key(fact: &Facts, kind: &str) -> Option<String> {
    match kind {
        "plan-review" => fact.plan.as_ref().map(|plan| plan.key.clone()),
        "informed-review" | "second-opinion" => {
            let work = fact.work.as_ref()?;
            work.patch_id.clone()
        }
        "test-drive" => fact.work.as_ref().map(|work| work.tree.clone()),
        _ => None,
    }
}

fn counts_of(findings: &[Stored]) -> Counts {
    let mut counts = Counts::default();
    for finding in findings {
        counts.add(finding.class);
    }
    counts
}

fn rows_of(reading: &Reading<'_>) -> Vec<Row> {
    let store = store_dir(reading.common);
    let mut rows = Vec::new();
    for logged in &reading.fact.entries {
        let Entry::Receipt {
            kind,
            key,
            result,
            author,
            by,
            rank,
            ..
        } = &logged.entry
        else {
            continue;
        };
        let doc = doc_of(&store, kind, key);
        let flag = |name: &str| {
            let doc = doc.as_ref()?;
            doc.get(name)?.as_bool()
        };
        let not_the_role = if flag("author_is_maker") == Some(false) {
            Some("maker")
        } else if flag("author_is_planner") == Some(false) {
            Some("planner")
        } else {
            None
        };
        rows.push(Row {
            kind: kind.clone(),
            key: short(key),
            current: current_key(reading.fact, kind).as_deref() == Some(key.as_str()),
            by: by.clone(),
            author: author.clone(),
            not_the_role,
            rank: rank.clone(),
            result: result.clone(),
            counts: doc
                .as_ref()
                .and_then(stored_findings)
                .map_or_else(Counts::default, |findings| counts_of(&findings)),
        });
    }
    rows
}

fn known_issues(reading: &Reading<'_>) -> (Vec<Known>, Vec<Answered>) {
    let store = store_dir(reading.common);
    let (mut known, mut answers) = (Vec::new(), Vec::new());
    for logged in &reading.fact.entries {
        match &logged.entry {
            Entry::KnownIssue {
                kind,
                key,
                finding,
                reason,
                by,
            } => {
                let found = doc_of(&store, kind, key)
                    .as_ref()
                    .and_then(stored_findings)
                    .and_then(|all| all.into_iter().find(|stored| stored.id == *finding));
                known.push(Known {
                    kind: kind.clone(),
                    finding: finding.clone(),
                    class: found
                        .as_ref()
                        .map_or(Class::NiceToFix, |stored| stored.class),
                    by: by.clone(),
                    text: found
                        .map_or_else(|| "(receipt not found)".to_owned(), |stored| stored.text),
                    reason: reason.clone(),
                });
            }
            Entry::Answer {
                kind,
                finding,
                text,
                by,
                ..
            } => answers.push(Answered {
                kind: kind.clone(),
                finding: finding.clone(),
                by: by.clone(),
                text: text.clone(),
            }),
            Entry::Receipt { .. }
            | Entry::Note { .. }
            | Entry::ShippedMerge { .. }
            | Entry::ShippedProof { .. }
            | Entry::Started { .. } => {}
        }
    }
    (known, answers)
}

// Open nice-to-fix findings of the current version that no known issue closed.
fn unfixed(fact: &Facts) -> Vec<Unfixed> {
    let work = fact.work.as_ref();
    let slots = [
        (
            "plan-review",
            fact.plan.as_ref().and_then(|plan| plan.review.as_ref()),
            fact.plan.as_ref().map(|plan| plan.key.as_str()),
        ),
        (
            "informed-review",
            work.and_then(|work| work.informed.as_ref()),
            work.and_then(|work| work.patch_id.as_deref()),
        ),
        (
            "second-opinion",
            work.and_then(|work| work.second.as_ref()),
            work.and_then(|work| work.patch_id.as_deref()),
        ),
        (
            "test-drive",
            work.and_then(|work| work.drive.as_ref()),
            work.map(|work| work.tree.as_str()),
        ),
    ];
    let mut found = Vec::new();
    for (kind, receipt, key) in slots {
        let (Some(receipt), Some(key)) = (receipt, key) else {
            continue;
        };
        let closed_ids = closed(&fact.entries, &fact.name, kind, key);
        for finding in open_findings(receipt, &closed_ids) {
            if finding.class == Class::NiceToFix {
                found.push(Unfixed {
                    kind: kind.to_owned(),
                    finding: finding.id.clone(),
                    text: finding.text.clone(),
                });
            }
        }
    }
    found
}

fn metrics_of(reading: &Reading<'_>, goal: &GoalName) -> Vec<MetricLine> {
    let Some(tree) = reading.fact.work.as_ref().map(|work| work.tree.clone()) else {
        return Vec::new();
    };
    let store = store_dir(reading.common);
    let records: Vec<_> = metric_records(&store)
        .iter()
        .filter_map(read_record)
        .collect();
    let (kept, _skipped) = kept_metrics(&store);
    let mut lines: Vec<MetricLine> = records
        .iter()
        .filter(|record| {
            record.key == tree && record.goal == *goal && record.slice == reading.fact.name
        })
        .map(|record| MetricLine {
            name: record.name.clone(),
            value: record.value,
            fence: outlier(
                record.value,
                &history(
                    &records,
                    &kept,
                    &record.name,
                    Some((goal, &reading.fact.name)),
                ),
            ),
        })
        .collect();
    lines.sort_by(|a, b| a.name.cmp(&b.name));
    lines
}

fn stand_ins_of(fact: &Facts) -> Vec<String> {
    let mut found: Vec<String> = fact
        .entries
        .iter()
        .filter_map(|logged| match &logged.entry {
            Entry::Receipt { author, by, .. } => Some([author.clone(), by.clone()]),
            Entry::Note { .. }
            | Entry::KnownIssue { .. }
            | Entry::Answer { .. }
            | Entry::ShippedMerge { .. }
            | Entry::ShippedProof { .. }
            | Entry::Started { .. } => None,
        })
        .flatten()
        .filter(|worker| worker.contains('@'))
        .collect();
    found.sort();
    found.dedup();
    found
}

fn summary_for(
    (git, common, policy): (&Git, &Path, &Setup),
    (goal, fact, judged): (&GoalName, &Facts, &View),
) -> String {
    let reading = Reading {
        git,
        common,
        policy,
        fact,
    };
    let (known, answers) = known_issues(&reading);
    let state = match &judged.merge_note {
        Some(note) => format!("{} (merge not matched: {note})", judged.state),
        None => judged.state.to_string(),
    };
    summary_md(&SummaryInput {
        name: fact.name.clone(),
        state,
        map_path: map_path(goal, &fact.name),
        done_when: done_when(&reading, goal),
        check: check_row(&reading),
        rows: rows_of(&reading),
        open: judged.open,
        stand_ins: stand_ins_of(fact),
        metrics: metrics_of(&reading, goal),
        known,
        unfixed: unfixed(fact),
        answers,
    })
}

fn summary_command(
    (git, common, policy, base_ref): (&Git, &Path, &Setup, &BaseRef),
    loaded: &[(GoalName, Roadmap)],
    (typed, flag_goal): (&str, Option<&GoalName>),
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let names: Vec<GoalName> = loaded.iter().map(|(goal, _)| goal.clone()).collect();
    let goal = which_goal(flag_goal, git.current_branch().as_deref(), &names)?;
    let (_, roadmap) = loaded
        .iter()
        .find(|(name, _)| *name == goal)
        .ok_or_else(|| Failure::usage(format!("no goal named \"{goal}\" in the store")))?;
    let unknown = || {
        Failure::usage(format!(
            "no slice named \"{typed}\" in the roadmap of goal \"{goal}\""
        ))
    };
    let slice = SliceName::parse(typed).ok_or_else(unknown)?;
    match element_named(roadmap, &slice).ok_or_else(unknown)? {
        Element::Slice(_) => {}
        Element::Proof(_) => {
            return Err(Failure::usage(format!(
                "\"{typed}\" is a proof step: it has no work to summarise"
            )));
        }
    }
    let facts = collect(git, common, base_ref, (&goal, roadmap))?;
    let shipped: Vec<SliceName> = facts
        .iter()
        .filter(|fact| is_shipped(&fact.entries))
        .map(|fact| fact.name.clone())
        .collect();
    let fact = facts
        .iter()
        .find(|fact| fact.name == slice)
        .ok_or_else(unknown)?;
    let judged = view(
        fact,
        &Policy {
            setup: policy,
            shipped: &shipped,
        },
    );
    write!(
        out,
        "{}",
        summary_for((git, common, policy), (&goal, fact, &judged))
    )
    .ok();
    Ok(())
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
    slices: &'a [View],
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
    if let Some(typed) = &args.summary {
        return summary_command(
            (&git, &common, &policy, &base_ref),
            &loaded,
            (typed, args.goal.as_ref()),
            out,
        );
    }
    let goals = judge_goals(&git, &common, (&policy, &base_ref), &loaded)?;
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
