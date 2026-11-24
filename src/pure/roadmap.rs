use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::ops::Bound;

use serde::Serialize;
use toml::{Table, Value};

use crate::pure::goal::{DoneWhen, GoalName, QualifiedDoneWhen, is_name, parse_done_when};
use crate::pure::messages::{
    AFTER_ARRAY, AFTER_ITEM, CYCLE, GOAL_NAME, LISTED_TWICE, MEETS_ARRAY, MEETS_ITEM, NO_SLICE,
    NOT_AN_ITEM, NOT_EMPTY, OBJECTS, OBJECTS_NON_EMPTY, OVERLAP, OWNS_ARRAY, OWNS_ITEM, OWNS_NONE,
    SELF_AFTER, SELF_SPLIT, SLICE_NAME, SPLIT_ARRAY, SPLIT_ITEM, SPLIT_UNHELD, TRUE_OR_FALSE,
    UNKNOWN_AFTER, UNKNOWN_SPLIT, USED_TWICE, say,
};
use crate::pure::repo_path::RepoPath;
use crate::pure::rules::{
    Parsed, Problem, Problems, at, fail, non_empty, object_at, optional, present, shape,
};
use crate::pure::text::comma_list;

const SLICE_KEYS: [&str; 5] = ["name", "after", "meets", "owns", "test_drive"];
const PROOF_KEYS: [&str; 4] = ["name", "after", "meets", "how"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub(crate) struct SliceName(String);

impl SliceName {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        (is_name(text) && text != "roadmap").then(|| Self(text.to_owned()))
    }
}

impl fmt::Display for SliceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// There is no branch field: it is `slice/<goal>/<name>`, and a stored copy could only disagree.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Slice {
    pub(crate) name: SliceName,
    pub(crate) after: Vec<SliceName>,
    pub(crate) meets: Vec<QualifiedDoneWhen>,
    pub(crate) owns: Vec<RepoPath>,
    pub(crate) test_drive: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Proof {
    pub(crate) name: SliceName,
    pub(crate) after: Vec<SliceName>,
    pub(crate) meets: Vec<QualifiedDoneWhen>,
    pub(crate) how: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Element {
    Slice(Slice),
    Proof(Proof),
}

impl Element {
    const fn name(&self) -> &SliceName {
        match self {
            Self::Slice(slice) => &slice.name,
            Self::Proof(proof) => &proof.name,
        }
    }

    fn after(&self) -> &[SliceName] {
        match self {
            Self::Slice(slice) => &slice.after,
            Self::Proof(proof) => &proof.after,
        }
    }

    fn meets(&self) -> &[QualifiedDoneWhen] {
        match self {
            Self::Slice(slice) => &slice.meets,
            Self::Proof(proof) => &proof.meets,
        }
    }

    fn owns(&self) -> &[RepoPath] {
        match self {
            Self::Slice(slice) => &slice.owns,
            Self::Proof(_) => &[],
        }
    }
}

// Roadmap order: the slices, then the proof steps.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Roadmap {
    pub(crate) goal: GoalName,
    pub(crate) elements: Vec<Element>,
}

struct Shared {
    name: SliceName,
    after: Vec<SliceName>,
    meets: Vec<DoneWhen>,
}

// Plain `DW<n>` is the roadmap's own goal; a qualified id must name that same goal.
fn own_done_when(text: &str, goal: Option<&GoalName>) -> Option<DoneWhen> {
    parse_done_when(text).or_else(|| {
        QualifiedDoneWhen::parse(text)
            .filter(|qualified| goal.is_none_or(|own| *own == qualified.goal))
            .map(|qualified| qualified.id)
    })
}

fn qualify(goal: &GoalName, ids: Vec<DoneWhen>) -> Vec<QualifiedDoneWhen> {
    ids.into_iter()
        .map(|id| QualifiedDoneWhen::of(goal, id))
        .collect()
}

