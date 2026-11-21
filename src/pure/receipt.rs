use std::fmt;

use serde::Serialize;
use serde_json::Value;

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

// Valid only for exactly one value: an object with kind "check", version 1 and key equal to the
// tree.
pub(crate) fn receipt_valid(docs: Option<&[Value]>, tree: &Oid) -> bool {
    let Some([doc]) = docs else {
        return false;
    };
    doc.get("kind").and_then(Value::as_str) == Some("check")
        && doc.get("version").and_then(Value::as_f64) == Some(1.0)
        && doc.get("key").and_then(Value::as_str) == Some(tree.as_str())
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
        fn a_receipt_is_valid_for_its_key_and_for_no_other(
            a in "[0-9a-f]{40}",
            b in "[0-9a-f]{40}",
        ) {
            prop_assume!(a != b);
            let (a, b) = (Oid::parse(&a).expect("hex"), Oid::parse(&b).expect("hex"));
            let docs = [receipt_for(a.clone())];
            prop_assert!(receipt_valid(Some(&docs), &a));
            prop_assert!(!receipt_valid(Some(&docs), &b));
        }

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

    fn oid_of(c: char) -> Oid {
        Oid::parse(&c.to_string().repeat(40)).expect("40 hex digits")
    }

    fn receipt_for(tree: Oid) -> Value {
        let setup = include_str!("../../tests/fixtures/setup-valid.json");
        let cap = crate::pure::setup::json_values(setup.as_bytes())
            .ok()
            .and_then(|docs| crate::pure::setup::parse_setup(&docs).ok())
            .map(|setup| setup.cap)
            .expect("the fixture parses");
        let receipt = CheckReceipt::new(
            tree,
            oid_of('b'),
            (3, cap),
            vec![],
            "2026-11-21T10:00:00Z".to_owned(),
            "shipit 0.1.0".to_owned(),
        );
        let text = serde_json::to_string(&receipt).expect("serializes");
        serde_json::from_str(&text).expect("reads back")
    }

    fn parsed(text: &str) -> Vec<Value> {
        crate::pure::setup::json_values(text.as_bytes()).expect("valid JSON")
    }

    #[test]
    fn a_receipt_is_valid_for_its_own_tree() {
        let docs = [receipt_for(oid_of('a'))];
        assert!(receipt_valid(Some(&docs), &oid_of('a')));
        assert!(!receipt_valid(Some(&docs), &oid_of('c')));
    }

    #[test]
    fn no_file_is_no_receipt() {
        assert!(!receipt_valid(None, &oid_of('a')));
    }

    #[test]
    fn receipts_of_the_wrong_shape_are_invalid() {
        let key = "a".repeat(40);
        let bad = [
            "[]".to_owned(),
            "null".to_owned(),
            "\"text\"".to_owned(),
            "42".to_owned(),
            String::new(),
            format!("{{\"kind\":\"review\",\"version\":1,\"key\":\"{key}\"}}"),
            format!("{{\"kind\":\"check\",\"version\":2,\"key\":\"{key}\"}}"),
            format!("{{\"kind\":\"check\",\"version\":\"1\",\"key\":\"{key}\"}}"),
            "{\"kind\":\"check\",\"version\":1}".to_owned(),
            format!("{{\"version\":1,\"key\":\"{key}\"}}"),
            format!(
                "{{\"kind\":\"check\",\"version\":1,\"key\":\"{key}\"}}{{\"kind\":\"check\",\"version\":1,\"key\":\"{key}\"}}"
            ),
        ];
        for text in &bad {
            assert!(
                !receipt_valid(Some(&parsed(text)), &oid_of('a')),
                "{text} must be invalid"
            );
        }
    }

    #[test]
    fn a_version_written_as_1_point_0_is_valid_as_in_jq() {
        let key = "a".repeat(40);
        let text = format!("{{\"kind\":\"check\",\"version\":1.0,\"key\":\"{key}\"}}");
        assert!(receipt_valid(Some(&parsed(&text)), &oid_of('a')));
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
