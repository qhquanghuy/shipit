use serde::Serialize;
use serde_json::Value;

use crate::pure::finding::{Finding, Outcome, ResultKind};
use crate::pure::rank::{Basis, Rank};
use crate::pure::roadmap::SliceName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiptKind {
    PlanReview,
}

impl ReceiptKind {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "plan-review" => Some(Self::PlanReview),
            _ => None,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::PlanReview => "plan-review",
        }
    }

    pub(crate) const fn result_kind(self) -> ResultKind {
        match self {
            Self::PlanReview => ResultKind::Review,
        }
    }
}

pub(crate) fn tool() -> String {
    format!("shipit {}", env!("CARGO_PKG_VERSION"))
}

// A file hash for a plan, the branch, commit, tree and merge base for work.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum Subject {
    File { file: String },
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn kinds_round_trip_and_the_time_is_read_from_one_document_only() {
        assert_eq!(
            ReceiptKind::parse("plan-review").map(ReceiptKind::name),
            Some("plan-review")
        );
        assert_eq!(ReceiptKind::parse("nope"), None);
        let one = [json!({"time": "2026-11-21T10:00:00Z"})];
        assert_eq!(
            receipt_time(Some(&one)),
            Some("2026-11-21T10:00:00Z".to_owned())
        );
        assert_eq!(receipt_time(Some(&[json!({}), json!({})])), None);
        assert_eq!(receipt_time(None), None);
    }
}
