use std::collections::BTreeMap;
use std::fmt::Write as _;

use toml::Table;

use crate::pure::finding::Class;
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::element_name;
use crate::pure::logbook::{Entry, Logged, is_shipped_by};
use crate::pure::metric::{MetricName, MetricRecord, SliceMetrics};
use crate::pure::review::Stored;
use crate::pure::roadmap::{Roadmap, SliceName};

const TITLE_CHARS: usize = 70;

#[derive(Debug, PartialEq)]
pub(crate) struct IssueDraft {
    pub(crate) title: String,
    pub(crate) body: String,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Plan {
    pub(crate) kept: SliceMetrics,
    pub(crate) issues: Vec<IssueDraft>,
}

// The findings of the receipts the known issues name, by (kind, key).
pub(crate) type Findings = BTreeMap<(String, String), Vec<Stored>>;

fn clipped(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    let mut clipped: String = line.chars().take(TITLE_CHARS).collect();
    if line.chars().count() > TITLE_CHARS {
        clipped.push('…');
    }
    clipped
}

fn draft(
    goal: &GoalName,
    slice: &SliceName,
    issue: &KnownIssue<'_>,
    found: Option<&Stored>,
) -> IssueDraft {
    let class = found.map_or(Class::NiceToFix, |stored| stored.class);
    let text = found.map_or("(the receipt is not in the store)", |stored| {
        stored.text.as_str()
    });
    let title = format!(
        "{goal}/{slice}: {} {}: {}",
        issue.kind,
        issue.finding,
        clipped(text)
    );
    let mut body = String::new();
    writeln!(
        body,
        "Known issue of slice {slice} of goal {goal}, found by {} ({class}) and left unfixed.\n",
        issue.kind
    )
    .ok();
    writeln!(body, "Finding {}: {text}\n", issue.finding).ok();
    writeln!(body, "Logged by {}. Reason: {}", issue.by, issue.reason).ok();
    IssueDraft { title, body }
}

struct KnownIssue<'a> {
    kind: &'a str,
    key: &'a str,
    finding: &'a str,
    reason: &'a str,
    by: &'a str,
}

// The newest record of each metric of each slice of the goal: numbers, one per slice and name.
fn kept_numbers(goal: &GoalName, records: &[MetricRecord]) -> SliceMetrics {
    let mut newest: BTreeMap<(&SliceName, &MetricName), &MetricRecord> = BTreeMap::new();
    for record in records.iter().filter(|record| record.goal == *goal) {
        let slot = (&record.slice, &record.name);
        if newest
            .get(&slot)
            .is_none_or(|older| older.time <= record.time)
        {
            newest.insert(slot, record);
        }
    }
    let mut kept = SliceMetrics::new();
    for ((slice, name), record) in newest {
        kept.entry(slice.clone())
            .or_default()
            .insert(name.clone(), record.value);
    }
    kept
}

// Err names the slices and proof steps that have not shipped.
pub(crate) fn plan(
    goal: &GoalName,
    roadmap: &Roadmap,
    entries: &[Logged],
    findings: &Findings,
    records: &[MetricRecord],
) -> Result<Plan, Vec<SliceName>> {
    let unshipped: Vec<SliceName> = roadmap
        .elements
        .iter()
        .map(element_name)
        .filter(|name| !is_shipped_by(entries, name))
        .cloned()
        .collect();
    if !unshipped.is_empty() {
        return Err(unshipped);
    }
    let issues = entries
        .iter()
        .filter_map(|logged| match &logged.entry {
            Entry::KnownIssue {
                kind,
                key,
                finding,
                reason,
                by,
            } => Some((
                &logged.slice,
                KnownIssue {
                    kind,
                    key,
                    finding,
                    reason,
                    by,
                },
            )),
            Entry::Receipt { .. }
            | Entry::Note { .. }
            | Entry::Answer { .. }
            | Entry::ShippedMerge { .. }
            | Entry::ShippedProof { .. }
            | Entry::Started { .. }
            | Entry::Raise { .. } => None,
        })
        .map(|(slice, issue)| {
            let found = findings
                .get(&(issue.kind.to_owned(), issue.key.to_owned()))
                .and_then(|all| all.iter().find(|stored| stored.id == issue.finding));
            draft(goal, slice, &issue, found)
        })
        .collect();
    Ok(Plan {
        kept: kept_numbers(goal, records),
        issues,
    })
}

