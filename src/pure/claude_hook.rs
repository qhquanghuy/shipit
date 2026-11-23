// What the hooks of Claude Code hand in and expect back: the part of a hook event the gates
// read, and the one decision object they may print. Only the fields an event needs are read.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::pure::agent_ledger::safe_id;
use crate::pure::branch::is_slice_branch;
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
    pub(crate) agent_type: Option<String>,
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
        agent_type: text("agent_type"),
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
        // A branch name cannot end in a full stop, so a sentence's own is not part of it.
        let word = rest
            .split_whitespace()
            .next()
            .map(|word| word.trim_end_matches('.'))
            .filter(|word| !word.is_empty());
        let Some(word) = word else {
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

// A folder the reader gives as `~/x` is the home folder's; with no home it stays under the working
// folder, where it names nothing and so no project.
pub(crate) fn resolve(cwd: &Path, home: Option<&Path>, dir: Option<&str>) -> PathBuf {
    let Some(dir) = dir else {
        return cwd.to_path_buf();
    };
    match (dir.strip_prefix("~/"), home) {
        (Some(under), Some(home)) => home.join(under),
        (Some(_) | None, Some(_) | None) => cwd.join(dir),
    }
}

const MCP_SHIP_WORDS: [&str; 3] = ["merge", "create_release", "create_tag"];
const MCP_FILE_WRITES: [&str; 5] = [
    "create_or_update_file",
    "delete_file",
    "push_files",
    "update_file",
    "write_file",
];
const MCP_READS: [&str; 6] = ["get_", "list_", "read_", "search_", "fetch_", "find_"];

// A tool of an MCP server that ships: one whose name says it merges, releases, tags or pushes,
// or that writes to a branch (a file write, or any tool that is given a `branch` and is not a
// read) unless that branch is a slice branch. A write that names no branch asks too.
pub(crate) fn mcp_ship(tool_name: &str, tool_input: Option<&Value>) -> Option<String> {
    let rest = tool_name.strip_prefix("mcp__")?;
    let tool = rest.rsplit("__").next().unwrap_or(rest);
    let calls = || Some(format!("calls {tool_name}"));
    if MCP_SHIP_WORDS.iter().any(|word| tool.contains(word)) {
        return calls();
    }
    let branch = tool_input.and_then(|input| input.get("branch"));
    let reads = MCP_READS.iter().any(|prefix| tool.starts_with(prefix));
    let writes_a_branch =
        MCP_FILE_WRITES.iter().any(|word| tool.contains(word)) || (branch.is_some() && !reads);
    if writes_a_branch {
        let slice = branch.and_then(Value::as_str).is_some_and(is_slice_branch);
        return (!slice)
            .then(|| format!("calls {tool_name} for a branch that is not a slice branch"));
    }
    if tool.contains("push") { calls() } else { None }
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
            agent_type: None,
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
            home in "/[a-z]{1,6}",
        ) {
            let base = Path::new(&cwd);
            let at_home = Path::new(&home);
            for home in [None, Some(at_home)] {
                prop_assert_eq!(resolve(base, home, None), base.to_path_buf());
                prop_assert_eq!(resolve(base, home, Some(&dir)), base.join(&dir));
                prop_assert_eq!(resolve(base, home, Some("/abs")), PathBuf::from("/abs"));
            }
            // `~/x` is the home folder's, and without a home it names nothing under the cwd.
            let tilde = format!("~/{dir}");
            prop_assert_eq!(resolve(base, Some(at_home), Some(&tilde)), at_home.join(&dir));
            prop_assert_eq!(resolve(base, None, Some(&tilde)), base.join(&tilde));
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
            // A branch name cannot end in a full stop, so a sentence's own is not the name's.
            prop_assert_eq!(call(format!("shipit-branch: {first}.")), gated(Some(&first)));
            prop_assert_eq!(
                call(format!("shipit-branch: {first}\nshipit-branch: {first}.")),
                gated(Some(&first))
            );
            prop_assert_eq!(
                call("shipit-branch: ..".to_owned()),
                Target::Bad("the shipit-branch line names no branch".to_owned())
            );
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
            prop_assume!(!["file", "update", "write", "delete"].iter().any(|word| tool.contains(word)));
            let name = format!("mcp__{server}__{tool}");
            let ship = ["merge", "create_release", "create_tag", "push"]
                .iter()
                .any(|word| tool.contains(word));
            prop_assert_eq!(mcp_ship(&name, None), ship.then(|| format!("calls {name}")));
            prop_assert_eq!(mcp_ship(&format!("Bash{tool}"), None), None);
            prop_assert_eq!(mcp_ship(&format!("{server}__merge"), None), None);
        }

        #[test]
        fn an_mcp_write_to_a_branch_asks_unless_the_branch_is_a_slice_branch(
            goal in "[a-z]{1,5}",
            slice in "[a-z]{1,5}",
            other in "[a-z]{1,6}",
            tool in prop_oneof![
                Just("create_or_update_file"), Just("delete_file"), Just("push_files"),
                Just("update_file"), Just("write_file"), Just("edit_things"), Just("create_branch"),
            ],
        ) {
            let name = format!("mcp__github__{tool}");
            let on = |branch: Value| mcp_ship(&name, Some(&json!({"branch": branch})));
            prop_assert_eq!(on(json!(format!("slice/{goal}/{slice}"))), None);
            prop_assert!(on(json!("main")).is_some());
            prop_assert!(on(json!(other)).is_some());
            prop_assert!(on(json!(7)).is_some());
            let file_tool = !matches!(tool, "edit_things" | "create_branch");
            let asks_without_a_branch = mcp_ship(&name, Some(&json!({"path": "x"}))).is_some();
            prop_assert_eq!(asks_without_a_branch, file_tool);
            // A read that is given a branch is not a write.
            for read in ["get_file_contents", "list_commits", "search_code"] {
                let name = format!("mcp__github__{read}");
                prop_assert_eq!(mcp_ship(&name, Some(&json!({"branch": "main"}))), None);
            }
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
