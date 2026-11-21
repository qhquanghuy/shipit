#![cfg(test)]

use std::ffi::OsString;

use crate::pure::base::BranchName;
use crate::pure::receipt::Oid;
use crate::pure::setup::{Cap, json_values, parse_setup};

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
    let text = include_str!("../../tests/fixtures/setup-valid.json")
        .replace("\"cap\": 60", &format!("\"cap\": {n}"));
    json_values(text.as_bytes())
        .ok()
        .and_then(|docs| parse_setup(&docs).ok())
        .map(|setup| setup.cap)
        .expect("the fixture parses")
}
