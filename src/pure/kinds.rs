use crate::pure::finding::ResultKind;
use crate::pure::template::{Check, Outcome, Spec};
use crate::pure::text::comma_list;

const ANSWER: Check = Check::Answer(&["none found: ", "concern: ", "not relevant: "]);

// Sorted by kind; the blank forms are data/templates/<kind>.md.
pub(crate) const KINDS: &[Spec] = &[
    Spec {
        kind: "decision-point",
        outcomes: &[Outcome {
            name: "waiting-for-the-owner",
            parts: &["What", "Context", "Recommendation", "Next"],
        }],
        owner_message: true,
        checks: &[],
        findings: None,
        none_allowed: &[],
    },
    Spec {
        kind: "explanation",
        outcomes: &[
            Outcome {
                name: "understood",
                parts: &["Parts"],
            },
            Outcome {
                name: "unclear",
                parts: &["Parts", "Unclear"],
            },
        ],
        owner_message: false,
        checks: &[("Parts", Check::FileLines), ("Unclear", Check::FileLines)],
        findings: None,
        none_allowed: &[],
    },
    Spec {
        kind: "informed-review",
        outcomes: &[
            Outcome {
                name: "approve",
                parts: &[
                    "Summary",
                    "Teach-back",
                    "Simple code",
                    "Security",
                    "Performance",
                ],
            },
            Outcome {
                name: "approve-after-fixes",
                parts: &[
                    "Summary",
                    "Teach-back",
                    "Simple code",
                    "Security",
                    "Performance",
                    "Findings",
                ],
            },
            Outcome {
                name: "rework",
                parts: &[
                    "Summary",
                    "Teach-back",
                    "Simple code",
                    "Security",
                    "Performance",
                    "Findings",
                ],
            },
        ],
        owner_message: false,
        checks: &[
            (
                "Teach-back",
                Check::Answer(&["[matches] ", "[wrong] ", "[unclear] "]),
            ),
            ("Security", ANSWER),
            ("Performance", ANSWER),
        ],
        findings: Some(("Findings", ResultKind::Review)),
        none_allowed: &[],
    },
    Spec {
        kind: "known-issue",
        outcomes: &[Outcome {
            name: "open",
            parts: &["Finding", "Reason", "Effect", "Found by"],
        }],
        owner_message: false,
        checks: &[],
        findings: None,
        none_allowed: &[],
    },
    Spec {
        kind: "model-advice",
        outcomes: &[Outcome {
            name: "ready-for-the-owner",
            parts: &["What", "Context", "Recommendation", "Sources", "Next"],
        }],
        owner_message: true,
        checks: &[("Sources", Check::Contains(&["https://", "http://"]))],
        findings: None,
        none_allowed: &[],
    },
    Spec {
        kind: "scout-report",
        outcomes: &[
            Outcome {
                name: "found",
                parts: &["Question", "Facts"],
            },
            Outcome {
                name: "partial",
                parts: &["Question", "Facts", "Missing", "Where I looked"],
            },
            Outcome {
                name: "not-found",
                parts: &["Question", "Missing", "Where I looked"],
            },
        ],
        owner_message: false,
        checks: &[("Facts", Check::Contains(&["(source: "]))],
        findings: None,
        none_allowed: &[],
    },
    Spec {
        kind: "second-opinion",
        outcomes: &[
            Outcome {
                name: "approve",
                parts: &["Summary", "Security", "Performance"],
            },
            Outcome {
                name: "approve-after-fixes",
                parts: &["Summary", "Security", "Performance", "Findings"],
            },
            Outcome {
                name: "rework",
                parts: &["Summary", "Security", "Performance", "Findings"],
            },
        ],
        owner_message: false,
        checks: &[("Security", ANSWER), ("Performance", ANSWER)],
        findings: Some(("Findings", ResultKind::Review)),
        none_allowed: &[],
    },
    Spec {
        kind: "test-drive",
        outcomes: &[
            Outcome {
                name: "pass",
                parts: &["Done-when", "Exploration"],
            },
            Outcome {
                name: "findings",
                parts: &["Done-when", "Exploration", "Findings"],
            },
        ],
        owner_message: false,
        checks: &[("Done-when", Check::Answer(&["[pass] ", "[fail] "]))],
        findings: Some(("Findings", ResultKind::TestDrive)),
        none_allowed: &[],
    },
];

pub(crate) fn kind(name: &str) -> Option<&'static Spec> {
    KINDS.iter().find(|spec| spec.kind == name)
}

pub(crate) fn kind_names() -> String {
    comma_list(KINDS.iter().map(|spec| spec.kind))
}

pub(crate) fn form(name: &str) -> Option<&'static str> {
    let key = format!("templates/{name}.md");
    crate::DATA
        .iter()
        .find(|(path, _)| *path == key)
        .map(|(_, text)| *text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pure::markdown::lines;
    use crate::pure::template::{header, parts};

    #[test]
    fn the_kinds_are_sorted_and_the_forms_are_exactly_the_kinds() {
        let names: Vec<&str> = KINDS.iter().map(|spec| spec.kind).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        let forms: Vec<&str> = crate::DATA
            .iter()
            .filter_map(|(path, _)| path.strip_prefix("templates/")?.strip_suffix(".md"))
            .collect();
        assert_eq!(forms, names);
    }

    #[test]
    fn every_form_agrees_with_its_spec() {
        for spec in KINDS {
            let text = form(spec.kind).expect("a form for every kind");
            let head = header(spec.kind, text).expect("the form's header is valid");
            assert_eq!(head.kind, spec.kind);
            let listed: Vec<&str> = head.outcome.split(" | ").collect();
            let wanted: Vec<&str> = spec.outcomes.iter().map(|outcome| outcome.name).collect();
            assert_eq!(listed, wanted, "{}", spec.kind);
            let found = parts(spec, lines(text).expect("closed fences"));
            for name in spec.part_names() {
                assert!(
                    found.iter().any(|part| part.name == name),
                    "{}: no marker for {name}",
                    spec.kind
                );
            }
        }
    }

    #[test]
    fn every_role_output_has_a_kind() {
        assert_eq!(
            kind_names(),
            "decision-point, explanation, informed-review, known-issue, model-advice, scout-report, second-opinion, test-drive"
        );
        assert!(kind("decision-point").is_some());
        assert!(kind("review").is_none());
    }
}
