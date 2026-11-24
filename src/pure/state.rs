use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

use serde::{Serialize, Serializer};

use crate::pure::finding::{Class, Outcome};
use crate::pure::goal::{GoalName, QualifiedDoneWhen, parse_done_when};
use crate::pure::logbook::{Entry, Logged, closed};
use crate::pure::review::{ReceiptView, counts, open_findings};
use crate::pure::roadmap::SliceName;
use crate::pure::setup::Setup;
use crate::pure::tests_ran::{Tested, is_coverage_line};
use crate::pure::text::{comma_list, plain};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum State {
    Planned,
    BuildPlanReady,
    Made,
    Checked,
    ReadyToShip,
    Shipped,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Planned => "planned",
            Self::BuildPlanReady => "build plan ready",
            Self::Made => "made",
            Self::Checked => "checked",
            Self::ReadyToShip => "ready to ship",
            Self::Shipped => "shipped",
        })
    }
}

impl Serialize for State {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct Counts {
    pub(crate) must_fix: u64,
    pub(crate) must_answer: u64,
    pub(crate) nice_to_fix: u64,
}

impl Counts {
    pub(crate) fn add(&mut self, class: Class) {
        match class {
            Class::MustFix => self.must_fix += 1,
            Class::MustAnswer => self.must_answer += 1,
            Class::NiceToFix => self.nice_to_fix += 1,
        }
    }

    pub(crate) const fn sum(self, other: Self) -> Self {
        Self {
            must_fix: self.must_fix + other.must_fix,
            must_answer: self.must_answer + other.must_answer,
            nice_to_fix: self.nice_to_fix + other.nice_to_fix,
        }
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.must_fix == 0 && self.must_answer == 0 && self.nice_to_fix == 0
    }

    // Only the classes that are not zero, as "must fix 1, nice to fix 2".
    fn listed(self) -> String {
        let parts = [
            ("must fix", self.must_fix),
            ("must answer", self.must_answer),
            ("nice to fix", self.nice_to_fix),
        ];
        comma_list(
            parts
                .iter()
                .filter(|(_, n)| *n > 0)
                .map(|(label, n)| format!("{label} {n}")),
        )
    }
}

#[derive(Debug)]
pub(crate) enum Kind {
    Slice { test_drive: bool },
    Proof { how: String },
}

#[derive(Debug)]
pub(crate) struct Plan {
    pub(crate) key: String,
    pub(crate) review: Option<ReceiptView>,
}

// What is known about the branch of a slice: its tip, and the receipts for that version.
#[derive(Debug)]
pub(crate) struct Work {
    pub(crate) commits: u64,
    pub(crate) checked: bool,
    pub(crate) patch_id: Option<String>,
    pub(crate) tree: String,
    pub(crate) informed: Option<ReceiptView>,
    pub(crate) second: Option<ReceiptView>,
    pub(crate) drive: Option<ReceiptView>,
    pub(crate) tested: Option<Tested>,
}

#[derive(Debug)]
pub(crate) struct Facts {
    pub(crate) goal: GoalName,
    pub(crate) name: SliceName,
    pub(crate) kind: Kind,
    pub(crate) after: Vec<SliceName>,
    pub(crate) meets: Vec<QualifiedDoneWhen>,
    pub(crate) branch: String,
    pub(crate) plan_path: String,
    pub(crate) plan: Option<Plan>,
    pub(crate) work: Option<Work>,
    pub(crate) entries: Vec<Logged>,
}

#[derive(Debug, Serialize)]
pub(crate) struct View {
    pub(crate) goal: GoalName,
    pub(crate) name: SliceName,
    pub(crate) state: State,
    pub(crate) next: String,
    pub(crate) open: Counts,
    pub(crate) started: bool,
    pub(crate) without_start: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) merge_note: Option<String>,
    // What the receipts of this version claim that the setup does not bear out.
    pub(crate) claims: Vec<String>,
    pub(crate) branch: Option<String>,
    pub(crate) test_drive: bool,
    pub(crate) meets: Vec<QualifiedDoneWhen>,
    #[serde(skip)]
    pub(crate) items: Vec<(QualifiedDoneWhen, Confirmation)>,
    pub(crate) unconfirmed: Vec<QualifiedDoneWhen>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Confirmation {
    Tests(usize),
    ProofStep,
    TestDrive,
    KnownIssue,
    RecordNotCounted,
    Missing,
}

impl Confirmation {
    pub(crate) const fn confirms(self) -> bool {
        match self {
            Self::Tests(_) | Self::ProofStep | Self::TestDrive | Self::KnownIssue => true,
            Self::RecordNotCounted | Self::Missing => false,
        }
    }
}

// A proof step is not tied to a version of the work, so it counts only once all it follows has shipped.
pub(crate) fn proven(facts: &[Facts]) -> Vec<QualifiedDoneWhen> {
    let shipped: Vec<&SliceName> = facts
        .iter()
        .filter(|fact| is_shipped(&fact.entries))
        .map(|fact| &fact.name)
        .collect();
    facts
        .iter()
        .filter(|fact| {
            matches!(fact.kind, Kind::Proof { .. })
                && shipped.contains(&&fact.name)
                && fact.after.iter().all(|name| shipped.contains(&name))
        })
        .flat_map(|fact| fact.meets.iter().cloned())
        .collect()
}

// A whole word, bare or qualified: DW21 and other:DW2 do not name DW2, and a bare id is of the
// slice's own goal.
fn names(text: &str, item: &QualifiedDoneWhen, own: &GoalName) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == ':' || c == '-'))
        .map(|word| word.trim_matches(|c| c == ':' || c == '-'))
        .any(|word| {
            (item.goal == *own && parse_done_when(word) == Some(item.id))
                || QualifiedDoneWhen::parse(word).as_ref() == Some(item)
        })
}

