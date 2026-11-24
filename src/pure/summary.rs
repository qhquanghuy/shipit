use std::fmt::Write as _;

use crate::pure::finding::Class;
use crate::pure::goal::QualifiedDoneWhen;
use crate::pure::metric::{Fence, MetricName, MetricValue};
use crate::pure::roadmap::SliceName;
use crate::pure::state::{Confirmation, Counts};
use crate::pure::text::{comma_list, plain};

#[derive(Debug)]
pub(crate) struct CheckRow {
    pub(crate) tree: String,
    pub(crate) files: u64,
    pub(crate) cap: u64,
    pub(crate) base: String,
}

#[derive(Debug)]
pub(crate) struct Row {
    pub(crate) kind: String,
    pub(crate) key: String,
    pub(crate) current: bool,
    pub(crate) by: String,
    pub(crate) author: String,
    pub(crate) not_the_role: Option<&'static str>,
    pub(crate) rank: String,
    pub(crate) result: String,
    pub(crate) counts: Counts,
}

#[derive(Debug)]
pub(crate) struct Known {
    pub(crate) kind: String,
    pub(crate) finding: String,
    pub(crate) class: Class,
    pub(crate) by: String,
    pub(crate) text: String,
    pub(crate) reason: String,
}

#[derive(Debug)]
pub(crate) struct Answered {
    pub(crate) kind: String,
    pub(crate) finding: String,
    pub(crate) by: String,
    pub(crate) text: String,
}

#[derive(Debug)]
pub(crate) struct Unfixed {
    pub(crate) kind: String,
    pub(crate) finding: String,
    pub(crate) text: String,
}

#[derive(Debug)]
pub(crate) struct MetricLine {
    pub(crate) name: MetricName,
    pub(crate) value: MetricValue,
    pub(crate) fence: Option<Fence>,
}

#[derive(Debug)]
pub(crate) struct SummaryInput {
    pub(crate) name: SliceName,
    pub(crate) state: String,
    pub(crate) map_path: String,
    pub(crate) done_when: Vec<(QualifiedDoneWhen, Confirmation)>,
    pub(crate) check: Option<CheckRow>,
    pub(crate) rows: Vec<Row>,
    pub(crate) open: Counts,
    pub(crate) stand_ins: Vec<String>,
    pub(crate) metrics: Vec<MetricLine>,
    pub(crate) known: Vec<Known>,
    pub(crate) unfixed: Vec<Unfixed>,
    pub(crate) answers: Vec<Answered>,
}

// `opus:high` reads as `opus high`, for people.
fn spoken(worker: &str) -> String {
    plain(&worker.replacen(':', " ", 1))
}

fn tick_text(id: &QualifiedDoneWhen, confirmation: Confirmation) -> String {
    match confirmation {
        Confirmation::Tests(tests) => {
            let noun = if tests == 1 { "test" } else { "tests" };
            format!("{id} ✓ local ({tests} {noun})")
        }
        Confirmation::ProofStep => format!("{id} ✓ proof step shipped"),
        Confirmation::TestDrive => format!("{id} ✓ test drive passed"),
        Confirmation::KnownIssue => format!("{id} known issue, not proven"),
        Confirmation::RecordNotCounted => {
            format!("{id} ✗ (record not from a shipit coverage line of the setup)")
        }
        Confirmation::Missing => format!("{id} ✗ not confirmed for this version"),
    }
}

fn counts_text(counts: Counts) -> String {
    format!(
        "must fix {}, must answer {}, nice to fix {}",
        counts.must_fix, counts.must_answer, counts.nice_to_fix
    )
}

fn row_line(row: &Row) -> String {
    let version = format!(
        "{} ({})",
        row.key,
        if row.current { "current" } else { "old" }
    );
    let above = match row.not_the_role {
        Some(role) => format!("{} (not the setup's {role})", spoken(&row.author)),
        None => spoken(&row.author),
    };
    let result = if row.counts == Counts::default() {
        row.result.clone()
    } else {
        let mut parts = Vec::new();
        for (label, n) in [
            ("must fix", row.counts.must_fix),
            ("must answer", row.counts.must_answer),
            ("nice to fix", row.counts.nice_to_fix),
        ] {
            if n > 0 {
                parts.push(format!("{label} {n}"));
            }
        }
        format!("{} ({})", row.result, comma_list(parts))
    };
    format!(
        "| {} | {version} | {} | {above} | {} | {result} |",
        row.kind,
        spoken(&row.by),
        row.rank
    )
}

