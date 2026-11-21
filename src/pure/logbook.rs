use serde::Serialize;
use serde_json::{Map, Value};

use crate::pure::roadmap::SliceName;

// One write_all of at most this many bytes, newline included, is not interleaved with another
// writer's line.
const MAX_LINE: usize = 4096;

// The two kinds of `shipped` share one event name; a slice ships with the commit that merged it,
// a proof step with its evidence, so no entry holds both or neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub(crate) enum Entry {
    Receipt {
        kind: String,
        key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tree: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        commit: Option<String>,
        result: String,
        findings: u64,
        author: String,
        by: String,
        rank: String,
    },
    Note {
        text: String,
    },
    KnownIssue {
        kind: String,
        key: String,
        finding: String,
        reason: String,
        by: String,
    },
    Answer {
        kind: String,
        key: String,
        finding: String,
        text: String,
        by: String,
    },
    #[serde(rename = "shipped")]
    ShippedMerge {
        commit: String,
        matched: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    #[serde(rename = "shipped")]
    ShippedProof {
        note: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Logged {
    pub(crate) slice: SliceName,
    pub(crate) entry: Entry,
}

#[derive(Serialize)]
struct Line<'a> {
    slice: &'a SliceName,
    #[serde(flatten)]
    entry: &'a Entry,
    time: &'a str,
    tool: &'a str,
}

// The line with its newline, or the refusal.
pub(crate) fn line(
    entry: &Entry,
    slice: &SliceName,
    time: &str,
    tool: &str,
) -> Result<String, String> {
    let mut text = serde_json::to_string(&Line {
        slice,
        entry,
        time,
        tool,
    })
    .map_err(|why| why.to_string())?;
    text.push('\n');
    if text.len() > MAX_LINE {
        return Err("entry too long; shorten the text".to_owned());
    }
    Ok(text)
}

fn text_field(object: &Map<String, Value>, name: &str) -> Option<String> {
    object.get(name)?.as_str().map(str::to_owned)
}

fn parse_entry(object: &Map<String, Value>) -> Option<Entry> {
    let field = |name: &str| text_field(object, name);
    let entry = match object.get("event")?.as_str()? {
        "receipt" => Entry::Receipt {
            kind: field("kind")?,
            key: field("key")?,
            tree: field("tree"),
            commit: field("commit"),
            result: field("result")?,
            findings: object.get("findings")?.as_u64()?,
            author: field("author")?,
            by: field("by")?,
            rank: field("rank")?,
        },
        "note" => Entry::Note {
            text: field("text")?,
        },
        "known-issue" => Entry::KnownIssue {
            kind: field("kind")?,
            key: field("key")?,
            finding: field("finding")?,
            reason: field("reason")?,
            by: field("by")?,
        },
        "answer" => Entry::Answer {
            kind: field("kind")?,
            key: field("key")?,
            finding: field("finding")?,
            text: field("text")?,
            by: field("by")?,
        },
        "shipped" if object.contains_key("commit") => Entry::ShippedMerge {
            commit: field("commit")?,
            matched: object.get("matched")?.as_bool()?,
            note: field("note"),
        },
        "shipped" => Entry::ShippedProof {
            note: field("note")?,
        },
        _ => return None,
    };
    Some(entry)
}

fn parse_line(text: &str) -> Option<Logged> {
    let value: Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    Some(Logged {
        slice: SliceName::parse(object.get("slice")?.as_str()?)?,
        entry: parse_entry(object)?,
    })
}

pub(crate) fn read_entries(text: &str) -> Result<Vec<Logged>, String> {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            parse_line(line).ok_or_else(|| format!("line {} is not a logbook entry", index + 1))
        })
        .collect()
}

