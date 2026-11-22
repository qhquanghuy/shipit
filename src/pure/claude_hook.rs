// What the hooks of Claude Code hand in and expect back: the part of a hook event the gates
// read, and the one decision object they may print. Only the fields an event needs are read.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::pure::ship_gate::{Answer, Mode};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HookInput {
    pub(crate) tool_name: Option<String>,
    pub(crate) tool_input: Option<Value>,
    pub(crate) cwd: PathBuf,
    pub(crate) mode: Mode,
}

// Only the folder is required: every other field is read by the event that needs it, so a
// missing one is that event's bad input and never another's.
pub(crate) fn parse_input(bytes: &[u8]) -> Result<HookInput, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|why| format!("not JSON: {why}"))?;
    let object = value.as_object().ok_or("not a JSON object")?;
    let cwd = object
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| !cwd.is_empty())
        .ok_or("no cwd")?;
    let text = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(HookInput {
        tool_name: text("tool_name"),
        tool_input: object.get("tool_input").cloned(),
        cwd: PathBuf::from(cwd),
        mode: Mode::parse(text("permission_mode").as_deref().unwrap_or("")),
    })
}

pub(crate) fn answer_json(answer: &Answer) -> Option<String> {
    let (decision, reason) = match answer {
        Answer::Pass => return None,
        Answer::Ask(reason) => ("ask", reason),
        Answer::Deny(reason) => ("deny", reason),
    };
    Some(
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": decision,
                "permissionDecisionReason": reason,
            }
        })
        .to_string(),
    )
}

pub(crate) fn resolve(cwd: &Path, dir: Option<&str>) -> PathBuf {
    dir.map_or_else(|| cwd.to_path_buf(), |dir| cwd.join(dir))
}

// A tool of an MCP server whose name says it merges, releases, tags or pushes.
pub(crate) fn mcp_ship(tool_name: &str) -> Option<String> {
    let rest = tool_name.strip_prefix("mcp__")?;
    let tool = rest.rsplit("__").next().unwrap_or(rest);
    ["merge", "create_release", "create_tag", "push"]
        .iter()
        .any(|word| tool.contains(word))
        .then(|| format!("calls {tool_name}"))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_folder_is_read_from_the_cwd_and_joined_with_a_relative_dir(
            cwd in "/[a-z]{1,6}(/[a-z]{1,6}){0,2}",
            dir in "[a-z]{1,6}(/[a-z]{1,6}){0,2}",
        ) {
            let base = Path::new(&cwd);
            prop_assert_eq!(resolve(base, None), base.to_path_buf());
            prop_assert_eq!(resolve(base, Some(&dir)), base.join(&dir));
            prop_assert_eq!(resolve(base, Some("/abs")), PathBuf::from("/abs"));
        }

        #[test]
        fn only_a_missing_or_non_string_cwd_or_non_json_is_refused(
            cwd in prop_oneof![
                Just(json!(null)), Just(json!(3)), Just(json!(["x"])), Just(json!({})),
                Just(json!("")), "/[a-z]{1,8}".prop_map(Value::from),
            ],
            with_cwd in any::<bool>(),
            name in proptest::option::of(prop_oneof![Just(json!("Bash")), Just(json!(7))]),
            mode in proptest::option::of(prop_oneof![Just(json!("auto")), Just(json!(false))]),
            input in proptest::option::of(Just(json!({"command": "ls"}))),
            junk in "[a-z ]{0,8}",
        ) {
            let mut object = serde_json::Map::new();
            if with_cwd {
                object.insert("cwd".to_owned(), cwd.clone());
            }
            for (key, value) in [("tool_name", name), ("permission_mode", mode), ("tool_input", input)] {
                if let Some(value) = value {
                    object.insert(key.to_owned(), value);
                }
            }
            let bytes = Value::Object(object).to_string();
            let parsed = parse_input(bytes.as_bytes());
            let good_cwd = with_cwd && cwd.as_str().is_some_and(|text| !text.is_empty());
            prop_assert_eq!(parsed.is_ok(), good_cwd);
            let broken = format!("x{junk}[");
            prop_assert!(parse_input(broken.as_bytes()).is_err());
            prop_assert!(parse_input(b"[1]").is_err());
        }

        #[test]
        fn mcp_names_that_say_merge_release_tag_or_push_are_ship_tools(
            server in "[a-z]{1,6}",
            tool in "[a-z_]{1,16}",
        ) {
            let name = format!("mcp__{server}__{tool}");
            let ship = ["merge", "create_release", "create_tag", "push"]
                .iter()
                .any(|word| tool.contains(word));
            prop_assert_eq!(mcp_ship(&name), ship.then(|| format!("calls {name}")));
            prop_assert_eq!(mcp_ship(&format!("Bash{tool}")), None);
            prop_assert_eq!(mcp_ship(&format!("{server}__merge")), None);
        }
    }

    #[test]
    fn the_other_fields_are_optional_and_read_when_present() {
        let input = parse_input(br#"{"cwd":"/r","tool_name":"Bash","permission_mode":"auto","tool_input":{"command":"ls"}}"#)
            .expect("reads");
        assert_eq!(input.tool_name.as_deref(), Some("Bash"));
        assert_eq!(input.mode, Mode::Auto);
        assert_eq!(input.tool_input, Some(json!({"command": "ls"})));
        let bare = parse_input(br#"{"cwd":"/r"}"#).expect("reads");
        assert_eq!(bare.tool_name, None);
        assert_eq!(bare.tool_input, None);
        assert_eq!(bare.mode, Mode::Unknown);
    }

    #[test]
    fn a_decision_is_exactly_one_object() {
        assert_eq!(answer_json(&Answer::Pass), None);
        for (answer, decision) in [
            (Answer::Ask("why".to_owned()), "ask"),
            (Answer::Deny("why".to_owned()), "deny"),
        ] {
            let text = answer_json(&answer).expect("a decision");
            let value: Value = serde_json::from_str(&text).expect("JSON");
            assert_eq!(
                value,
                json!({"hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": decision,
                    "permissionDecisionReason": "why",
                }})
            );
            assert!(!text.contains('\n'));
        }
    }
}