// The reviewer's finding names the item, not the lead's reason; the tie of a known issue to a
// version is the one `closed` uses.
fn known_issue_names(facts: &Facts, item: &QualifiedDoneWhen) -> bool {
    let Some(work) = facts.work.as_ref() else {
        return false;
    };
    facts.entries.iter().any(|logged| match &logged.entry {
        Entry::KnownIssue {
            kind, key, finding, ..
        } => {
            let receipt = match kind.as_str() {
                "informed-review" if Some(key.as_str()) == work.patch_id.as_deref() => {
                    work.informed.as_ref()
                }
                "second-opinion" if Some(key.as_str()) == work.patch_id.as_deref() => {
                    work.second.as_ref()
                }
                "test-drive" if *key == work.tree => work.drive.as_ref(),
                _ => None,
            };
            logged.slice == facts.name
                && receipt
                    .and_then(|receipt| {
                        receipt.findings.iter().find(|stored| stored.id == *finding)
                    })
                    .is_some_and(|stored| names(&stored.text, item, &facts.goal))
        }
        Entry::Receipt { .. }
        | Entry::Note { .. }
        | Entry::Answer { .. }
        | Entry::ShippedMerge { .. }
        | Entry::ShippedProof { .. }
        | Entry::Started { .. } => false,
    })
}

fn confirmation(
    item: &QualifiedDoneWhen,
    facts: &Facts,
    policy: &Policy<'_>,
    drive_passed: bool,
) -> Confirmation {
    let recorded = facts.work.as_ref().and_then(|work| {
        let tested = work.tested.as_ref().filter(|_| item.goal == facts.goal)?;
        let tests = *tested.items.get(&item.id)?;
        let counts = tested.key == work.tree
            && is_coverage_line(&tested.check)
            && policy
                .setup
                .gate
                .check
                .iter()
                .any(|line| line.as_str() == tested.check);
        Some((tests, counts))
    });
    if let Some((tests, true)) = recorded {
        Confirmation::Tests(tests)
    } else if policy.proven.contains(item) {
        Confirmation::ProofStep
    } else if drive_passed {
        Confirmation::TestDrive
    } else if known_issue_names(facts, item) {
        Confirmation::KnownIssue
    } else if recorded.is_some() {
        Confirmation::RecordNotCounted
    } else {
        Confirmation::Missing
    }
}

// Why an owner's merge was accepted without a patch id match, when that is how it shipped.
pub(crate) fn merge_note(entries: &[Logged]) -> Option<String> {
    entries.iter().find_map(|logged| match &logged.entry {
        Entry::ShippedMerge {
            matched: false,
            note,
            ..
        } => Some(note.clone().unwrap_or_default()),
        Entry::ShippedMerge { matched: true, .. }
        | Entry::Receipt { .. }
        | Entry::Note { .. }
        | Entry::KnownIssue { .. }
        | Entry::Answer { .. }
        | Entry::ShippedProof { .. }
        | Entry::Started { .. } => None,
    })
}

pub(crate) fn is_shipped(entries: &[Logged]) -> bool {
    entries.iter().any(|logged| match &logged.entry {
        Entry::ShippedMerge { .. } | Entry::ShippedProof { .. } => true,
        Entry::Receipt { .. }
        | Entry::Note { .. }
        | Entry::KnownIssue { .. }
        | Entry::Answer { .. }
        | Entry::Started { .. } => false,
    })
}

// The trees whose reviews or test drive did not pass: each is one round of fixes.
fn rounds_used(entries: &[Logged]) -> u64 {
    let trees: BTreeSet<&str> = entries
        .iter()
        .filter_map(|logged| match &logged.entry {
            Entry::Receipt {
                kind,
                tree: Some(tree),
                result,
                ..
            } if kind != "plan-review" && result != "approve" && result != "pass" => {
                Some(tree.as_str())
            }
            Entry::Receipt { .. }
            | Entry::Note { .. }
            | Entry::KnownIssue { .. }
            | Entry::Answer { .. }
            | Entry::ShippedMerge { .. }
            | Entry::ShippedProof { .. }
            | Entry::Started { .. } => None,
        })
        .collect();
    u64::try_from(trees.len()).unwrap_or(u64::MAX)
}

// One receipt of the current version: does it count, and which of its findings are still open.
struct Slot<'a> {
    kind: &'static str,
    label: &'static str,
    receipt: Option<&'a ReceiptView>,
    key: &'a str,
}

struct Judged {
    counts: bool,
    open_blocking: Vec<String>,
    rework: bool,
    tally: Counts,
    claims: Vec<String>,
}

fn judge(slot: &Slot<'_>, facts: &Facts, policy: &Setup) -> Judged {
    let mut judged = Judged {
        counts: false,
        open_blocking: Vec::new(),
        rework: false,
        tally: Counts::default(),
        claims: Vec::new(),
    };
    let Some(receipt) = slot.receipt else {
        return judged;
    };
    let closed_ids = closed(&facts.entries, &facts.name, slot.kind, slot.key);
    for finding in open_findings(receipt, &closed_ids) {
        judged.tally.add(finding.class);
        if finding.class != Class::NiceToFix {
            judged
                .open_blocking
                .push(format!("{} {}", slot.kind, finding.id));
        }
    }
    judged.rework = receipt.is_rework();
    judged.counts = counts(receipt, policy, slot.key, &closed_ids);
    if let Some(role) = receipt.not_the_role {
        judged
            .claims
            .push(format!("{} author is not the setup's {role}", slot.kind));
    }
    if receipt.fresh {
        judged.claims.push(format!("{} used --fresh", slot.kind));
    }
    judged
}

fn wait_for(facts: &Facts, shipped: &[SliceName]) -> Option<String> {
    let waiting: Vec<&SliceName> = facts
        .after
        .iter()
        .filter(|name| !shipped.contains(name))
        .collect();
    (!waiting.is_empty()).then(|| format!("wait for {} to ship", comma_list(waiting)))
}

fn plan_step(facts: &Facts, plan_open: &[String]) -> String {
    if facts.plan.is_none() {
        return format!("planner: write the build plan at {}", facts.plan_path);
    }
    let open = if plan_open.is_empty() {
        String::new()
    } else {
        format!(" (open: {})", plan_open.join(", "))
    };
    format!(
        "reviewer above the planner: review the build plan, then shipit receipt plan-review {} \
         --goal {}{open}",
        facts.name, facts.goal
    )
}

pub(crate) struct Policy<'a> {
    pub(crate) setup: &'a Setup,
    pub(crate) shipped: &'a [SliceName],
    pub(crate) proven: &'a [QualifiedDoneWhen],
}

fn chain(facts: &Facts, shipped: bool, plan_ok: bool, work_ok: bool) -> State {
    let made = facts.work.as_ref().is_some_and(|work| work.commits >= 1);
    let checked = facts.work.as_ref().is_some_and(|work| work.checked);
    if shipped {
        State::Shipped
    } else if !plan_ok || matches!(facts.kind, Kind::Proof { .. }) {
        State::Planned
    } else if !made {
        State::BuildPlanReady
    } else if !checked {
        State::Made
    } else if !work_ok {
        State::Checked
    } else {
        State::ReadyToShip
    }
}

