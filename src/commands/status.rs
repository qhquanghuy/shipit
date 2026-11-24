use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::edge::records::{
    build_plan_path, coverage_record_path, goal_dir, kept_metrics, load_goals, metric_records,
    newest_roadmap, read_store_text, receipt_doc, receipt_path, store_dir,
};
use crate::edge::slice_git::{
    Checked, Tip, branch_tip, changed_with_status, commits_beyond, file_hash, load_policy,
    map_paths_at, slice_branches, tip_of,
};
use crate::edge::store::{check_receipt_path, read_history, read_json, versions};
use crate::pure::base::BaseRef;
use crate::pure::branch::{parse_slice_branch, slice_branch};
use crate::pure::coverage_map::parse_map;
use crate::pure::failure::Failure;
use crate::pure::finding::{Class, Outcome};
use crate::pure::flags::{goal_flag, once, unknown_argument};
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::{element_name, element_named, which_goal};
use crate::pure::logbook::{Entry, Event, Logged, closed};
use crate::pure::metric::{history, outlier, read_record};
use crate::pure::receipt::Oid;
use crate::pure::review::{
    ReceiptKind, Stored, check_base, counts, open_findings, read_receipt, stored_findings,
};
use crate::pure::risk::{DiffFacts, Hunk, hunk, touched_tests};
use crate::pure::roadmap::{Element, Roadmap, SliceName};
use crate::pure::setup::{HEAD, Setup, toml_table};
use crate::pure::state::{
    Counts, Facts, GoalView, Kind, Plan, Policy, Unknown, View, Work, is_shipped, proven, render,
    started_count, view,
};
use crate::pure::store_changes::{flag_line, flags, item_changes, span, span_line, started_at};
use crate::pure::store_path::OwnerFile;
use crate::pure::summary::{
    Answered, CheckRow, Known, MetricLine, Row, SummaryInput, Unfixed, summary_md,
};
use crate::pure::tests_ran::{map_path, recorded};

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

fn check_doc(common: &Path, tree: &Oid) -> Option<Value> {
    read_json(&check_receipt_path(common, tree))?
        .into_iter()
        .next()
}

// The base branch a check receipt names, with a remote part dropped: origin/main is main.
fn checked_branch(doc: &Value) -> Option<String> {
    let full = doc.get("base_ref")?.as_str()?;
    if let Some(name) = full.strip_prefix("refs/heads/") {
        return Some(name.to_owned());
    }
    let (_remote, name) = full.strip_prefix("refs/remotes/")?.split_once('/')?;
    Some(name.to_owned())
}

fn policy_branch(base_ref: &BaseRef) -> String {
    match base_ref {
        BaseRef::Local(name) | BaseRef::Origin(name) => name.to_string(),
    }
}

