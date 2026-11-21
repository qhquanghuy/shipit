use serde::Serialize;
use serde_json::Value;

use crate::pure::finding::{Finding, Outcome, ResultKind};
use crate::pure::rank::{Basis, Rank};
use crate::pure::receipt::Oid;
use crate::pure::roadmap::SliceName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiptKind {
    PlanReview,
    InformedReview,
    SecondOpinion,
    TestDrive,
}

impl ReceiptKind {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "plan-review" => Some(Self::PlanReview),
            "informed-review" => Some(Self::InformedReview),
            "second-opinion" => Some(Self::SecondOpinion),
            "test-drive" => Some(Self::TestDrive),
            _ => None,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::PlanReview => "plan-review",
            Self::InformedReview => "informed-review",
            Self::SecondOpinion => "second-opinion",
            Self::TestDrive => "test-drive",
        }
    }

    pub(crate) const fn result_kind(self) -> ResultKind {
        match self {
            Self::PlanReview | Self::InformedReview | Self::SecondOpinion => ResultKind::Review,
            Self::TestDrive => ResultKind::TestDrive,
        }
    }
}

pub(crate) fn tool() -> String {
    format!("shipit {}", env!("CARGO_PKG_VERSION"))
}

// A file for a plan; the branch, commit, tree and merge base for work.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum Subject {
    File {
        file: String,
    },
    Work {
        branch: String,
        commit: String,
        tree: String,
        merge_base: String,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct Receipt<'a> {
    pub(crate) kind: &'static str,
    pub(crate) version: u8,
    pub(crate) key: String,
    pub(crate) slice: Option<&'a SliceName>,
    pub(crate) subject: Subject,
    pub(crate) author: &'a Rank,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) author_is_maker: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) author_is_planner: Option<bool>,
    pub(crate) by: &'a Rank,
    pub(crate) rank: Basis,
    pub(crate) result: Outcome,
    pub(crate) findings: &'a [Finding],
    pub(crate) time: String,
    pub(crate) tool: String,
}

// The time of a receipt file that already exists, for the refusal that names it.
pub(crate) fn receipt_time(docs: Option<&[Value]>) -> Option<String> {
    let Some([doc]) = docs else {
        return None;
    };
    doc.get("time").and_then(Value::as_str).map(str::to_owned)
}

// What a check receipt says it was compared with: the ref's short name and the commit.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CheckBase {
    pub(crate) name: String,
    pub(crate) commit: Oid,
}

pub(crate) fn check_base(doc: &Value) -> Option<CheckBase> {
    let full = doc.get("base_ref")?.as_str()?;
    let name = full
        .strip_prefix("refs/heads/")
        .or_else(|| full.strip_prefix("refs/remotes/"))?;
    Some(CheckBase {
        name: name.to_owned(),
        commit: Oid::parse(doc.get("base_commit")?.as_str()?)?,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    const KINDS: [ReceiptKind; 4] = [
        ReceiptKind::PlanReview,
        ReceiptKind::InformedReview,
        ReceiptKind::SecondOpinion,
        ReceiptKind::TestDrive,
    ];

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_kind_parses_back_from_its_name(index in 0..KINDS.len(), junk in "[a-z-]{0,12}") {
            let kind = KINDS.get(index).copied().expect("a kind");
            prop_assert_eq!(ReceiptKind::parse(kind.name()), Some(kind));
            let known = KINDS.iter().any(|kind| kind.name() == junk);
            prop_assert_eq!(ReceiptKind::parse(&junk).is_some(), known);
        }

        #[test]
        fn the_base_of_a_check_receipt_is_the_short_ref_and_the_commit(
            name in "[a-z][a-z0-9/-]{0,10}",
            origin in any::<bool>(),
        ) {
            let full = if origin {
                format!("refs/remotes/{name}")
            } else {
                format!("refs/heads/{name}")
            };
            let commit = "a".repeat(40);
            let doc = json!({"base_ref": full, "base_commit": commit});
            prop_assert_eq!(
                check_base(&doc),
                Some(CheckBase { name, commit: Oid::parse(&commit).expect("an oid") })
            );
            prop_assert_eq!(check_base(&json!({"base_ref": full})), None);
            prop_assert_eq!(check_base(&json!({"base_ref": "main", "base_commit": commit})), None);
        }
    }

    #[test]
    fn the_time_is_read_from_one_document_only() {
        let one = [json!({"time": "2026-11-21T10:00:00Z"})];
        assert_eq!(
            receipt_time(Some(&one)),
            Some("2026-11-21T10:00:00Z".to_owned())
        );
        assert_eq!(receipt_time(Some(&[json!({}), json!({})])), None);
        assert_eq!(receipt_time(None), None);
    }
}
