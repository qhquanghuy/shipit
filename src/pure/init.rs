use crate::pure::base::BranchName;
use crate::pure::failure::Failure;
use crate::pure::machine::ToolState;
use crate::pure::quote::json_string;
use crate::pure::setup::SETUP_PATH;
use crate::pure::text::comma_list;

pub(crate) const PACK_KEY: &str = "packs/model/claude/starting-setup.toml";
pub(crate) const CI_KEY: &str = "ci/shipit.yml";
pub(crate) const HOOK_KEY: &str = "git-hooks/pre-push";

pub(crate) const CI_FILE: &str = ".github/workflows/shipit.yml";

// The files in `data/`, as the build embeds them.
pub(crate) fn embedded(key: &str) -> Option<&'static str> {
    crate::DATA
        .iter()
        .find(|(name, _text)| *name == key)
        .map(|(_name, text)| *text)
}

// The base is given; else the branch origin's HEAD names; else the only local branch; else the
// branch an empty repository's HEAD points at.
pub(crate) fn choose_base(
    given: Option<BranchName>,
    origin_head: Option<&str>,
    locals: &[String],
    unborn: Option<&str>,
) -> Option<BranchName> {
    given
        .or_else(|| {
            origin_head
                .and_then(|name| name.strip_prefix("origin/"))
                .and_then(BranchName::parse)
        })
        .or_else(|| match locals {
            [only] => BranchName::parse(only),
            [] | [_, _, ..] => None,
        })
        .or_else(|| unborn.and_then(BranchName::parse))
}

// `pack` is the model pack's `[agents]` table. The base is quoted as a JSON string, which is a
// TOML string too.
pub(crate) fn starting_setup(base: &BranchName, pack: &str) -> String {
    let base = json_string(base.as_str());
    format!(
        "version = 1\n\
         \n\
         [gate]\n\
         base = {base}\n\
         max_files = 60\n\
         check = []  # your check commands, in order\n\
         \n\
         [gate.environment]\n\
         prefix = []  # for example [\"nix\", \"develop\", \"--command\"]\n\
         \n\
         {pack}\n\
         [packs]\n\
         skill = \"superpowers\"\n\
         domain = \"coding\"\n\
         model = \"claude\"\n\
         \n\
         [owner]\n\
         max_lines = 30\n\
         status_update_max_lines = 10\n"
    )
}