// `refuse` runs on every item, so a repeat never hides another problem in the same list.
fn distinct<T: PartialEq + fmt::Display>(
    value: &Value,
    path: &str,
    (array, item): (&str, &str),
    parse: impl Fn(&str) -> Option<T>,
    refuse: impl Fn(&T) -> Option<String>,
) -> Parsed<Vec<T>> {
    let Some(items) = value.as_array() else {
        return fail(path, array);
    };
    let mut problems = Vec::new();
    let mut found: Vec<T> = Vec::new();
    for (index, raw) in items.iter().enumerate() {
        let here = format!("{path}[{index}]");
        match raw.as_str().and_then(&parse) {
            None => problems.push(Problem::new(here, item)),
            Some(next) => {
                if let Some(why) = refuse(&next) {
                    problems.push(Problem::new(here, why));
                } else if found.contains(&next) {
                    problems.push(Problem::new(here, say(LISTED_TWICE, &[("item", &next)])));
                } else {
                    found.push(next);
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

// An `Err` with no problems means the key is missing: `shape` has already reported it.
fn parse_shared(
    object: &Table,
    path: &str,
    goal: Option<&GoalName>,
    names: &mut Vec<SliceName>,
) -> Parsed<Shared> {
    let mut problems = Problems::default();
    let name = problems.take(at(
        object,
        "name",
        path,
        |value| non_empty(value).and_then(SliceName::parse),
        SLICE_NAME,
    ));
    if let Some(name) = &name {
        if names.contains(name) {
            problems.push(Problem::new(
                format!("{path}.name"),
                say(USED_TWICE, &[("name", name)]),
            ));
        } else {
            names.push(name.clone());
        }
    }
    let after = problems.take(present(object, "after", |value| {
        distinct(
            value,
            &format!("{path}.after"),
            (AFTER_ARRAY, AFTER_ITEM),
            SliceName::parse,
            |next| (Some(next) == name.as_ref()).then(|| say(SELF_AFTER, &[("name", next)])),
        )
    }));
    let meets = problems.take(present(object, "meets", |value| {
        distinct(
            value,
            &format!("{path}.meets"),
            (MEETS_ARRAY, MEETS_ITEM),
            |text| own_done_when(text, goal),
            |_| None,
        )
    }));
    problems.finish(|| {
        Some(Shared {
            name: name?,
            after: after?,
            meets: meets?,
        })
    })
}

fn parse_owns(value: &Value, path: &str) -> Parsed<Vec<RepoPath>> {
    if value.as_array().is_some_and(Vec::is_empty) {
        return fail(path, OWNS_NONE);
    }
    distinct(
        value,
        path,
        (OWNS_ARRAY, OWNS_ITEM),
        RepoPath::parse,
        |_| None,
    )
}

fn parse_split(value: &Value, path: &str, own: Option<&SliceName>) -> Parsed<Vec<SliceName>> {
    if value.as_array().is_some_and(Vec::is_empty) {
        return fail(path, SPLIT_ARRAY);
    }
    distinct(
        value,
        path,
        (SPLIT_ARRAY, SPLIT_ITEM),
        SliceName::parse,
        |next| (Some(next) == own).then(|| say(SELF_SPLIT, &[("name", next)])),
    )
}

// The parts a slice is split into come beside the slice: `Slice` itself does not carry them.
fn parse_slice(
    value: &Value,
    index: usize,
    goal: Option<&GoalName>,
    names: &mut Vec<SliceName>,
) -> Parsed<(Slice, Vec<SliceName>)> {
    let path = format!(".slice[{index}]");
    let object = object_at(value, &path)?;
    let mut problems = Problems::default();
    problems.add(shape(&path, object, &SLICE_KEYS, &["split_into"]));
    let shared = problems.take(parse_shared(object, &path, goal, names));
    let split = problems.take(optional(object, "split_into", |value| {
        parse_split(
            value,
            &format!("{path}.split_into"),
            shared.as_ref().map(|shared| &shared.name),
        )
    }));
    let owns = problems.take(present(object, "owns", |value| {
        parse_owns(value, &format!("{path}.owns"))
    }));
    let test_drive = problems.take(at(
        object,
        "test_drive",
        &path,
        Value::as_bool,
        TRUE_OR_FALSE,
    ));
    problems.finish(|| {
        let Shared { name, after, meets } = shared?;
        let slice = Slice {
            name,
            after,
            meets: qualify(goal?, meets),
            owns: owns?,
            test_drive: test_drive?,
        };
        Some((slice, split?.unwrap_or_default()))
    })
}

fn parse_proof(
    value: &Value,
    index: usize,
    goal: Option<&GoalName>,
    names: &mut Vec<SliceName>,
) -> Parsed<Proof> {
    let path = format!(".proof[{index}]");
    let object = object_at(value, &path)?;
    let mut problems = Problems::default();
    problems.add(shape(&path, object, &PROOF_KEYS, &[]));
    let shared = problems.take(parse_shared(object, &path, goal, names));
    let how = problems.take(at(
        object,
        "how",
        &path,
        |value| non_empty(value).map(str::to_owned),
        NOT_EMPTY,
    ));
    problems.finish(|| {
        let Shared { name, after, meets } = shared?;
        Some(Proof {
            name,
            after,
            meets: qualify(goal?, meets),
            how: how?,
        })
    })
}

fn tables<T>(
    value: &Value,
    path: &str,
    at_least_one: bool,
    mut each: impl FnMut(&Value, usize) -> Parsed<T>,
) -> Parsed<Vec<T>> {
    let Some(items) = value
        .as_array()
        .filter(|items| !at_least_one || !items.is_empty())
    else {
        return fail(
            path,
            if at_least_one {
                OBJECTS_NON_EMPTY
            } else {
                OBJECTS
            },
        );
    };
    let mut problems = Problems::default();
    let found: Vec<Option<T>> = items
        .iter()
        .enumerate()
        .map(|(index, item)| problems.take(each(item, index)))
        .collect();
    problems.finish(|| found.into_iter().collect())
}

// The rules that need the whole roadmap run when every element is valid on its own.
pub(crate) fn parse_roadmap(table: &Table) -> Parsed<Roadmap> {
    let mut problems = Problems::default();
    problems.add(shape("", table, &["goal", "slice"], &["proof"]));
    let goal = problems.take(at(
        table,
        "goal",
        "",
        |value| non_empty(value).and_then(GoalName::parse),
        GOAL_NAME,
    ));
    let mut names = Vec::new();
    let slices = problems.take(present(table, "slice", |value| {
        tables(value, ".slice", true, |item, index| {
            parse_slice(item, index, goal.as_ref(), &mut names)
        })
    }));
    let proofs = problems
        .take(optional(table, "proof", |value| {
            tables(value, ".proof", false, |item, index| {
                parse_proof(item, index, goal.as_ref(), &mut names)
            })
        }))
        .map(Option::unwrap_or_default);
    let (roadmap, splits) = problems.finish(|| {
        let (slices, splits): (Vec<Slice>, Vec<Vec<SliceName>>) = slices?.into_iter().unzip();
        let elements = slices
            .into_iter()
            .map(Element::Slice)
            .chain(proofs?.into_iter().map(Element::Proof))
            .collect();
        let roadmap = Roadmap {
            goal: goal?,
            elements,
        };
        Some((roadmap, splits))
    })?;
    let rest = whole_roadmap_problems(&roadmap, &splits);
    if rest.is_empty() {
        Ok(roadmap)
    } else {
        Err(rest)
    }
}

fn labelled(roadmap: &Roadmap) -> Vec<(String, &Element)> {
    let mut slices = 0_usize;
    let mut proofs = 0_usize;
    roadmap
        .elements
        .iter()
        .map(|element| match element {
            Element::Slice(_) => {
                slices += 1;
                (format!(".slice[{}]", slices - 1), element)
            }
            Element::Proof(_) => {
                proofs += 1;
                (format!(".proof[{}]", proofs - 1), element)
            }
        })
        .collect()
}

fn targets(adjacent: &[Vec<usize>], from: usize) -> &[usize] {
    adjacent.get(from).map_or(&[], Vec::as_slice)
}

// Each `after` as the position of the element it names.
fn adjacency(roadmap: &Roadmap) -> Result<Vec<Vec<usize>>, Vec<Problem>> {
    let positions: BTreeMap<&SliceName, usize> = roadmap
        .elements
        .iter()
        .enumerate()
        .map(|(position, element)| (element.name(), position))
        .collect();
    let mut problems = Vec::new();
    let mut adjacent = Vec::new();
    for (path, element) in labelled(roadmap) {
        let mut found = Vec::new();
        for (index, name) in element.after().iter().enumerate() {
            match positions.get(name) {
                Some(position) => found.push(*position),
                None => problems.push(Problem::new(
                    format!("{path}.after[{index}]"),
                    say(UNKNOWN_AFTER, &[("name", name)]),
                )),
            }
        }
        adjacent.push(found);
    }
    if problems.is_empty() {
        Ok(adjacent)
    } else {
        Err(problems)
    }
}

// Tarjan's strongly connected components, without recursion: a long chain must not overflow
// the stack. An element is on a cycle when its component holds another (no element is after itself).
struct Components {
    order: BTreeMap<usize, usize>,
    low: BTreeMap<usize, usize>,
    stack: Vec<usize>,
    on_stack: BTreeSet<usize>,
}

impl Components {
    fn discover(&mut self, node: usize) {
        let number = self.order.len();
        self.order.insert(node, number);
        self.low.insert(node, number);
        self.stack.push(node);
        self.on_stack.insert(node);
    }

    fn lower(&mut self, node: usize, to: usize) {
        self.low.entry(node).and_modify(|low| *low = (*low).min(to));
    }

    fn on_cycles(adjacent: &[Vec<usize>]) -> BTreeSet<usize> {
        let mut state = Self {
            order: BTreeMap::new(),
            low: BTreeMap::new(),
            stack: Vec::new(),
            on_stack: BTreeSet::new(),
        };
        let mut cyclic = BTreeSet::new();
        for root in 0..adjacent.len() {
            if state.order.contains_key(&root) {
                continue;
            }
            state.discover(root);
            let mut work = vec![(root, 0_usize)];
            while let Some((node, edge)) = work.pop() {
                if let Some(&next) = targets(adjacent, node).get(edge) {
                    work.push((node, edge + 1));
                    match state.order.get(&next).copied() {
                        None => {
                            state.discover(next);
                            work.push((next, 0));
                        }
                        Some(seen) if state.on_stack.contains(&next) => state.lower(node, seen),
                        Some(_) => {}
                    }
                    continue;
                }
                let (Some(&mine), Some(&low)) = (state.order.get(&node), state.low.get(&node))
                else {
                    continue;
                };
                if low == mine {
                    let mut size = 0;
                    let mut members = Vec::new();
                    while let Some(member) = state.stack.pop() {
                        state.on_stack.remove(&member);
                        members.push(member);
                        size += 1;
                        if member == node {
                            break;
                        }
                    }
                    if size > 1 {
                        cyclic.extend(members);
                    }
                }
                if let Some(&(parent, _)) = work.last() {
                    state.lower(parent, low);
                }
            }
        }
        cyclic
    }
}

// Breadth first with a parent map: the shortest way from `start` back to itself.
fn shortest_cycle(adjacent: &[Vec<usize>], start: usize) -> Option<Vec<usize>> {
    let mut parent: BTreeMap<usize, usize> = BTreeMap::new();
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        for &next in targets(adjacent, node) {
            if next == start {
                let mut back = Vec::new();
                let mut here = node;
                while here != start {
                    back.push(here);
                    here = *parent.get(&here)?;
                }
                let mut cycle = vec![start];
                cycle.extend(back.into_iter().rev());
                cycle.push(start);
                return Some(cycle);
            }
            if let std::collections::btree_map::Entry::Vacant(slot) = parent.entry(next) {
                slot.insert(node);
                queue.push_back(next);
            }
        }
    }
    None
}

// The first element, in roadmap order, that lies on a cycle; a proof step's path is `.proof`.
fn cycle_problem(roadmap: &Roadmap, adjacent: &[Vec<usize>]) -> Option<Problem> {
    let cyclic = Components::on_cycles(adjacent);
    let start = (0..adjacent.len()).find(|node| cyclic.contains(node))?;
    let cycle = shortest_cycle(adjacent, start)?;
    let names = comma_list(
        cycle
            .iter()
            .filter_map(|position| roadmap.elements.get(*position))
            .map(Element::name),
    );
    let path = match roadmap.elements.get(start) {
        Some(Element::Proof(_)) => ".proof",
        Some(Element::Slice(_)) | None => ".slice",
    };
    Some(Problem::new(path, say(CYCLE, &[("names", &names)])))
}

fn reachable(adjacent: &[Vec<usize>], from: usize) -> BTreeSet<usize> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from];
    while let Some(next) = stack.pop() {
        for &after in targets(adjacent, next) {
            if seen.insert(after) {
                stack.push(after);
            }
        }
    }
    seen
}

// Reachability is worked out only for the pairs whose paths overlap.
struct Reach<'a> {
    adjacent: &'a [Vec<usize>],
    known: BTreeMap<usize, BTreeSet<usize>>,
}

impl Reach<'_> {
    fn reaches(&mut self, from: usize, to: usize) -> bool {
        let adjacent = self.adjacent;
        self.known
            .entry(from)
            .or_insert_with(|| reachable(adjacent, from))
            .contains(&to)
    }

    fn ordered(&mut self, a: usize, b: usize) -> bool {
        self.reaches(a, b) || self.reaches(b, a)
    }
}

