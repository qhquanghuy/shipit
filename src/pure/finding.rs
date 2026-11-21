use std::fmt;

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Label {
    Issue,
    Suggestion,
    Question,
    Nitpick,
}

impl Label {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "issue" => Some(Self::Issue),
            "suggestion" => Some(Self::Suggestion),
            "question" => Some(Self::Question),
            "nitpick" => Some(Self::Nitpick),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    MustFix,
    MustAnswer,
    NiceToFix,
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MustFix => "must fix",
            Self::MustAnswer => "must answer",
            Self::NiceToFix => "nice to fix",
        })
    }
}

impl Serialize for Class {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Finding {
    pub(crate) id: String,
    pub(crate) label: Label,
    pub(crate) class: Class,
    pub(crate) perspectives: Vec<String>,
    pub(crate) text: String,
}

fn is_perspective(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c == '-')
}

// The label is the leading lowercase word; what follows is ": " or " (decorations): ".
fn split_finding(text: &str) -> Option<(Label, Option<&str>, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_lowercase())
        .unwrap_or(text.len());
    let (word, rest) = text.split_at_checked(end)?;
    let label = Label::parse(word)?;
    if let Some(body) = rest.strip_prefix(": ") {
        return Some((label, None, body));
    }
    let (decorations, body) = rest.strip_prefix(" (")?.split_once("): ")?;
    Some((label, Some(decorations), body))
}

// Decorations are blocking, non-blocking, or a perspective; the first two decide the class.
fn class_of(label: Label, blocking: usize, non_blocking: usize) -> Result<Class, &'static str> {
    match (label, blocking, non_blocking) {
        (Label::Issue | Label::Suggestion, 1, 0) => Ok(Class::MustFix),
        (Label::Issue | Label::Suggestion, 0, 1) | (Label::Nitpick, 0, _) => Ok(Class::NiceToFix),
        (Label::Issue | Label::Suggestion, _, _) => {
            Err("issue and suggestion need (blocking) or (non-blocking)")
        }
        (Label::Nitpick, _, _) => Err("a nitpick is never blocking"),
        (Label::Question, 0, 0) => Ok(Class::MustAnswer),
        (Label::Question, _, _) => Err("a question is neither blocking nor non-blocking"),
    }
}

