use crate::pure::finding::ResultKind;
use crate::pure::template::{Check, Contradiction, Embedded, Outcome, Spec};
use crate::pure::text::comma_list;

const ANSWER: Check = Check::Answer(&["none found: ", "concern: ", "not relevant: "]);

// The claims every review and report ends with: what ran, what was reasoned, what could not be
// checked. Each may be the single item `none`; only Verified must name the command or test, in a
// code span.
const VERIFIED: (&str, Check) = ("Verified", Check::CodeSpan);
const INFERRED: (&str, Check) = ("Inferred", Check::Item);
const UNVERIFIABLE: (&str, Check) = ("Unverifiable", Check::Item);
const CLAIMS_LAST: &[&str] = &["Verified", "Inferred", "Unverifiable"];

const TEACH_BACK: (&str, Check) = (
    "Teach-back",
    Check::Answer(&["[matches] ", "[wrong] ", "[unclear] "]),
);
const READING_PATH: (&str, Check) = ("Reading path", Check::Place);
const LOOK_HERE: (&str, Check) = ("Look here", Check::Place);
// A group of files on no path says how much care it needs.
const OTHER_FILES: (&str, Check) = (
    "Other files",
    Check::Contains(&["risk: high", "risk: medium", "risk: low"]),
);

// A concern is a finding in waiting: an approval that holds one says two things, even when a
// non-blocking finding names it, so it is approve-after-fixes.
const CONCERN_SECURITY: Contradiction = Contradiction {
    outcome: "approve",
    part: "Security",
    start: "concern: ",
};
const CONCERN_PERFORMANCE: Contradiction = Contradiction {
    outcome: "approve",
    part: "Performance",
    start: "concern: ",
};