// The elements that own a path equal to, above or below `owned`, found by path, not pair by pair.
fn rivals(owners: &BTreeMap<&str, Vec<usize>>, owned: &RepoPath) -> BTreeSet<usize> {
    let below = format!("{owned}/");
    let end = format!("{owned}0");
    let range = owners.range::<str, _>((
        Bound::Included(below.as_str()),
        Bound::Excluded(end.as_str()),
    ));
    let above = owned
        .self_and_parents()
        .filter_map(|folder| owners.get(folder));
    above
        .chain(range.map(|(_, found)| found))
        .flatten()
        .copied()
        .collect()
}

// The message names the first earlier slice, so the later one is the one to change.
// A slice that names its parts leaves its paths to them, so it takes no part in the check.
fn overlap_problems(
    roadmap: &Roadmap,
    adjacent: &[Vec<usize>],
    splits: &[Vec<SliceName>],
) -> Vec<Problem> {
    let labelled = labelled(roadmap);
    let owned_all: Vec<&[RepoPath]> = labelled
        .iter()
        .enumerate()
        .map(|(position, (_, element))| {
            if splits.get(position).is_some_and(|parts| !parts.is_empty()) {
                &[]
            } else {
                element.owns()
            }
        })
        .collect();
    let mut owners: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (position, owned_here) in owned_all.iter().enumerate() {
        for owned in *owned_here {
            owners.entry(owned.as_str()).or_default().push(position);
        }
    }
    let mut reach = Reach {
        adjacent,
        known: BTreeMap::new(),
    };
    let mut problems = Vec::new();
    for (later, (path, _)) in labelled.iter().enumerate() {
        let owned_here = owned_all.get(later).copied().unwrap_or_default();
        for (index, owned) in owned_here.iter().enumerate() {
            let rival = rivals(&owners, owned)
                .into_iter()
                .filter(|earlier| *earlier < later)
                .find(|earlier| !reach.ordered(*earlier, later))
                .and_then(|earlier| roadmap.elements.get(earlier));
            if let Some(other) = rival {
                problems.push(Problem::new(
                    format!("{path}.owns[{index}]"),
                    say(OVERLAP, &[("path", owned), ("name", other.name())]),
                ));
            }
        }
    }
    problems
}

