use std::fmt;

use serde_json::Value;

use crate::pure::json::{Parsed, Problem, whole};
use crate::pure::setup::Setup;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Flow {
    base: String,
    cap: u64,
    check: Vec<String>,
    // The whole object, as jq compared it: a base file may carry keys the schema does not know.
    environment: Value,
}

impl Flow {
    pub(crate) fn of(setup: &Setup) -> Self {
        let run = setup
            .environment
            .run
            .iter()
            .map(|command| Value::String(command.as_str().to_owned()))
            .collect();
        let mut environment = serde_json::Map::new();
        environment.insert("run".to_owned(), Value::Array(run));
        Self {
            base: setup.base.as_str().to_owned(),
            cap: setup.cap.get(),
            check: setup
                .check
                .iter()
                .map(|command| command.as_str().to_owned())
                .collect(),
            environment: Value::Object(environment),
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

impl fmt::Display for FlowField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Base => "base",
            Self::Cap => "cap",
            Self::Check => "check",
            Self::Environment => "environment",
        })
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

fn all_strings(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

// Only the four fields the comparison reads, and only their types: growing the schema never
// breaks a base branch's file.
pub(crate) fn parse_baseline(docs: &[Value]) -> Parsed<Flow> {
    let [doc] = docs else {
        return Err(vec![Problem::new(
            ".",
            format!("must hold exactly one JSON document, found {}", docs.len()),
        )]);
    };
    let Some(object) = doc.as_object() else {
        return Err(vec![Problem::new(".", "must be a JSON object")]);
    };
    let base = object
        .get("base")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty());
    let cap = object.get("cap").and_then(whole).filter(|cap| *cap >= 1);
    let check = object.get("check").and_then(all_strings);
    let environment = object
        .get("environment")
        .filter(|value| value.get("run").and_then(all_strings).is_some());
    let mut problems = Vec::new();
    if base.is_none() {
        problems.push(Problem::new(".base", "must be a non-empty string"));
    }
    if cap.is_none() {
        problems.push(Problem::new(".cap", "must be an integer of at least 1"));
    }
    if check.is_none() {
        problems.push(Problem::new(".check", "must be an array of strings"));
    }
    if environment.is_none() {
        problems.push(Problem::new(
            ".environment.run",
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
    use serde_json::json;

    use super::*;

    fn flow() -> Flow {
        Flow {
            base: "main".to_owned(),
            cap: 5,
            check: vec!["a".to_owned(), "b".to_owned()],
            environment: json!({"run": []}),
        }
    }

    fn names(changes: &[FlowField]) -> Vec<String> {
        changes.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn setup_changes_names_the_fields_in_order() {
        let same = flow();
        assert_eq!(names(&setup_changes(&same, &same)), Vec::<String>::new());
        let cap = Flow { cap: 6, ..flow() };
        assert_eq!(names(&setup_changes(&same, &cap)), ["cap"]);
        let reordered = Flow {
            check: vec!["b".to_owned(), "a".to_owned()],
            ..flow()
        };
        assert_eq!(names(&setup_changes(&same, &reordered)), ["check"]);
        let several = Flow {
            base: "x".to_owned(),
            check: vec!["a".to_owned()],
            environment: json!({"run": ["true"]}),
            ..flow()
        };
        assert_eq!(
            names(&setup_changes(&same, &several)),
            ["base", "check", "environment"]
        );
    }

    #[test]
    fn the_whole_environment_object_is_compared() {
        let extra = Flow {
            environment: json!({"run": [], "extra": 1}),
            ..flow()
        };
        assert_eq!(names(&setup_changes(&extra, &flow())), ["environment"]);
    }

    fn problems(doc: &Value) -> Vec<String> {
        parse_baseline(std::slice::from_ref(doc))
            .err()
            .unwrap_or_default()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn a_base_file_of_another_schema_is_read_for_its_four_fields() {
        let doc = json!({
            "base": "main", "cap": 60, "check": [], "environment": {"run": []},
            "from_the_future": {"a": 1}
        });
        assert_eq!(
            parse_baseline(&[doc]),
            Ok(Flow {
                cap: 60,
                check: Vec::new(),
                ..flow()
            })
        );
    }

    #[test]
    fn every_wrong_field_is_listed_in_order() {
        let doc = json!({"base": 1, "cap": "60", "check": [1], "environment": {"run": [2]}});
        assert_eq!(
            problems(&doc),
            [
                ".base: must be a non-empty string",
                ".cap: must be an integer of at least 1",
                ".check: must be an array of strings",
                ".environment.run: must be an array of strings",
            ]
        );
        assert_eq!(problems(&json!({})).len(), 4);
        assert_eq!(
            problems(&json!({"base": "", "cap": 0, "check": [], "environment": "x"})),
            [
                ".base: must be a non-empty string",
                ".cap: must be an integer of at least 1",
                ".environment.run: must be an array of strings",
            ]
        );
    }

    #[test]
    fn a_whole_cap_written_with_a_fraction_part_is_accepted() {
        let doc = json!({"base": "m", "cap": 60.0, "check": [], "environment": {"run": []}});
        assert_eq!(parse_baseline(&[doc]).map(|f| f.cap), Ok(60));
        let bad = json!({"base": "m", "cap": 1.5, "check": [], "environment": {"run": []}});
        assert_eq!(problems(&bad), [".cap: must be an integer of at least 1"]);
    }

    #[test]
    fn the_documents_of_a_file_are_counted() {
        assert_eq!(
            parse_baseline(&[]),
            Err(vec![Problem::new(
                ".",
                "must hold exactly one JSON document, found 0"
            )])
        );
        assert_eq!(
            parse_baseline(&[json!({}), json!({})]),
            Err(vec![Problem::new(
                ".",
                "must hold exactly one JSON document, found 2"
            )])
        );
        assert_eq!(
            parse_baseline(&[json!([1])]),
            Err(vec![Problem::new(".", "must be a JSON object")])
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
                environment: json!({ "run": run }),
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

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
