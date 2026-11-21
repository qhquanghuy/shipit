use std::fmt;

use toml::{Table, Value};

use crate::pure::rules::{Parsed, Problem, whole};
use crate::pure::setup::{SETUP_PATH, Setup};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Flow {
    base: String,
    cap: u64,
    check: Vec<String>,
    // The whole table: a base file may carry keys the schema does not know.
    environment: Value,
}

impl Flow {
    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    pub(crate) fn of(setup: &Setup) -> Self {
        let prefix = setup
            .gate
            .environment
            .prefix
            .iter()
            .map(|command| Value::String(command.as_str().to_owned()))
            .collect();
        let mut environment = Table::new();
        environment.insert("prefix".to_owned(), Value::Array(prefix));
        Self {
            base: setup.gate.base.as_str().to_owned(),
            cap: setup.gate.max_files.get(),
            check: setup
                .gate
                .check
                .iter()
                .map(|command| command.as_str().to_owned())
                .collect(),
            environment: Value::Table(environment),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlowField {
    Base,
    Cap,
    Check,
    Environment,
}

// Against no setup at all, every field is the slice's own.
pub(crate) const ALL_FIELDS: [FlowField; 4] = [
    FlowField::Base,
    FlowField::Cap,
    FlowField::Check,
    FlowField::Environment,
];

impl fmt::Display for FlowField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Base => "gate.base",
            Self::Cap => "gate.max_files",
            Self::Check => "gate.check",
            Self::Environment => "gate.environment",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SetupVerdict {
    Same,
    // The base branch's setup is not the one the slice forked from: merge or rebase first.
    Moved(Vec<FlowField>),
    // The slice itself changed these fields, as against the setup it forked from.
    Changed(Vec<FlowField>),
}

// A slice whose setup is the tip's own adds nothing the base has not got, whatever the fork
// looked like (a squash merge of its parent leaves the fork's setup different). Else the
// slice's own change is judged against the base setup at the merge base, so a base that moved
// on cannot be mistaken for the slice's work. Without a merge-base setup nothing can be told
// apart, and any difference from the tip counts as moved.
pub(crate) fn setup_verdict(tip: &Flow, merge_base: Option<&Flow>, slice: &Flow) -> SetupVerdict {
    let ahead = setup_changes(tip, slice);
    if ahead.is_empty() {
        return SetupVerdict::Same;
    }
    let Some(forked) = merge_base else {
        return SetupVerdict::Moved(ahead);
    };
    let moved = setup_changes(forked, tip);
    if !moved.is_empty() {
        return SetupVerdict::Moved(moved);
    }
    match setup_changes(forked, slice) {
        changes if changes.is_empty() => SetupVerdict::Same,
        changes => SetupVerdict::Changed(changes),
    }
}

pub(crate) fn setup_changes(base: &Flow, branch: &Flow) -> Vec<FlowField> {
    [
        (FlowField::Base, base.base == branch.base),
        (FlowField::Cap, base.cap == branch.cap),
        (FlowField::Check, base.check == branch.check),
        (
            FlowField::Environment,
            base.environment == branch.environment,
        ),
    ]
    .into_iter()
    .filter(|(_, same)| !same)
    .map(|(field, _)| field)
    .collect()
}

// A diff that touches the setup file and nothing else.
pub(crate) fn setup_only(names: &[Vec<u8>]) -> bool {
    matches!(names, [only] if only == SETUP_PATH.as_bytes())
}

fn all_strings(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

// Only the four `gate` fields the comparison reads, and only their types: growing the schema
// never breaks a base branch's file.
pub(crate) fn parse_baseline(object: &Table) -> Parsed<Flow> {
    let gate = object.get("gate").and_then(Value::as_table);
    let base = gate
        .and_then(|gate| gate.get("base"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty());
    let cap = gate
        .and_then(|gate| gate.get("max_files"))
        .and_then(whole)
        .filter(|cap| *cap >= 1);
    let check = gate
        .and_then(|gate| gate.get("check"))
        .and_then(all_strings);
    let environment = gate
        .and_then(|gate| gate.get("environment"))
        .filter(|value| value.get("prefix").and_then(all_strings).is_some());
    let mut problems = Vec::new();
    if base.is_none() {
        problems.push(Problem::new(".gate.base", "must be a non-empty string"));
    }
    if cap.is_none() {
        problems.push(Problem::new(
            ".gate.max_files",
            "must be an integer of at least 1",
        ));
    }
    if check.is_none() {
        problems.push(Problem::new(".gate.check", "must be an array of strings"));
    }
    if environment.is_none() {
        problems.push(Problem::new(
            ".gate.environment.prefix",
            "must be an array of strings",
        ));
    }
    match (base, cap, check, environment) {
        (Some(base), Some(cap), Some(check), Some(environment)) => Ok(Flow {
            base: base.to_owned(),
            cap,
            check,
            environment: environment.clone(),
        }),
        _ => Err(problems),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::setup::parse_setup;

    fn strings(items: &[String]) -> Value {
        Value::Array(items.iter().cloned().map(Value::String).collect())
    }

    fn environment(run: &[String]) -> Value {
        let mut table = Table::new();
        table.insert("prefix".to_owned(), strings(run));
        Value::Table(table)
    }

    fn table(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    fn flow() -> Flow {
        Flow {
            base: "main".to_owned(),
            cap: 5,
            check: vec!["a".to_owned(), "b".to_owned()],
            environment: environment(&[]),
        }
    }

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

    fn names(changes: &[FlowField]) -> Vec<String> {
        changes.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_whole_environment_table_is_compared() {
        let mut with_extra = Table::new();
        with_extra.insert("prefix".to_owned(), strings(&[]));
        with_extra.insert("extra".to_owned(), Value::Integer(1));
        let extra = Flow {
            environment: Value::Table(with_extra),
            ..flow()
        };
        assert_eq!(names(&setup_changes(&extra, &flow())), ["gate.environment"]);
    }

    fn problems(doc: &Table) -> Vec<String> {
        parse_baseline(doc)
            .err()
            .unwrap_or_default()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn a_base_file_of_another_schema_is_read_for_its_four_fields() {
        let doc = table(
            "version = 1\n[gate]\nbase = \"main\"\nmax_files = 60\ncheck = []\n\
             [gate.environment]\nprefix = []\n[from_the_future]\na = 1\n",
        );
        assert_eq!(
            parse_baseline(&doc),
            Ok(Flow {
                cap: 60,
                check: Vec::new(),
                ..flow()
            })
        );
    }

    #[test]
    fn every_wrong_field_is_listed_in_order() {
        let doc = table(
            "[gate]\nbase = 1\nmax_files = \"60\"\ncheck = [1]\n[gate.environment]\nprefix = [2]\n",
        );
        assert_eq!(
            problems(&doc),
            [
                ".gate.base: must be a non-empty string",
                ".gate.max_files: must be an integer of at least 1",
                ".gate.check: must be an array of strings",
                ".gate.environment.prefix: must be an array of strings",
            ]
        );
        assert_eq!(problems(&Table::new()).len(), 4);
        let doc = table("[gate]\nbase = \"\"\nmax_files = 0\ncheck = []\nenvironment = \"x\"\n");
        assert_eq!(
            problems(&doc),
            [
                ".gate.base: must be a non-empty string",
                ".gate.max_files: must be an integer of at least 1",
                ".gate.environment.prefix: must be an array of strings",
            ]
        );
    }

    fn arb_flow() -> impl Strategy<Value = Flow> {
        (
            "[a-c]{1,2}",
            1_u64..4,
            proptest::collection::vec("[a-c]{0,2}", 0..3),
            proptest::collection::vec("[a-c]{0,2}", 0..3),
        )
            .prop_map(|(base, cap, check, run)| Flow {
                base,
                cap,
                check,
                environment: environment(&run),
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn the_verdict_follows_the_merge_base_not_the_tip(
            forked in arb_flow(),
            tip in arb_flow(),
            slice in arb_flow(),
        ) {
            prop_assume!(!setup_changes(&tip, &slice).is_empty());
            let verdict = setup_verdict(&tip, Some(&forked), &slice);
            let moved = setup_changes(&forked, &tip);
            let changed = setup_changes(&forked, &slice);
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
            let expected = setup_changes(&tip, &slice);
            let verdict = setup_verdict(&tip, None, &slice);
            if expected.is_empty() {
                prop_assert_eq!(verdict, SetupVerdict::Same);
            } else {
                prop_assert_eq!(verdict, SetupVerdict::Moved(expected));
            }
        }

        #[test]
        fn a_setup_and_its_own_file_read_as_the_same_flow(
            base in "[a-z]{1,6}",
            cap in 1_i64..1000,
            check in proptest::collection::vec("[a-z ]{1,4}", 0..3),
            run in proptest::collection::vec("[a-z]{1,4}", 0..3),
            fix_rounds in 0_i64..9,
        ) {
            let mut doc = table(include_str!("../../tests/fixtures/setup-valid.toml"));
            let mut gate = Table::new();
            gate.insert("base".to_owned(), Value::String(base));
            gate.insert("max_files".to_owned(), Value::Integer(cap));
            gate.insert("check".to_owned(), strings(&check));
            gate.insert("environment".to_owned(), environment(&run));
            doc.insert("gate".to_owned(), Value::Table(gate));
            if let Some(Value::Table(agents)) = doc.get_mut("agents") {
                agents.insert("max_fix_rounds".to_owned(), Value::Integer(fix_rounds));
            }
            let setup = parse_setup(&doc)
                .map_err(|_why| TestCaseError::fail("the changed fixture parses"))?;
            prop_assert_eq!(Some(Flow::of(&setup)), parse_baseline(&doc).ok());
        }

        #[test]
        fn setup_changes_names_exactly_the_fields_that_differ(a in arb_flow(), b in arb_flow()) {
            let expected: Vec<FlowField> = [
                (FlowField::Base, a.base != b.base),
                (FlowField::Cap, a.cap != b.cap),
                (FlowField::Check, a.check != b.check),
                (FlowField::Environment, a.environment != b.environment),
            ]
            .into_iter()
            .filter(|(_, differs)| *differs)
            .map(|(field, _)| field)
            .collect();
            prop_assert_eq!(setup_changes(&a, &b), expected);
        }
    }
}