// Sorted by kind; the blank forms are data/templates/<kind>.md.
pub(crate) const KINDS: &[Spec] = &[
    Spec {
        kind: "build-plan",
        outcomes: &[Outcome {
            name: "ready-for-review",
            parts: &[
                "Goal",
                "Architecture",
                "Global constraints",
                "Done-when coverage",
                "Review focus",
                "Steps",
                "Needs from other slices",
                "Decisions for the owner",
            ],
        }],
        flows: &[],
        last: &[],
        owner_message: false,
        checks: &[],
        findings: None,
        none_allowed: &[],
        embedded: Embedded::CoverageMap,
        forbidden: &[],
        contradicts: &[],
    },
    Spec {
        kind: "decision-point",
        outcomes: &[Outcome {
            name: "waiting-for-the-owner",
            parts: &["What", "Context", "Recommendation", "Next"],
        }],
        flows: &[],
        last: &[],
        owner_message: true,
        checks: &[],
        findings: None,
        none_allowed: &[],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[],
    },
    Spec {
        kind: "explanation",
        outcomes: &[
            Outcome {
                name: "understood",
                parts: &["Parts", "Verified", "Inferred", "Unverifiable"],
            },
            Outcome {
                name: "unclear",
                parts: &["Parts", "Unclear", "Verified", "Inferred", "Unverifiable"],
            },
        ],
        flows: &[],
        last: CLAIMS_LAST,
        owner_message: false,
        checks: &[
            ("Parts", Check::FileLines),
            ("Unclear", Check::FileLines),
            VERIFIED,
            INFERRED,
            UNVERIFIABLE,
        ],
        findings: None,
        none_allowed: &["Verified", "Inferred", "Unverifiable"],
        embedded: Embedded::Nothing,
        forbidden: &[("understood", "Unclear")],
        contradicts: &[],
    },
    Spec {
        kind: "informed-review",
        outcomes: &[
            Outcome {
                name: "approve",
                parts: &[
                    "Summary",
                    "Teach-back",
                    "Reading path",
                    "Other files",
                    "Look here",
                    "Simple code",
                    "Security",
                    "Performance",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "approve-after-fixes",
                parts: &[
                    "Summary",
                    "Teach-back",
                    "Reading path",
                    "Other files",
                    "Look here",
                    "Simple code",
                    "Security",
                    "Performance",
                    "Findings",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "rework",
                parts: &[
                    "Summary",
                    "Teach-back",
                    "Reading path",
                    "Other files",
                    "Look here",
                    "Simple code",
                    "Security",
                    "Performance",
                    "Findings",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
        ],
        flows: &["Reading path"],
        last: CLAIMS_LAST,
        owner_message: false,
        checks: &[
            TEACH_BACK,
            READING_PATH,
            OTHER_FILES,
            LOOK_HERE,
            ("Security", ANSWER),
            ("Performance", ANSWER),
            VERIFIED,
            INFERRED,
            UNVERIFIABLE,
        ],
        findings: Some(("Findings", ResultKind::Review)),
        none_allowed: &[
            "Reading path",
            "Other files",
            "Verified",
            "Inferred",
            "Unverifiable",
        ],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[CONCERN_SECURITY, CONCERN_PERFORMANCE],
    },
    Spec {
        kind: "known-issue",
        outcomes: &[Outcome {
            name: "open",
            parts: &["Finding", "Reason", "Effect", "Found by"],
        }],
        flows: &[],
        last: &[],
        owner_message: false,
        checks: &[],
        findings: None,
        none_allowed: &[],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[],
    },
    Spec {
        kind: "model-advice",
        outcomes: &[Outcome {
            name: "ready-for-the-owner",
            parts: &["What", "Context", "Recommendation", "Sources", "Next"],
        }],
        flows: &[],
        last: &[],
        owner_message: true,
        checks: &[("Sources", Check::Contains(&["https://", "http://"]))],
        findings: None,
        none_allowed: &[],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[],
    },
    Spec {
        kind: "plan-review",
        outcomes: &[
            Outcome {
                name: "approve",
                parts: &[
                    "Summary",
                    "Done-when",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "approve-after-fixes",
                parts: &[
                    "Summary",
                    "Done-when",
                    "Findings",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "rework",
                parts: &[
                    "Summary",
                    "Done-when",
                    "Findings",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
        ],
        flows: &[],
        last: CLAIMS_LAST,
        owner_message: false,
        checks: &[
            (
                "Done-when",
                Check::AnswerForDoneWhen(&["[covered] ", "[gap] "]),
            ),
            VERIFIED,
            INFERRED,
            UNVERIFIABLE,
        ],
        findings: Some(("Findings", ResultKind::Review)),
        none_allowed: &["Verified", "Inferred", "Unverifiable"],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[Contradiction {
            outcome: "approve",
            part: "Done-when",
            start: "[gap] ",
        }],
    },
    Spec {
        kind: "review",
        outcomes: &[
            Outcome {
                name: "approve",
                parts: &[
                    "Summary",
                    "Claim",
                    "Teach-back",
                    "Reading path",
                    "Other files",
                    "Look here",
                    "Security",
                    "Performance",
                    "Skipped",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "approve-after-fixes",
                parts: &[
                    "Summary",
                    "Claim",
                    "Teach-back",
                    "Reading path",
                    "Other files",
                    "Look here",
                    "Findings",
                    "Security",
                    "Performance",
                    "Skipped",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "rework",
                parts: &[
                    "Summary",
                    "Claim",
                    "Teach-back",
                    "Reading path",
                    "Other files",
                    "Look here",
                    "Findings",
                    "Security",
                    "Performance",
                    "Skipped",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
        ],
        flows: &["Reading path"],
        last: CLAIMS_LAST,
        owner_message: false,
        checks: &[
            TEACH_BACK,
            READING_PATH,
            OTHER_FILES,
            LOOK_HERE,
            ("Security", ANSWER),
            ("Performance", ANSWER),
            ("Skipped", Check::Item),
            VERIFIED,
            INFERRED,
            UNVERIFIABLE,
        ],
        findings: Some(("Findings", ResultKind::Review)),
        none_allowed: &[
            "Reading path",
            "Other files",
            "Skipped",
            "Verified",
            "Inferred",
            "Unverifiable",
        ],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[CONCERN_SECURITY, CONCERN_PERFORMANCE],
    },
    Spec {
        kind: "roadmap",
        outcomes: &[Outcome {
            name: "ready-for-review",
            parts: &[
                "Goal",
                "Slices",
                "Shared contracts",
                "Owner decisions",
                "Risks",
            ],
        }],
        flows: &[],
        last: &[],
        owner_message: false,
        checks: &[
            (
                "Shared contracts",
                Check::NamesElement(&["lands first in slice "]),
            ),
            ("Owner decisions", Check::NamesElement(&["before slice "])),
            (
                "Risks",
                Check::NamesElement(&["settled by slice ", "settled by spike "]),
            ),
        ],
        findings: None,
        none_allowed: &["Shared contracts", "Owner decisions", "Risks"],
        embedded: Embedded::Roadmap,
        forbidden: &[],
        contradicts: &[],
    },
    Spec {
        kind: "scout-report",
        outcomes: &[
            Outcome {
                name: "found",
                parts: &["Question", "Facts", "Verified", "Inferred", "Unverifiable"],
            },
            Outcome {
                name: "partial",
                parts: &[
                    "Question",
                    "Facts",
                    "Missing",
                    "Where I looked",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "not-found",
                parts: &[
                    "Question",
                    "Missing",
                    "Where I looked",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
        ],
        flows: &[],
        last: CLAIMS_LAST,
        owner_message: false,
        checks: &[
            ("Facts", Check::Contains(&["(source: "])),
            VERIFIED,
            INFERRED,
            UNVERIFIABLE,
        ],
        findings: None,
        none_allowed: &["Verified", "Inferred", "Unverifiable"],
        embedded: Embedded::Nothing,
        forbidden: &[("found", "Missing")],
        contradicts: &[],
    },
    Spec {
        kind: "second-opinion",
        outcomes: &[
            Outcome {
                name: "approve",
                parts: &[
                    "Summary",
                    "Security",
                    "Performance",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "approve-after-fixes",
                parts: &[
                    "Summary",
                    "Security",
                    "Performance",
                    "Findings",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "rework",
                parts: &[
                    "Summary",
                    "Security",
                    "Performance",
                    "Findings",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
        ],
        flows: &[],
        last: CLAIMS_LAST,
        owner_message: false,
        checks: &[
            ("Security", ANSWER),
            ("Performance", ANSWER),
            VERIFIED,
            INFERRED,
            UNVERIFIABLE,
        ],
        findings: Some(("Findings", ResultKind::Review)),
        none_allowed: &["Verified", "Inferred", "Unverifiable"],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[CONCERN_SECURITY, CONCERN_PERFORMANCE],
    },
    Spec {
        kind: "test-drive",
        outcomes: &[
            Outcome {
                name: "pass",
                parts: &[
                    "Done-when",
                    "Exploration",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
            Outcome {
                name: "findings",
                parts: &[
                    "Done-when",
                    "Exploration",
                    "Findings",
                    "Verified",
                    "Inferred",
                    "Unverifiable",
                ],
            },
        ],
        flows: &[],
        last: CLAIMS_LAST,
        owner_message: false,
        checks: &[
            ("Done-when", Check::Answer(&["[pass] ", "[fail] "])),
            VERIFIED,
            INFERRED,
            UNVERIFIABLE,
        ],
        findings: Some(("Findings", ResultKind::TestDrive)),
        none_allowed: &["Verified", "Inferred", "Unverifiable"],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[Contradiction {
            outcome: "pass",
            part: "Done-when",
            start: "[fail] ",
        }],
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
            "build-plan, decision-point, explanation, informed-review, known-issue, model-advice, plan-review, review, roadmap, scout-report, second-opinion, test-drive"
        );
        assert!(kind("decision-point").is_some());
        assert!(kind("review").is_some());
        assert!(kind("plan-review").is_some());
        assert!(kind("plan").is_none());
    }

    // The owner's line caps apply to the messages the owner answers; a review is read in a file.
    #[test]
    fn only_the_messages_to_the_owner_carry_the_line_caps() {
        let capped: Vec<&str> = KINDS
            .iter()
            .filter(|spec| spec.owner_message)
            .map(|spec| spec.kind)
            .collect();
        assert_eq!(capped, ["decision-point", "model-advice"]);
    }
}