fn split_problems(roadmap: &Roadmap, splits: &[Vec<SliceName>]) -> Vec<Problem> {
    let known: BTreeSet<&SliceName> = roadmap
        .elements
        .iter()
        .filter(|element| matches!(element, Element::Slice(_)))
        .map(Element::name)
        .collect();
    let mut problems = Vec::new();
    for (index, parts) in splits.iter().enumerate() {
        for (part, name) in parts.iter().enumerate() {
            if !known.contains(name) {
                problems.push(Problem::new(
                    format!(".slice[{index}].split_into[{part}]"),
                    say(UNKNOWN_SPLIT, &[("name", name)]),
                ));
            }
        }
    }
    problems
}

// The overlap check skips a split slice, so its paths are safe only while its parts own them.
fn unheld_problems(roadmap: &Roadmap, splits: &[Vec<SliceName>]) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (index, (parts, element)) in splits.iter().zip(&roadmap.elements).enumerate() {
        let held: BTreeSet<&str> = roadmap
            .elements
            .iter()
            .filter(|other| parts.contains(other.name()))
            .flat_map(Element::owns)
            .map(RepoPath::as_str)
            .collect();
        for (place, owned) in element.owns().iter().enumerate() {
            if !parts.is_empty() && !owned.self_and_parents().any(|path| held.contains(path)) {
                problems.push(Problem::new(
                    format!(".slice[{index}].owns[{place}]"),
                    say(SPLIT_UNHELD, &[("path", owned)]),
                ));
            }
        }
    }
    problems
}

// Unknown names (`after`, then `split_into`) first, then a split slice's paths its parts do not
// own, then a cycle, then overlapping paths: each needs the one before it.
fn whole_roadmap_problems(roadmap: &Roadmap, splits: &[Vec<SliceName>]) -> Vec<Problem> {
    let unknown_splits = split_problems(roadmap, splits);
    let adjacent = match adjacency(roadmap) {
        Ok(adjacent) if unknown_splits.is_empty() => adjacent,
        Ok(_) => return unknown_splits,
        Err(mut problems) => {
            problems.extend(unknown_splits);
            return problems;
        }
    };
    let unheld = unheld_problems(roadmap, splits);
    if !unheld.is_empty() {
        return unheld;
    }
    if let Some(problem) = cycle_problem(roadmap, &adjacent) {
        return vec![problem];
    }
    overlap_problems(roadmap, &adjacent, splits)
}

pub(crate) fn waves(roadmap: &Roadmap) -> Vec<Vec<&SliceName>> {
    let positions: BTreeMap<&SliceName, usize> = roadmap
        .elements
        .iter()
        .enumerate()
        .map(|(position, element)| (element.name(), position))
        .collect();
    let mut waiting: BTreeMap<usize, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (position, element) in roadmap.elements.iter().enumerate() {
        waiting.insert(position, element.after().len());
        for name in element.after() {
            if let Some(before) = positions.get(name) {
                dependents.entry(*before).or_default().push(position);
            }
        }
    }
    let mut layer: Vec<usize> = waiting
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(position, _)| *position)
        .collect();
    let mut grouped = Vec::new();
    while !layer.is_empty() {
        let mut next = BTreeSet::new();
        for done in &layer {
            for waiter in dependents.get(done).into_iter().flatten() {
                if let Some(count) = waiting.get_mut(waiter) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        next.insert(*waiter);
                    }
                }
            }
        }
        grouped.push(
            layer
                .iter()
                .filter_map(|position| roadmap.elements.get(*position))
                .map(Element::name)
                .collect(),
        );
        layer = next.into_iter().collect();
    }
    grouped
}