fn rolled_up(all: &[&Judged]) -> (Counts, Vec<String>) {
    let open = all
        .iter()
        .fold(Counts::default(), |sum, judged| sum.sum(judged.tally));
    let claims = all
        .iter()
        .flat_map(|judged| judged.claims.iter().cloned())
        .collect();
    (open, claims)
}

// The items of a slice with how each is confirmed; a drive receipt passes only with a `pass` result,
// and only a slice marked for a test drive has one. A shipped slice keeps no items.
fn confirmed_items(
    facts: &Facts,
    policy: &Policy<'_>,
    shipped: bool,
    drive_counts: bool,
) -> (
    Vec<(QualifiedDoneWhen, Confirmation)>,
    Vec<QualifiedDoneWhen>,
) {
    let drive_passed = drive_counts
        && facts
            .work
            .as_ref()
            .and_then(|work| work.drive.as_ref())
            .is_some_and(|receipt| receipt.result == Outcome::Pass);
    let items: Vec<(QualifiedDoneWhen, Confirmation)> = match facts.kind {
        Kind::Slice { .. } if !shipped => facts
            .meets
            .iter()
            .map(|item| {
                (
                    item.clone(),
                    confirmation(item, facts, policy, drive_passed),
                )
            })
            .collect(),
        Kind::Slice { .. } | Kind::Proof { .. } => Vec::new(),
    };
    let unconfirmed = items
        .iter()
        .filter(|(_, confirmation)| !confirmation.confirms())
        .map(|(item, _)| item.clone())
        .collect();
    (items, unconfirmed)
}

// The chain of conditions, each reached only with all before it; `shipped` overrides.
pub(crate) fn view(facts: &Facts, policy: &Policy<'_>) -> View {
    let shipped = is_shipped(&facts.entries);
    let test_drive = matches!(facts.kind, Kind::Slice { test_drive: true });
    let work = facts.work.as_ref();
    let none = String::new();
    let plan_key = facts.plan.as_ref().map_or(&none, |plan| &plan.key);
    let patch = work.and_then(|work| work.patch_id.as_deref()).unwrap_or("");
    let tree = work.map_or("", |work| work.tree.as_str());
    let plan_slot = Slot {
        kind: "plan-review",
        label: "build plan review",
        receipt: facts.plan.as_ref().and_then(|plan| plan.review.as_ref()),
        key: plan_key,
    };
    let informed_slot = Slot {
        kind: "informed-review",
        label: "informed review",
        receipt: work.and_then(|work| work.informed.as_ref()),
        key: patch,
    };
    let second_slot = Slot {
        kind: "second-opinion",
        label: "second opinion",
        receipt: work.and_then(|work| work.second.as_ref()),
        key: patch,
    };
    let drive_slot = Slot {
        kind: "test-drive",
        label: "test drive",
        receipt: work.and_then(|work| work.drive.as_ref()),
        key: tree,
    };
    let judge_it = |slot| judge(slot, facts, policy.setup);
    let plan = judge_it(&plan_slot);
    let informed = judge_it(&informed_slot);
    let second = judge_it(&second_slot);
    let drive = judge_it(&drive_slot);
    let (open, claims) = rolled_up(&[&plan, &informed, &second, &drive]);
    let (items, unconfirmed) = confirmed_items(facts, policy, shipped, test_drive && drive.counts);
    let work_ok =
        informed.counts && second.counts && (!test_drive || drive.counts) && unconfirmed.is_empty();
    let state = chain(facts, shipped, plan.counts, work_ok);
    let started = !shipped && matches!(facts.kind, Kind::Slice { .. }) && work.is_some();
    let without_start = started
        && !facts
            .entries
            .iter()
            .any(|logged| matches!(logged.entry, Entry::Started { .. }));
    let next = match (&facts.kind, state) {
        (_, State::Shipped) => "nothing".to_owned(),
        (Kind::Proof { how }, _) => wait_for(facts, policy.shipped).unwrap_or_else(|| {
            format!(
                "owner: {how}, then shipit log {} shipped --note <evidence> --goal {}",
                facts.name, facts.goal
            )
        }),
        (Kind::Slice { .. }, State::Planned) => plan_step(facts, &plan.open_blocking),
        (Kind::Slice { .. }, State::BuildPlanReady) if work.is_some() => {
            format!("maker: make the slice on {}", facts.branch)
        }
        (Kind::Slice { .. }, State::BuildPlanReady) => wait_for(facts, policy.shipped)
            .unwrap_or_else(|| {
                format!(
                    "lead: start it with shipit start {} --goal {}",
                    facts.name, facts.goal
                )
            }),
        (Kind::Slice { .. }, State::Made) => format!("run shipit check on {}", facts.branch),
        (Kind::Slice { .. }, State::Checked) => checked_step(
            facts,
            policy.setup,
            [
                (&informed, &informed_slot),
                (&second, &second_slot),
                (&drive, &drive_slot),
            ],
            test_drive,
            &unconfirmed,
        ),
        (Kind::Slice { .. }, State::ReadyToShip) => format!(
            "owner: ship it, then shipit log {} shipped --merge <merge commit> --goal {}",
            facts.name, facts.goal
        ),
    };
    View {
        goal: facts.goal.clone(),
        name: facts.name.clone(),
        state,
        next,
        open,
        started,
        without_start,
        merge_note: merge_note(&facts.entries),
        claims,
        branch: work.map(|_| facts.branch.clone()),
        test_drive,
        meets: facts.meets.clone(),
        items,
        unconfirmed,
    }
}