// The findings of one receipt (its kind and key) that a known issue or an answer closed.
pub(crate) fn closed(entries: &[Logged], slice: &SliceName, kind: &str, key: &str) -> Vec<String> {
    entries
        .iter()
        .filter(|logged| logged.slice == *slice)
        .filter_map(|logged| match &logged.entry {
            Entry::KnownIssue {
                kind: k,
                key: x,
                finding,
                ..
            }
            | Entry::Answer {
                kind: k,
                key: x,
                finding,
                ..
            } if k == kind && x == key => Some(finding.clone()),
            Entry::KnownIssue { .. }
            | Entry::Answer { .. }
            | Entry::Receipt { .. }
            | Entry::Note { .. }
            | Entry::ShippedMerge { .. }
            | Entry::ShippedProof { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn text() -> impl Strategy<Value = String> {
        "[ -~]{0,12}"
    }

    fn entry() -> impl Strategy<Value = Entry> {
        prop_oneof![
            (
                text(),
                text(),
                proptest::option::of(text()),
                proptest::option::of(text())
            )
                .prop_flat_map(|(kind, key, tree, commit)| {
                    (text(), 0..9_u64, text(), text(), text()).prop_map(
                        move |(result, findings, author, by, rank)| Entry::Receipt {
                            kind: kind.clone(),
                            key: key.clone(),
                            tree: tree.clone(),
                            commit: commit.clone(),
                            result,
                            findings,
                            author,
                            by,
                            rank,
                        },
                    )
                }),
            text().prop_map(|text| Entry::Note { text }),
            (text(), text(), text(), text(), text()).prop_map(
                |(kind, key, finding, reason, by)| Entry::KnownIssue {
                    kind,
                    key,
                    finding,
                    reason,
                    by
                }
            ),
            (text(), text(), text(), text(), text()).prop_map(|(kind, key, finding, text, by)| {
                Entry::Answer {
                    kind,
                    key,
                    finding,
                    text,
                    by,
                }
            }),
            (text(), any::<bool>(), proptest::option::of(text())).prop_map(
                |(commit, matched, note)| Entry::ShippedMerge {
                    commit,
                    matched,
                    note
                }
            ),
            text().prop_map(|note| Entry::ShippedProof { note }),
        ]
    }

    fn slice(name: &str) -> SliceName {
        SliceName::parse(name).expect("a slice name")
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn entries_round_trip_and_a_damaged_line_is_named(
            entries in proptest::collection::vec(entry(), 1..6),
            damaged in 0..6_usize,
        ) {
            let lines: Vec<String> = entries
                .iter()
                .map(|entry| line(entry, &slice("beta"), "2026-11-21T10:00:00Z", "shipit 0.1.0"))
                .collect::<Result<_, _>>()
                .expect("short entries");
            let logged: Vec<Logged> = entries
                .into_iter()
                .map(|entry| Logged { slice: slice("beta"), entry })
                .collect();
            prop_assert_eq!(read_entries(&lines.concat()), Ok(logged));
            let at = damaged % lines.len();
            let broken: String = lines
                .iter()
                .enumerate()
                .map(|(index, line)| if index == at { "{\n" } else { line.as_str() })
                .collect();
            prop_assert_eq!(
                read_entries(&broken),
                Err(format!("line {} is not a logbook entry", at + 1))
            );
        }

        #[test]
        fn an_entry_is_one_line_within_the_limit_or_refused(key in "\\PC{0,5000}") {
            let entry = Entry::KnownIssue {
                kind: "second-opinion".to_owned(),
                key: key.clone(),
                finding: "F1".to_owned(),
                reason: "r".to_owned(),
                by: "opus:high".to_owned(),
            };
            match line(&entry, &slice("beta"), "2026-11-21T10:00:00Z", "shipit 0.1.0") {
                Ok(text) => {
                    prop_assert!(text.len() <= MAX_LINE);
                    prop_assert_eq!(text.matches('\n').count(), 1);
                    prop_assert!(text.ends_with('\n'));
                }
                Err(why) => prop_assert_eq!(why, "entry too long; shorten the text"),
            }
        }
    }

    #[test]
    fn known_issue_and_answer_close_only_their_finding() {
        let issue = |slice_name: &str, kind: &str, key: &str, finding: &str| Logged {
            slice: slice(slice_name),
            entry: Entry::KnownIssue {
                kind: kind.to_owned(),
                key: key.to_owned(),
                finding: finding.to_owned(),
                reason: "later".to_owned(),
                by: "opus:high".to_owned(),
            },
        };
        let answer = Logged {
            slice: slice("beta"),
            entry: Entry::Answer {
                kind: "second-opinion".to_owned(),
                key: "k1".to_owned(),
                finding: "F2".to_owned(),
                text: "yes".to_owned(),
                by: "opus:high".to_owned(),
            },
        };
        let entries = [
            issue("beta", "second-opinion", "k1", "F1"),
            answer,
            issue("beta", "second-opinion", "k2", "F3"),
            issue("beta", "informed-review", "k1", "F4"),
            issue("gamma", "second-opinion", "k1", "F5"),
        ];
        assert_eq!(
            closed(&entries, &slice("beta"), "second-opinion", "k1"),
            ["F1", "F2"]
        );
    }
}
