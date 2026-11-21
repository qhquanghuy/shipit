use serde::Serialize;

use crate::pure::roadmap::SliceName;

// One write_all of at most this many bytes, newline included, is not interleaved with another
// writer's line.
const MAX_LINE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub(crate) enum Entry {
    Receipt {
        kind: String,
        key: String,
        result: String,
        findings: u64,
        author: String,
        by: String,
        rank: String,
    },
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

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::Value;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn an_entry_is_one_line_within_the_limit_or_refused(
            key in "\\PC{0,5000}",
            slice in "[a-z][a-z0-9-]{0,8}",
        ) {
            prop_assume!(slice != "roadmap");
            let slice_text = slice.clone();
            let slice = SliceName::parse(&slice).expect("a slice name");
            let entry = Entry::Receipt {
                kind: "plan-review".to_owned(),
                key: key.clone(),
                result: "approve".to_owned(),
                findings: 0,
                author: "opus:high".to_owned(),
                by: "fable:high".to_owned(),
                rank: "tier".to_owned(),
            };
            match line(&entry, &slice, "2026-11-21T10:00:00Z", "shipit 0.1.0") {
                Ok(text) => {
                    prop_assert!(text.len() <= MAX_LINE);
                    prop_assert_eq!(text.matches('\n').count(), 1);
                    prop_assert!(text.ends_with('\n'));
                    let value: Value = serde_json::from_str(&text).expect("JSON");
                    prop_assert_eq!(value.get("event").and_then(Value::as_str), Some("receipt"));
                    prop_assert_eq!(value.get("key").and_then(Value::as_str), Some(key.as_str()));
                    prop_assert_eq!(
                        value.get("slice").and_then(Value::as_str),
                        Some(slice_text.as_str())
                    );
                }
                Err(why) => {
                    prop_assert_eq!(why, "entry too long; shorten the text");
                }
            }
        }
    }
}