fn checked_step(
    facts: &Facts,
    policy: &Setup,
    works: [(&Judged, &Slot<'_>); 3],
    test_drive: bool,
    unconfirmed: &[QualifiedDoneWhen],
) -> String {
    let limit = policy.agents.max_fix_rounds;
    let rounds = rounds_used(&facts.entries);
    let mut blocking: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for (index, (judged, slot)) in works.iter().enumerate() {
        if judged.counts || (index == 2 && !test_drive) {
            continue;
        }
        if !judged.open_blocking.is_empty() {
            blocking.extend(judged.open_blocking.iter().cloned());
        } else if judged.rework {
            blocking.push(slot.kind.to_owned());
        } else {
            missing.push(slot.label.to_owned());
        }
    }
    if blocking.is_empty() {
        let [.., (drive, _)] = &works;
        if !test_drive || drive.counts {
            missing.extend(unconfirmed.iter().map(|item| format!("proof of {item}")));
        }
        return format!("missing: {}", comma_list(missing));
    }
    if rounds > limit {
        return format!(
            "lead: fix-round limit ({limit}) reached: decide, then record it with shipit log {} \
             note --text <decision> --goal {}",
            facts.name, facts.goal
        );
    }
    format!(
        "maker: fix round {rounds} of {limit}: fix {} or log them as known issues",
        comma_list(blocking)
    )
}

pub(crate) struct StartFacts<'a> {
    pub(crate) view: &'a View,
    pub(crate) waiting: &'a [&'a SliceName],
    // One goal name for each slice started and not shipped, in every goal.
    pub(crate) started_goals: &'a [GoalName],
    pub(crate) cap: u64,
}

// The first rule that stops `shipit start`, in this order.
pub(crate) fn start_refusal(facts: &StartFacts<'_>) -> Option<String> {
    let view = facts.view;
    if view.state == State::Shipped {
        return Some(format!("{} is shipped", view.name));
    }
    if view.state == State::Planned {
        return Some(format!(
            "{} is {}: its build plan needs a counting review first",
            view.name, view.state
        ));
    }
    if !facts.waiting.is_empty() {
        return Some(format!(
            "wait for {} to ship",
            comma_list(facts.waiting.iter().copied())
        ));
    }
    if view.started {
        let branch = view.branch.as_deref().unwrap_or_default();
        return Some(format!("{branch} already exists: the slice is started"));
    }
    let started = u64::try_from(facts.started_goals.len()).unwrap_or(u64::MAX);
    if started >= facts.cap {
        let mut goals = facts.started_goals.to_vec();
        goals.sort();
        goals.dedup();
        return Some(format!(
            "{started} slices are started and not shipped, in {} ([agents.parallel] slices = \
             {}): ship one first",
            comma_list(goals),
            facts.cap
        ));
    }
    None
}

pub(crate) fn started_count(views: &[&View], unknown: usize) -> u64 {
    let started = views.iter().filter(|view| view.started).count() + unknown;
    u64::try_from(started).unwrap_or(u64::MAX)
}

pub(crate) fn header(open: Counts, started: u64, cap: u64) -> String {
    let over = if started > cap { ", over the cap" } else { "" };
    format!(
        "open findings: must fix {}, must answer {}, nice to fix {} · started: {started} of {cap}{over}",
        open.must_fix, open.must_answer, open.nice_to_fix
    )
}

#[derive(Debug, Serialize)]
pub(crate) struct Unknown {
    pub(crate) goal: GoalName,
    pub(crate) branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) slice: Option<SliceName>,
}

impl Unknown {
    fn line(&self) -> String {
        match &self.slice {
            None => format!(
                "unknown goal {}: {}. Next: put its roadmap in the store, or delete the branch",
                self.goal, self.branch
            ),
            Some(slice) => format!(
                "unknown slice {slice} in goal {}: {}. Next: add it to the roadmap, or delete the \
                 branch",
                self.goal, self.branch
            ),
        }
    }
}

#[derive(Debug)]
pub(crate) struct GoalView {
    pub(crate) goal: GoalName,
    pub(crate) reviewed: bool,
    pub(crate) views: Vec<View>,
}

impl GoalView {
    pub(crate) fn open(&self) -> Counts {
        self.views
            .iter()
            .fold(Counts::default(), |all, view| all.sum(view.open))
    }
}

fn goal_lines(goal: &GoalView, lines: &mut Vec<String>) {
    let name = &goal.goal;
    lines.push(if goal.reviewed {
        format!("goal {name}: roadmap reviewed")
    } else {
        format!(
            "goal {name}: roadmap needs a review (shipit receipt plan-review roadmap --goal {name})"
        )
    });
    for view in goal
        .views
        .iter()
        .filter(|view| view.state != State::Shipped)
    {
        let mut notes = String::new();
        if !view.open.is_empty() {
            write!(notes, " Open: {}.", view.open.listed()).ok();
        }
        let not_proven: Vec<&QualifiedDoneWhen> = view
            .items
            .iter()
            .filter(|(_, confirmation)| *confirmation == Confirmation::KnownIssue)
            .map(|(item, _)| item)
            .collect();
        if !not_proven.is_empty() {
            write!(notes, " Not proven: {}.", plain(&comma_list(not_proven))).ok();
        }
        if !view.claims.is_empty() {
            write!(notes, " Claims: {}.", plain(&view.claims.join("; "))).ok();
        }
        if view.without_start {
            notes.push_str(" Started without shipit start.");
        }
        let dot = if notes.is_empty() { "" } else { "." };
        lines.push(format!(
            "{name}/{}: {}. Next: {}{dot}{notes}",
            view.name, view.state, view.next
        ));
    }
    let done: Vec<String> = goal
        .views
        .iter()
        .filter(|view| view.state == State::Shipped)
        .map(|view| match &view.merge_note {
            Some(note) => format!("{} (merge not matched: {})", view.name, plain(note)),
            None => view.name.to_string(),
        })
        .collect();
    if !done.is_empty() {
        lines.push(format!("shipped ({name}): {}", comma_list(done)));
    }
    if goal.views.iter().all(|view| view.state == State::Shipped) {
        lines.push(format!(
            "goal {name} is shipped. Next: owner: shipit complete {name} (a dry run), then shipit \
             complete {name} --confirm"
        ));
    }
}

