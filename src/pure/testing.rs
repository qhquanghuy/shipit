#![cfg(test)]

use std::ffi::OsString;

use crate::pure::base::BranchName;
use crate::pure::receipt::Oid;
use crate::pure::setup::{Cap, parse_setup, toml_table};

pub(crate) fn oid(c: char) -> Oid {
    Oid::parse(&c.to_string().repeat(40)).expect("40 hex digits")
}

pub(crate) fn name(text: &str) -> BranchName {
    BranchName::parse(text).expect("a branch name")
}

pub(crate) fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

pub(crate) fn cap(n: u64) -> Cap {
    let text = include_str!("../../tests/fixtures/setup-valid.toml")
        .replace("max_files = 60", &format!("max_files = {n}"));
    toml_table(text.as_bytes())
        .ok()
        .and_then(|table| parse_setup(&table).ok())
        .map(|setup| setup.gate.max_files)
        .expect("the fixture parses")
}
