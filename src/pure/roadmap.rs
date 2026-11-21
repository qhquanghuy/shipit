use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use serde::Serialize;
use toml::{Table, Value};

use crate::pure::base::BranchName;
use crate::pure::goal::{DoneWhen, GoalName, QualifiedDoneWhen, parse_done_when};
use crate::pure::rules::{
    Parsed, Problem, Problems, at, fail, non_empty, object_at, optional, present, shape,
};
use crate::pure::text::comma_list;

const NAME: &str =
    "must be lowercase letters, digits and \"-\", starting with a letter, and not \"roadmap\"";
const GOAL: &str = "must be lowercase letters, digits and \"-\", starting with a letter";
const BRANCH: &str = "must be a branch name (see git check-ref-format --branch)";
const NOT_EMPTY: &str = "must be a non-empty string";
const SLICE_KEYS: [&str; 6] = ["name", "after", "meets", "owns", "test_drive", "branch"];
const PROOF_KEYS: [&str; 4] = ["name", "after", "meets", "how"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub(crate) struct SliceName(String);

impl SliceName {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        let valid = chars.next().is_some_and(|first| first.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        (valid && text != "roadmap").then(|| Self(text.to_owned()))
    }
}

impl fmt::Display for SliceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Slice {
    pub(crate) name: SliceName,
    pub(crate) after: Vec<SliceName>,
    pub(crate) meets: Vec<QualifiedDoneWhen>,
    pub(crate) owns: Vec<String>,
    pub(crate) test_drive: bool,
    pub(crate) branch: BranchName,
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

    fn owns(&self) -> &[String] {
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

fn distinct<T: PartialEq + fmt::Display>(
    value: &Value,
    path: &str,
    (plural, singular): (&str, &str),
    parse: impl Fn(&str) -> Option<T>,
) -> Parsed<Vec<T>> {
    let Some(items) = value.as_array() else {
        return fail(path, &format!("must be an array of {plural}"));
    };
    let mut problems = Vec::new();
    let mut found: Vec<T> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let here = format!("{path}[{index}]");
        match item.as_str().and_then(&parse) {
            None => problems.push(Problem::new(here, format!("must be {singular}"))),
            Some(next) if found.contains(&next) => {
                problems.push(Problem::new(here, format!("\"{next}\" is listed twice")));
            }
            Some(next) => found.push(next),
        }
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

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
        NAME,
    ));
    if let Some(name) = &name {
        if names.contains(name) {
            problems.push(Problem::new(
                format!("{path}.name"),
                format!("\"{name}\" is used twice"),
            ));
        } else {
            names.push(name.clone());
        }
    }
    let after = problems.take(present(object, "after", |value| {
        distinct(
            value,
            &format!("{path}.after"),
            ("slice or proof step names", "a slice or proof step name"),
            SliceName::parse,
        )
    }));
    if let (Some(name), Some(after)) = (&name, &after) {
        for (index, next) in after.iter().enumerate() {
            if next == name {
                problems.push(Problem::new(
                    format!("{path}.after[{index}]"),
                    format!("\"{name}\" cannot come after itself"),
                ));
            }
        }
    }
    let meets = problems.take(present(object, "meets", |value| {
        distinct(
            value,
            &format!("{path}.meets"),
            (
                "done-when ids",
                "a done-when id of this goal, written DW4 or <goal>:DW4",
            ),
            |text| own_done_when(text, goal),
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

fn parse_owns(value: &Value, path: &str) -> Parsed<Vec<String>> {
    let Some(items) = value.as_array() else {
        return fail(path, "must be an array of non-empty strings");
    };
    if items.is_empty() {
        return fail(path, "must list at least one path");
    }
    let mut problems = Vec::new();
    let mut found = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match non_empty(item) {
            Some(text) => found.push(text.to_owned()),
            None => problems.push(Problem::new(format!("{path}[{index}]"), NOT_EMPTY)),
        }
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

fn parse_slice(
    value: &Value,
    index: usize,
    base: &BranchName,
    goal: Option<&GoalName>,
    names: &mut Vec<SliceName>,
) -> Parsed<Slice> {
    let path = format!(".slice[{index}]");
    let object = object_at(value, &path)?;
    let mut problems = Problems::default();
    problems.add(shape(&path, object, &SLICE_KEYS, &[]));
    let shared = problems.take(parse_shared(object, &path, goal, names));
    let owns = problems.take(present(object, "owns", |value| {
        parse_owns(value, &format!("{path}.owns"))
    }));
    let test_drive = problems.take(at(
        object,
        "test_drive",
        &path,
        Value::as_bool,
        "must be true or false",
    ));
    let branch = problems.take(at(
        object,
        "branch",
        &path,
        |value| non_empty(value).and_then(BranchName::parse),
        BRANCH,
    ));
    if let Some(branch) = &branch {
        // The exact name also makes branches unique, as slice names are.
        let expected = goal
            .zip(shared.as_ref())
            .map(|(goal, shared)| format!("slice/{goal}/{}", shared.name));
        if branch == base {
            problems.push(Problem::new(
                format!("{path}.branch"),
                format!("must not be the base branch \"{base}\""),
            ));
        } else if let Some(expected) = expected.filter(|expected| expected != branch.as_str()) {
            problems.push(Problem::new(
                format!("{path}.branch"),
                format!("must be \"{expected}\""),
            ));
        }
    }
    problems.finish(|| {
        let Shared { name, after, meets } = shared?;
        Some(Slice {
            name,
            after,
            meets: qualify(goal?, meets),
            owns: owns?,
            test_drive: test_drive?,
            branch: branch?,
        })
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
        let message = if at_least_one {
            "must be a non-empty array of objects"
        } else {
            "must be an array of objects"
        };
        return fail(path, message);
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
pub(crate) fn parse_roadmap(table: &Table, base: &BranchName) -> Parsed<Roadmap> {
    let mut problems = Problems::default();
    problems.add(shape("", table, &["goal", "slice"], &["proof"]));
    let goal = problems.take(at(
        table,
        "goal",
        "",
        |value| non_empty(value).and_then(GoalName::parse),
        GOAL,
    ));
    let mut names = Vec::new();
    let slices = problems.take(present(table, "slice", |value| {
        tables(value, ".slice", true, |item, index| {
            parse_slice(item, index, base, goal.as_ref(), &mut names)
        })
    }));
    let proofs = problems
        .take(optional(table, "proof", |value| {
            tables(value, ".proof", false, |item, index| {
                parse_proof(item, index, goal.as_ref(), &mut names)
            })
        }))
        .map(Option::unwrap_or_default);
    let roadmap = problems.finish(|| {
        let elements = slices?
            .into_iter()
            .map(Element::Slice)
            .chain(proofs?.into_iter().map(Element::Proof))
            .collect();
        Some(Roadmap {
            goal: goal?,
            elements,
        })
    })?;
    let rest = whole_roadmap_problems(&roadmap);
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

// Breadth first, so the cycle found through `start` is the shortest.
fn cycle_from(adjacent: &[Vec<usize>], start: usize) -> Option<Vec<usize>> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([vec![start]]);
    while let Some(path) = queue.pop_front() {
        let Some(&last) = path.last() else { continue };
        for &next in targets(adjacent, last) {
            let mut longer = path.clone();
            longer.push(next);
            if next == start {
                return Some(longer);
            }
            if seen.insert(next) {
                queue.push_back(longer);
            }
        }
    }
    None
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

// A folder ends in "/" and holds itself and every path below it.
fn holds(folder: &str, path: &str) -> bool {
    folder == path || (folder.ends_with('/') && path.starts_with(folder))
}

fn overlaps(a: &str, b: &str) -> bool {
    holds(a, b) || holds(b, a)
}

fn whole_roadmap_problems(roadmap: &Roadmap) -> Vec<Problem> {
    let labelled = labelled(roadmap);
    let mut problems = Vec::new();
    let mut adjacent: Vec<Vec<usize>> = Vec::new();
    for (path, element) in &labelled {
        let mut found = Vec::new();
        for (index, name) in element.after().iter().enumerate() {
            match roadmap
                .elements
                .iter()
                .position(|other| other.name() == name)
            {
                Some(position) => found.push(position),
                None => problems.push(Problem::new(
                    format!("{path}.after[{index}]"),
                    format!("no slice or proof step named \"{name}\""),
                )),
            }
        }
        adjacent.push(found);
    }
    if !problems.is_empty() {
        return problems;
    }
    let cycle = (0..adjacent.len()).find_map(|start| cycle_from(&adjacent, start));
    if let Some(cycle) = cycle {
        let named = comma_list(
            cycle
                .iter()
                .filter_map(|position| roadmap.elements.get(*position))
                .map(Element::name),
        );
        let path = match cycle.first().and_then(|first| roadmap.elements.get(*first)) {
            Some(Element::Proof(_)) => ".proof",
            Some(Element::Slice(_)) | None => ".slice",
        };
        return vec![Problem::new(path, format!("after makes a cycle: {named}"))];
    }
    let reach: Vec<BTreeSet<usize>> = (0..adjacent.len())
        .map(|from| reachable(&adjacent, from))
        .collect();
    let ordered = |a: usize, b: usize| {
        reach.get(a).is_some_and(|set| set.contains(&b))
            || reach.get(b).is_some_and(|set| set.contains(&a))
    };
    for (later, (path, element)) in labelled.iter().enumerate() {
        for (index, owned) in element.owns().iter().enumerate() {
            let rival = labelled
                .iter()
                .enumerate()
                .take(later)
                .filter(|(earlier, _)| !ordered(*earlier, later))
                .find(|(_, (_, other))| other.owns().iter().any(|theirs| overlaps(owned, theirs)));
            if let Some((_, (_, other))) = rival {
                problems.push(Problem::new(
                    format!("{path}.owns[{index}]"),
                    format!(
                        "\"{owned}\" is also owned by {}, which runs in parallel",
                        other.name()
                    ),
                ));
            }
        }
    }
    problems
}

// An element with no `after` is wave 1; otherwise one more than its latest dependency.
pub(crate) fn waves(roadmap: &Roadmap) -> Vec<Vec<&SliceName>> {
    let mut wave: BTreeMap<&SliceName, usize> = BTreeMap::new();
    loop {
        let mut progressed = false;
        for element in &roadmap.elements {
            if wave.contains_key(element.name()) {
                continue;
            }
            let latest: Option<Vec<usize>> = element
                .after()
                .iter()
                .map(|name| wave.get(name).copied())
                .collect();
            if let Some(latest) = latest {
                wave.insert(element.name(), 1 + latest.into_iter().max().unwrap_or(0));
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    let last = wave.values().copied().max().unwrap_or(0);
    (1..=last)
        .map(|number| {
            roadmap
                .elements
                .iter()
                .map(Element::name)
                .filter(|name| wave.get(name) == Some(&number))
                .collect()
        })
        .collect()
}

// Unknown items in path order, then the items nobody meets in number order.
pub(crate) fn meets_problems(roadmap: &Roadmap, goal: &[DoneWhen]) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (path, element) in labelled(roadmap) {
        for (index, item) in element.meets().iter().enumerate() {
            if !goal.contains(&item.id) {
                problems.push(Problem::new(
                    format!("{path}.meets[{index}]"),
                    format!("\"{}\" is not a done-when item of the goal", item.id),
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
            .map(|item| Problem::new(item.to_string(), "no slice")),
    );
    problems
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::setup::toml_table;
    use crate::pure::testing::name;

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
            "[[slice]]\nname = \"{0}\"\nafter = {1}\nmeets = {2}\nowns = {3}\ntest_drive = false\nbranch = \"slice/g/{0}\"\n\n",
            spec.name,
            quoted(&spec.after),
            quoted(&meets),
            quoted(&spec.owns),
        )
    }

    fn parse_raw(text: &str) -> Parsed<Roadmap> {
        let table = toml_table(text.as_bytes()).map_err(|why| vec![Problem::new("toml", why)])?;
        parse_roadmap(&table, &name("main"))
    }

    // Every roadmap under test is the goal "g", so its slices' branches are slice/g/<name>.
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

        #[test]
        fn any_cycle_is_refused_and_named(len in 2_usize..6, extra in 0_usize..3) {
            let ring = (0..len).map(|i| {
                let next = format!("s{}", (i + 1) % len);
                spec(&format!("s{i}"), &[&next], &[&format!("f/s{i}.rs")])
            });
            let hangers = (0..extra)
                .map(|j| spec(&format!("e{j}"), &["s0"], &[&format!("g/e{j}.rs")]));
            let text: String = ring.chain(hangers).map(|item| slice_text(&item)).collect();
            let problems = parse(&text).expect_err("a cycle is refused");
            let mut wanted: Vec<String> = (0..len).map(|i| format!("s{i}")).collect();
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
        ) {
            prop_assert_eq!(SliceName::parse(&valid).is_some(), valid != "roadmap");
            prop_assert_eq!(SliceName::parse(&format!("{prefix}{bad}{suffix}")), None);
            prop_assert_eq!(SliceName::parse(&digit_first), None);
            for refused in ["../x", "a/b", "Alpha", "roadmap", ""] {
                prop_assert_eq!(SliceName::parse(refused), None);
            }
        }

        #[test]
        fn parallel_elements_with_overlapping_owns_are_refused(
            dir in "[a-z]{1,4}",
            file in "[a-z]{1,4}",
            folder_first in any::<bool>(),
        ) {
            let folder = format!("{dir}/");
            let inside = format!("{dir}/{file}.rs");
            let (first, second) = if folder_first { (&folder, &inside) } else { (&inside, &folder) };
            let text = format!(
                "{}{}",
                slice_text(&spec("a", &[], &[first])),
                slice_text(&spec("b", &[], &[second])),
            );
            let problems = parse(&text).expect_err("parallel slices may not share a path");
            prop_assert_eq!(
                lines(&problems),
                vec![format!(".slice[1].owns[0]: \"{second}\" is also owned by a, which runs in parallel")]
            );
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
        fn a_folder_holds_only_paths_below_it(
            dir in "[a-z]{1,4}",
            tail in "[a-z]{1,4}(/[a-z]{1,4}){0,2}",
            letters in "[a-z.]{1,3}",
        ) {
            let folder = format!("{dir}/");
            let below = format!("{dir}/{tail}");
            let sibling = format!("{dir}{letters}");
            prop_assert!(overlaps(&folder, &below));
            prop_assert!(overlaps(&below, &folder));
            prop_assert!(overlaps(&folder, &folder));
            prop_assert!(!overlaps(&folder, &sibling));
            prop_assert!(!overlaps(&dir, &sibling));
            prop_assert!(!overlaps(&dir, &below));
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
branch = \"main\"
extra = 1

[[slice]]
name = \"alpha\"
after = [\"alpha\", \"alpha\", \"Beta\"]
meets = [\"DW1\", \"DW1\", \"dw2\"]
owns = [\"a\", \"\"]
test_drive = false
branch = \"slice/x\"

[[slice]]
name = \"gamma\"
after = []
meets = []
test_drive = true
branch = \"slice/x\"

[[proof]]
name = \"pf\"
after = []
meets = []
how = \"\"
branch = \"slice/y\"
";
        let problems = parse(text).expect_err("broken");
        assert_eq!(
            lines(&problems),
            [
                ".slice[0].extra: unknown key",
                ".slice[0].owns: must list at least one path",
                ".slice[0].test_drive: must be true or false",
                ".slice[0].branch: must not be the base branch \"main\"",
                ".slice[1].name: \"alpha\" is used twice",
                ".slice[1].after[1]: \"alpha\" is listed twice",
                ".slice[1].after[2]: must be a slice or proof step name",
                ".slice[1].meets[1]: \"DW1\" is listed twice",
                ".slice[1].meets[2]: must be a done-when id of this goal, written DW4 or <goal>:DW4",
                ".slice[1].owns[1]: must be a non-empty string",
                ".slice[2].owns: missing",
                ".slice[2].branch: must be \"slice/g/gamma\"",
                ".proof[0].branch: unknown key",
                ".proof[0].how: must be a non-empty string",
            ]
        );
        let itself = parse(&slice_text(&spec("a", &["a"], &["x"]))).expect_err("broken");
        assert_eq!(
            lines(&itself),
            [".slice[0].after[0]: \"a\" cannot come after itself"]
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

        #[test]
        fn a_branch_must_be_slice_goal_slice(
            goal in "[a-z][a-z0-9-]{0,6}",
            slice in "[a-z][a-z0-9-]{0,6}",
            other in "[a-z][a-z0-9-]{0,6}",
        ) {
            prop_assume!(slice != "roadmap" && other != goal && other != slice);
            let text = |branch: &str| {
                format!(
                    "goal = \"{goal}\"\n{}",
                    slice_text(&spec(&slice, &[], &["f/"]))
                        .replace(&format!("slice/g/{slice}"), branch)
                )
            };
            let right = format!("slice/{goal}/{slice}");
            let roadmap = parse_raw(&text(&right)).expect("the exact branch is accepted");
            prop_assert_eq!(roadmap.goal.to_string(), goal.clone());
            for wrong in [
                format!("slice/{slice}"),
                format!("slice/{other}/{slice}"),
                format!("slice/{goal}/{other}"),
                format!("{right}/x"),
            ] {
                let problems = parse_raw(&text(&wrong)).expect_err("another branch is refused");
                prop_assert_eq!(
                    lines(&problems),
                    vec![format!(".slice[0].branch: must be \"{right}\"")]
                );
            }
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
                    "goal = \"{goal}\"\n[[slice]]\nname = \"a\"\nafter = []\nmeets = {meets}\nowns = [\"f/\"]\ntest_drive = false\nbranch = \"slice/{goal}/a\"\n"
                )
            };
            let qualified = format!("{goal}:DW{n}");
            for meets in [format!("[\"DW{n}\"]"), format!("[\"{qualified}\"]")] {
                let roadmap = parse_raw(&text(&meets)).expect("an id of its own goal");
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
}