// `index` counts from 0; the id is F<index + 1>.
pub(crate) fn parse_finding(index: usize, text: &str) -> Result<Finding, String> {
    let refuse = |why: &str| format!("finding \"{text}\": {why}");
    let (label, decorations, body) = split_finding(text).ok_or_else(|| {
        refuse("must start with issue, suggestion, question or nitpick, then \": \"")
    })?;
    let mut blocking = 0;
    let mut non_blocking = 0;
    let mut perspectives = Vec::new();
    for word in decorations.into_iter().flat_map(|all| all.split(',')) {
        let word = word.trim();
        match word {
            "blocking" => blocking += 1,
            "non-blocking" => non_blocking += 1,
            _ if is_perspective(word) => perspectives.push(word.to_owned()),
            _ => return Err(refuse(&format!("\"{word}\" is not a decoration"))),
        }
    }
    let class = class_of(label, blocking, non_blocking).map_err(refuse)?;
    let body = body.trim();
    if body.is_empty() {
        return Err(refuse("the text after \": \" is empty"));
    }
    Ok(Finding {
        id: format!("F{}", index + 1),
        label,
        class,
        perspectives,
        text: body.to_owned(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResultKind {
    Review,
    TestDrive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Approve,
    ApproveAfterFixes,
    Rework,
    Pass,
    Findings,
}

impl Outcome {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "approve" => Some(Self::Approve),
            "approve-after-fixes" => Some(Self::ApproveAfterFixes),
            "rework" => Some(Self::Rework),
            "pass" => Some(Self::Pass),
            "findings" => Some(Self::Findings),
            _ => None,
        }
    }

    const fn kind(self) -> ResultKind {
        match self {
            Self::Approve | Self::ApproveAfterFixes | Self::Rework => ResultKind::Review,
            Self::Pass | Self::Findings => ResultKind::TestDrive,
        }
    }

    const fn needs_must_fix(self) -> bool {
        match self {
            Self::ApproveAfterFixes | Self::Rework | Self::Findings => true,
            Self::Approve | Self::Pass => false,
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Approve => "approve",
            Self::ApproveAfterFixes => "approve-after-fixes",
            Self::Rework => "rework",
            Self::Pass => "pass",
            Self::Findings => "findings",
        })
    }
}

impl Serialize for Outcome {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

// Must-answer findings go with any result, and nice-to-fix findings never decide one.
pub(crate) fn outcome_problems(
    kind: ResultKind,
    outcome: Outcome,
    findings: &[Finding],
) -> Vec<String> {
    if outcome.kind() != kind {
        return vec![match kind {
            ResultKind::Review => format!("{outcome} is a test-drive result"),
            ResultKind::TestDrive => format!("{outcome} is a review result"),
        }];
    }
    let must_fix = findings
        .iter()
        .any(|finding| finding.class == Class::MustFix);
    match (outcome.needs_must_fix(), must_fix) {
        (true, false) => vec![format!("{outcome} needs a must-fix finding")],
        (false, true) => vec![format!("{outcome} allows no must-fix finding")],
        (true, true) | (false, false) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn finding(text: &str) -> Finding {
        parse_finding(0, text).expect("a finding")
    }

    const LABELS: [(&str, Label); 4] = [
        ("issue", Label::Issue),
        ("suggestion", Label::Suggestion),
        ("question", Label::Question),
        ("nitpick", Label::Nitpick),
    ];

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn labels_and_decorations_give_their_class(
            which in 0_usize..4,
            blocking in any::<bool>(),
            non_blocking in any::<bool>(),
            perspectives in proptest::collection::vec("[a-z][a-z-]{0,6}", 0..3),
            body in "[A-Za-z0-9][A-Za-z0-9 ,:]{0,12}",
            index in 0_usize..50,
        ) {
            prop_assume!(perspectives.iter().all(|word| word != "blocking" && word != "non-blocking"));
            let (word, label) = LABELS[which];
            let mut decorations: Vec<String> = Vec::new();
            if blocking {
                decorations.push("blocking".to_owned());
            }
            if non_blocking {
                decorations.push("non-blocking".to_owned());
            }
            decorations.extend(perspectives.iter().cloned());
            let text = if decorations.is_empty() {
                format!("{word}: {body}")
            } else {
                format!("{word} ({}): {body}", decorations.join(", "))
            };
            let expected = match (label, blocking, non_blocking) {
                (Label::Issue | Label::Suggestion, true, false) => Ok(Class::MustFix),
                (Label::Issue | Label::Suggestion, false, true) | (Label::Nitpick, false, _) => {
                    Ok(Class::NiceToFix)
                }
                (Label::Issue | Label::Suggestion, _, _) => {
                    Err("issue and suggestion need (blocking) or (non-blocking)")
                }
                (Label::Nitpick, true, _) => Err("a nitpick is never blocking"),
                (Label::Question, false, false) => Ok(Class::MustAnswer),
                (Label::Question, _, _) => Err("a question is neither blocking nor non-blocking"),
            };
            match (parse_finding(index, &text), expected) {
                (Ok(found), Ok(class)) => prop_assert_eq!(
                    found,
                    Finding {
                        id: format!("F{}", index + 1),
                        label,
                        class,
                        perspectives,
                        text: body.trim().to_owned(),
                    }
                ),
                (Err(message), Err(why)) => {
                    prop_assert_eq!(message, format!("finding \"{text}\": {why}"));
                }
                (Ok(_), Err(_)) | (Err(_), Ok(_)) => {
                    let shown = format!("the outcome differs for {text}");
                    prop_assert!(false, "{}", shown);
                }
            }
        }

        #[test]
        fn outcomes_follow_the_blocking_rule(
            picks in proptest::collection::vec(0_usize..4, 0..5),
            kind in prop::sample::select(vec![ResultKind::Review, ResultKind::TestDrive]),
            outcome in prop::sample::select(vec![
                Outcome::Approve,
                Outcome::ApproveAfterFixes,
                Outcome::Rework,
                Outcome::Pass,
                Outcome::Findings,
            ]),
        ) {
            let texts = ["issue (blocking): a", "issue (non-blocking): b", "question: c", "nitpick: d"];
            let findings: Vec<Finding> = picks.iter().map(|pick| finding(texts[*pick])).collect();
            let must_fix = picks.contains(&0);
            let right_kind = matches!(
                (kind, outcome),
                (ResultKind::Review, Outcome::Approve | Outcome::ApproveAfterFixes | Outcome::Rework)
                    | (ResultKind::TestDrive, Outcome::Pass | Outcome::Findings)
            );
            let fixes_needed = matches!(
                outcome,
                Outcome::ApproveAfterFixes | Outcome::Rework | Outcome::Findings
            );
            let wanted: Vec<String> = if !right_kind {
                let other = if kind == ResultKind::Review { "test-drive" } else { "review" };
                vec![format!("{outcome} is a {other} result")]
            } else if fixes_needed && !must_fix {
                vec![format!("{outcome} needs a must-fix finding")]
            } else if !fixes_needed && must_fix {
                vec![format!("{outcome} allows no must-fix finding")]
            } else {
                Vec::new()
            };
            prop_assert_eq!(outcome_problems(kind, outcome, &findings), wanted);
        }

        #[test]
        fn outcomes_print_and_parse_as_their_machine_spelling(
            outcome in prop::sample::select(vec![
                Outcome::Approve,
                Outcome::ApproveAfterFixes,
                Outcome::Rework,
                Outcome::Pass,
                Outcome::Findings,
            ]),
        ) {
            prop_assert_eq!(Outcome::parse(&outcome.to_string()), Some(outcome));
            prop_assert_eq!(
                serde_json::to_string(&outcome).ok(),
                Some(format!("\"{outcome}\""))
            );
        }
    }

    #[test]
    fn the_refusals_for_bad_findings() {
        let refused = |text: &str| parse_finding(0, text).expect_err("refused");
        let start = "must start with issue, suggestion, question or nitpick, then \": \"";
        assert_eq!(refused("oops"), format!("finding \"oops\": {start}"));
        assert_eq!(refused("issue:x"), format!("finding \"issue:x\": {start}"));
        assert_eq!(
            refused("Issue: x"),
            format!("finding \"Issue: x\": {start}")
        );
        assert_eq!(
            refused("issue: x"),
            "finding \"issue: x\": issue and suggestion need (blocking) or (non-blocking)"
        );
        assert_eq!(
            refused("nitpick (blocking): x"),
            "finding \"nitpick (blocking): x\": a nitpick is never blocking"
        );
        assert_eq!(
            refused("question (non-blocking): x"),
            "finding \"question (non-blocking): x\": a question is neither blocking nor non-blocking"
        );
        assert_eq!(
            refused("issue (blocking, Loud): x"),
            "finding \"issue (blocking, Loud): x\": \"Loud\" is not a decoration"
        );
        assert_eq!(
            refused("issue (blocking,): x"),
            "finding \"issue (blocking,): x\": \"\" is not a decoration"
        );
        assert_eq!(
            refused("question:  "),
            "finding \"question:  \": the text after \": \" is empty"
        );
    }
}
