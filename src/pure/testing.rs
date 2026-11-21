#![cfg(test)]

use std::ffi::OsString;

use crate::pure::base::BranchName;
use crate::pure::receipt::Oid;
use toml::{Table, Value};

use crate::pure::setup::{Cap, Setup, parse_setup, toml_table};

pub(crate) fn oid(c: char) -> Oid {
    Oid::parse(&c.to_string().repeat(40)).expect("40 hex digits")
}

pub(crate) fn name(text: &str) -> BranchName {
    BranchName::parse(text).expect("a branch name")
}

pub(crate) fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

const FIXTURE: &str = include_str!("../../tests/fixtures/setup-valid.toml");

fn set(table: &mut Table, path: &str, value: Value) {
    match path.split_once('.') {
        Some((head, rest)) => match table.get_mut(head) {
            Some(Value::Table(inner)) => set(inner, rest, value),
            Some(_) | None => panic!("no table at {head} in the fixture setup"),
        },
        None => {
            table.insert(path.to_owned(), value);
        }
    }
}

// The valid fixture setup with some keys replaced, each named by its dotted path through tables
// (`gate.max_files`); a key inside an array of tables cannot be named.
pub(crate) fn setup_with(edits: &[(&str, Value)]) -> Setup {
    let mut table = toml_table(FIXTURE.as_bytes()).expect("the fixture is TOML");
    for (path, value) in edits {
        set(&mut table, path, value.clone());
    }
    parse_setup(&table).expect("the edited fixture is a valid setup")
}

pub(crate) fn cap(n: u64) -> Cap {
    let n = i64::try_from(n).expect("a small cap");
    setup_with(&[("gate.max_files", Value::Integer(n))])
        .gate
        .max_files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_path_replaces_one_value_and_leaves_the_rest() {
        let plain = setup_with(&[]);
        let edited = setup_with(&[
            ("gate.max_files", Value::Integer(7)),
            ("agents.max_fix_rounds", Value::Integer(5)),
        ]);
        assert_eq!(edited.gate.max_files.get(), 7);
        assert_eq!(edited.gate.base, plain.gate.base);
        assert_eq!(edited.gate.check, plain.gate.check);
        assert_ne!(edited.agents.max_fix_rounds, plain.agents.max_fix_rounds);
        assert_eq!(cap(9).get(), 9);
    }
}
