use toml::{Table, Value};

use crate::pure::baseline::setup_changes;
use crate::pure::branch::is_slice_branch;
use crate::pure::quote::{json_string, summary_safe, warning_safe};
use crate::pure::rules::{Parsed, Problem, fail};
use crate::pure::setup::toml_table;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Slice,
    Plain,
}

// One rule for the hook and for CI: a name that starts `slice/` gets the full rules.
pub(crate) fn mode(branch: &str) -> Mode {
    if is_slice_branch(branch) {
        Mode::Slice
    } else {
        Mode::Plain
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SetupStatus {
    Unchanged,
    Added,
    Removed,
    Changed(Vec<String>),
    Unreadable,
}

pub(crate) fn setup_change(merge_base: Option<&str>, head: Option<&str>) -> SetupStatus {
    match (merge_base, head) {
        (None, None) => SetupStatus::Unchanged,
        (None, Some(_)) => SetupStatus::Added,
        (Some(_), None) => SetupStatus::Removed,
        (Some(before), Some(after)) if before == after => SetupStatus::Unchanged,
        (Some(before), Some(after)) => {
            match (toml_table(before.as_bytes()), toml_table(after.as_bytes())) {
                (Ok(before), Ok(after)) => {
                    let changes = setup_changes(&before, &after);
                    if changes.is_empty() {
                        SetupStatus::Unchanged
                    } else {
                        SetupStatus::Changed(changes)
                    }
                }
                (Err(_why), _) | (_, Err(_why)) => SetupStatus::Unreadable,
            }
        }
    }
}

const CI_FOLDERS: [&str; 2] = [".github/workflows/", ".github/actions/"];
const CI_TOP_FILES: [&str; 2] = ["action.yml", "action.yaml"];

pub(crate) fn ci_paths(names: &[Vec<u8>]) -> Vec<String> {
    names
        .iter()
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .filter(|name| {
            CI_FOLDERS.iter().any(|folder| name.starts_with(folder))
                || CI_TOP_FILES.contains(&name.as_str())
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Checks {
    pub(crate) commands: Vec<String>,
    pub(crate) prefix: Vec<String>,
}

fn words(value: Option<&Value>, path: &str) -> Parsed<Vec<String>> {
    let Some(value) = value else {
        return fail(path, "missing");
    };
    let Some(items) = value.as_array() else {
        return fail(path, "must be an array of non-empty strings");
    };
    let mut problems = Vec::new();
    let mut found = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item.as_str().filter(|text| !text.is_empty()) {
            Some(text) => found.push(text.to_owned()),
            None => problems.push(Problem::new(
                format!("{path}[{index}]"),
                "must be a non-empty string",
            )),
        }
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

// Only these two keys are read, so that a newer schema on the base never stops an older ShipIt.
pub(crate) fn plain_checks(table: &Table) -> Parsed<Checks> {
    let gate = table.get("gate").and_then(Value::as_table);
    let prefix = gate
        .and_then(|gate| gate.get("environment"))
        .and_then(Value::as_table)
        .and_then(|environment| environment.get("prefix"));
    let commands = words(gate.and_then(|gate| gate.get("check")), ".gate.check");
    let prefix = words(prefix, ".gate.environment.prefix");
    match (commands, prefix) {
        (Ok(commands), Ok(prefix)) => Ok(Checks { commands, prefix }),
        (Err(mut problems), Err(more)) => {
            problems.extend(more);
            Err(problems)
        }
        (Err(problems), Ok(_)) | (Ok(_), Err(problems)) => Err(problems),
    }
}

#[derive(Debug)]
pub(crate) struct Report {
    pub(crate) base: String,
    pub(crate) branch: String,
    pub(crate) mode: Mode,
    pub(crate) adopting: bool,
    // HEAD is the base's own tip: a push to the base. Never decided by a branch name, which a
    // pull request's author picks.
    pub(crate) at_base_tip: bool,
    pub(crate) setup: SetupStatus,
    pub(crate) ci: Vec<String>,
}

fn quoted_list(values: &[String], one: fn(&str) -> String) -> String {
    values
        .iter()
        .map(|value| one(value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn setup_text(status: &SetupStatus, one: fn(&str) -> String) -> Option<String> {
    match status {
        SetupStatus::Unchanged => None,
        SetupStatus::Added => Some("added".to_owned()),
        SetupStatus::Removed => Some("removed".to_owned()),
        SetupStatus::Changed(keys) => Some(format!("changed: {}", quoted_list(keys, one))),
        SetupStatus::Unreadable => Some("changed (not readable as TOML)".to_owned()),
    }
}

// A value from git or from the pull request, as the summary shows it: quoted, and with nothing
// Markdown could turn into a tag, a link or an image.
fn shown(value: &str) -> String {
    summary_safe(&json_string(value))
}

// Each line is its own paragraph: a single line break would run them together on GitHub.
pub(crate) fn summary(report: &Report, check: Result<(), u8>) -> String {
    let base = shown(&report.base);
    let branch = shown(&report.branch);
    let mode = match report.mode {
        Mode::Slice => format!(
            "mode: slice (branch {branch}): size cap, setup comparison and coverage map apply"
        ),
        Mode::Plain => format!(
            "mode: plain (branch {branch} is not slice/*): only the base's check commands ran"
        ),
    };
    let gate = match (report.mode, report.adopting) {
        (Mode::Slice, false) => {
            format!("gate (max_files and check): from the base branch {base}")
        }
        (Mode::Slice, true) => format!(
            "adopting: {base} has no setup file, so this pull request may change the setup file only"
        ),
        (Mode::Plain, false) => format!("checks: from the base branch {base}"),
        (Mode::Plain, true) => {
            "checks: from this pull request's setup (the base has none)".to_owned()
        }
    };
    let setup = setup_text(&report.setup, shown).unwrap_or_else(|| "unchanged".to_owned());
    let ci = if report.ci.is_empty() {
        "unchanged".to_owned()
    } else {
        format!(
            "CHANGED: {} \u{2014} the pull request edits its own CI; read these files in the diff, recheck cannot judge them",
            quoted_list(&report.ci, shown)
        )
    };
    let verdict = match check {
        Ok(()) => "passed".to_owned(),
        Err(code) => format!("failed (exit {code})"),
    };
    let lines = [
        "## ShipIt recheck".to_owned(),
        mode,
        gate,
        format!("setup file: {setup}"),
        format!("CI job: {ci}"),
        format!("check: {verdict}"),
    ];
    format!("{}\n", lines.join("\n\n"))
}

// The line a failure before the checks leaves in the summary, instead of the report.
pub(crate) fn stopped(message: &str) -> String {
    format!("recheck stopped: {}\n", shown(message))
}

const WARNING: &str = "::warning title=ShipIt::";

pub(crate) fn warnings(report: &Report) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(text) = setup_text(&report.setup, json_string) {
        found.push(format!("this pull request changes the setup file: {text}"));
    }
    if !report.ci.is_empty() {
        found.push(format!(
            "this pull request changes the CI job: {}",
            quoted_list(&report.ci, json_string)
        ));
    }
    if report.adopting {
        found.push(format!(
            "this pull request adopts ShipIt: {} has no setup file",
            json_string(&report.base)
        ));
    }
    if report.mode == Mode::Plain && !report.at_base_tip {
        found.push(
            "not a slice/* branch: size cap and setup comparison were not applied".to_owned(),
        );
    }
    found
        .iter()
        .map(|text| format!("{WARNING}{}", warning_safe(text)))
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const NASTY: &str = "[a-z<>&`%:!#\n\r/.@()\\[\\]-]{0,12}";

    fn setup_text_of(version: u8, max_files: u8) -> String {
        format!("version = {version}\n[gate]\nmax_files = {max_files}\n")
    }

    fn report() -> impl Strategy<Value = Report> {
        (
            NASTY,
            NASTY,
            (any::<bool>(), any::<bool>(), any::<bool>()),
            proptest::collection::vec(NASTY, 0..3),
            proptest::collection::vec(NASTY, 0..3),
            0_u8..5,
        )
            .prop_map(
                |(base, branch, (slice, adopting, at_base_tip), keys, ci, kind)| Report {
                    base,
                    branch,
                    mode: if slice { Mode::Slice } else { Mode::Plain },
                    adopting,
                    at_base_tip,
                    setup: match kind {
                        0 => SetupStatus::Unchanged,
                        1 => SetupStatus::Added,
                        2 => SetupStatus::Removed,
                        3 => SetupStatus::Changed(keys),
                        _ => SetupStatus::Unreadable,
                    },
                    ci,
                },
            )
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn mode_is_slice_exactly_for_the_slice_prefix(text in ".{0,20}") {
            let expected = if text.starts_with("slice/") { Mode::Slice } else { Mode::Plain };
            prop_assert_eq!(mode(&text), expected);
            for plain in ["slice", "Slice/x", "x/slice/y", "slices/x", ""] {
                prop_assert_eq!(mode(plain), Mode::Plain);
            }
            prop_assert_eq!(mode(&format!("slice/{text}")), Mode::Slice);
        }

        #[test]
        fn setup_change_names_exactly_the_differing_keys(
            before in (0_u8..3, 0_u8..3),
            after in (0_u8..3, 0_u8..3),
        ) {
            let status = setup_change(
                Some(&setup_text_of(before.0, before.1)),
                Some(&setup_text_of(after.0, after.1)),
            );
            let mut named = Vec::new();
            if before.0 != after.0 {
                named.push("version".to_owned());
            }
            if before.1 != after.1 {
                named.push("gate.max_files".to_owned());
            }
            let expected = if named.is_empty() {
                SetupStatus::Unchanged
            } else {
                SetupStatus::Changed(named)
            };
            prop_assert_eq!(status, expected);
        }

        #[test]
        fn a_file_present_on_one_side_is_added_or_removed(text in "[a-z =0-9\n]{0,20}") {
            prop_assert_eq!(setup_change(None, Some(&text)), SetupStatus::Added);
            prop_assert_eq!(setup_change(Some(&text), None), SetupStatus::Removed);
            prop_assert_eq!(setup_change(Some(&text), Some(&text)), SetupStatus::Unchanged);
            prop_assert_eq!(setup_change(None, None), SetupStatus::Unchanged);
        }

        #[test]
        fn unreadable_toml_is_its_own_case(junk in "[a-z]{1,6} [a-z]{1,6}") {
            let good = setup_text_of(1, 1);
            prop_assert_eq!(setup_change(Some(&good), Some(&junk)), SetupStatus::Unreadable);
            prop_assert_eq!(setup_change(Some(&junk), Some(&good)), SetupStatus::Unreadable);
        }

        #[test]
        fn ci_paths_are_exactly_the_workflow_and_action_paths(
            tail in "[a-z/.]{0,10}",
            other in "[a-z]{1,6}/[a-z.]{1,6}",
        ) {
            let inside = [
                format!(".github/workflows/{tail}"),
                format!(".github/actions/{tail}"),
                "action.yml".to_owned(),
                "action.yaml".to_owned(),
            ];
            let outside = [
                format!(".github/{tail}x"),
                format!("sub/action.yml{tail}"),
                format!("sub/{other}/action.yml"),
                "action.yml.bak".to_owned(),
                other,
            ];
            let names: Vec<Vec<u8>> = inside
                .iter()
                .chain(outside.iter())
                .map(|name| name.as_bytes().to_vec())
                .collect();
            prop_assert_eq!(ci_paths(&names), inside.to_vec());
        }

        #[test]
        fn plain_checks_reads_only_check_and_environment(
            commands in proptest::collection::vec("[a-z ]{1,8}", 0..3),
            prefix in proptest::collection::vec("[a-z]{1,5}", 0..3),
            extra in 0_u8..9,
        ) {
            let list = |items: &[String]| {
                items.iter().map(|item| json_string(item)).collect::<Vec<_>>().join(", ")
            };
            let text = format!(
                "extra = {extra}\n[gate]\nbase = 1\nmax_files = \"x\"\ncheck = [{}]\nunknown = [1]\n[gate.environment]\nprefix = [{}]\nmore = {extra}\n[agents]\nroles = 3\n",
                list(&commands),
                list(&prefix),
            );
            let table = toml_table(text.as_bytes()).expect("toml");
            prop_assert_eq!(plain_checks(&table), Ok(Checks { commands, prefix }));
        }

        #[test]
        fn a_wrong_kind_in_check_or_prefix_names_its_path(extra in 0_u8..9) {
            let text = format!(
                "[gate]\ncheck = \"x\"\n[gate.environment]\nprefix = [{extra}]\n"
            );
            let table = toml_table(text.as_bytes()).expect("toml");
            let problems = plain_checks(&table).expect_err("wrong kinds");
            let shown: Vec<String> = problems.iter().map(ToString::to_string).collect();
            prop_assert_eq!(
                shown,
                vec![
                    ".gate.check: must be an array of non-empty strings".to_owned(),
                    ".gate.environment.prefix[0]: must be a non-empty string".to_owned(),
                ]
            );
            let none = toml_table(b"version = 1\n").expect("toml");
            prop_assert_eq!(plain_checks(&none).expect_err("missing").len(), 2);
        }

        #[test]
        fn no_value_can_start_a_command_or_hide_lines(report in report(), code in 1_u8..4) {
            for check in [Ok(()), Err(code)] {
                let text = summary(&report, check);
                let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
                prop_assert_eq!(lines.len(), 6);
                // a blank line between every two, so that each shows as its own paragraph
                prop_assert_eq!(text.lines().count(), 11);
                let prefixes = [
                    vec!["## ShipIt recheck"],
                    vec!["mode: slice (branch ", "mode: plain (branch "],
                    vec!["gate (max_files and check): ", "adopting: ", "checks: "],
                    vec!["setup file: "],
                    vec!["CI job: "],
                    vec!["check: "],
                ];
                for (line, allowed) in lines.iter().zip(&prefixes) {
                    prop_assert!(allowed.iter().any(|prefix| line.starts_with(prefix)), "{}", line);
                    // nothing a value says can open a tag, a code span, a link, an image or
                    // an autolink
                    prop_assert!(!line.contains(['<', '`', '[', ']', '@']));
                    prop_assert!(!line.contains("://"));
                }
            }
            for line in warnings(&report) {
                prop_assert!(line.starts_with("::warning title=ShipIt::"));
                prop_assert!(!line.contains(['\n', '\r']));
            }
            prop_assert_eq!(stopped(&report.branch).lines().count(), 1);
        }

        #[test]
        fn a_changed_ci_job_is_loud(
            mut report in report(),
            paths in proptest::collection::vec("[a-z/.]{1,8}", 1..3),
        ) {
            report.ci = paths;
            let text = summary(&report, Ok(()));
            let ci_line = text.lines().filter(|line| !line.is_empty()).nth(4);
            prop_assert!(ci_line.is_some_and(|line| line.starts_with("CI job: CHANGED: ")));
            let loud = warnings(&report)
                .iter()
                .filter(|line| line.contains("changes the CI job: "))
                .count();
            prop_assert_eq!(loud, 1);
        }

        #[test]
        fn the_plain_warning_is_given_except_at_the_base_tip(mut report in report()) {
            let plain = |report: &Report| {
                warnings(report).iter().any(|line| line.contains("not a slice/* branch"))
            };
            report.mode = Mode::Plain;
            // the branch name decides nothing: a fork's `main` is a branch named like the base
            report.branch.clone_from(&report.base);
            report.at_base_tip = false;
            prop_assert!(plain(&report));
            report.at_base_tip = true;
            prop_assert!(!plain(&report));
            report.mode = Mode::Slice;
            report.at_base_tip = false;
            prop_assert!(!plain(&report));
        }
    }
}
