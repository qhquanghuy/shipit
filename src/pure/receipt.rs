use std::fmt;

use serde::Serialize;
use serde_json::Value;

use crate::pure::setup::Cap;

// The documents of a receipt file: more or fewer than one make it invalid, so all are read.
pub(crate) fn json_values(bytes: &[u8]) -> Option<Vec<Value>> {
    serde_json::Deserializer::from_slice(bytes)
        .into_iter::<Value>()
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

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

// The setup was changed by the slice itself, and nothing else was: the owner approves it by
// merging (docs/setup.md).
#[derive(Debug, Serialize)]
struct SetupChange {
    setup_change: bool,
    setup_fields: Vec<String>,
}

// The ref the slice was compared with (the full name) and the commit it was at.
#[derive(Debug)]
pub(crate) struct BaseUsed {
    pub(crate) name: String,
    pub(crate) commit: Oid,
    pub(crate) given: bool,
}

// Field order is the file's order (docs/store.md).
#[derive(Debug, Serialize)]
pub(crate) struct CheckReceipt {
    kind: &'static str,
    version: u8,
    key: Oid,
    commit: Oid,
    base_ref: String,
    base_commit: Oid,
    base_given: bool,
    size: ReceiptSize,
    commands: Vec<String>,
    #[serde(flatten)]
    setup_change: Option<SetupChange>,
    time: String,
    tool: String,
}

impl CheckReceipt {
    pub(crate) fn new(
        (tree, commit): (Oid, Oid),
        base: BaseUsed,
        (files, cap): (u64, Cap),
        commands: Vec<String>,
        setup_fields: Option<Vec<String>>,
        time: String,
    ) -> Self {
        Self {
            kind: "check",
            version: 1,
            key: tree,
            commit,
            base_ref: base.name,
            base_commit: base.commit,
            base_given: base.given,
            size: ReceiptSize { files, cap },
            commands,
            setup_change: setup_fields.map(|setup_fields| SetupChange {
                setup_change: true,
                setup_fields,
            }),
            time,
            tool: format!("shipit {}", env!("CARGO_PKG_VERSION")),
        }
    }
}

// What verify reads from a valid receipt: whether the slice changed the setup, and the ref
// `--base` named when the receipt says it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Valid {
    setup_change: bool,
    given_base: Option<String>,
}

// Valid only for exactly one value: an object with kind "check", version 1 and key equal to the
// tree. A receipt without `base_given`, or with one that is not a boolean, is an old receipt:
// the base was not given.
pub(crate) fn receipt_validity(docs: Option<&[Value]>, tree: &Oid) -> Option<Valid> {
    let Some([doc]) = docs else {
        return None;
    };
    let valid = doc.get("kind").and_then(Value::as_str) == Some("check")
        && doc.get("version").and_then(Value::as_f64) == Some(1.0)
        && doc.get("key").and_then(Value::as_str) == Some(tree.as_str());
    valid.then(|| Valid {
        setup_change: doc.get("setup_change") == Some(&Value::Bool(true)),
        given_base: if doc.get("base_given") == Some(&Value::Bool(true)) {
            doc.get("base_ref")
                .and_then(Value::as_str)
                .map(str::to_owned)
        } else {
            None
        },
    })
}