// Unknown items in path order, then the items nobody meets in number order.
pub(crate) fn meets_problems(roadmap: &Roadmap, goal: &[DoneWhen]) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (path, element) in labelled(roadmap) {
        for (index, item) in element.meets().iter().enumerate() {
            if !goal.contains(&item.id) {
                problems.push(Problem::new(
                    format!("{path}.meets[{index}]"),
                    say(NOT_AN_ITEM, &[("id", &item.id)]),
                ));
            }
        }
    }
    let met: BTreeSet<&DoneWhen> = roadmap
        .elements
        .iter()
        .flat_map(Element::meets)
        .map(|item| &item.id)
        .collect();
    let unmet: BTreeSet<&DoneWhen> = goal.iter().filter(|item| !met.contains(item)).collect();
    problems.extend(
        unmet
            .into_iter()
            .map(|item| Problem::new(item.to_string(), NO_SLICE)),
    );
    problems
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::setup::toml_table;

    struct Spec {
        name: String,
        after: Vec<String>,
        meets: Vec<u64>,
        owns: Vec<String>,
    }

    fn spec(name: &str, after: &[&str], owns: &[&str]) -> Spec {
        Spec {
            name: name.to_owned(),
            after: after.iter().map(|item| (*item).to_owned()).collect(),
            meets: Vec::new(),
            owns: owns.iter().map(|item| (*item).to_owned()).collect(),
        }
    }

    fn quoted<T: fmt::Display>(items: &[T]) -> String {
        let items: Vec<String> = items.iter().map(|item| format!("\"{item}\"")).collect();
        format!("[{}]", items.join(", "))
    }

    fn slice_text(spec: &Spec) -> String {
        let meets: Vec<String> = spec.meets.iter().map(|n| format!("DW{n}")).collect();
        format!(
            "[[slice]]\nname = \"{}\"\nafter = {}\nmeets = {}\nowns = {}\ntest_drive = false\n\n",
            spec.name,
            quoted(&spec.after),
            quoted(&meets),
            quoted(&spec.owns),
        )
    }

    fn parse_raw(text: &str) -> Parsed<Roadmap> {
        let table = toml_table(text.as_bytes()).map_err(|why| vec![Problem::new("toml", why)])?;
        parse_roadmap(&table)
    }

    // Every roadmap under test is the goal "g".
    fn parse(text: &str) -> Parsed<Roadmap> {
        parse_raw(&format!("goal = \"g\"\n{text}"))
    }

    fn lines(problems: &[Problem]) -> Vec<String> {
        problems.iter().map(ToString::to_string).collect()
    }

    // n elements s0..s(n-1); element i comes after the earlier elements its mask picks, and the
    // roadmap lists them in the order of `keys`, so `after` may name a later element.
    fn dag() -> impl Strategy<Value = (usize, Vec<u8>, Vec<u32>)> {
        (1_usize..8).prop_flat_map(|n| {
            (
                Just(n),
                proptest::collection::vec(any::<u8>(), n),
                proptest::collection::vec(any::<u32>(), n),
            )
        })
    }

    fn numbers(least: usize) -> impl Strategy<Value = Vec<u64>> {
        proptest::collection::btree_set(1_u64..30, least..10)
            .prop_map(|set| set.into_iter().collect::<Vec<_>>())
            .prop_shuffle()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn any_acyclic_after_gives_waves_after_every_dependency((n, masks, keys) in dag()) {
            let deps = |i: usize| -> Vec<usize> {
                (0..i).filter(|k| (masks[i] >> k) & 1 == 1).collect()
            };
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by_key(|i| (keys[*i], *i));
            let text: String = order
                .into_iter()
                .map(|i| {
                    let after: Vec<String> = deps(i).iter().map(|k| format!("s{k}")).collect();
                    let owns = format!("f/s{i}.rs");
                    slice_text(&Spec {
                        name: format!("s{i}"),
                        after,
                        meets: Vec::new(),
                        owns: vec![owns],
                    })
                })
                .collect();
            let roadmap = parse(&text).expect("an acyclic roadmap parses");
            let grouped = waves(&roadmap);
            let wave_of = |i: usize| {
                grouped
                    .iter()
                    .position(|group| group.iter().any(|name| name.to_string() == format!("s{i}")))
                    .map(|position| position + 1)
                    .expect("every element is in a wave")
            };
            prop_assert_eq!(grouped.iter().map(Vec::len).sum::<usize>(), n);
            for i in 0..n {
                let expected = deps(i).into_iter().map(wave_of).max().map_or(1, |w| w + 1);
                prop_assert_eq!(wave_of(i), expected);
            }
        }

        // With the chord, s0 is also after s2, so the shortest way back to s0 skips s1.
        #[test]
        fn any_cycle_is_refused_and_named_by_its_shortest_way_round(
            len in 2_usize..7,
            chord in any::<bool>(),
            extra in 0_usize..3,
        ) {
            let chord = chord && len >= 3;
            let ring = (0..len).map(|i| {
                let next = format!("s{}", (i + 1) % len);
                let mut after = vec![next.as_str()];
                if chord && i == 0 {
                    after.push("s2");
                }
                spec(&format!("s{i}"), &after, &[&format!("f/s{i}.rs")])
            });
            let hangers = (0..extra)
                .map(|j| spec(&format!("e{j}"), &["s0"], &[&format!("g/e{j}.rs")]));
            let text: String = ring.chain(hangers).map(|item| slice_text(&item)).collect();
            let problems = parse(&text).expect_err("a cycle is refused");
            let skipped = if chord { 2 } else { 1 };
            let mut wanted = vec!["s0".to_owned()];
            wanted.extend((skipped..len).map(|i| format!("s{i}")));
            wanted.push("s0".to_owned());
            prop_assert_eq!(
                lines(&problems),
                vec![format!(".slice: after makes a cycle: {}", wanted.join(", "))]
            );
        }

        #[test]
        fn names_outside_the_pattern_are_refused(
            valid in "[a-z][a-z0-9-]{0,8}",
            prefix in "[a-z0-9-]{0,4}",
            bad in "[^a-z0-9-]",
            suffix in "[a-z0-9-]{0,4}",
            digit_first in "[0-9-][a-z0-9-]{0,5}",
            anything in "[a-zA-Z0-9 /._-]{0,6}",
        ) {
            prop_assert_eq!(
                SliceName::parse(&anything).is_some(),
                GoalName::parse(&anything).is_some() && anything != "roadmap"
            );
            prop_assert_eq!(SliceName::parse(&valid).is_some(), valid != "roadmap");
            prop_assert_eq!(SliceName::parse(&format!("{prefix}{bad}{suffix}")), None);
            prop_assert_eq!(SliceName::parse(&digit_first), None);
            for refused in ["../x", "a/b", "Alpha", "roadmap", ""] {
                prop_assert_eq!(SliceName::parse(refused), None);
            }
        }

        // `kind` picks how the two slices spell what they own: a folder over a file, a file under
        // a folder, or the same folder written with and without its trailing slash.
        #[test]
        fn parallel_elements_with_overlapping_owns_are_refused(
            dir in "[a-z]{1,4}",
            file in "[a-z]{1,4}",
            kind in 0_usize..3,
        ) {
            let folder = format!("{dir}/");
            let inside = format!("{dir}/{file}.rs");
            let (first, second, shown) = match kind {
                0 => (&folder, &inside, inside.clone()),
                1 => (&inside, &folder, dir.clone()),
                _ => (&dir, &folder, dir.clone()),
            };
            let text = format!(
                "{}{}",
                slice_text(&spec("a", &[], &[first])),
                slice_text(&spec("b", &[], &[second])),
            );
            let problems = parse(&text).expect_err("parallel slices may not share a path");
            prop_assert_eq!(
                lines(&problems),
                vec![format!(".slice[1].owns[0]: \"{shown}\" is also owned by a, which runs in parallel")]
            );
        }

        #[test]
        fn a_path_beside_another_is_not_an_overlap(dir in "[a-z]{1,4}", extra in "[a-z.]{1,3}") {
            let beside = format!("{dir}{extra}");
            let text = format!(
                "{}{}",
                slice_text(&spec("a", &[], &[&format!("{dir}/")])),
                slice_text(&spec("b", &[], &[&beside])),
            );
            prop_assert!(parse(&text).is_ok());
        }

        #[test]
        fn ordered_elements_may_own_the_same_path(len in 2_usize..6, path in "[a-z]{1,4}/") {
            let text: String = (0..len)
                .map(|i| {
                    let before = format!("s{}", i.saturating_sub(1));
                    let after: &[&str] = if i == 0 { &[] } else { &[&before] };
                    slice_text(&spec(&format!("s{i}"), after, &[&path]))
                })
                .collect();
            prop_assert!(parse(&text).is_ok());
        }

        #[test]
        fn every_unmet_item_is_named_once_in_order(goal in numbers(1), meets in numbers(0)) {
            let roadmap = parse(&slice_text(&Spec {
                meets: meets.clone(),
                ..spec("a", &[], &["f/"])
            }))
            .expect("one slice");
            let goal_items: Vec<DoneWhen> = goal
                .iter()
                .map(|n| parse_done_when(&format!("DW{n}")).expect("an id"))
                .collect();
            let mut wanted: Vec<String> = meets
                .iter()
                .enumerate()
                .filter(|(_, n)| !goal.contains(n))
                .map(|(i, n)| {
                    format!(".slice[0].meets[{i}]: \"DW{n}\" is not a done-when item of the goal")
                })
                .collect();
            let mut unmet: Vec<u64> = goal.iter().copied().filter(|n| !meets.contains(n)).collect();
            unmet.sort_unstable();
            wanted.extend(unmet.into_iter().map(|n| format!("DW{n}: no slice")));
            prop_assert_eq!(lines(&meets_problems(&roadmap, &goal_items)), wanted);
        }

        #[test]
        fn done_when_ids_in_a_roadmap_are_plain_or_qualified_by_its_own_goal(
            goal in "[a-z][a-z0-9-]{0,6}",
            other in "[a-z][a-z0-9-]{0,6}",
            n in 1_u64..100,
        ) {
            prop_assume!(other != goal);
            let text = |meets: &str| {
                format!(
                    "goal = \"{goal}\"\n[[slice]]\nname = \"a\"\nafter = []\nmeets = {meets}\nowns = [\"f/\"]\ntest_drive = false\n"
                )
            };
            let qualified = format!("{goal}:DW{n}");
            for meets in [format!("[\"DW{n}\"]"), format!("[\"{qualified}\"]")] {
                let roadmap = parse_raw(&text(&meets)).expect("an id of its own goal");
                prop_assert_eq!(roadmap.goal.to_string(), goal.clone());
                prop_assert_eq!(
                    serde_json::to_string(&roadmap).ok().map(|json| json.contains(&format!("\"meets\":[\"{qualified}\"]"))),
                    Some(true)
                );
            }
            let both = parse_raw(&text(&format!("[\"DW{n}\", \"{qualified}\"]"))).expect_err("one id twice");
            prop_assert_eq!(
                lines(&both),
                vec![format!(".slice[0].meets[1]: \"DW{n}\" is listed twice")]
            );
            let foreign = parse_raw(&text(&format!("[\"{other}:DW{n}\"]"))).expect_err("another goal's id");
            prop_assert_eq!(
                lines(&foreign),
                vec![".slice[0].meets[0]: must be a done-when id of this goal, written DW4 or <goal>:DW4".to_owned()]
            );
        }
    }

    #[test]
    fn the_problem_list_for_a_broken_roadmap() {
        let text = "
[[slice]]
name = \"alpha\"
after = []
meets = []
owns = []
test_drive = \"yes\"
extra = 1
branch = \"slice/g/alpha\"

[[slice]]
name = \"alpha\"
after = [\"alpha\", \"alpha\", \"Beta\"]
meets = [\"DW1\", \"DW1\", \"dw2\"]
owns = [\"a\", \"/b\", \"a/\"]
test_drive = false

[[slice]]
name = \"gamma\"
after = []
meets = []
test_drive = true

[[proof]]
name = \"pf\"
after = []
meets = []
how = \"\"
branch = \"slice/g/pf\"
";
        let problems = parse(text).expect_err("broken");
        assert_eq!(
            lines(&problems),
            [
                ".slice[0].extra: unknown key",
                ".slice[0].branch: unknown key",
                ".slice[0].owns: must list at least one path",
                ".slice[0].test_drive: must be true or false",
                ".slice[1].name: \"alpha\" is used twice",
                ".slice[1].after[0]: \"alpha\" cannot come after itself",
                ".slice[1].after[1]: \"alpha\" cannot come after itself",
                ".slice[1].after[2]: must be a slice or proof step name",
                ".slice[1].meets[1]: \"DW1\" is listed twice",
                ".slice[1].meets[2]: must be a done-when id of this goal, written DW4 or <goal>:DW4",
                ".slice[1].owns[1]: must be a path inside the repository, written like src/a.rs or docs/",
                ".slice[1].owns[2]: \"a\" is listed twice",
                ".slice[2].owns: missing",
                ".proof[0].branch: unknown key",
                ".proof[0].how: must be a non-empty string",
            ]
        );
        let top = parse_raw("[[proofs]]\nname = \"x\"\n").expect_err("broken");
        assert_eq!(
            lines(&top),
            [".proofs: unknown key", ".goal: missing", ".slice: missing"]
        );
        let goal = parse_raw("goal = \"Bad\"\n[[slice]]\n").expect_err("broken");
        assert_eq!(
            lines(&goal)[0],
            ".goal: must be lowercase letters, digits and \"-\", starting with a letter"
        );
        let kinds = parse("slice = []\nproof = 1\n").expect_err("broken");
        assert_eq!(
            lines(&kinds),
            [
                ".slice: must be a non-empty array of objects",
                ".proof: must be an array of objects"
            ]
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // Long chains are where a recursive or quadratic check fails first.
        #[test]
        fn a_chain_or_a_ring_in_any_order_is_read_whole(
            order in (2_usize..150).prop_flat_map(|n| Just((0..n).collect::<Vec<_>>()).prop_shuffle()),
        ) {
            let n = order.len();
            let chain: String = order
                .iter()
                .map(|&i| {
                    let before = format!("s{}", i.saturating_sub(1));
                    let after: &[&str] = if i == 0 { &[] } else { &[&before] };
                    slice_text(&spec(&format!("s{i}"), after, &[&format!("f/s{i}.rs")]))
                })
                .collect();
            let roadmap = parse(&chain).expect("a chain is acyclic");
            let grouped: Vec<Vec<String>> = waves(&roadmap)
                .iter()
                .map(|group| group.iter().map(ToString::to_string).collect())
                .collect();
            let wanted: Vec<Vec<String>> = (0..n).map(|i| vec![format!("s{i}")]).collect();
            prop_assert_eq!(grouped, wanted);

            let ring: String = order
                .iter()
                .map(|&i| {
                    let next = format!("s{}", (i + 1) % n);
                    slice_text(&spec(&format!("s{i}"), &[&next], &[&format!("f/s{i}.rs")]))
                })
                .collect();
            let first = order.first().copied().unwrap_or(0);
            let named: Vec<String> = (0..=n).map(|k| format!("s{}", (first + k) % n)).collect();
            prop_assert_eq!(
                lines(&parse(&ring).expect_err("a ring is a cycle")),
                vec![format!(".slice: after makes a cycle: {}", named.join(", "))]
            );
        }
    }

    #[test]
    fn a_repeat_does_not_hide_a_self_reference() {
        let text = slice_text(&spec("a", &["b", "b", "a"], &["x"]));
        let problems = parse(&text).expect_err("broken");
        assert_eq!(
            lines(&problems),
            [
                ".slice[0].after[1]: \"b\" is listed twice",
                ".slice[0].after[2]: \"a\" cannot come after itself",
            ]
        );
    }

    #[test]
    fn unknown_after_names_are_refused_in_path_order() {
        let text = format!(
            "{}{}[[proof]]\nname = \"pf\"\nafter = [\"a\", \"nope\"]\nmeets = []\nhow = \"by hand\"\n",
            slice_text(&spec("a", &["ghost"], &["x"])),
            slice_text(&spec("b", &["a"], &["y"])),
        );
        let problems = parse(&text).expect_err("unknown names");
        assert_eq!(
            lines(&problems),
            [
                ".slice[0].after[0]: no slice or proof step named \"ghost\"",
                ".proof[0].after[1]: no slice or proof step named \"nope\"",
            ]
        );
    }

    #[test]
    fn a_cycle_is_named_at_the_kind_of_its_first_element() {
        let proofs = "[[proof]]\nname = \"p0\"\nafter = [\"p1\"]\nmeets = []\nhow = \"x\"\n\n[[proof]]\nname = \"p1\"\nafter = [\"p0\"]\nmeets = []\nhow = \"x\"\n";
        let only_proofs = format!("{}{proofs}", slice_text(&spec("a", &[], &["x"])));
        assert_eq!(
            lines(&parse(&only_proofs).expect_err("a cycle")),
            [".proof: after makes a cycle: p0, p1, p0"]
        );
        let both = format!(
            "{}{}{proofs}",
            slice_text(&spec("a", &["b"], &["x"])),
            slice_text(&spec("b", &["a"], &["y"])),
        );
        assert_eq!(
            lines(&parse(&both).expect_err("a cycle")),
            [".slice: after makes a cycle: a, b, a"]
        );
        let after_a_cycle = format!(
            "{}{}{}",
            slice_text(&spec("first", &["a"], &["z"])),
            slice_text(&spec("a", &["b"], &["x"])),
            slice_text(&spec("b", &["a"], &["y"])),
        );
        assert_eq!(
            lines(&parse(&after_a_cycle).expect_err("a cycle")),
            [".slice: after makes a cycle: a, b, a"]
        );
    }

    #[test]
    fn a_chain_of_four_thousand_is_read_and_its_ring_is_named() {
        let size = 4000;
        let element = |i: usize, previous: usize| {
            let before = format!("s{previous}");
            let after: &[&str] = if i == 0 { &[] } else { &[&before] };
            slice_text(&spec(&format!("s{i}"), after, &[&format!("f/s{i}.rs")]))
        };
        let reversed: String = (0..size)
            .rev()
            .map(|i| element(i, i.saturating_sub(1)))
            .collect();
        let ring: String = (0..size)
            .map(|i| {
                let next = format!("s{}", (i + 1) % size);
                slice_text(&spec(&format!("s{i}"), &[&next], &[&format!("f/s{i}.rs")]))
            })
            .collect();
        assert!(parse(&reversed).is_ok());
        let cycle = parse(&ring).expect_err("a ring is a cycle");
        let shown = lines(&cycle).remove(0);
        assert!(shown.starts_with(".slice: after makes a cycle: s0, s1, s2, "));
        assert!(shown.ends_with(", s3999, s0"));
    }

    fn split_slice(name: &str, owns: &str, split: Option<&str>) -> String {
        let split = split.map_or_else(String::new, |list| format!("split_into = {list}\n"));
        format!(
            "[[slice]]\nname = \"{name}\"\nafter = []\nmeets = []\nowns = [\"{owns}\"]\n{split}test_drive = false\n\n"
        )
    }

    #[test]
    fn split_into_problems_name_each_bad_item() {
        let broken = format!(
            "{}{}",
            split_slice("a", "x/", Some("[\"a\", \"b\", \"b\", 3]")),
            split_slice("b", "y/", Some("[]")),
        );
        assert_eq!(
            lines(&parse(&broken).expect_err("broken")),
            [
                ".slice[0].split_into[0]: \"a\" cannot be split into itself",
                ".slice[0].split_into[2]: \"b\" is listed twice",
                ".slice[0].split_into[3]: must be a slice name",
                ".slice[1].split_into: must be a non-empty array of slice names",
            ]
        );
        let unknown = format!(
            "{}{}[[proof]]\nname = \"p\"\nafter = []\nmeets = []\nhow = \"h\"\n",
            split_slice("a", "x/", Some("[\"c\", \"p\"]")),
            split_slice("b", "y/", None),
        );
        assert_eq!(
            lines(&parse(&unknown).expect_err("unknown")),
            [
                ".slice[0].split_into[0]: no slice named \"c\"",
                ".slice[0].split_into[1]: no slice named \"p\"",
            ]
        );
        let proof = "[[slice]]\nname = \"a\"\nafter = []\nmeets = []\nowns = [\"x/\"]\ntest_drive = false\n[[proof]]\nname = \"p\"\nafter = []\nmeets = []\nhow = \"h\"\nsplit_into = [\"a\"]\n";
        assert_eq!(
            lines(&parse(proof).expect_err("proof")),
            [".proof[0].split_into: unknown key"]
        );
    }

    #[test]
    fn a_split_slice_leaves_its_paths_to_its_parts() {
        let split = format!(
            "{}{}",
            split_slice("a", "src/a.rs", Some("[\"b\"]")),
            split_slice("b", "src/a.rs", None),
        );
        assert!(parse(&split).is_ok());
        let plain = format!(
            "{}{}",
            split_slice("a", "src/a.rs", None),
            split_slice("b", "src/a.rs", None),
        );
        assert_eq!(
            lines(&parse(&plain).expect_err("overlap")),
            [".slice[1].owns[0]: \"src/a.rs\" is also owned by a, which runs in parallel"]
        );
        let unheld = format!(
            "{}{}{}",
            split_slice("beta", "src/beta.rs", Some("[\"dummy\"]")),
            split_slice("dummy", "other/", None),
            split_slice("gamma", "src/beta.rs", None),
        );
        assert_eq!(
            lines(&parse(&unheld).expect_err("unheld")),
            [".slice[0].owns[0]: \"src/beta.rs\" is not owned by a slice it is split into"]
        );
        let held_above = format!(
            "{}{}",
            split_slice("a", "src/a.rs", Some("[\"b\"]")),
            split_slice("b", "src", None),
        );
        assert!(parse(&held_above).is_ok());
    }
}
