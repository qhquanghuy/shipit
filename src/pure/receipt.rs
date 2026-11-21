use std::fmt;

use serde::Serialize;

use crate::pure::setup::Cap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct Oid(String);

impl Oid {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let hex = text.bytes().all(|byte| byte.is_ascii_hexdigit());
        (hex && matches!(text.len(), 40 | 64)).then(|| Self(text.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Serialize)]
struct ReceiptSize {
    files: u64,
    cap: Cap,
}

// Field order is the file's order (docs/store.md).
#[derive(Debug, Serialize)]
pub(crate) struct CheckReceipt {
    kind: &'static str,
    version: u8,
    key: Oid,
    commit: Oid,
    size: ReceiptSize,
    commands: Vec<String>,
    time: String,
    tool: String,
}

impl CheckReceipt {
    pub(crate) fn new(
        tree: Oid,
        commit: Oid,
        (files, cap): (u64, Cap),
        commands: Vec<String>,
        time: String,
        tool: String,
    ) -> Self {
        Self {
            kind: "check",
            version: 1,
            key: tree,
            commit,
            size: ReceiptSize { files, cap },
            commands,
            time,
            tool,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn an_oid_is_exactly_40_or_64_hex_digits(text in "[0-9a-fA-Fg ]{0,70}") {
            let expected = matches!(text.len(), 40 | 64)
                && text.bytes().all(|byte| byte.is_ascii_hexdigit());
            prop_assert_eq!(Oid::parse(&text).is_some(), expected);
        }

        #[test]
        fn any_40_or_64_hex_digits_are_an_oid(
            text in prop_oneof!["[0-9a-f]{40}", "[0-9a-f]{64}"],
        ) {
            prop_assert_eq!(Oid::parse(&text).map(|oid| oid.to_string()), Some(text));
        }

        #[test]
        fn any_other_text_is_not_an_oid(text in "\\PC{0,80}") {
            prop_assume!(!(matches!(text.len(), 40 | 64)
                && text.bytes().all(|byte| byte.is_ascii_hexdigit())));
            prop_assert_eq!(Oid::parse(&text), None);
        }
    }

    #[test]
    fn the_receipt_serializes_in_the_documented_order() {
        let oid = |c: char| Oid::parse(&c.to_string().repeat(40)).expect("40 hex digits");
        let setup = include_str!("../../tests/fixtures/setup-valid.json");
        let cap = crate::pure::setup::json_values(setup.as_bytes())
            .ok()
            .and_then(|docs| crate::pure::setup::parse_setup(&docs).ok())
            .map(|setup| setup.cap)
            .expect("the fixture parses");
        let receipt = CheckReceipt::new(
            oid('a'),
            oid('b'),
            (3, cap),
            vec!["true".to_owned()],
            "2026-11-21T10:00:00Z".to_owned(),
            "shipit 0.1.0".to_owned(),
        );
        let (a, b) = ("a".repeat(40), "b".repeat(40));
        assert_eq!(
            serde_json::to_string(&receipt).ok(),
            Some(format!(
                "{{\"kind\":\"check\",\"version\":1,\"key\":\"{a}\",\"commit\":\"{b}\",\
                 \"size\":{{\"files\":3,\"cap\":60}},\"commands\":[\"true\"],\
                 \"time\":\"2026-11-21T10:00:00Z\",\"tool\":\"shipit 0.1.0\"}}"
            ))
        );
    }
}
