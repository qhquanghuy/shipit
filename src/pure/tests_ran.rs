use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::pure::coverage_map::CoverageMap;
use crate::pure::goal::{DoneWhen, GoalName, QualifiedDoneWhen, parse_done_when};
use crate::pure::repo_path::RepoPath;
use crate::pure::roadmap::SliceName;
use crate::pure::text::comma_list;

const MAP_DIR: &str = ".shipit/coverage/";
const SETUP_FILE: &str = ".config/shipit.toml";

// A map at `.shipit/coverage/<goal>/<slice>.toml`; any other path is no map.
pub(crate) fn parse_map_path(path: &str) -> Option<(GoalName, SliceName)> {
    let rest = path.strip_prefix(MAP_DIR)?.strip_suffix(".toml")?;
    let (goal, slice) = rest.split_once('/')?;
    Some((GoalName::parse(goal)?, SliceName::parse(slice)?))
}

pub(crate) fn map_path(goal: &GoalName, slice: &SliceName) -> String {
    format!("{MAP_DIR}{goal}/{slice}.toml")
}

// The Nix build log puts `<derivation>> ` before every line.
fn without_log_prefix(line: &str) -> &str {
    match line.split_once("> ") {
        Some((word, rest)) if !word.is_empty() && !word.contains(char::is_whitespace) => rest,
        Some(_) | None => line,
    }
}

fn skipped(rest: &str) -> bool {
    let lower = rest.to_ascii_lowercase();
    lower.contains("# skip") || lower.contains("# todo")
}

fn passed_name(line: &str) -> Option<&str> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("test ") {
        return rest.strip_suffix(" ... ok");
    }
    if let Some(rest) = line.strip_prefix("PASS ") {
        return rest.split_whitespace().last();
    }
    let rest = line.strip_prefix("ok ")?;
    let (number, name) = rest.split_once(' ')?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let name = name.strip_prefix("- ").unwrap_or(name);
    (!skipped(name)).then_some(name)
}

// libtest `test <name> ... ok`, nextest `PASS [<time>] <name>` and TAP `ok <n> <name>`.
pub(crate) fn passed_tests(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| passed_name(without_log_prefix(line)))
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Found {
    Ran,
    Missing,
    Ambiguous(Vec<String>),
}