pub(crate) fn valid_line(short: &str, valid: &Valid) -> String {
    let mut notes: Vec<String> = Vec::new();
    if valid.setup_change {
        notes.push("setup change".to_owned());
    }
    if let Some(base_ref) = &valid.given_base {
        notes.push(format!("--base {base_ref}"));
    }
    if notes.is_empty() {
        format!("valid receipt for {short}")
    } else {
        format!("valid receipt for {short} ({})", notes.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::{cap, oid};

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
            prop_assert_eq!(receipt_validity(Some(&docs), &a), Some(plain()));
            prop_assert_eq!(receipt_validity(Some(&docs), &b), None);
        }

        #[test]
        fn a_kind_other_than_check_is_invalid(kind in "[a-z]{0,8}") {
            prop_assume!(kind != "check");
            let key = "a".repeat(40);
            let text = format!("{{\"kind\":\"{kind}\",\"version\":1,\"key\":\"{key}\"}}");
            prop_assert!(!valid(&parsed(&text), &oid('a')));
        }

        #[test]
        fn a_version_other_than_1_is_invalid(version in any::<i64>()) {
            prop_assume!(version != 1);
            let key = "a".repeat(40);
            let text = format!("{{\"kind\":\"check\",\"version\":{version},\"key\":\"{key}\"}}");
            prop_assert!(!valid(&parsed(&text), &oid('a')));
        }

        #[test]
        fn an_oid_is_exactly_40_or_64_hex_digits(
            text in prop_oneof!["[0-9a-fA-Fg ]{0,70}", "\\PC{0,80}"],
        ) {
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
        fn a_missing_base_given_reads_as_not_given(
            base_ref in "[a-z/]{1,12}",
            mark in proptest::sample::select(vec![
                None, Some("true"), Some("false"), Some("1"), Some("\"true\""), Some("null"),
            ]),
        ) {
            let key = "a".repeat(40);
            let given = mark.map_or_else(String::new, |mark| format!(",\"base_given\":{mark}"));
            let text = format!(
                "{{\"kind\":\"check\",\"version\":1,\"key\":\"{key}\",\"base_ref\":\"{base_ref}\"{given}}}"
            );
            let expected = (mark == Some("true")).then_some(base_ref);
            prop_assert_eq!(
                receipt_validity(Some(&parsed(&text)), &oid('a')),
                Some(Valid { setup_change: false, given_base: expected })
            );
        }
    }

    fn base(given: bool) -> BaseUsed {
        BaseUsed {
            name: "refs/heads/main".to_owned(),
            commit: oid('c'),
            given,
        }
    }

    fn plain() -> Valid {
        Valid {
            setup_change: false,
            given_base: None,
        }
    }

    fn receipt(tree: Oid, setup_fields: Option<Vec<String>>) -> CheckReceipt {
        receipt_given(tree, setup_fields, false)
    }

    fn receipt_given(tree: Oid, setup_fields: Option<Vec<String>>, given: bool) -> CheckReceipt {
        CheckReceipt::new(
            (tree, oid('b')),
            base(given),
            (3, cap(60)),
            vec!["true".to_owned()],
            setup_fields,
            "2026-11-21T10:00:00Z".to_owned(),
        )
    }

    fn receipt_for(tree: Oid) -> Value {
        let text = serde_json::to_string(&receipt(tree, None)).expect("serializes");
        serde_json::from_str(&text).expect("reads back")
    }

    fn parsed(text: &str) -> Vec<Value> {
        json_values(text.as_bytes()).expect("valid JSON")
    }

    fn valid(docs: &[Value], tree: &Oid) -> bool {
        receipt_validity(Some(docs), tree).is_some()
    }

    #[test]
    fn no_file_is_no_receipt() {
        assert_eq!(receipt_validity(None, &oid('a')), None);
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
            format!("{{\"kind\":\"check\",\"version\":\"1\",\"key\":\"{key}\"}}"),
            "{\"kind\":\"check\",\"version\":1}".to_owned(),
            format!("{{\"version\":1,\"key\":\"{key}\"}}"),
            format!(
                "{{\"kind\":\"check\",\"version\":1,\"key\":\"{key}\"}}{{\"kind\":\"check\",\"version\":1,\"key\":\"{key}\"}}"
            ),
        ];
        for text in &bad {
            assert!(!valid(&parsed(text), &oid('a')), "{text} must be invalid");
        }
    }

    #[test]
    fn a_version_written_as_1_point_0_is_valid_as_in_jq() {
        let key = "a".repeat(40);
        let text = format!("{{\"kind\":\"check\",\"version\":1.0,\"key\":\"{key}\"}}");
        assert!(valid(&parsed(&text), &oid('a')));
    }

    #[test]
    fn the_receipt_serializes_in_the_documented_order() {
        let (a, b, c) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
        let tool = env!("CARGO_PKG_VERSION");
        assert_eq!(
            serde_json::to_string(&receipt(oid('a'), None)).ok(),
            Some(format!(
                "{{\"kind\":\"check\",\"version\":1,\"key\":\"{a}\",\"commit\":\"{b}\",\
                 \"base_ref\":\"refs/heads/main\",\"base_commit\":\"{c}\",\"base_given\":false,\
                 \"size\":{{\"files\":3,\"cap\":60}},\"commands\":[\"true\"],\
                 \"time\":\"2026-11-21T10:00:00Z\",\"tool\":\"shipit {tool}\"}}"
            ))
        );
    }

    #[test]
    fn a_setup_change_is_marked_with_its_fields_and_verify_sees_the_mark() {
        let marked = receipt(oid('a'), Some(vec!["cap".to_owned(), "check".to_owned()]));
        let text = serde_json::to_string(&marked).expect("serializes");
        assert!(
            text.contains("\"commands\":[\"true\"],\"setup_change\":true,\"setup_fields\":[\"cap\",\"check\"],\"time\""),
            "{text}"
        );
        assert_eq!(
            receipt_validity(Some(&parsed(&text)), &oid('a')),
            Some(Valid {
                setup_change: true,
                given_base: None
            })
        );
    }

    #[test]
    fn a_receipt_made_with_base_reads_back_as_given() {
        let text = serde_json::to_string(&receipt_given(oid('a'), None, true)).expect("serializes");
        assert_eq!(
            receipt_validity(Some(&parsed(&text)), &oid('a')),
            Some(Valid {
                setup_change: false,
                given_base: Some("refs/heads/main".to_owned())
            })
        );
    }

    #[test]
    fn valid_line_forms() {
        let line = |setup_change, given: Option<&str>| {
            valid_line(
                "abc1234",
                &Valid {
                    setup_change,
                    given_base: given.map(str::to_owned),
                },
            )
        };
        assert_eq!(line(false, None), "valid receipt for abc1234");
        assert_eq!(line(true, None), "valid receipt for abc1234 (setup change)");
        assert_eq!(
            line(false, Some("refs/heads/main")),
            "valid receipt for abc1234 (--base refs/heads/main)"
        );
        assert_eq!(
            line(true, Some("refs/heads/main")),
            "valid receipt for abc1234 (setup change; --base refs/heads/main)"
        );
    }

    #[test]
    fn only_the_boolean_true_marks_a_setup_change() {
        let key = "a".repeat(40);
        for mark in ["false", "1", "\"true\"", "null"] {
            let text = format!(
                "{{\"kind\":\"check\",\"version\":1,\"key\":\"{key}\",\"setup_change\":{mark}}}"
            );
            assert_eq!(
                receipt_validity(Some(&parsed(&text)), &oid('a')),
                Some(plain()),
                "{mark}"
            );
        }
    }
}
