use std::collections::BTreeMap;

use serde::Serialize;
use toml::{Table, Value};

use crate::pure::goal::{DoneWhen, parse_done_when};
use crate::pure::rules::{Parsed, Problem, Problems, fail, non_empty};

// An empty map is valid: the change meets no done-when item.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct CoverageMap {
    pub(crate) items: BTreeMap<DoneWhen, Vec<String>>,
}

// A test name never carries a done-when id: ids live in the plan, not in the code.
fn carries_done_when_id(name: &str) -> bool {
    name.to_ascii_lowercase()
        .split("dw")
        .skip(1)
        .any(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

fn parse_names(value: &Value, path: &str) -> Parsed<Vec<String>> {
    let Some(items) = value.as_array() else {
        return fail(path, "must be an array of non-empty strings");
    };
    if items.is_empty() {
        return fail(path, "must list at least one test name");
    }
    let mut problems = Vec::new();
    let mut found: Vec<String> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let here = format!("{path}[{index}]");
        match non_empty(item) {
            None => problems.push(Problem::new(here, "must be a non-empty string")),
            Some(name) if carries_done_when_id(name) => problems.push(Problem::new(
                here,
                "a test name must not carry a done-when id",
            )),
            Some(name) if found.iter().any(|seen| seen == name) => {
                problems.push(Problem::new(here, format!("\"{name}\" is listed twice")));
            }
            Some(name) => found.push(name.to_owned()),
        }
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

pub(crate) fn parse_map(table: &Table) -> Parsed<CoverageMap> {
    let mut problems = Problems::default();
    let mut items = BTreeMap::new();
    for (key, value) in table {
        let path = format!(".{key}");
        match parse_done_when(key) {
            None => problems.push(Problem::new(path, "must be a done-when id such as \"DW4\"")),
            Some(id) => {
                if let Some(names) = problems.take(parse_names(value, &path)) {
                    items.insert(id, names);
                }
            }
        }
    }
    problems.finish(|| Some(CoverageMap { items }))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::setup::toml_table;

    fn names() -> impl Strategy<Value = Vec<String>> {
        proptest::collection::btree_set("[a-z][a-z :,_-]{0,12}", 1..4)
            .prop_map(|set| set.into_iter().collect::<Vec<_>>())
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn any_valid_map_parses_back_to_its_items(
            items in proptest::collection::btree_map(1_u64..40, names(), 0..6),
        ) {
            let mut table = Table::new();
            for (n, tests) in &items {
                table.insert(
                    format!("DW{n}"),
                    Value::Array(tests.iter().map(|name| Value::String(name.clone())).collect()),
                );
            }
            let text = toml::to_string(&table).expect("a table prints");
            let parsed = parse_map(&toml_table(text.as_bytes()).expect("it prints valid toml"))
                .expect("a valid map parses");
            let wanted: BTreeMap<DoneWhen, Vec<String>> = items
                .iter()
                .map(|(n, tests)| (parse_done_when(&format!("DW{n}")).expect("an id"), tests.clone()))
                .collect();
            prop_assert_eq!(parsed.items, wanted);
        }

        #[test]
        fn a_name_with_a_done_when_id_is_refused(
            before in "[a-z ]{0,5}",
            case in prop::sample::select(vec!["dw", "DW", "Dw", "dW"]),
            digits in 0_u64..1000,
            after in "[a-z ]{0,5}",
        ) {
            let bad = format!("{before}{case}{digits}{after}");
            let mut table = Table::new();
            table.insert(
                "DW4".to_owned(),
                Value::Array(vec![Value::String("fine".to_owned()), Value::String(bad)]),
            );
            let problems = parse_map(&table).expect_err("an id in a name is refused");
            prop_assert_eq!(
                problems.iter().map(ToString::to_string).collect::<Vec<_>>(),
                vec![".DW4[1]: a test name must not carry a done-when id".to_owned()]
            );
            let mut fine = Table::new();
            fine.insert(
                "DW4".to_owned(),
                Value::Array(vec![Value::String(format!("{before}dwarf{after}"))]),
            );
            prop_assert!(parse_map(&fine).is_ok());
        }
    }

    #[test]
    fn the_problems_of_a_broken_map() {
        let table = toml_table(
            b"DW4 = []\nDW5 = [\"a\", \"a\", \"\", 3]\nDW6 = \"x\"\nDW07 = [\"a\"]\nother = [\"a\"]\n",
        )
        .expect("toml");
        let problems = parse_map(&table).expect_err("broken");
        assert_eq!(
            problems.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [
                ".DW4: must list at least one test name",
                ".DW5[1]: \"a\" is listed twice",
                ".DW5[2]: must be a non-empty string",
                ".DW5[3]: must be a non-empty string",
                ".DW6: must be an array of non-empty strings",
                ".DW07: must be a done-when id such as \"DW4\"",
                ".other: must be a done-when id such as \"DW4\"",
            ]
        );
    }
}
