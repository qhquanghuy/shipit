// What the hooks of Claude Code hand in and expect back: the part of a hook event the gates
// read, and the one decision object they may print. Only the fields an event needs are read.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::pure::agent_ledger::safe_id;
use crate::pure::ship_gate::{Answer, Mode};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HookInput {
    pub(crate) tool_name: Option<String>,
    pub(crate) tool_input: Option<Value>,
    pub(crate) cwd: PathBuf,
    pub(crate) mode: Mode,
    pub(crate) event: String,
    pub(crate) session_id: Option<String>,
    pub(crate) tool_use_id: Option<String>,
    pub(crate) agent_id: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) background_subagents: Option<usize>,
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
        event: text("hook_event_name").unwrap_or_default(),
        session_id: text("session_id"),
        tool_use_id: text("tool_use_id"),
        agent_id: text("agent_id"),
        source: text("source"),
        background_subagents: object
            .get("background_tasks")
            .and_then(Value::as_array)
            .map(|tasks| {
                tasks
                    .iter()
                    .filter(|task| task.get("type").and_then(Value::as_str) == Some("subagent"))
                    .count()
            }),
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

pub(crate) const WORK_REVIEWER: &str = "shipit:work-reviewer";
pub(crate) const TESTER: &str = "shipit:tester";
const BRANCH_LINE: &str = "shipit-branch:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    NotGated,
    Gated {
        role: String,
        branch: Option<String>,
    },
    Bad(String),
}

// A reviewer of the work or a tester is started with the Agent tool; the branch it is to judge
// is the one named by a line of the prompt. Two different names, or an empty one, cannot be
// judged and are refused.
pub(crate) fn review_target(input: &HookInput) -> Target {
    let tool_input = input.tool_input.as_ref();
    let text = |key: &str| {
        tool_input
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
    };
    let Some(role) = text("subagent_type").filter(|role| [WORK_REVIEWER, TESTER].contains(role))
    else {
        return Target::NotGated;
    };
    if !agent_call(input) {
        return Target::NotGated;
    }
    let mut named: Vec<&str> = Vec::new();
    for line in text("prompt").unwrap_or_default().lines() {
        let Some(rest) = line.trim().strip_prefix(BRANCH_LINE) else {
            continue;
        };
        let Some(word) = rest.split_whitespace().next() else {
            return Target::Bad("the shipit-branch line names no branch".to_owned());
        };
        if !named.contains(&word) {
            named.push(word);
        }
    }
    match named.as_slice() {
        [] => Target::Gated {
            role: role.to_owned(),
            branch: None,
        },
        [branch] => Target::Gated {
            role: role.to_owned(),
            branch: Some((*branch).to_owned()),
        },
        [_, _, ..] => Target::Bad("the prompt names two branches".to_owned()),
    }
}

// A field that becomes a file name: it must be there and be made of safe characters.
pub(crate) fn safe_field<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    value
        .filter(|text| safe_id(text))
        .ok_or_else(|| format!("the hook input has no safe {name}"))
}

pub(crate) fn agent_call(input: &HookInput) -> bool {
    matches!(input.tool_name.as_deref(), Some("Agent" | "Task"))
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

    fn bare(tool: &str, tool_input: Value) -> HookInput {
        HookInput {
            tool_name: Some(tool.to_owned()),
            tool_input: Some(tool_input),
            cwd: PathBuf::from("/r"),
            mode: Mode::Auto,
            event: String::new(),
            session_id: None,
            tool_use_id: None,
            agent_id: None,
            source: None,
            background_subagents: None,
        }
    }

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
        fn branch_line_rules(
            first in "[a-z][a-z/0-9-]{0,10}",
            second in "[a-z][a-z/0-9-]{0,10}",
            before in "[a-zA-Z ]{0,10}",
            role in prop_oneof![Just(WORK_REVIEWER), Just(TESTER)],
        ) {
            let call = |prompt: String| {
                review_target(&bare("Agent", json!({"subagent_type": role, "prompt": prompt})))
            };
            let gated = |branch: Option<&str>| Target::Gated {
                role: role.to_owned(),
                branch: branch.map(str::to_owned),
            };
            prop_assert_eq!(call(before.clone()), gated(None));
            prop_assert_eq!(
                call(format!("{before}\n  shipit-branch: {first}\nmore")),
                gated(Some(&first))
            );
            prop_assert_eq!(
                call(format!("shipit-branch: {first}\n{before}\nshipit-branch:   {first}")),
                gated(Some(&first))
            );
            if first != second {
                prop_assert_eq!(
                    call(format!("shipit-branch: {first}\nshipit-branch: {second}")),
                    Target::Bad("the prompt names two branches".to_owned())
                );
            }
            prop_assert_eq!(
                call("shipit-branch:  ".to_owned()),
                Target::Bad("the shipit-branch line names no branch".to_owned())
            );
        }

        #[test]
        fn gated_iff_work_reviewer_or_tester(
            tool in prop_oneof![Just("Agent"), Just("Task"), Just("Bash"), Just("Read")],
            kind in prop_oneof![
                Just(WORK_REVIEWER.to_owned()), Just(TESTER.to_owned()),
                Just("shipit:plan-reviewer".to_owned()), Just("general-purpose".to_owned()),
                "[a-z:-]{0,16}",
            ],
        ) {
            let target = review_target(&bare(tool, json!({"subagent_type": kind, "prompt": "p"})));
            let gated = matches!(tool, "Agent" | "Task") && (kind == WORK_REVIEWER || kind == TESTER);
            prop_assert_eq!(target != Target::NotGated, gated);
        }

        #[test]
        fn background_tasks_count_only_the_subagents(subagents in 0_usize..5, others in 0_usize..5) {
            let mut tasks = vec![json!({"type": "subagent"}); subagents];
            tasks.extend(vec![json!({"type": "shell"}); others]);
            tasks.push(json!("not an object"));
            let text = json!({"cwd": "/r", "background_tasks": tasks}).to_string();
            prop_assert_eq!(
                parse_input(text.as_bytes()).map(|input| input.background_subagents),
                Ok(Some(subagents))
            );
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
        let minimal = parse_input(br#"{"cwd":"/r"}"#).expect("reads");
        assert_eq!(minimal.tool_name, None);
        assert_eq!(minimal.tool_input, None);
        assert_eq!(minimal.mode, Mode::Unknown);
        assert_eq!(minimal.event, "");
        assert_eq!(minimal.session_id, None);
        assert_eq!(minimal.background_subagents, None);
        let event = parse_input(
            br#"{"cwd":"/r","hook_event_name":"SubagentStart","session_id":"s","tool_use_id":"t","agent_id":"a","source":"startup"}"#,
        )
        .expect("reads");
        assert_eq!(event.event, "SubagentStart");
        assert_eq!(
            (
                event.session_id,
                event.tool_use_id,
                event.agent_id,
                event.source
            ),
            (
                Some("s".to_owned()),
                Some("t".to_owned()),
                Some("a".to_owned()),
                Some("startup".to_owned())
            )
        );
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