pub(crate) fn ci_job(template: &str, base: &BranchName) -> String {
    template.replace("BASE_BRANCH", &json_string(base.as_str()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookMode {
    Full,
    NoHook,
    HookOnly,
}

#[derive(Debug)]
pub(crate) struct InitFacts {
    pub(crate) setup_exists: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InitSteps {
    pub(crate) write_setup: bool,
    pub(crate) hook: bool,
}

// Every refusal is decided here, before the first file is written.
pub(crate) fn init_plan(facts: &InitFacts, mode: HookMode) -> Result<InitSteps, Failure> {
    let write_setup = mode != HookMode::HookOnly;
    if write_setup && facts.setup_exists {
        return Err(Failure::no(
            "a setup file already exists; init never overwrites it (in a new clone, run shipit init --hook-only)",
        ));
    }
    Ok(InitSteps {
        write_setup,
        hook: mode != HookMode::NoHook,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Start {
    NoCommits,
    NoOrigin,
    Pushed,
}

pub(crate) fn next_lines(base: &BranchName, start: Start) -> Vec<String> {
    let mut lines = Vec::new();
    match start {
        Start::NoCommits => lines.push(format!(
            "No commits yet: make a first commit on {base} (for example a README) and push it, then go on."
        )),
        Start::NoOrigin => lines.push(format!(
            "No origin/{base} yet: push {base} first, so check and CI compare with it."
        )),
        Start::Pushed => {}
    }
    lines.push(format!(
        "Next: fill in check in {SETUP_PATH}; commit that file alone on a new branch; shipit check; push; open pull request 1 (the setup file only)."
    ));
    lines.push(
        "After it merges: on a new branch, shipit init --ci-job, set SHIPIT_COMMIT to a ShipIt commit hash, and ship it as a normal slice (pull request 2)."
            .to_owned(),
    );
    lines.push(format!(
        "A branch with no commits beyond {base} exits 2 in shipit check: commit first, or pass --base {base}."
    ));
    lines
}

// None when every tool is there; a tool that gave no answer is not missing.
pub(crate) fn tools_line(tools: &[(String, ToolState)]) -> Option<String> {
    let missing: Vec<&str> = tools
        .iter()
        .filter(|(_tool, state)| *state == ToolState::Missing)
        .map(|(tool, _state)| tool.as_str())
        .collect();
    (!missing.is_empty()).then(|| {
        format!(
            "tools missing: {} (shipit doctor shows all)",
            comma_list(missing)
        )
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;
    use crate::pure::setup::{parse_setup, toml_table};
    use crate::pure::testing::name;

    fn base_name() -> impl Strategy<Value = BranchName> {
        "[a-zA-Z0-9\"\\\\/._é-]{1,12}"
            .prop_filter_map("a branch name", |text| BranchName::parse(&text))
    }

    fn pack() -> &'static str {
        embedded(PACK_KEY).expect("the pack is embedded")
    }

    fn parsed(text: &str) -> serde_json::Value {
        let table = toml_table(text.as_bytes()).expect("TOML");
        let setup = parse_setup(&table).expect("a valid setup");
        serde_json::to_value(&setup).expect("JSON")
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn starting_setup_parses_for_any_base(base in base_name()) {
            let setup = parsed(&starting_setup(&base, pack()));
            prop_assert_eq!(setup.pointer("/gate/base"), Some(&json!(base.as_str())));
            prop_assert_eq!(setup.pointer("/gate/max_files"), Some(&json!(60)));
            prop_assert_eq!(setup.pointer("/agents/parallel/slices"), Some(&json!(3)));
            prop_assert_eq!(setup.pointer("/agents/parallel/agents"), Some(&json!(4)));
        }

        #[test]
        fn choose_base_rule(
            given in proptest::option::of("[a-z]{1,4}"),
            origin in proptest::option::of("[a-z]{1,4}"),
            locals in proptest::collection::vec("[a-z]{1,4}", 0..3),
            unborn in proptest::option::of("[a-z]{1,4}"),
        ) {
            let origin_head = origin.as_ref().map(|name| format!("origin/{name}"));
            let chosen = choose_base(
                given.as_deref().and_then(BranchName::parse),
                origin_head.as_deref(),
                &locals,
                unborn.as_deref(),
            );
            let expected = given
                .or(origin)
                .or(match locals.as_slice() {
                    [only] => Some(only.clone()),
                    _ => None,
                })
                .or(unborn);
            prop_assert_eq!(chosen.map(|base| base.to_string()), expected);
        }

        #[test]
        fn init_plan_refuses_before_any_step(exists in any::<bool>(), mode in prop_oneof![
            Just(HookMode::Full), Just(HookMode::NoHook), Just(HookMode::HookOnly),
        ]) {
            let planned = init_plan(&InitFacts { setup_exists: exists }, mode);
            if exists && mode != HookMode::HookOnly {
                prop_assert_eq!(planned.map_err(|failure| failure.exit.code()), Err(1));
            } else {
                prop_assert_eq!(planned, Ok(InitSteps {
                    write_setup: mode != HookMode::HookOnly,
                    hook: mode != HookMode::NoHook,
                }));
            }
        }

        #[test]
        fn ci_job_quotes_the_base(base in base_name()) {
            let template = embedded(CI_KEY).expect("the CI job is embedded");
            let job = ci_job(template, &base);
            prop_assert!(!job.contains("BASE_BRANCH"));
            let wanted = format!("branches: [{}]", json_string(base.as_str()));
            prop_assert!(job.contains(&wanted));
        }

        #[test]
        fn next_lines_per_start(base in base_name()) {
            let common = next_lines(&base, Start::Pushed);
            prop_assert_eq!(common.len(), 3);
            let none = next_lines(&base, Start::NoCommits);
            prop_assert!(none.first().is_some_and(|line| line.starts_with("No commits yet: ")));
            prop_assert_eq!(none.get(1..), Some(common.as_slice()));
            let unpushed = next_lines(&base, Start::NoOrigin);
            prop_assert!(unpushed.first().is_some_and(|line| line.starts_with("No origin/")));
            prop_assert_eq!(unpushed.get(1..), Some(common.as_slice()));
            prop_assert!(common.iter().any(|line| line.contains("pull request 1")));
            prop_assert!(common.iter().any(|line| line.contains("pull request 2")));
        }

        #[test]
        fn only_a_missing_tool_is_listed(
            states in proptest::collection::vec(
                prop_oneof![
                    Just(ToolState::Missing),
                    Just(ToolState::NoAnswer),
                    "[a-z]{1,4}".prop_map(ToolState::Have),
                ],
                0..6,
            ),
        ) {
            let tools: Vec<(String, ToolState)> = states
                .iter()
                .enumerate()
                .map(|(at, state)| (format!("t{at}"), state.clone()))
                .collect();
            let line = tools_line(&tools);
            let missing: Vec<String> = tools
                .iter()
                .filter(|(_tool, state)| *state == ToolState::Missing)
                .map(|(tool, _state)| tool.clone())
                .collect();
            prop_assert_eq!(line.is_some(), !missing.is_empty());
            for tool in &tools {
                let listed = line.as_deref().is_some_and(|text| text.contains(&tool.0));
                prop_assert_eq!(listed, missing.contains(&tool.0));
            }
        }
    }

    #[test]
    fn the_embedded_files_are_there_and_the_hook_is_the_owner_line() {
        for key in [PACK_KEY, CI_KEY, HOOK_KEY] {
            assert!(embedded(key).is_some_and(|text| !text.is_empty()), "{key}");
        }
        assert_eq!(embedded("nope"), None);
        let hook = embedded(HOOK_KEY).expect("embedded");
        let lines: Vec<&str> = hook.lines().collect();
        assert_eq!(lines.first(), Some(&"#!/bin/sh"));
        assert_eq!(lines.get(1), Some(&"# installed by shipit init"));
        assert_eq!(lines.get(2), Some(&crate::pure::hooks::GUARDED_LINE));
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn the_pack_values_reach_the_setup() {
        let setup = parsed(&starting_setup(&name("main"), pack()));
        assert_eq!(setup.pointer("/agents/effort_cap"), Some(&json!("high")));
        assert_eq!(setup.pointer("/agents/roles/maker/tier"), Some(&json!(2)));
        assert_eq!(setup.pointer("/agents/roles/planner/tier"), Some(&json!(3)));
        assert_eq!(
            setup.pointer("/agents/tiers/3/stand_in/model"),
            Some(&json!("opus"))
        );
    }
}