fn section(title: &str, lines: Vec<String>, out: &mut String) {
    if lines.is_empty() {
        writeln!(out, "{title} none").ok();
        return;
    }
    writeln!(out, "{title}").ok();
    for line in lines {
        writeln!(out, "- {}", plain(&line)).ok();
    }
}

pub(crate) fn summary_md(input: &SummaryInput) -> String {
    let mut out = format!("### ShipIt: {} is {}\n\n", input.name, plain(&input.state));
    let items = if input.done_when.is_empty() {
        "no done-when item".to_owned()
    } else {
        input
            .done_when
            .iter()
            .map(|(id, confirmation)| tick_text(id, *confirmation))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    writeln!(out, "Done-when ({}): {items}", input.map_path).ok();
    out.push_str(
        "(✓ local: this machine's run; CI's recheck prints the same coverage line on the pull request.)\n\n",
    );
    out.push_str("| Receipt | Version | By | Above | Rank | Result |\n|---|---|---|---|---|---|\n");
    if let Some(check) = &input.check {
        writeln!(
            out,
            "| check | {} (current) | shipit | | | pass, size {} files (cap {}), base {} |",
            check.tree, check.files, check.cap, check.base
        )
        .ok();
    }
    for row in &input.rows {
        writeln!(out, "{}", plain(&row_line(row))).ok();
    }
    writeln!(out, "\nOpen findings: {}", counts_text(input.open)).ok();
    let stand_ins = if input.stand_ins.is_empty() {
        "none".to_owned()
    } else {
        plain(&comma_list(&input.stand_ins))
    };
    writeln!(out, "Stand-ins used: {stand_ins}").ok();
    let metrics = if input.metrics.is_empty() {
        "none".to_owned()
    } else {
        input
            .metrics
            .iter()
            .map(|line| match line.fence {
                Some(fence) => format!(
                    "{} {} (outlier: above {} from {} slices)",
                    line.name, line.value, fence.above, fence.from
                ),
                None => format!("{} {}", line.name, line.value),
            })
            .collect::<Vec<_>>()
            .join(" · ")
    };
    writeln!(out, "Metrics (outliers from local history): {metrics}\n").ok();
    section(
        "Known issues:",
        input
            .known
            .iter()
            .map(|issue| {
                format!(
                    "{} {} ({}), logged by {}: {}. Reason: {}",
                    issue.kind, issue.finding, issue.class, issue.by, issue.text, issue.reason
                )
            })
            .collect(),
        &mut out,
    );
    out.push('\n');
    section(
        "Known issues (not fixed, nice to fix):",
        input
            .unfixed
            .iter()
            .map(|nice| format!("{} {}: {}", nice.kind, nice.finding, nice.text))
            .collect(),
        &mut out,
    );
    out.push('\n');
    section(
        "Answers:",
        input
            .answers
            .iter()
            .map(|answer| {
                format!(
                    "{} {}, answered by {}: {}",
                    answer.kind, answer.finding, answer.by, answer.text
                )
            })
            .collect(),
        &mut out,
    );
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn slice(name: &str) -> SliceName {
        SliceName::parse(name).expect("a slice name")
    }

    fn id(text: &str) -> QualifiedDoneWhen {
        QualifiedDoneWhen::parse(text).expect("an id")
    }

    fn row(key: &str, current: bool) -> Row {
        Row {
            kind: "second-opinion".to_owned(),
            key: key.to_owned(),
            current,
            by: "opus:high".to_owned(),
            author: "sonnet:high".to_owned(),
            not_the_role: Some("maker"),
            rank: "tier".to_owned(),
            result: "rework".to_owned(),
            counts: Counts {
                must_fix: 1,
                must_answer: 0,
                nice_to_fix: 1,
            },
        }
    }

    fn input(rows: Vec<Row>) -> SummaryInput {
        SummaryInput {
            name: slice("beta"),
            state: "ready to ship".to_owned(),
            map_path: ".shipit/coverage/v1/beta.toml".to_owned(),
            done_when: vec![
                (id("v1:DW1"), Confirmation::Tests(3)),
                (id("v1:DW2"), Confirmation::ProofStep),
                (id("v1:DW3"), Confirmation::TestDrive),
                (id("v1:DW4"), Confirmation::KnownIssue),
                (id("v1:DW5"), Confirmation::RecordNotCounted),
                (id("v1:DW6"), Confirmation::Missing),
            ],
            check: Some(CheckRow {
                tree: "0123456789ab".to_owned(),
                files: 24,
                cap: 60,
                base: "origin/main".to_owned(),
            }),
            rows,
            open: Counts {
                must_fix: 0,
                must_answer: 0,
                nice_to_fix: 1,
            },
            stand_ins: Vec::new(),
            metrics: vec![
                MetricLine {
                    name: MetricName::parse("clone-ratio").expect("a name"),
                    value: MetricValue::parse("2.1").expect("a number"),
                    fence: None,
                },
                MetricLine {
                    name: MetricName::parse("cognitive-complexity").expect("a name"),
                    value: MetricValue::parse("31").expect("a number"),
                    fence: Some(Fence {
                        above: 24.0,
                        from: 12,
                    }),
                },
            ],
            known: vec![Known {
                kind: "second-opinion".to_owned(),
                finding: "F1".to_owned(),
                class: Class::MustFix,
                by: "sonnet:high".to_owned(),
                text: "a problem".to_owned(),
                reason: "later".to_owned(),
            }],
            unfixed: vec![Unfixed {
                kind: "second-opinion".to_owned(),
                finding: "F2".to_owned(),
                text: "a detail".to_owned(),
            }],
            answers: Vec::new(),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn every_receipt_entry_appears_once(count in 0..8_usize, current in any::<bool>()) {
            let rows: Vec<Row> = (0..count)
                .map(|n| row(&format!("{n:012x}"), current))
                .collect();
            let text = summary_md(&input(rows));
            for n in 0..count {
                let key = format!("| {n:012x} (");
                prop_assert_eq!(text.matches(&key).count(), 1);
            }
            let table_lines = text.lines().filter(|line| line.starts_with("| ")).count();
            prop_assert_eq!(table_lines, count + 2);
        }
    }

    #[test]
    fn typed_text_cannot_fake_a_line_or_act_on_a_screen() {
        let mut shown = input(vec![row("abcdef012345", true)]);
        shown.state =
            "shipped (merge not matched: x\n### ShipIt: alpha is ready to ship)".to_owned();
        if let Some(issue) = shown.known.first_mut() {
            issue.reason = "late\n| check | 0 (current) | shipit | | | pass |\u{1b}[31m".to_owned();
        }
        if let Some(row) = shown.rows.first_mut() {
            row.result = "ok\n| fake |".to_owned();
        }
        let text = summary_md(&shown);
        assert!(!text.contains('\u{1b}'));
        assert_eq!(
            text.lines()
                .filter(|line| line.starts_with("### ShipIt:"))
                .count(),
            1
        );
        let check_rows = text.lines().filter(|line| line.starts_with("| check |"));
        assert_eq!(check_rows.count(), 1);
        assert_eq!(text.matches("| fake |").count(), 1);
        assert!(text.contains("\\u{1b}[31m"));
    }

    #[test]
    fn the_summary_for_a_known_walk() {
        let mut shown = input(vec![row("abcdef012345", false)]);
        shown.map_path = ".shipit/coverage/v1/beta.toml".to_owned();
        let expected = "\
### ShipIt: beta is ready to ship

Done-when (.shipit/coverage/v1/beta.toml): v1:DW1 ✓ local (3 tests) · v1:DW2 ✓ proof step shipped · v1:DW3 ✓ test drive passed · v1:DW4 known issue, not proven · v1:DW5 ✗ (record not from a shipit coverage line of the setup) · v1:DW6 ✗ not confirmed for this version
(✓ local: this machine's run; CI's recheck prints the same coverage line on the pull request.)

| Receipt | Version | By | Above | Rank | Result |
|---|---|---|---|---|---|
| check | 0123456789ab (current) | shipit | | | pass, size 24 files (cap 60), base origin/main |
| second-opinion | abcdef012345 (old) | opus high | sonnet high (not the setup's maker) | tier | rework (must fix 1, nice to fix 1) |

Open findings: must fix 0, must answer 0, nice to fix 1
Stand-ins used: none
Metrics (outliers from local history): clone-ratio 2.1 · cognitive-complexity 31 (outlier: above 24 from 12 slices)

Known issues:
- second-opinion F1 (must fix), logged by sonnet:high: a problem. Reason: later

Known issues (not fixed, nice to fix):
- second-opinion F2: a detail

Answers: none
";
        assert_eq!(summary_md(&shown), expected);
    }
}