// Numbers only, one table per slice; the `[complete]` table keeps the issues made on the way out,
// which is the only text in the file.
pub(crate) fn metrics_toml(kept: &SliceMetrics, issues: &[String]) -> String {
    let mut text = String::new();
    for (slice, numbers) in kept {
        writeln!(text, "[slices.{slice}]").ok();
        for (name, value) in numbers {
            writeln!(text, "{name} = {value}").ok();
        }
        text.push('\n');
    }
    let urls: Vec<String> = issues.iter().map(|url| format!("{url:?}")).collect();
    writeln!(text, "[complete]\nissues = [{}]", urls.join(", ")).ok();
    text
}

// The issues a half-done completion recorded; None for a file that is not one.
pub(crate) fn recorded_issues(table: &Table) -> Option<Vec<String>> {
    table
        .get("complete")?
        .as_table()?
        .get("issues")?
        .as_array()?
        .iter()
        .map(|url| url.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::metric::MetricValue;
    use crate::pure::roadmap::{Element, Slice};

    fn goal() -> GoalName {
        GoalName::parse("demo").expect("a goal name")
    }

    fn slice(name: &str) -> SliceName {
        SliceName::parse(name).expect("a slice name")
    }

    fn roadmap(names: &[&str]) -> Roadmap {
        Roadmap {
            goal: goal(),
            elements: names
                .iter()
                .map(|name| {
                    Element::Slice(Slice {
                        name: slice(name),
                        after: Vec::new(),
                        meets: Vec::new(),
                        owns: Vec::new(),
                        test_drive: false,
                    })
                })
                .collect(),
        }
    }

    fn shipped(name: &str) -> Logged {
        Logged {
            slice: slice(name),
            entry: Entry::ShippedProof {
                note: "evidence".to_owned(),
            },
        }
    }

    fn issue(name: &str, kind: &str, key: &str, finding: &str) -> Logged {
        Logged {
            slice: slice(name),
            entry: Entry::KnownIssue {
                kind: kind.to_owned(),
                key: key.to_owned(),
                finding: finding.to_owned(),
                reason: "later".to_owned(),
                by: "opus:high".to_owned(),
            },
        }
    }

    fn stored(id: &str, class: Class, text: &str) -> Stored {
        Stored {
            id: id.to_owned(),
            is_question: false,
            class,
            text: text.to_owned(),
        }
    }

    fn record(slice_name: &str, name: &str, value: u32, time: &str) -> MetricRecord {
        MetricRecord {
            key: "k".to_owned(),
            goal: goal(),
            slice: slice(slice_name),
            name: MetricName::parse(name).expect("a name"),
            value: MetricValue::parse(&value.to_string()).expect("a number"),
            time: time.to_owned(),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_goal_is_complete_iff_every_element_has_a_shipped_entry(
            done in proptest::collection::vec(any::<bool>(), 4),
        ) {
            let names = ["alpha", "beta", "gamma", "delta"];
            let entries: Vec<Logged> = names
                .iter()
                .zip(&done)
                .filter(|(_, done)| **done)
                .map(|(name, _)| shipped(name))
                .collect();
            let planned = plan(&goal(), &roadmap(&names), &entries, &Findings::new(), &[]);
            if done.iter().all(|done| *done) {
                prop_assert!(planned.is_ok());
            } else {
                let missing: Vec<SliceName> = names
                    .iter()
                    .zip(&done)
                    .filter(|(_, done)| !**done)
                    .map(|(name, _)| slice(name))
                    .collect();
                prop_assert_eq!(planned, Err(missing));
            }
        }

        #[test]
        fn one_issue_per_known_issue_entry_and_none_for_a_closed_finding(
            kinds in proptest::collection::vec(0..3_usize, 0..6),
        ) {
            let mut entries = vec![shipped("alpha")];
            for (index, kind) in kinds.iter().enumerate() {
                let finding = format!("F{}", index + 1);
                entries.push(match kind {
                    0 => issue("alpha", "second-opinion", "k", &finding),
                    1 => Logged {
                        slice: slice("alpha"),
                        entry: Entry::Answer {
                            kind: "second-opinion".to_owned(),
                            key: "k".to_owned(),
                            finding,
                            text: "yes".to_owned(),
                            by: "opus:high".to_owned(),
                        },
                    },
                    _ => Logged {
                        slice: slice("alpha"),
                        entry: Entry::Note { text: "a note".to_owned() },
                    },
                });
            }
            let planned = plan(&goal(), &roadmap(&["alpha"]), &entries, &Findings::new(), &[])
                .expect("complete");
            prop_assert_eq!(planned.issues.len(), kinds.iter().filter(|kind| **kind == 0).count());
        }

        #[test]
        fn a_nice_to_fix_finding_never_becomes_an_issue(
            texts in proptest::collection::vec("[a-z ]{1,10}", 1..5),
        ) {
            let findings: Vec<Stored> = texts
                .iter()
                .enumerate()
                .map(|(index, text)| stored(&format!("F{}", index + 1), Class::NiceToFix, text))
                .collect();
            let known = Findings::from([(("second-opinion".to_owned(), "k".to_owned()), findings)]);
            let planned = plan(&goal(), &roadmap(&["alpha"]), &[shipped("alpha")], &known, &[])
                .expect("complete");
            prop_assert!(planned.issues.is_empty());
        }

        #[test]
        fn the_plan_keeps_only_numbers_and_one_value_per_slice_and_metric(
            values in proptest::collection::vec((0..3_usize, 0..2_usize, 0..100_u32, 0..9_u32), 0..12),
        ) {
            let slices = ["alpha", "beta", "gamma"];
            let names = ["clone-ratio", "cognitive-complexity"];
            let records: Vec<MetricRecord> = values
                .iter()
                .map(|(s, n, value, hour)| record(
                    slices.get(*s).copied().expect("a slice"),
                    names.get(*n).copied().expect("a name"),
                    *value,
                    &format!("2026-11-21T0{hour}:00:00Z"),
                ))
                .collect();
            let planned = plan(&goal(), &roadmap(&["alpha"]), &[shipped("alpha")], &Findings::new(), &records)
                .expect("complete");
            for (slice_name, numbers) in &planned.kept {
                for (name, value) in numbers {
                    let recorded = records
                        .iter()
                        .any(|r| r.slice == *slice_name && r.name == *name && r.value == *value);
                    prop_assert!(recorded);
                }
            }
            let text = metrics_toml(&planned.kept, &[]);
            let table: Table = toml::from_str(&text).expect("TOML");
            prop_assert_eq!(recorded_issues(&table), Some(Vec::new()));
            let back = crate::pure::metric::parse_kept(goal(), &table).expect("kept");
            prop_assert_eq!(back.slices, planned.kept);
        }
    }

    #[test]
    fn the_metrics_toml_for_a_known_goal() {
        let records = [
            record("alpha", "clone-ratio", 3, "2026-11-21T10:00:00Z"),
            record("alpha", "clone-ratio", 4, "2026-11-21T11:00:00Z"),
            record("beta", "cognitive-complexity", 31, "2026-11-21T10:00:00Z"),
        ];
        let planned = plan(
            &goal(),
            &roadmap(&["alpha", "beta"]),
            &[shipped("alpha"), shipped("beta")],
            &Findings::new(),
            &records,
        )
        .expect("complete");
        assert_eq!(
            metrics_toml(
                &planned.kept,
                &["https://x/1".to_owned(), "https://x/2".to_owned()]
            ),
            "[slices.alpha]\nclone-ratio = 4\n\n[slices.beta]\ncognitive-complexity = 31\n\n[complete]\nissues = [\"https://x/1\", \"https://x/2\"]\n"
        );
    }

    #[test]
    fn a_slice_named_complete_keeps_its_metrics_beside_the_issues() {
        let records = [record("complete", "clone-ratio", 3, "2026-11-21T10:00:00Z")];
        let planned = plan(
            &goal(),
            &roadmap(&["complete"]),
            &[shipped("complete")],
            &Findings::new(),
            &records,
        )
        .expect("complete");
        let text = metrics_toml(&planned.kept, &["https://x/1".to_owned()]);
        let table: Table = toml::from_str(&text).expect("valid TOML, no table twice");
        assert_eq!(
            recorded_issues(&table),
            Some(vec!["https://x/1".to_owned()])
        );
        let back = crate::pure::metric::parse_kept(goal(), &table).expect("kept");
        assert_eq!(back.slices, planned.kept);
        assert_eq!(back.slices.len(), 1);
    }

    #[test]
    fn an_issue_names_the_finding_it_came_from() {
        let known = Findings::from([(
            ("second-opinion".to_owned(), "k".to_owned()),
            vec![stored("F1", Class::MustFix, "a problem")],
        )]);
        let planned = plan(
            &goal(),
            &roadmap(&["alpha"]),
            &[
                shipped("alpha"),
                issue("alpha", "second-opinion", "k", "F1"),
            ],
            &known,
            &[],
        )
        .expect("complete");
        assert_eq!(planned.issues.len(), 1);
        assert_eq!(
            planned.issues[0].title,
            "demo/alpha: second-opinion F1: a problem"
        );
        assert!(planned.issues[0].body.contains("Finding F1: a problem"));
        assert!(
            planned.issues[0]
                .body
                .contains("Logged by opus:high. Reason: later")
        );
    }
}