// The mapped tests at the fork that the change edits or removes, and the maps that could not
// be read. Nothing here ends status: what cannot be read counts against the slice.
fn mapped_touched(
    git: &Git,
    fork: &Oid,
    tip: &Oid,
    changed: &[(char, String)],
) -> (Vec<String>, Vec<String>) {
    let edited: Vec<&str> = changed
        .iter()
        .filter(|(status, _)| *status != 'A')
        .map(|(_, path)| path.as_str())
        .collect();
    if edited.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let at = OsStr::new(fork.as_str());
    let mut mapped = BTreeSet::new();
    let mut unreadable = Vec::new();
    match map_paths_at(git, at) {
        Err(_why) => unreadable.push(".shipit/coverage/".to_owned()),
        Ok(paths) => {
            for path in paths {
                let map = git
                    .show_file(at, &path)
                    .and_then(|bytes| toml_table(&bytes).ok())
                    .and_then(|table| parse_map(&table).ok());
                match map {
                    Some(map) => mapped.extend(map.items.into_values().flatten()),
                    None => unreadable.push(path),
                }
            }
        }
    }
    let mut touched = BTreeSet::new();
    for path in edited {
        let Some(bytes) = git.show_file(at, path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let held = touched_tests(&text, &[], &mapped);
        if held.is_empty() {
            continue;
        }
        let literal = format!(":(literal){path}");
        let diff = git.output(&[
            OsStr::new("diff"),
            OsStr::new("-U0"),
            OsStr::new("--text"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--no-color"),
            OsStr::new("--no-textconv"),
            OsStr::new("--no-renames"),
            OsStr::new("--end-of-options"),
            at,
            OsStr::new(tip.as_str()),
            OsStr::new("--"),
            OsStr::new(&literal),
        ]);
        match diff {
            Ok(bytes) => {
                let hunks: Vec<Hunk> = String::from_utf8_lossy(&bytes)
                    .lines()
                    .filter_map(hunk)
                    .collect();
                touched.extend(touched_tests(&text, &hunks, &mapped));
            }
            Err(_why) => touched.extend(held),
        }
    }
    (touched.into_iter().collect(), unreadable)
}

fn diff_facts(
    git: &Git,
    common: &Path,
    base_ref: &BaseRef,
    (tip, checked): (&Tip, &Checked),
) -> Result<DiffFacts, Failure> {
    let changed = changed_with_status(git, &checked.merge_base, OsStr::new(tip.commit.as_str()))?;
    let doc = check_doc(common, &tip.tree);
    let base = doc.as_ref().and_then(check_base);
    let files = doc
        .as_ref()
        .and_then(|doc| doc.get("size")?.get("files")?.as_u64())
        .unwrap_or_else(|| u64::try_from(changed.len()).unwrap_or(u64::MAX));
    let given = doc.as_ref().and_then(|doc| doc.get("base_given")) == Some(&Value::Bool(true));
    let policy = policy_branch(base_ref);
    let other_base = match (doc.as_ref().and_then(checked_branch), &base) {
        (Some(branch), Some(base)) if branch != policy => {
            Some((base.name.clone(), base_ref.to_string()))
        }
        (Some(_) | None, _) => None,
    };
    let (mapped, unreadable) = mapped_touched(git, &checked.merge_base, &tip.commit, &changed);
    Ok(DiffFacts {
        names: changed.into_iter().map(|(_, path)| path).collect(),
        files,
        mapped,
        unreadable,
        given_base: base.filter(|_| given).map(|base| base.name),
        other_base,
    })
}

fn work_of(
    git: &Git,
    common: &Path,
    base_ref: &BaseRef,
    branch: &str,
    (goal, slice): (&GoalName, &SliceName),
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
    let tested = read_json(&coverage_record_path(&store, &tip.tree))
        .and_then(|docs| docs.into_iter().next())
        .and_then(|doc| recorded(&doc, goal, slice));
    let checked = match &tip.checked {
        Some(checked) => Some(diff_facts(git, common, base_ref, (&tip, checked))?),
        None => None,
    };
    Ok(Some(Work {
        commits,
        checked,
        informed: patch
            .as_ref()
            .and_then(|key| receipt_of(&store, ReceiptKind::InformedReview, key)),
        second: patch
            .as_ref()
            .and_then(|key| receipt_of(&store, ReceiptKind::SecondOpinion, key)),
        drive: receipt_of(&store, ReceiptKind::TestDrive, &tip.tree),
        tested,
        patch_id: patch.map(|key| key.to_string()),
        tree: tip.tree.to_string(),
    }))
}

const OWNER_FILES: [OwnerFile; 2] = [OwnerFile::Goal, OwnerFile::Roadmap];

fn goal_text_of(store: &Path, goal: &GoalName, version: u64) -> Result<String, String> {
    let parts = [
        "goals".to_owned(),
        goal.to_string(),
        OwnerFile::Goal.name(version),
    ];
    read_store_text(store, &parts).map_err(|_why| format!("goal v{version} cannot be read"))
}

// What the goal's and roadmap's versions in the store say against the logbook: the lines of
// each version that is not recorded or was changed by hand.
fn flags_of(
    git: &Git,
    (store, goal): (&Path, &GoalName),
    history: &[Event],
    held: &[(OwnerFile, Vec<u64>)],
) -> Vec<String> {
    held.iter()
        .flat_map(|(kind, versions)| {
            let in_store: Vec<(u64, Option<Oid>)> = versions
                .iter()
                .map(|version| {
                    let path = goal_dir(store, goal).join(kind.name(*version));
                    (*version, file_hash(git, &path).ok())
                })
                .collect();
            flags(history, *kind, &in_store)
                .into_iter()
                .map(|(version, flag)| flag_line(*kind, version, flag))
                .collect::<Vec<_>>()
        })
        .collect()
}

// What changed in the goal and the roadmap since the slice started; nothing when no start is
// logged.
fn changes_of(
    (store, goal): (&Path, &GoalName),
    history: &[Event],
    held: &[(OwnerFile, Vec<u64>)],
    slice: &SliceName,
) -> Vec<String> {
    let Some(start) = started_at(history, slice) else {
        return Vec::new();
    };
    held.iter()
        .filter_map(|(kind, versions)| {
            let (from, to) = span(history, start, *kind, versions)?;
            let items = match (kind, from) {
                (OwnerFile::Goal, Some(from)) => Some(
                    goal_text_of(store, goal, from)
                        .and_then(|old| item_changes(&old, &goal_text_of(store, goal, to)?)),
                ),
                (OwnerFile::Goal, None) | (OwnerFile::Roadmap, _) => None,
            };
            Some(span_line(*kind, (from, to), items))
        })
        .collect()
}

// The facts of every element of a goal, read and not judged.
pub(crate) fn collect(
    git: &Git,
    common: &Path,
    base_ref: &BaseRef,
    (goal, roadmap): (&GoalName, &Roadmap),
) -> Result<(Vec<Facts>, Vec<String>), Failure> {
    let store = store_dir(common);
    let history = read_history(&store, goal)?;
    let logbook: Vec<Logged> = history
        .iter()
        .filter_map(|event| match event {
            Event::Logged(logged) => Some(logged.clone()),
            Event::Stored(_) => None,
        })
        .collect();
    let held: Vec<(OwnerFile, Vec<u64>)> = OWNER_FILES
        .iter()
        .map(|kind| (*kind, versions(&store, goal, *kind)))
        .collect();
    let at = (store.as_path(), goal);
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
        let (kind, after, meets, plan, work, owns) = match element {
            Element::Proof(proof) => (
                Kind::Proof {
                    how: proof.how.clone(),
                },
                proof.after.clone(),
                proof.meets.clone(),
                None,
                None,
                Vec::new(),
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
                    // A shipped slice needs no work: status reads no git for it.
                    if is_shipped(&entries) {
                        None
                    } else {
                        work_of(git, common, base_ref, &branch, (goal, &name))?
                    },
                    slice.owns.clone(),
                )
            }
        };
        let changes = changes_of(at, &history, &held, &name);
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
            changes,
            owns,
        });
    }
    Ok((all, flags_of(git, at, &history, &held)))
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
        let (facts, flags) = collect(git, common, base_ref, (goal, roadmap))?;
        let shipped: Vec<_> = facts
            .iter()
            .filter(|fact| is_shipped(&fact.entries))
            .map(|fact| fact.name.clone())
            .collect();
        let proven = proven(&facts);
        let context = Policy {
            setup: policy,
            shipped: &shipped,
            proven: &proven,
        };
        let views = facts.iter().map(|fact| view(fact, &context)).collect();
        goals.push(GoalView {
            goal: goal.clone(),
            reviewed: roadmap_reviewed(git, common, policy, goal),
            flags,
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
    common: &'a Path,
    fact: &'a Facts,
}

fn short(text: &str) -> String {
    text.get(..12).unwrap_or(text).to_owned()
}

fn check_row(reading: &Reading<'_>) -> Option<CheckRow> {
    let work = reading
        .fact
        .work
        .as_ref()
        .filter(|work| work.checked.is_some())?;
    let tree = Oid::parse(&work.tree)?;
    let doc = check_doc(reading.common, &tree)?;
    let size = doc.get("size")?;
    Some(CheckRow {
        tree: short(&work.tree),
        files: size.get("files")?.as_u64()?,
        cap: size.get("cap")?.as_u64()?,
        base: check_base(&doc)?.name,
    })
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
        let doc = receipt_doc(&store, kind, key);
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
                let found = receipt_doc(&store, kind, key)
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
    common: &Path,
    (goal, fact, judged, flags): (&GoalName, &Facts, &View, &[String]),
) -> String {
    let reading = Reading { common, fact };
    let (known, answers) = known_issues(&reading);
    let state = match &judged.merge_note {
        Some(note) => format!("{} (merge not matched: {note})", judged.state),
        None => judged.state.to_string(),
    };
    summary_md(&SummaryInput {
        name: fact.name.clone(),
        state,
        map_path: map_path(goal, &fact.name),
        done_when: judged.items.clone(),
        check: check_row(&reading),
        rows: rows_of(&reading),
        open: judged.open,
        stand_ins: stand_ins_of(fact),
        metrics: metrics_of(&reading, goal),
        known,
        unfixed: unfixed(fact),
        answers,
        changes: fact.changes.iter().chain(flags).cloned().collect(),
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
    let (facts, flags) = collect(git, common, base_ref, (&goal, roadmap))?;
    let shipped: Vec<SliceName> = facts
        .iter()
        .filter(|fact| is_shipped(&fact.entries))
        .map(|fact| fact.name.clone())
        .collect();
    let fact = facts
        .iter()
        .find(|fact| fact.name == slice)
        .ok_or_else(unknown)?;
    let proven = proven(&facts);
    let judged = view(
        fact,
        &Policy {
            setup: policy,
            shipped: &shipped,
            proven: &proven,
        },
    );
    write!(
        out,
        "{}",
        summary_for(common, (&goal, fact, &judged, &flags))
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
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    version_flags: &'a [String],
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
                    version_flags: &goal.flags,
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
