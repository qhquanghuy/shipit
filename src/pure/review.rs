use serde::Serialize;
use serde_json::Value;

use crate::pure::finding::{Class, Finding, Outcome, ResultKind};
use crate::pure::rank::{Basis, Rank, Worker, rank_verdict};
use crate::pure::receipt::Oid;
use crate::pure::roadmap::SliceName;
use crate::pure::setup::{Effort, Role, Setup};

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
#[derive(Debug, Clone, Serialize)]
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

// A finding as a receipt file keeps it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) id: String,
    pub(crate) is_question: bool,
    pub(crate) class: Class,
    pub(crate) text: String,
}

fn parse_class(text: &str) -> Option<Class> {
    match text {
        "must fix" => Some(Class::MustFix),
        "must answer" => Some(Class::MustAnswer),
        "nice to fix" => Some(Class::NiceToFix),
        _ => None,
    }
}

pub(crate) fn stored_findings(doc: &Value) -> Option<Vec<Stored>> {
    doc.get("findings")?
        .as_array()?
        .iter()
        .map(|finding| {
            let text_of = |name: &str| finding.get(name)?.as_str();
            Some(Stored {
                id: text_of("id")?.to_owned(),
                is_question: text_of("label")? == "question",
                class: parse_class(text_of("class")?)?,
                text: text_of("text")?.to_owned(),
            })
        })
        .collect()
}

// What status reads from a receipt file.
#[derive(Debug)]
pub(crate) struct ReceiptView {
    pub(crate) kind: ReceiptKind,
    pub(crate) key: String,
    pub(crate) result: Outcome,
    pub(crate) author: Worker,
    pub(crate) by: Worker,
    pub(crate) fresh: bool,
    // The role the receipt says its `--author` is not: a claim that status shows.
    pub(crate) not_the_role: Option<&'static str>,
    pub(crate) findings: Vec<Stored>,
}

// The setup's role that a reviewer of this kind must outrank, besides the author.
pub(crate) fn role_of(policy: &Setup, kind: ReceiptKind) -> (&'static str, &Role) {
    match kind {
        ReceiptKind::PlanReview => ("planner", &policy.agents.roles.planner),
        ReceiptKind::InformedReview | ReceiptKind::SecondOpinion | ReceiptKind::TestDrive => {
            ("maker", &policy.agents.roles.maker)
        }
    }
}

fn worker_of(value: &Value) -> Option<Worker> {
    let tier = value.get("tier")?.as_u64()?;
    Some(Worker {
        model: value.get("model")?.as_str()?.to_owned(),
        effort: Effort::parse(value.get("effort")?.as_str()?)?,
        stands_in: value.get("stand_in")?.as_bool()?.then_some(tier),
    })
}

pub(crate) fn read_receipt(docs: Option<&[Value]>) -> Option<ReceiptView> {
    let Some([doc]) = docs else {
        return None;
    };
    let flag = |name: &str| doc.get(name).and_then(Value::as_bool) == Some(false);
    let not_the_role = if flag("author_is_maker") {
        Some("maker")
    } else if flag("author_is_planner") {
        Some("planner")
    } else {
        None
    };
    Some(ReceiptView {
        kind: ReceiptKind::parse(doc.get("kind")?.as_str()?)?,
        key: doc.get("key")?.as_str()?.to_owned(),
        result: Outcome::parse(doc.get("result")?.as_str()?)?,
        author: worker_of(doc.get("author")?)?,
        by: worker_of(doc.get("by")?)?,
        fresh: doc.get("rank")?.as_str()? == "fresh",
        not_the_role,
        findings: stored_findings(doc)?,
    })
}

impl ReceiptView {
    pub(crate) fn is_rework(&self) -> bool {
        self.result == Outcome::Rework
    }
}

// The findings that no known issue or answer has closed.
pub(crate) fn open_findings<'a>(view: &'a ReceiptView, closed: &[String]) -> Vec<&'a Stored> {
    view.findings
        .iter()
        .filter(|finding| !closed.contains(&finding.id))
        .collect()
}

// A receipt counts for a version when it is that version's, its ranks still hold under the
// current setup, it is not a rework, and every blocking finding is closed.
pub(crate) fn counts(view: &ReceiptView, policy: &Setup, key: &str, closed: &[String]) -> bool {
    view.key == key
        && !view.is_rework()
        && rank_verdict(
            policy,
            &view.author,
            &view.by,
            view.fresh,
            role_of(policy, view.kind),
        )
        .is_ok()
        && open_findings(view, closed)
            .iter()
            .all(|finding| finding.class == Class::NiceToFix)
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
    use crate::pure::finding::parse_finding;

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
        fn a_stored_finding_reads_back_what_a_receipt_wrote(
            body in "[a-z][a-z ]{0,15}[a-z]",
            which in 0..4_usize,
        ) {
            let leads = ["issue (blocking)", "suggestion (non-blocking)", "nitpick", "question"];
            let lead = leads.get(which).copied().expect("a lead");
            let finding = parse_finding(0, &format!("{lead}: {body}")).expect("a finding");
            let doc = json!({"findings": [serde_json::to_value(&finding).expect("json")]});
            prop_assert_eq!(
                stored_findings(&doc),
                Some(vec![Stored {
                    id: "F1".to_owned(),
                    is_question: which == 3,
                    class: finding.class,
                    text: body,
                }])
            );
            prop_assert_eq!(stored_findings(&json!({"findings": [{"id": "F1"}]})), None);
        }

        #[test]
        fn a_receipt_counts_iff_its_rank_holds_and_its_blocking_findings_are_closed(
            author in 0..3_usize,
            by in 0..4_usize,
            result in 0..3_usize,
            classes in proptest::collection::vec(0..3_usize, 0..5),
            closed in proptest::collection::vec(any::<bool>(), 5),
            same_key in any::<bool>(),
        ) {
            let models = ["haiku", "sonnet", "opus", "fable"];
            let worker = |tier: usize| Worker {
                model: models.get(tier).copied().expect("a model").to_owned(),
                effort: Effort::High,
                stands_in: None,
            };
            let all = [Class::MustFix, Class::MustAnswer, Class::NiceToFix];
            let results = [Outcome::Approve, Outcome::ApproveAfterFixes, Outcome::Rework];
            let findings: Vec<Stored> = classes
                .iter()
                .enumerate()
                .map(|(index, class)| Stored {
                    id: format!("F{index}"),
                    is_question: false,
                    class: all.get(*class).copied().expect("a class"),
                    text: "t".to_owned(),
                })
                .collect();
            let closed_ids: Vec<String> = findings
                .iter()
                .zip(&closed)
                .filter(|(_, closed)| **closed)
                .map(|(finding, _)| finding.id.clone())
                .collect();
            let blocking_closed = findings
                .iter()
                .all(|finding| finding.class == Class::NiceToFix || closed_ids.contains(&finding.id));
            let outcome = results.get(result).copied().expect("a result");
            let view = ReceiptView {
                kind: ReceiptKind::InformedReview,
                key: "k".to_owned(),
                result: outcome,
                author: worker(author),
                by: worker(by),
                fresh: false,
                not_the_role: None,
                findings,
            };
            let policy = crate::pure::testing::setup_with(&[]);
            // The maker is sonnet (index 1) at high effort, so a reviewer must be above it too.
            let expected = by > author && by > 1 && outcome != Outcome::Rework && blocking_closed && same_key;
            let key = if same_key { "k" } else { "other" };
            prop_assert_eq!(counts(&view, &policy, key, &closed_ids), expected);
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