// The lines of `shipit status`; `limit` is `owner.status_update_max_lines`, None for `--all`.
pub(crate) fn render(
    shown: &[&GoalView],
    unknown: &[&Unknown],
    (started, cap): (u64, u64),
    limit: Option<u64>,
) -> Vec<String> {
    let open = shown
        .iter()
        .fold(Counts::default(), |all, goal| all.sum(goal.open()));
    let mut lines = vec![header(open, started, cap)];
    for goal in shown {
        goal_lines(goal, &mut lines);
    }
    lines.extend(unknown.iter().map(|item| item.line()));
    let Some(limit) = limit else {
        return lines;
    };
    let total = u64::try_from(lines.len()).unwrap_or(u64::MAX);
    if total <= limit {
        return lines;
    }
    let keep = usize::try_from(limit.saturating_sub(1)).unwrap_or(0);
    let hidden = total - u64::try_from(keep).unwrap_or(0);
    lines.truncate(keep);
    lines.push(format!("… {hidden} more: shipit status --all"));
    lines
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use std::collections::BTreeMap;

    use toml::Value;

    use super::*;
    use crate::pure::rank::parse_worker;
    use crate::pure::review::{ReceiptKind, Stored};
    use crate::pure::testing::setup_with;

    fn name(text: &str) -> SliceName {
        SliceName::parse(text).expect("a slice name")
    }

    fn id(text: &str) -> QualifiedDoneWhen {
        QualifiedDoneWhen::parse(text).expect("a done-when id")
    }

    fn goal() -> GoalName {
        GoalName::parse("demo").expect("a goal name")
    }

    fn finding(id: &str, class: Class) -> Stored {
        Stored {
            id: id.to_owned(),
            is_question: class == Class::MustAnswer,
            class,
            text: "text".to_owned(),
        }
    }

    // A receipt that counts for `key`: a fable review of sonnet work (above every role).
    fn counting(key: &str) -> ReceiptView {
        ReceiptView {
            kind: ReceiptKind::InformedReview,
            key: key.to_owned(),
            result: Outcome::Approve,
            author: parse_worker("sonnet:high").expect("a worker"),
            by: parse_worker("fable:high").expect("a worker"),
            fresh: false,
            not_the_role: None,
            findings: Vec::new(),
        }
    }

    fn rework(key: &str, findings: Vec<Stored>) -> ReceiptView {
        ReceiptView {
            result: Outcome::Rework,
            findings,
            ..counting(key)
        }
    }

    #[derive(Debug, Clone, Copy)]
    #[expect(
        clippy::struct_excessive_bools,
        reason = "a table of independent switches"
    )]
    struct Given {
        plan_file: bool,
        plan_ok: bool,
        made: bool,
        checked: bool,
        informed: bool,
        second: bool,
        drive: bool,
        test_drive: bool,
        shipped: bool,
    }

    fn facts(given: &Given) -> Facts {
        let work = given.made.then(|| Work {
            commits: 1,
            checked: given.checked,
            patch_id: Some("patch".to_owned()),
            tree: "tree".to_owned(),
            informed: given.informed.then(|| counting("patch")),
            second: given.second.then(|| counting("patch")),
            drive: given.drive.then(|| counting("tree")),
            tested: None,
        });
        let entries = if given.shipped {
            vec![Logged {
                slice: name("beta"),
                entry: Entry::ShippedMerge {
                    commit: "c".to_owned(),
                    matched: true,
                    note: None,
                },
            }]
        } else {
            Vec::new()
        };
        Facts {
            goal: goal(),
            name: name("beta"),
            kind: Kind::Slice {
                test_drive: given.test_drive,
            },
            after: Vec::new(),
            meets: Vec::new(),
            branch: "slice/demo/beta".to_owned(),
            plan_path: "/store/goals/demo/slices/beta/build-plan.md".to_owned(),
            plan: given.plan_file.then(|| Plan {
                key: "plan".to_owned(),
                review: given.plan_ok.then(|| counting("plan")),
            }),
            work,
            entries,
        }
    }

    fn judged_with(facts: &Facts, shipped: &[SliceName], proven: &[QualifiedDoneWhen]) -> View {
        let setup = setup_with(&[]);
        view(
            facts,
            &Policy {
                setup: &setup,
                shipped,
                proven,
            },
        )
    }

    fn judged(facts: &Facts, shipped: &[SliceName]) -> View {
        judged_with(facts, shipped, &[])
    }

    fn given() -> impl Strategy<Value = Given> {
        (any::<[bool; 8]>(), any::<bool>()).prop_map(|(flags, shipped)| {
            let [
                plan_file,
                plan_ok,
                made,
                checked,
                informed,
                second,
                drive,
                test_drive,
            ] = flags;
            Given {
                plan_file,
                plan_ok: plan_ok && plan_file,
                made,
                checked: checked && made,
                informed: informed && made,
                second: second && made,
                drive: drive && made,
                test_drive,
                shipped,
            }
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn the_state_is_the_longest_chain_that_holds(
            given in given(),
            confirmed in proptest::collection::vec(any::<bool>(), 0..4),
        ) {
            let item = |n: usize| id(&format!("demo:DW{}", n + 1));
            let mut facts = facts(&given);
            facts.meets = (0..confirmed.len()).map(item).collect();
            let proven: Vec<QualifiedDoneWhen> = confirmed
                .iter()
                .enumerate()
                .filter(|(_, flag)| **flag)
                .map(|(n, _)| item(n))
                .collect();
            let unconfirmed: Vec<QualifiedDoneWhen> = confirmed
                .iter()
                .enumerate()
                .filter(|(_, flag)| !**flag)
                .map(|(n, _)| item(n))
                .collect();
            let ready = given.informed && given.second && (!given.test_drive || given.drive);
            let expected = if given.shipped {
                State::Shipped
            } else if !given.plan_ok {
                State::Planned
            } else if !given.made {
                State::BuildPlanReady
            } else if !given.checked {
                State::Made
            } else if !ready || !unconfirmed.is_empty() {
                State::Checked
            } else {
                State::ReadyToShip
            };
            let seen = judged_with(&facts, &[], &proven);
            prop_assert_eq!(seen.state, expected);
            let listed = if given.shipped { Vec::new() } else { unconfirmed.clone() };
            prop_assert_eq!(&seen.unconfirmed, &listed);
            prop_assert_eq!(seen.items.is_empty(), given.shipped || confirmed.is_empty());
            if expected == State::Checked && ready {
                let proofs = unconfirmed.iter().map(|item| format!("proof of {item}"));
                prop_assert_eq!(&seen.next, &format!("missing: {}", comma_list(proofs)));
            }
            let waits_for_drive = given.plan_ok && given.made && given.checked && given.informed
                && given.second && given.test_drive && !given.drive && !given.shipped;
            if waits_for_drive {
                prop_assert_eq!(seen.next.as_str(), "missing: test drive");
            }
        }

        #[test]
        fn an_item_is_confirmed_by_the_first_source_that_holds(
            record in 0..5_usize,
            tests in 1..5_usize,
            proof in any::<bool>(),
            drive in any::<bool>(),
            known in 0..7_usize,
            text_names in any::<bool>(),
            other_goal in any::<bool>(),
        ) {
            let [coverage, flake] = ["shipit coverage -- cargo test", "nix flake check --print-build-logs"];
            let setup = setup_with(&[(
                "gate.check",
                Value::Array(vec![Value::String(coverage.to_owned()), Value::String(flake.to_owned())]),
            )]);
            let item = id(if other_goal { "other:DW1" } else { "demo:DW1" });
            let mut facts = facts(&Given {
                plan_file: true, plan_ok: true, made: true, checked: true,
                informed: true, second: true, drive: true, test_drive: true, shipped: false,
            });
            let (check, key) = match record {
                1 => (Some(coverage), "tree"),
                2 => (Some(flake), "tree"),
                3 => (Some("shipit coverage -- other"), "tree"),
                4 => (Some(coverage), "old"),
                _ => (None, "tree"),
            };
            if let Some(work) = facts.work.as_mut() {
                work.tested = check.map(|check| Tested {
                    key: key.to_owned(),
                    check: check.to_owned(),
                    items: BTreeMap::from([(item.id, tests)]),
                });
                let said = if text_names { format!("waits: {item}") } else { "waits for later".to_owned() };
                for receipt in [work.informed.as_mut(), work.second.as_mut(), work.drive.as_mut()].into_iter().flatten() {
                    receipt.findings = vec![Stored { text: said.clone(), ..finding("F1", Class::NiceToFix) }];
                }
            }
            let issue = |kind: &str, key: &str| Logged {
                slice: name("beta"),
                entry: Entry::KnownIssue {
                    kind: kind.to_owned(),
                    key: key.to_owned(),
                    finding: "F1".to_owned(),
                    reason: format!("waits: {item}"),
                    by: "opus:high".to_owned(),
                },
            };
            match known {
                1 => facts.entries.push(issue("informed-review", "patch")),
                2 => facts.entries.push(issue("plan-review", "plan")),
                3 => facts.entries.push(issue("informed-review", "old-patch")),
                4 => facts.entries.push(issue("second-opinion", "patch")),
                5 => facts.entries.push(issue("test-drive", "tree")),
                6 => facts.entries.push(issue("test-drive", "old-tree")),
                _ => {}
            }
            let proven = if proof { vec![item.clone()] } else { Vec::new() };
            let policy = Policy { setup: &setup, shipped: &[], proven: &proven };
            let counted = record == 1 && !other_goal;
            let expected = if counted {
                Confirmation::Tests(tests)
            } else if proof {
                Confirmation::ProofStep
            } else if drive {
                Confirmation::TestDrive
            } else if matches!(known, 1 | 4 | 5) && text_names {
                Confirmation::KnownIssue
            } else if record >= 2 && !other_goal {
                Confirmation::RecordNotCounted
            } else {
                Confirmation::Missing
            };
            prop_assert_eq!(confirmation(&item, &facts, &policy, drive), expected);
        }

        #[test]
        fn a_known_issue_names_an_item_only_as_a_whole_word(
            before in "[a-z ]{0,6}",
            after in "[a-z ]{0,6}",
            n in 1..40_u64,
            extra in proptest::option::of(0..10_u64),
            prefix in 0..3_usize,
            tail in prop::sample::select(vec!["", ":", ".", "-"]),
            own in any::<bool>(),
        ) {
            let lead = ["", "demo:", "other:"].get(prefix).copied().expect("a prefix");
            let extra_text = extra.map(|digit| digit.to_string()).unwrap_or_default();
            let text = format!("{before} {lead}DW{n}{extra_text}{tail} {after}");
            let theirs = if own { goal() } else { GoalName::parse("mine").expect("a goal name") };
            let wanted = extra.is_none() && match prefix {
                0 => own,
                1 => true,
                _ => false,
            };
            prop_assert_eq!(names(&text, &id(&format!("demo:DW{n}")), &theirs), wanted);
        }

        #[test]
        fn a_test_drive_confirms_only_a_passing_drive_on_a_marked_slice(
            pass in any::<bool>(),
            marked in any::<bool>(),
        ) {
            let mut facts = facts(&Given {
                plan_file: true, plan_ok: true, made: true, checked: true,
                informed: true, second: true, drive: true, test_drive: marked, shipped: false,
            });
            facts.meets = vec![id("demo:DW1")];
            if let Some(work) = facts.work.as_mut() {
                work.drive = Some(ReceiptView {
                    result: if pass { Outcome::Pass } else { Outcome::Findings },
                    ..counting("tree")
                });
            }
            let confirmed = judged(&facts, &[])
                .items
                .iter()
                .any(|(_, how)| *how == Confirmation::TestDrive);
            prop_assert_eq!(confirmed, pass && marked);
        }

        #[test]
        fn a_shipped_proof_step_confirms_the_items_it_meets(
            slices in any::<[bool; 2]>(),
            steps in proptest::collection::vec((any::<bool>(), 0..4_usize, 1..9_u64), 0..4),
        ) {
            let [alpha_shipped, beta_shipped] = slices;
            let bare = |shipped| Given {
                plan_file: false, plan_ok: false, made: false, checked: false,
                informed: false, second: false, drive: false, test_drive: false, shipped,
            };
            let element = |text: &str, kind: Kind, after: &[&str], meets: Vec<QualifiedDoneWhen>, shipped: bool| {
                let mut fact = facts(&bare(shipped));
                fact.name = name(text);
                fact.kind = kind;
                fact.after = after.iter().map(|each| name(each)).collect();
                fact.meets = meets;
                fact
            };
            let mut all = vec![
                element("alpha", Kind::Slice { test_drive: false }, &[], Vec::new(), alpha_shipped),
                element("beta", Kind::Slice { test_drive: false }, &[], Vec::new(), beta_shipped),
            ];
            let mut expected = Vec::new();
            for (index, (shipped, follows, number)) in steps.iter().enumerate() {
                let after: &[&str] = match follows {
                    0 => &[],
                    1 => &["alpha"],
                    2 => &["beta"],
                    _ => &["alpha", "beta"],
                };
                let meets = vec![id(&format!("demo:DW{number}"))];
                let all_shipped = after.iter().all(|each| {
                    if *each == "alpha" { alpha_shipped } else { beta_shipped }
                });
                if *shipped && all_shipped {
                    expected.extend(meets.clone());
                }
                all.push(element(
                    &format!("step{index}"),
                    Kind::Proof { how: "by hand".to_owned() },
                    after,
                    meets,
                    *shipped,
                ));
            }
            prop_assert_eq!(proven(&all), expected);
        }

        #[test]
        fn open_findings_are_counted_by_class(
            classes in proptest::collection::vec(0..3_usize, 0..8),
            closed_mask in proptest::collection::vec(any::<bool>(), 8),
        ) {
            let all = [Class::MustFix, Class::MustAnswer, Class::NiceToFix];
            let findings: Vec<Stored> = classes
                .iter()
                .enumerate()
                .map(|(index, class)| finding(&format!("F{}", index + 1), all.get(*class).copied().expect("a class")))
                .collect();
            let mut facts = facts(&Given {
                plan_file: true, plan_ok: true, made: true, checked: true,
                informed: true, second: true, drive: true, test_drive: false, shipped: false,
            });
            let mut review = counting("patch");
            review.findings = findings;
            let ids: Vec<String> = review.findings.iter().map(|f| f.id.clone()).collect();
            if let Some(work) = facts.work.as_mut() {
                work.informed = Some(review);
            }
            for (id, closed) in ids.iter().zip(&closed_mask) {
                if *closed {
                    facts.entries.push(Logged {
                        slice: name("beta"),
                        entry: Entry::KnownIssue {
                            kind: "informed-review".to_owned(),
                            key: "patch".to_owned(),
                            finding: id.clone(),
                            reason: "later".to_owned(),
                            by: "opus:high".to_owned(),
                        },
                    });
                }
            }
            let mut expected = Counts::default();
            for ((class, id), closed) in classes.iter().zip(&ids).zip(&closed_mask) {
                if !*closed {
                    expected.add(all.get(*class).copied().expect("a class"));
                }
                let _ = id;
            }
            let seen = judged(&facts, &[]);
            prop_assert_eq!(seen.open, expected);
            let blocking_open = expected.must_fix + expected.must_answer;
            let want = if blocking_open == 0 { State::ReadyToShip } else { State::Checked };
            prop_assert_eq!(seen.state, want);
        }

        #[test]
        fn over_the_cap_iff_started_exceeds_the_cap(started in 0..9_u64, cap in 1..9_u64) {
            prop_assert_eq!(
                header(Counts::default(), started, cap).ends_with(", over the cap"),
                started > cap
            );
        }

        #[test]
        fn started_counts_every_goal_and_status_of_one_goal_keeps_the_total(
            first in proptest::collection::vec(any::<bool>(), 0..4),
            second in proptest::collection::vec(any::<bool>(), 0..4),
            unknown in 0..3_usize,
        ) {
            let make = |flags: &[bool]| -> Vec<View> {
                flags
                    .iter()
                    .map(|started| View {
                        goal: goal(),
                        name: name("beta"),
                        state: State::Planned,
                        next: String::new(),
                        open: Counts::default(),
                        started: *started,
                        without_start: false,
                        merge_note: None,
                        claims: Vec::new(),
                        branch: None,
                        test_drive: false,
                        meets: Vec::new(),
                        items: Vec::new(),
                        unconfirmed: Vec::new(),
                    })
                    .collect()
            };
            let (a, b) = (make(&first), make(&second));
            let every: Vec<&View> = a.iter().chain(&b).collect();
            let total = started_count(&every, unknown);
            let one: Vec<&View> = a.iter().collect();
            prop_assert_eq!(
                total,
                started_count(&one, 0) + started_count(&b.iter().collect::<Vec<_>>(), unknown)
            );
            let shown = GoalView { goal: goal(), reviewed: true, views: a };
            let lines = render(&[&shown], &[], (total, 3), None);
            let wanted = format!("started: {total} of 3");
            prop_assert!(lines.first().is_some_and(|line| line.contains(&wanted)));
        }

        #[test]
        fn a_start_is_refused_in_the_order_of_the_rules(
            state in 0..6_usize,
            waiting in any::<bool>(),
            started in any::<bool>(),
            count in 0..6_usize,
            cap in 1..5_u64,
        ) {
            let all = [
                State::Planned,
                State::BuildPlanReady,
                State::Made,
                State::Checked,
                State::ReadyToShip,
                State::Shipped,
            ];
            let state = all.get(state).copied().expect("a state");
            let view = View {
                goal: goal(),
                name: name("beta"),
                state,
                next: String::new(),
                open: Counts::default(),
                started,
                without_start: false,
                merge_note: None,
                claims: Vec::new(),
                branch: started.then(|| "slice/demo/beta".to_owned()),
                test_drive: false,
                meets: Vec::new(),
                items: Vec::new(),
                unconfirmed: Vec::new(),
            };
            let before = [name("alpha")];
            let waits: Vec<&SliceName> = if waiting { before.iter().collect() } else { Vec::new() };
            let goals: Vec<GoalName> = (0..count).map(|_| goal()).collect();
            let said = start_refusal(&StartFacts {
                view: &view,
                waiting: &waits,
                started_goals: &goals,
                cap,
            });
            let wanted = if state == State::Shipped {
                Some("beta is shipped".to_owned())
            } else if state == State::Planned {
                Some("beta is planned: its build plan needs a counting review first".to_owned())
            } else if waiting {
                Some("wait for alpha to ship".to_owned())
            } else if started {
                Some("slice/demo/beta already exists: the slice is started".to_owned())
            } else if u64::try_from(count).unwrap_or(0) >= cap {
                Some(format!("{count} slices are started and not shipped, in demo ([agents.parallel] slices = {cap}): ship one first"))
            } else {
                None
            };
            prop_assert_eq!(said, wanted);
        }

        #[test]
        fn the_output_stays_within_the_limit(count in 0..12_usize, limit in 1..8_u64) {
            let views: Vec<View> = (0..count)
                .map(|_| judged(&facts(&Given {
                    plan_file: false, plan_ok: false, made: false, checked: false,
                    informed: false, second: false, drive: false, test_drive: false, shipped: false,
                }), &[]))
                .collect();
            let shown = GoalView { goal: goal(), reviewed: false, views };
            let full = render(&[&shown], &[], (0, 3), None);
            let cut = render(&[&shown], &[], (0, 3), Some(limit));
            let wanted = usize::try_from(limit).unwrap_or(usize::MAX);
            if full.len() <= wanted {
                prop_assert_eq!(&cut, &full);
            } else {
                prop_assert_eq!(cut.len(), wanted);
                let keep = wanted - 1;
                prop_assert_eq!(cut.get(..keep), full.get(..keep));
                let hidden = full.len() - keep;
                prop_assert_eq!(cut.last(), Some(&format!("… {hidden} more: shipit status --all")));
            }
        }
    }

    #[test]
    fn shipped_overrides_the_chain() {
        let seen = judged(
            &facts(&Given {
                plan_file: false,
                plan_ok: false,
                made: false,
                checked: false,
                informed: false,
                second: false,
                drive: false,
                test_drive: true,
                shipped: true,
            }),
            &[],
        );
        assert_eq!(
            (seen.state, seen.next.as_str(), seen.started),
            (State::Shipped, "nothing", false)
        );
    }

    #[test]
    fn the_next_step_texts() {
        let none = Given {
            plan_file: false,
            plan_ok: false,
            made: false,
            checked: false,
            informed: false,
            second: false,
            drive: false,
            test_drive: false,
            shipped: false,
        };
        let next = |given: &Given, shipped: &[SliceName]| judged(&facts(given), shipped).next;
        assert_eq!(
            next(&none, &[]),
            "planner: write the build plan at /store/goals/demo/slices/beta/build-plan.md"
        );
        let filed = Given {
            plan_file: true,
            ..none
        };
        assert_eq!(
            next(&filed, &[]),
            "reviewer above the planner: review the build plan, then shipit receipt plan-review beta --goal demo"
        );
        let ready = Given {
            plan_ok: true,
            ..filed
        };
        assert_eq!(
            next(&ready, &[]),
            "lead: start it with shipit start beta --goal demo"
        );
        let mut waiting = facts(&ready);
        waiting.after = vec![name("alpha")];
        assert_eq!(judged(&waiting, &[]).next, "wait for alpha to ship");
        assert_eq!(
            judged(&waiting, &[name("alpha")]).next,
            "lead: start it with shipit start beta --goal demo"
        );
        let made = Given {
            made: true,
            ..ready
        };
        assert_eq!(next(&made, &[]), "run shipit check on slice/demo/beta");
        let checked = Given {
            checked: true,
            ..made
        };
        assert_eq!(
            next(&checked, &[]),
            "missing: informed review, second opinion"
        );
        let both = Given {
            informed: true,
            second: true,
            test_drive: true,
            ..checked
        };
        assert_eq!(next(&both, &[]), "missing: test drive");
        let done = Given {
            drive: true,
            ..both
        };
        assert_eq!(
            next(&done, &[]),
            "owner: ship it, then shipit log beta shipped --merge <merge commit> --goal demo"
        );
    }

    #[test]
    fn a_fix_round_and_the_limit() {
        let mut blocked = facts(&Given {
            plan_file: true,
            plan_ok: true,
            made: true,
            checked: true,
            informed: false,
            second: true,
            drive: false,
            test_drive: false,
            shipped: false,
        });
        if let Some(work) = blocked.work.as_mut() {
            work.informed = Some(rework(
                "patch",
                vec![
                    finding("F1", Class::MustFix),
                    finding("F2", Class::MustAnswer),
                ],
            ));
        }
        let receipt = |tree: &str| Logged {
            slice: name("beta"),
            entry: Entry::Receipt {
                kind: "informed-review".to_owned(),
                key: "patch".to_owned(),
                tree: Some(tree.to_owned()),
                commit: None,
                result: "rework".to_owned(),
                findings: 2,
                author: "sonnet:high".to_owned(),
                by: "opus:high".to_owned(),
                rank: "tier".to_owned(),
            },
        };
        blocked.entries.push(receipt("t1"));
        assert_eq!(
            judged(&blocked, &[]).next,
            "maker: fix round 1 of 2: fix informed-review F1, informed-review F2 or log them as known issues"
        );
        blocked.entries.push(receipt("t2"));
        assert_eq!(
            judged(&blocked, &[]).next,
            "maker: fix round 2 of 2: fix informed-review F1, informed-review F2 or log them as known issues"
        );
        blocked.entries.push(receipt("t3"));
        assert_eq!(
            judged(&blocked, &[]).next,
            "lead: fix-round limit (2) reached: decide, then record it with shipit log beta note --text <decision> --goal demo"
        );
    }

    #[test]
    fn a_claimed_author_or_a_fresh_pass_shows_in_the_line() {
        let mut claimed = facts(&Given {
            plan_file: true,
            plan_ok: true,
            made: true,
            checked: true,
            informed: true,
            second: true,
            drive: false,
            test_drive: false,
            shipped: false,
        });
        if let Some(work) = claimed.work.as_mut() {
            if let Some(review) = work.informed.as_mut() {
                review.not_the_role = Some("maker");
            }
            if let Some(review) = work.second.as_mut() {
                review.fresh = true;
            }
        }
        let seen = judged(&claimed, &[]);
        assert_eq!(
            seen.claims,
            [
                "informed-review author is not the setup's maker",
                "second-opinion used --fresh",
            ]
        );
        let shown = GoalView {
            goal: goal(),
            reviewed: true,
            views: vec![seen],
        };
        let lines = render(&[&shown], &[], (1, 3), None);
        assert!(lines.iter().any(|line| line.ends_with(
            "Claims: informed-review author is not the setup's maker; second-opinion used --fresh. Started without shipit start."
        )), "{lines:?}");
    }

    #[test]
    fn the_lines_of_a_goal() {
        let mut view = judged(
            &facts(&Given {
                plan_file: true,
                plan_ok: true,
                made: true,
                checked: false,
                informed: false,
                second: false,
                drive: false,
                test_drive: false,
                shipped: false,
            }),
            &[],
        );
        view.open = Counts {
            must_fix: 1,
            must_answer: 0,
            nice_to_fix: 2,
        };
        view.without_start = true;
        let shown = GoalView {
            goal: goal(),
            reviewed: false,
            views: vec![view],
        };
        assert_eq!(
            render(&[&shown], &[], (1, 3), None),
            [
                "open findings: must fix 1, must answer 0, nice to fix 2 · started: 1 of 3",
                "goal demo: roadmap needs a review (shipit receipt plan-review roadmap --goal demo)",
                "demo/beta: made. Next: run shipit check on slice/demo/beta. Open: must fix 1, nice to fix 2. Started without shipit start.",
            ]
        );
    }
}