// An equal name wins; else exactly one passed name that ends with `::<named>`.
pub(crate) fn find(named: &str, passed: &BTreeSet<String>) -> Found {
    if passed.contains(named) {
        return Found::Ran;
    }
    let tail = format!("::{named}");
    let matches: Vec<String> = passed
        .iter()
        .filter(|name| name.ends_with(&tail))
        .cloned()
        .collect();
    match matches.as_slice() {
        [] => Found::Missing,
        [_] => Found::Ran,
        [_, _, ..] => Found::Ambiguous(matches),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Covered {
    pub(crate) goal: GoalName,
    pub(crate) slice: SliceName,
    pub(crate) items: Vec<(DoneWhen, usize)>,
}

impl Covered {
    // `coverage <goal>/<slice>: <goal>:DW4 ✓ (3 tests), ...`
    pub(crate) fn line(&self) -> String {
        let head = format!("coverage {}/{}:", self.goal, self.slice);
        if self.items.is_empty() {
            return format!("{head} no done-when item");
        }
        let items = comma_list(self.items.iter().map(|(id, tests)| {
            let noun = if *tests == 1 { "test" } else { "tests" };
            format!(
                "{} ✓ ({tests} {noun})",
                QualifiedDoneWhen::of(&self.goal, *id)
            )
        }));
        format!("{head} {items}")
    }
}

pub(crate) type Maps = [(GoalName, SliceName, CoverageMap)];

pub(crate) fn verdict(maps: &Maps, passed: &BTreeSet<String>) -> Result<Vec<Covered>, Vec<String>> {
    let mut problems = Vec::new();
    let mut covered = Vec::new();
    for (goal, slice, map) in maps {
        let mut items = Vec::new();
        for (id, names) in &map.items {
            for named in names {
                match find(named, passed) {
                    Found::Ran => {}
                    Found::Missing => problems.push(format!(
                        "{id}: test not found: {named} ({})",
                        map_path(goal, slice)
                    )),
                    Found::Ambiguous(all) => problems.push(format!(
                        "{named}: matches several tests ({}): give the path",
                        comma_list(all)
                    )),
                }
            }
            items.push((*id, names.len()));
        }
        covered.push(Covered {
            goal: goal.clone(),
            slice: slice.clone(),
            items,
        });
    }
    if problems.is_empty() {
        Ok(covered)
    } else {
        Err(problems)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MapRule {
    NotNeeded,
    Added,
    Missing,
}

// An empty change and a change of the setup file and the files the base lists alone need no map
// (setup-only and adoption changes); any other change must add one, an empty map included. The branch `slice/<goal>/<slice>`
// names its own map, so another slice's map added, or an older one removed, does not count.
pub(crate) fn map_rule(
    changes: &[(char, String)],
    own: Option<(&GoalName, &SliceName)>,
    setup_files: &[RepoPath],
) -> MapRule {
    if changes.iter().all(|(_, path)| {
        path == SETUP_FILE
            || setup_files
                .iter()
                .any(|listed| listed.touches(path.as_bytes()))
    }) {
        return MapRule::NotNeeded;
    }
    let added: Vec<(GoalName, SliceName)> = changes
        .iter()
        .filter(|(status, _)| *status == 'A')
        .filter_map(|(_, path)| parse_map_path(path))
        .collect();
    let has_own = own.is_none_or(|(goal, slice)| added.contains(&(goal.clone(), slice.clone())));
    if added.is_empty() || !has_own {
        MapRule::Missing
    } else {
        MapRule::Added
    }
}

// What the store's roadmap and build plan add to the committed map; they are not in CI.
pub(crate) fn local_problems(
    map: &CoverageMap,
    meets: &[QualifiedDoneWhen],
    plan_block: Option<&CoverageMap>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for item in meets {
        if !map.items.contains_key(&item.id) {
            problems.push(format!("{}: in meets but not in the map", item.id));
        }
    }
    for id in map.items.keys() {
        if !meets.iter().any(|item| item.id == *id) {
            problems.push(format!("{id}: in the map but not in meets"));
        }
    }
    if plan_block.is_some_and(|block| block != map) {
        problems.push(
            "the map differs from the build plan's \"## Done-when coverage\" block".to_owned(),
        );
    }
    problems
}

// The coverage record of one version for one slice: its key, the check line that wrote it, and how many tests each item of the slice's map named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tested {
    pub(crate) key: String,
    pub(crate) check: String,
    pub(crate) items: BTreeMap<DoneWhen, usize>,
}

pub(crate) fn recorded(doc: &Value, goal: &GoalName, slice: &SliceName) -> Option<Tested> {
    let key = doc.get("key")?.as_str()?.to_owned();
    let check = doc.get("check")?.as_str()?.to_owned();
    let items = doc
        .get("maps")
        .and_then(|maps| maps.get(format!("{goal}/{slice}")))
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(id, names)| Some((parse_done_when(id)?, names.as_array()?.len())))
                .collect()
        })
        .unwrap_or_default();
    Some(Tested { key, check, items })
}

// Only a coverage line writes a record whose tests ran; any other setup line could write the file by hand.
pub(crate) fn is_coverage_line(line: &str) -> bool {
    let mut words = line.split_whitespace();
    matches!(
        (words.next(), words.next(), words.next()),
        (Some(program), Some("coverage"), Some("--")) if program.rsplit('/').next() == Some("shipit")
    )
}

#[derive(Debug, Serialize)]
pub(crate) struct CoverageRecord<'a> {
    kind: &'static str,
    version: u8,
    key: String,
    check: &'a str,
    maps: BTreeMap<String, &'a CoverageMap>,
    time: String,
    tool: String,
}

