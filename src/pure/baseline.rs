use std::collections::BTreeSet;

use toml::{Table, Value};

use crate::pure::failure::Failure;
use crate::pure::repo_path::RepoPath;
use crate::pure::rules::{Parsed, Problem};
use crate::pure::setup::{
    AGENTS_KEYS, AGENTS_OPTIONAL, GATE_KEYS, GATE_OPTIONAL, OWNER_KEYS, OWNER_OPTIONAL, PACKS_KEYS,
    SETUP_PATH, TOP_KEYS,
};

// A setup file as the comparison sees it: the base it names, and every key kept raw, so the
// schema can grow without breaking a base branch's file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Flow {
    base: String,
    table: Table,
}

impl Flow {
    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    pub(crate) fn table(&self) -> &Table {
        &self.table
    }

    // The files whose change counts as a setup change. Raw like the rest of the baseline: an
    // entry that is not a path is skipped, and a list that is not an array is empty.
    pub(crate) fn setup_files(&self) -> Vec<RepoPath> {
        self.table
            .get("gate")
            .and_then(Value::as_table)
            .and_then(|gate| gate.get("setup_files"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().and_then(RepoPath::parse))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SetupVerdict {
    Same,
    // The base branch's setup is not the one the slice forked from: merge or rebase first.
    Moved(Vec<String>),
    // The slice itself changed these fields, as against the setup it forked from.
    Changed(Vec<String>),
}

// A slice whose setup is the tip's own adds nothing the base has not got, whatever the fork
// looked like (a squash merge of its parent leaves the fork's setup different). Else the
// slice's own change is judged against the base setup at the merge base, so a base that moved
// on cannot be mistaken for the slice's work. Without a merge-base setup nothing can be told
// apart, and any difference from the tip counts as moved.
pub(crate) fn setup_verdict(tip: &Flow, merge_base: Option<&Flow>, slice: &Flow) -> SetupVerdict {
    let ahead = setup_changes(tip.table(), slice.table());
    if ahead.is_empty() {
        return SetupVerdict::Same;
    }
    let Some(forked) = merge_base else {
        return SetupVerdict::Moved(ahead);
    };
    let moved = setup_changes(forked.table(), tip.table());
    if !moved.is_empty() {
        return SetupVerdict::Moved(moved);
    }
    match setup_changes(forked.table(), slice.table()) {
        changes if changes.is_empty() => SetupVerdict::Same,
        changes => SetupVerdict::Changed(changes),
    }
}

// The schema's keys in the schema's order, then the other keys by name.
fn ordered_keys(schema: &[&str], base: &Table, branch: &Table) -> Vec<String> {
    let known = schema
        .iter()
        .filter(|key| base.contains_key(**key) || branch.contains_key(**key))
        .map(|key| (*key).to_owned());
    let unknown: BTreeSet<&String> = base
        .keys()
        .chain(branch.keys())
        .filter(|key| !schema.contains(&key.as_str()))
        .collect();
    known.chain(unknown.into_iter().cloned()).collect()
}

fn schema_of(table: &str) -> Vec<&'static str> {
    match table {
        "gate" => GATE_KEYS.iter().chain(&GATE_OPTIONAL).copied().collect(),
        "agents" => AGENTS_KEYS
            .iter()
            .chain(&AGENTS_OPTIONAL)
            .copied()
            .collect(),
        "packs" => PACKS_KEYS.to_vec(),
        "owner" => OWNER_KEYS.iter().chain(&OWNER_OPTIONAL).copied().collect(),
        _ => Vec::new(),
    }
}

fn table_changes(name: &str, base: &Table, branch: &Table, changes: &mut Vec<String>) {
    for key in ordered_keys(&schema_of(name), base, branch) {
        if base.get(&key) != branch.get(&key) {
            changes.push(format!("{name}.{key}"));
        }
    }
}

// The changed field paths: a table is compared key by key, one level deep (`agents.tiers` is
// one field). A table present on one side only is compared with an empty one; if it has no
// keys, its own name is the change, so an empty table added or removed never hides. Any other
// value is compared whole.
pub(crate) fn setup_changes(base: &Table, branch: &Table) -> Vec<String> {
    let none = Table::new();
    let mut changes = Vec::new();
    for key in ordered_keys(&TOP_KEYS, base, branch) {
        match (base.get(&key), branch.get(&key)) {
            (Some(Value::Table(a)), Some(Value::Table(b))) => {
                table_changes(&key, a, b, &mut changes);
            }
            (Some(Value::Table(only)), None) | (None, Some(Value::Table(only))) => {
                if only.is_empty() {
                    changes.push(key);
                } else {
                    table_changes(&key, only, &none, &mut changes);
                }
            }
            (before, after) => {
                if before != after {
                    changes.push(key);
                }
            }
        }
    }
    changes
}

fn is_setup_file(name: &[u8], listed: &[RepoPath]) -> bool {
    name == SETUP_PATH.as_bytes() || listed.iter().any(|path| path.covers(name))
}

// A diff that touches the setup file or listed files and nothing else.
pub(crate) fn setup_only(names: &[Vec<u8>], listed: &[RepoPath]) -> bool {
    !names.is_empty() && names.iter().all(|name| is_setup_file(name, listed))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Touched {
    // The changed setup fields, then the changed listed files by path.
    pub(crate) fields: Vec<String>,
    // Nothing but the setup file and listed files is in the diff.
    pub(crate) alone: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Judgement {
    Clear,
    Moved(Vec<String>),
    Touched(Touched),
}

// None when the diff changes no setup field and no listed file: it leaves the checks alone.
fn setup_touch(mut fields: Vec<String>, listed: &[RepoPath], names: &[Vec<u8>]) -> Option<Touched> {
    fields.extend(
        names
            .iter()
            .filter(|name| name.as_slice() != SETUP_PATH.as_bytes())
            .filter(|name| listed.iter().any(|path| path.covers(name)))
            .map(|name| String::from_utf8_lossy(name).into_owned()),
    );
    (!fields.is_empty()).then(|| Touched {
        fields,
        alone: setup_only(names, listed),
    })
}

// The listed files are the base tip's, never the slice's: a slice that drops a file from the
// list is still judged by the list it forked from. The diff is listed only when it can matter.
pub(crate) fn judge_diff(
    tip: &Flow,
    merge_base: Option<&Flow>,
    slice: &Flow,
    names: impl FnOnce() -> Result<Vec<Vec<u8>>, Failure>,
) -> Result<Judgement, Failure> {
    let fields = match setup_verdict(tip, merge_base, slice) {
        SetupVerdict::Moved(fields) => return Ok(Judgement::Moved(fields)),
        SetupVerdict::Same => Vec::new(),
        SetupVerdict::Changed(fields) => fields,
    };
    let listed = tip.setup_files();
    if fields.is_empty() && listed.is_empty() {
        return Ok(Judgement::Clear);
    }
    Ok(setup_touch(fields, &listed, &names()?).map_or(Judgement::Clear, Judgement::Touched))
}

// Only `gate.base` is read, because the base's own setup says which branch the slice is judged
// by; everything else is kept raw.
pub(crate) fn parse_baseline(object: &Table) -> Parsed<Flow> {
    object
        .get("gate")
        .and_then(Value::as_table)
        .and_then(|gate| gate.get("base"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|base| Flow {
            base: base.to_owned(),
            table: object.clone(),
        })
        .ok_or_else(|| vec![Problem::new(".gate.base", "must be a non-empty string")])
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    // Every field path of the schema, then one unknown key inside `gate` (placed after its
    // schema keys) and one at the top; `zzz` is a top-level table with no keys.
    const PATHS: [&str; 19] = [
        "version",
        "gate.base",
        "gate.max_files",
        "gate.check",
        "gate.environment",
        "gate.setup_files",
        "gate.extra",
        "agents.max_fix_rounds",
        "agents.effort_cap",
        "agents.tiers",
        "agents.roles",
        "agents.parallel",
        "packs.skill",
        "packs.domain",
        "packs.model",
        "owner.max_lines",
        "owner.status_update_max_lines",
        "owner.example_max_lines",
        "extra",
    ];

    fn built(values: &[Option<i64>], empty_table: bool) -> Table {
        let mut doc = Table::new();
        for (path, value) in PATHS.iter().zip(values) {
            let Some(n) = value else { continue };
            match path.split_once('.') {
                Some((name, key)) => {
                    let mut merged = doc
                        .get(name)
                        .and_then(Value::as_table)
                        .cloned()
                        .unwrap_or_default();
                    merged.insert(key.to_owned(), Value::Integer(*n));
                    doc.insert(name.to_owned(), Value::Table(merged));
                }
                None => {
                    doc.insert((*path).to_owned(), Value::Integer(*n));
                }
            }
        }
        if empty_table {
            doc.insert("zzz".to_owned(), Value::Table(Table::new()));
        }
        doc
    }

    fn arb_flow() -> impl Strategy<Value = Flow> {
        (
            "[a-c]{1,2}",
            1_i64..4,
            proptest::collection::vec("[a-c]{0,2}", 0..3),
            proptest::option::of(0_i64..3),
        )
            .prop_map(|(base, cap, check, fix_rounds)| {
                let mut gate = Table::new();
                gate.insert("base".to_owned(), Value::String(base));
                gate.insert("max_files".to_owned(), Value::Integer(cap));
                gate.insert(
                    "check".to_owned(),
                    Value::Array(check.into_iter().map(Value::String).collect()),
                );
                let mut doc = Table::new();
                doc.insert("gate".to_owned(), Value::Table(gate));
                if let Some(n) = fix_rounds {
                    let mut agents = Table::new();
                    agents.insert("max_fix_rounds".to_owned(), Value::Integer(n));
                    doc.insert("agents".to_owned(), Value::Table(agents));
                }
                parse_baseline(&doc).unwrap()
            })
    }

    const SETUP: &str = ".config/shipit.toml";
    // Names a diff can hold: the setup file, files a base may list (alone or as a folder), and
    // near misses of each.
    const NAMES: [&str; 12] = [
        SETUP,
        "deny.toml",
        "ci",
        "ci/a.yml",
        "ci/b/c.yml",
        "src/a.rs",
        "deny.toml.bak",
        "ci-x/a",
        ".config/shipit.json",
        ".config/shipit.toml.bak",
        "denyatoml",
        "deny.toml/x",
    ];
    const LISTABLE: [&str; 2] = ["deny.toml", "ci"];

    fn covered(name: &str, listed: &[&str]) -> bool {
        listed
            .iter()
            .any(|path| name == *path || name.starts_with(&format!("{path}/")))
    }

    fn flow_with(cap: i64, listed: Option<&Vec<&str>>) -> Flow {
        let mut gate = Table::new();
        gate.insert("base".to_owned(), Value::String("main".to_owned()));
        gate.insert("max_files".to_owned(), Value::Integer(cap));
        if let Some(paths) = listed {
            gate.insert(
                "setup_files".to_owned(),
                Value::Array(
                    paths
                        .iter()
                        .map(|p| Value::String((*p).to_owned()))
                        .collect(),
                ),
            );
        }
        let mut doc = Table::new();
        doc.insert("gate".to_owned(), Value::Table(gate));
        parse_baseline(&doc).unwrap()
    }

    fn arb_list() -> impl Strategy<Value = Option<Vec<&'static str>>> {
        proptest::option::of(proptest::sample::subsequence(
            LISTABLE.to_vec(),
            0..=LISTABLE.len(),
        ))
    }

    fn arb_names() -> impl Strategy<Value = Vec<&'static str>> {
        proptest::sample::subsequence(NAMES.to_vec(), 0..=NAMES.len())
    }

    fn bytes(names: &[&str]) -> Vec<Vec<u8>> {
        names.iter().map(|name| name.as_bytes().to_vec()).collect()
    }

    fn changes(a: &Flow, b: &Flow) -> Vec<String> {
        setup_changes(a.table(), b.table())
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // The base tip is the policy: the slice's own list (even one that drops or adds the
        // file it changes) only shows up as a changed field, never as a listed file.
        #[test]
        fn a_diff_is_judged_by_the_base_list_and_setup_only_means_nothing_else_is_in_it(
            base_list in arb_list(),
            slice_list in arb_list(),
            raised in any::<bool>(),
            names in arb_names(),
        ) {
            let tip = flow_with(1, base_list.as_ref());
            let slice = flow_with(if raised { 2 } else { 1 }, slice_list.as_ref());
            let judged = judge_diff(&tip, Some(&tip), &slice, || Ok(bytes(&names)));
            let listed = base_list.clone().unwrap_or_default();
            let mut fields = Vec::new();
            if raised {
                fields.push("gate.max_files".to_owned());
            }
            if base_list != slice_list {
                fields.push("gate.setup_files".to_owned());
            }
            fields.extend(
                names
                    .iter()
                    .filter(|name| **name != SETUP && covered(name, &listed))
                    .map(|name| (*name).to_owned()),
            );
            let expected = if fields.is_empty() {
                Judgement::Clear
            } else {
                let alone = !names.is_empty()
                    && names.iter().all(|name| *name == SETUP || covered(name, &listed));
                Judgement::Touched(Touched { fields, alone })
            };
            prop_assert_eq!(judged, Ok(expected));
        }

        #[test]
        fn the_diff_is_listed_only_when_it_can_matter(
            base_list in arb_list(),
            raised in any::<bool>(),
            moved in any::<bool>(),
        ) {
            let tip = flow_with(if moved { 3 } else { 1 }, base_list.as_ref());
            let forked = flow_with(1, base_list.as_ref());
            let slice = flow_with(if raised { 2 } else { 1 }, base_list.as_ref());
            let asked = std::cell::Cell::new(false);
            let judged = judge_diff(&tip, Some(&forked), &slice, || {
                asked.set(true);
                Ok(Vec::new())
            });
            let listed_anything = base_list.as_ref().is_some_and(|list| !list.is_empty());
            if moved {
                prop_assert!(matches!(judged, Ok(Judgement::Moved(_))));
            }
            prop_assert_eq!(asked.get(), !moved && (raised || listed_anything));
        }

        #[test]
        fn the_base_list_keeps_its_paths_and_skips_what_is_not_one(
            good in proptest::collection::vec("[a-z]{1,4}(/[a-z]{1,4}){0,2}", 0..4),
            junk in proptest::collection::vec(
                prop_oneof![
                    Just(Value::Integer(1)),
                    Just(Value::Boolean(true)),
                    Just(Value::String("/abs".to_owned())),
                    Just(Value::String("a/../b".to_owned())),
                    Just(Value::String("*.toml".to_owned())),
                ],
                0..3,
            ),
            as_array in any::<bool>(),
        ) {
            let mut gate = Table::new();
            gate.insert("base".to_owned(), Value::String("main".to_owned()));
            let items: Vec<Value> = good
                .iter()
                .map(|path| Value::String(path.clone()))
                .chain(junk)
                .collect();
            gate.insert(
                "setup_files".to_owned(),
                if as_array { Value::Array(items) } else { Value::String("deny.toml".to_owned()) },
            );
            let mut doc = Table::new();
            doc.insert("gate".to_owned(), Value::Table(gate));
            let kept: Vec<String> = parse_baseline(&doc)
                .unwrap()
                .setup_files()
                .iter()
                .map(ToString::to_string)
                .collect();
            prop_assert_eq!(kept, if as_array { good } else { Vec::new() });
        }

        #[test]
        fn setup_changes_names_every_differing_field(
            a in proptest::collection::vec(proptest::option::of(0_i64..3), PATHS.len()),
            b in proptest::collection::vec(proptest::option::of(0_i64..3), PATHS.len()),
            empty in (any::<bool>(), any::<bool>()),
        ) {
            let mut expected: Vec<String> = PATHS
                .iter()
                .zip(a.iter().zip(&b))
                .filter(|(_, (x, y))| x != y)
                .map(|(path, _)| (*path).to_owned())
                .collect();
            if empty.0 != empty.1 {
                expected.push("zzz".to_owned());
            }
            prop_assert_eq!(setup_changes(&built(&a, empty.0), &built(&b, empty.1)), expected);
        }

        #[test]
        fn only_a_missing_base_is_a_problem(
            base in proptest::option::of(prop_oneof![
                Just(Value::String(String::new())),
                Just(Value::Integer(1)),
                Just(Value::Boolean(true)),
                "[a-z]{1,6}".prop_map(Value::String),
            ]),
            other in proptest::collection::vec(proptest::option::of(0_i64..3), PATHS.len()),
        ) {
            let mut doc = built(&other, false);
            if let Some(base) = &base {
                let mut gate = doc.get("gate").and_then(Value::as_table).cloned().unwrap_or_default();
                gate.insert("base".to_owned(), base.clone());
                doc.insert("gate".to_owned(), Value::Table(gate));
            } else if let Some(Value::Table(gate)) = doc.get_mut("gate") {
                gate.remove("base");
            }
            let expected = match base.as_ref().and_then(Value::as_str).filter(|text| !text.is_empty()) {
                Some(name) => Ok(Flow { base: name.to_owned(), table: doc.clone() }),
                None => Err(vec![Problem::new(".gate.base", "must be a non-empty string")]),
            };
            prop_assert_eq!(parse_baseline(&doc), expected);
        }

        #[test]
        fn the_verdict_follows_the_merge_base_not_the_tip(
            forked in arb_flow(),
            tip in arb_flow(),
            slice in arb_flow(),
        ) {
            prop_assume!(!changes(&tip, &slice).is_empty());
            let verdict = setup_verdict(&tip, Some(&forked), &slice);
            let moved = changes(&forked, &tip);
            let changed = changes(&forked, &slice);
            if !moved.is_empty() {
                prop_assert_eq!(verdict, SetupVerdict::Moved(moved));
            } else if changed.is_empty() {
                prop_assert_eq!(verdict, SetupVerdict::Same);
            } else {
                prop_assert_eq!(verdict, SetupVerdict::Changed(changed));
            }
        }

        #[test]
        fn a_slice_with_the_tips_own_setup_is_the_same_whatever_the_fork_was(
            forked in proptest::option::of(arb_flow()),
            tip in arb_flow(),
        ) {
            prop_assert_eq!(
                setup_verdict(&tip, forked.as_ref(), &tip),
                SetupVerdict::Same
            );
        }

        #[test]
        fn without_a_merge_base_setup_any_difference_from_the_tip_is_moved(
            tip in arb_flow(),
            slice in arb_flow(),
        ) {
            let expected = changes(&tip, &slice);
            let verdict = setup_verdict(&tip, None, &slice);
            if expected.is_empty() {
                prop_assert_eq!(verdict, SetupVerdict::Same);
            } else {
                prop_assert_eq!(verdict, SetupVerdict::Moved(expected));
            }
        }
    }
}
