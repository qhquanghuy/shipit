use std::collections::BTreeSet;

use toml::{Table, Value};

use crate::pure::rules::{Parsed, Problem};
use crate::pure::setup::{
    AGENTS_KEYS, AGENTS_OPTIONAL, GATE_KEYS, OWNER_KEYS, OWNER_OPTIONAL, PACKS_KEYS, SETUP_PATH,
    TOP_KEYS,
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
        "gate" => GATE_KEYS.to_vec(),
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

// A diff that touches the setup file and nothing else.
pub(crate) fn setup_only(names: &[Vec<u8>]) -> bool {
    matches!(names, [only] if only == SETUP_PATH.as_bytes())
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

    #[test]
    fn only_a_diff_of_the_setup_file_alone_is_setup_only() {
        let setup = SETUP_PATH.as_bytes().to_vec();
        assert!(setup_only(std::slice::from_ref(&setup)));
        assert!(!setup_only(&[]));
        assert!(!setup_only(&[b"src/main.rs".to_vec()]));
        assert!(!setup_only(&[setup.clone(), b"a".to_vec()]));
        assert!(!setup_only(&[b"a".to_vec(), setup]));
        assert!(!setup_only(&[b".config/shipit.toml.bak".to_vec()]));
        assert!(!setup_only(&[b".config/shipit.json".to_vec()]));
    }

    // Every field path of the schema, then one unknown key inside `gate` (placed after its
    // schema keys) and one at the top; `zzz` is a top-level table with no keys.
    const PATHS: [&str; 18] = [
        "version",
        "gate.base",
        "gate.max_files",
        "gate.check",
        "gate.environment",
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

    fn changes(a: &Flow, b: &Flow) -> Vec<String> {
        setup_changes(a.table(), b.table())
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

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