impl<'a> CoverageRecord<'a> {
    pub(crate) fn new(
        key: String,
        check: &'a str,
        maps: &'a Maps,
        (time, tool): (String, String),
    ) -> Self {
        Self {
            kind: "coverage",
            version: 1,
            key,
            check,
            maps: maps
                .iter()
                .map(|(goal, slice, map)| (format!("{goal}/{slice}"), map))
                .collect(),
            time,
            tool,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn goal(text: &str) -> GoalName {
        GoalName::parse(text).expect("a goal name")
    }

    fn slice(text: &str) -> SliceName {
        SliceName::parse(text).expect("a slice name")
    }

    fn map_of(names: &[&str]) -> CoverageMap {
        let mut items = BTreeMap::new();
        if let Some(id) = parse_done_when("DW4") {
            items.insert(id, names.iter().map(|name| (*name).to_owned()).collect());
        }
        CoverageMap { items }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn passed_lines_are_read_in_libtest_nextest_and_tap_forms(
            name in "[a-z][a-z0-9_:]{0,10}",
            words in "[a-z][a-z ]{0,9}",
            prefix in proptest::option::of("[a-z-]{1,8}"),
        ) {
            let lead = prefix.map(|word| format!("{word}> ")).unwrap_or_default();
            let tap = words.trim().to_owned();
            let output = format!(
                "{lead}test {name} ... ok\n{lead}        PASS [   0.004s] crate {name}\n{lead}ok 3 {tap}\n{lead}ok 4 - {tap}\n"
            );
            let found = passed_tests(&output);
            prop_assert!(found.contains(&name));
            prop_assert!(found.contains(&tap));
        }

        #[test]
        fn failed_skipped_and_ignored_tests_never_count(
            name in "[a-z][a-z0-9_:]{0,10}",
            directive in prop_oneof![Just("# skip"), Just("# SKIP why"), Just("# todo"), Just("# Todo later")],
        ) {
            let output = format!(
                "test {name} ... FAILED\ntest {name} ... ignored\nnot ok 1 {name}\nok 2 {name} {directive}\n\
                 FAIL [ 0.1s] crate {name}\ntest result: ok. 3 passed\nok\n"
            );
            prop_assert!(passed_tests(&output).is_empty());
        }

        #[test]
        fn a_named_test_matches_exactly_or_as_a_unique_path_suffix(
            leaf in "[a-z][a-z0-9_]{0,8}",
            parents in proptest::collection::btree_set("[a-z]{1,4}", 0..4),
        ) {
            let passed: BTreeSet<String> = parents.iter().map(|parent| format!("{parent}::{leaf}")).collect();
            let found = find(&leaf, &passed);
            match passed.len() {
                0 => prop_assert_eq!(found, Found::Missing),
                1 => prop_assert_eq!(found, Found::Ran),
                _ => prop_assert_eq!(found, Found::Ambiguous(passed.iter().cloned().collect())),
            }
            let mut with_exact = passed;
            with_exact.insert(leaf.clone());
            prop_assert_eq!(find(&leaf, &with_exact), Found::Ran);
        }

        #[test]
        fn a_map_is_needed_unless_the_change_is_empty_or_setup_only(
            others in proptest::collection::vec("[a-z]{1,6}(/[a-z]{1,6}){0,2}", 0..3),
            statuses in proptest::collection::vec(prop_oneof![Just('A'), Just('M'), Just('D')], 3),
            with_map in any::<bool>(),
            with_setup in any::<bool>(),
            listed_changed in any::<bool>(),
            listed_dropped in any::<bool>(),
            above_changed in any::<bool>(),
        ) {
            let listed = RepoPath::parse("deny.toml").unwrap();
            let nested = RepoPath::parse("conf/lint.toml").unwrap();
            // The files the base lists; a path that only looks like one is a code change.
            let lists = if listed_dropped { Vec::new() } else { vec![listed, nested] };
            let mut changes: Vec<(char, String)> = others
                .iter()
                .zip(&statuses)
                .map(|(path, status)| (*status, format!("src/{path}")))
                .collect();
            if with_setup {
                changes.push(('M', ".config/shipit.toml".to_owned()));
            }
            if listed_changed {
                changes.push(('M', "deny.toml".to_owned()));
            }
            if above_changed {
                changes.push(('A', "conf".to_owned()));
            }
            if with_map {
                changes.push(('A', ".shipit/coverage/demo/beta.toml".to_owned()));
            }
            let code = !others.is_empty()
                || ((listed_changed || above_changed) && listed_dropped);
            let expected = if !code && !with_map {
                MapRule::NotNeeded
            } else if with_map {
                MapRule::Added
            } else {
                MapRule::Missing
            };
            prop_assert_eq!(map_rule(&changes, None, &lists), expected);
        }

        #[test]
        fn only_this_branchs_own_map_counts_as_the_added_map(
            other in "[a-z]{1,5}",
            removed in any::<bool>(),
            own_added in any::<bool>(),
        ) {
            let (demo, beta) = (goal("demo"), slice("beta"));
            let mut changes = vec![
                ('A', format!(".shipit/coverage/{other}/zz.toml")),
                ('M', "src/a.rs".to_owned()),
            ];
            if removed {
                changes.push(('D', ".shipit/coverage/v1/old.toml".to_owned()));
            }
            if own_added {
                changes.push(('A', ".shipit/coverage/demo/beta.toml".to_owned()));
            }
            let expected = if own_added { MapRule::Added } else { MapRule::Missing };
            prop_assert_eq!(map_rule(&changes, Some((&demo, &beta)), &[]), expected);
        }

        #[test]
        fn a_map_path_parses_back_to_its_goal_and_slice(
            g in "[a-z][a-z0-9-]{0,6}",
            s in "[a-z][a-z0-9-]{0,6}",
        ) {
            prop_assume!(s != "roadmap");
            let (goal, slice) = (goal(&g), slice(&s));
            prop_assert_eq!(parse_map_path(&map_path(&goal, &slice)), Some((goal, slice)));
            prop_assert_eq!(parse_map_path(&format!(".shipit/coverage/{g}/{s}/x.toml")), None);
            prop_assert_eq!(parse_map_path(&format!(".shipit/coverage/{g}.toml")), None);
        }
    }

    fn counts() -> impl Strategy<Value = BTreeMap<u64, usize>> {
        proptest::collection::btree_map(1..9_u64, 1..4_usize, 0..4)
    }

    fn map_from(counts: &BTreeMap<u64, usize>) -> CoverageMap {
        CoverageMap {
            items: counts
                .iter()
                .filter_map(|(n, count)| {
                    let id = parse_done_when(&format!("DW{n}"))?;
                    Some((id, (0..*count).map(|i| format!("test_{i}")).collect()))
                })
                .collect(),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_coverage_record_reads_back_as_the_items_of_its_slice(
            mine in counts(),
            other in counts(),
            key in "[0-9a-f]{40}",
            check in "[a-z ]{1,12}",
        ) {
            let (demo, beta) = (goal("demo"), slice("beta"));
            let maps = [
                (demo.clone(), beta.clone(), map_from(&mine)),
                (demo.clone(), slice("gamma"), map_from(&other)),
            ];
            let record = CoverageRecord::new(key.clone(), &check, &maps, ("time".to_owned(), "tool".to_owned()));
            let doc = serde_json::to_value(record).expect("a record is JSON");
            let items = map_from(&mine)
                .items
                .iter()
                .map(|(id, names)| (*id, names.len()))
                .collect();
            prop_assert_eq!(recorded(&doc, &demo, &beta), Some(Tested { key: key.clone(), check: check.clone(), items }));
            let elsewhere = recorded(&doc, &demo, &slice("delta")).map(|tested| tested.items);
            prop_assert_eq!(elsewhere, Some(BTreeMap::new()));
            let mut bare = doc;
            if let Some(object) = bare.as_object_mut() {
                object.remove("check");
            }
            prop_assert_eq!(recorded(&bare, &demo, &beta), None);
        }

        #[test]
        fn a_coverage_line_is_shipit_coverage_and_a_double_dash(
            dir in proptest::option::of("/[a-z]{1,5}(/[a-z]{1,5}){0,2}"),
            rest in "[a-z ]{0,10}",
            word in 0..4_usize,
            sub in prop::sample::select(vec!["coverage", "lint"]),
            dashes in any::<bool>(),
        ) {
            let words = ["shipit", "shipitx", "xshipit", "nix"];
            let program = words.get(word).copied().expect("a program word");
            let path = dir.map_or_else(|| program.to_owned(), |dir| format!("{dir}/{program}"));
            let dd = if dashes { "--" } else { "-x" };
            let line = format!("{path} {sub} {dd} {rest}");
            prop_assert_eq!(
                is_coverage_line(&line),
                program == "shipit" && sub == "coverage" && dashes
            );
        }
    }

    #[test]
    fn the_coverage_problems() {
        let maps = [(goal("demo"), slice("beta"), map_of(&["one", "two"]))];
        let passed: BTreeSet<String> = ["a::two", "b::two", "one"].map(str::to_owned).into();
        assert_eq!(
            verdict(&maps, &passed),
            Err(vec![
                "two: matches several tests (a::two, b::two): give the path".to_owned()
            ])
        );
        let passed: BTreeSet<String> = ["a::two"].map(str::to_owned).into();
        assert_eq!(
            verdict(&maps, &passed),
            Err(vec![
                "DW4: test not found: one (.shipit/coverage/demo/beta.toml)".to_owned()
            ])
        );
        let passed: BTreeSet<String> = ["one", "a::two"].map(str::to_owned).into();
        let covered = verdict(&maps, &passed).expect("every test ran");
        assert_eq!(
            covered.iter().map(Covered::line).collect::<Vec<_>>(),
            ["coverage demo/beta: demo:DW4 ✓ (2 tests)"]
        );
        let empty = [(
            goal("demo"),
            slice("beta"),
            CoverageMap {
                items: BTreeMap::new(),
            },
        )];
        assert_eq!(
            verdict(&empty, &passed).expect("nothing to confirm")[0].line(),
            "coverage demo/beta: no done-when item"
        );
        let meets = [QualifiedDoneWhen::parse("demo:DW2").expect("an id")];
        let problems = local_problems(&map_of(&["one"]), &meets, Some(&map_of(&["two"])));
        assert_eq!(
            problems,
            [
                "DW2: in meets but not in the map",
                "DW4: in the map but not in meets",
                "the map differs from the build plan's \"## Done-when coverage\" block",
            ]
        );
    }
}
