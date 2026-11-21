// What `init` does about the pre-push hook. Nothing here replaces or rewrites a file: a hook is
// written new, or one line is appended to it, or the line is only printed with the reason.

// Guarded, so that a teammate without ShipIt pushes as before; the `&&` form would fail their push.
pub(crate) const GUARDED_LINE: &str =
    "if command -v shipit >/dev/null 2>&1; then exec shipit pre-push \"$@\"; fi";

const ALREADY: &str = "pre-push hook: already installed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookFile {
    Missing,
    Present(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unsafe {
    OutsideProject,
    NotShell,
    EndsEarly,
    ReadsInput,
}

impl Unsafe {
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Self::OutsideProject => "it is outside this project",
            Self::NotShell => "it is not a sh or bash script",
            Self::EndsEarly => "a line starts with exit or exec, so a line after it never runs",
            Self::ReadsInput => {
                "it uses git lfs or reads git's input, which ShipIt's line needs too"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookStep {
    Write { path: String, text: String },
    Append { path: String, line: String },
    Print { reason: String, text: String },
    Nothing(String),
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn is_shell(name: &str) -> bool {
    matches!(name, "sh" | "bash")
}

// `#!/bin/sh`, `#!/usr/bin/env bash`: the interpreter, or the program `env` is asked for.
fn shebang_is_shell(first: &str) -> bool {
    let Some(rest) = first.strip_prefix("#!") else {
        return false;
    };
    let mut words = rest.split_whitespace();
    match words.next().map(basename) {
        Some("env") => words
            .find(|word| !word.starts_with('-'))
            .is_some_and(|word| is_shell(basename(word))),
        Some(name) => is_shell(name),
        None => false,
    }
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn ends_early(line: &str) -> bool {
    let line = line.trim_start();
    ["exit", "exec"].iter().any(|word| {
        line.strip_prefix(word)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
}

fn reads_input(line: &str) -> bool {
    if is_comment(line) {
        return false;
    }
    let word = |text: &str| {
        line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|token| token == text)
    };
    word("lfs")
        || word("read")
        || word("cat")
        || line.contains("<&0")
        || line.split_whitespace().any(|token| token == "-")
}

// In this order; the first that holds is the reason. A husky file may have no shebang, because
// husky runs its hooks with sh.
pub(crate) fn append_safety(inside_project: bool, husky: bool, text: &str) -> Result<(), Unsafe> {
    if !inside_project {
        return Err(Unsafe::OutsideProject);
    }
    let first = text.lines().next().unwrap_or_default();
    let shell = if first.starts_with("#!") {
        shebang_is_shell(first)
    } else {
        husky
    };
    if !shell {
        return Err(Unsafe::NotShell);
    }
    if text.lines().any(ends_early) {
        return Err(Unsafe::EndsEarly);
    }
    if text.lines().any(reads_input) {
        return Err(Unsafe::ReadsInput);
    }
    Ok(())
}

pub(crate) fn already_installed(text: &str) -> bool {
    text.lines().any(|line| line.contains("shipit pre-push"))
}

pub(crate) fn plain_hook_plan(
    path: &str,
    inside_project: bool,
    husky: bool,
    hook: &HookFile,
    new_hook: &str,
) -> HookStep {
    let print = |reason: Unsafe| HookStep::Print {
        reason: reason.reason().to_owned(),
        text: GUARDED_LINE.to_owned(),
    };
    match hook {
        HookFile::Missing if inside_project => HookStep::Write {
            path: path.to_owned(),
            text: new_hook.to_owned(),
        },
        HookFile::Missing => print(Unsafe::OutsideProject),
        HookFile::Present(text) if already_installed(text) => HookStep::Nothing(ALREADY.to_owned()),
        HookFile::Present(text) => match append_safety(inside_project, husky, text) {
            Ok(()) => HookStep::Append {
                path: path.to_owned(),
                line: GUARDED_LINE.to_owned(),
            },
            Err(reason) => print(reason),
        },
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::init::embedded;

    const SAFE: &str = "#!/bin/sh\necho hello\n";

    fn new_hook() -> &'static str {
        embedded("git-hooks/pre-push").expect("the hook is embedded")
    }

    fn word() -> impl Strategy<Value = String> {
        "[a-z]{1,6}".prop_filter("not a stop word", |w| w != "exit" && w != "exec")
    }

    fn unsafe_line() -> impl Strategy<Value = (Unsafe, String)> {
        prop_oneof![
            word().prop_map(|w| (Unsafe::EndsEarly, format!("exit {w}"))),
            word().prop_map(|w| (Unsafe::EndsEarly, format!("  exec {w}"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("git lfs pre-push {w}"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("while read {w}; do :; done"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("cat | {w}"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("{w} <&0"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("{w} -"))),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn append_safety_reasons(
            (reason, line) in unsafe_line(),
            shebang in prop_oneof![
                Just("#!/bin/sh"), Just("#!/bin/bash"), Just("#!/usr/bin/env bash"),
                Just("#!/usr/bin/env -S sh"),
            ],
            comment in "[a-z ]{0,8}",
        ) {
            let script = format!("{shebang}\n# {comment}\necho hi\n");
            prop_assert_eq!(append_safety(true, false, &script), Ok(()));
            let with = format!("{script}{line}\n");
            prop_assert_eq!(append_safety(true, false, &with), Err(reason));
            // a comment never counts
            let commented = format!("{script}# {line}\n");
            prop_assert_eq!(append_safety(true, false, &commented), Ok(()));
            // the reasons are tried in the order: project, shell, ends early, input
            prop_assert_eq!(append_safety(false, false, &with), Err(Unsafe::OutsideProject));
            let python = with.replacen(shebang, "#!/usr/bin/python3", 1);
            prop_assert_eq!(append_safety(true, false, &python), Err(Unsafe::NotShell));
            let both = format!("{script}git lfs pre-push\nexit 0\n");
            prop_assert_eq!(append_safety(true, false, &both), Err(Unsafe::EndsEarly));
        }

        #[test]
        fn a_husky_file_without_a_shebang_is_sh(body in "[a-z ]{0,10}") {
            let text = format!("echo {body}\n");
            prop_assert_eq!(append_safety(true, true, &text), Ok(()));
            prop_assert_eq!(append_safety(true, false, &text), Err(Unsafe::NotShell));
            let python = format!("#!/usr/bin/python3\n{text}");
            prop_assert_eq!(append_safety(true, true, &python), Err(Unsafe::NotShell));
        }

        #[test]
        fn plain_hook_plan_never_replaces(
            text in prop_oneof![Just(SAFE.to_owned()), "[a-z#!/ \n]{0,30}"],
            present in any::<bool>(),
            inside in any::<bool>(),
        ) {
            let hook = if present { HookFile::Present(text) } else { HookFile::Missing };
            match plain_hook_plan("p", inside, false, &hook, new_hook()) {
                HookStep::Write { .. } => prop_assert_eq!(hook, HookFile::Missing),
                HookStep::Append { .. } => prop_assert!(matches!(hook, HookFile::Present(_))),
                HookStep::Print { .. } | HookStep::Nothing(_) => {}
            }
        }

        #[test]
        fn a_second_plan_never_writes(
            text in prop_oneof![
                Just(SAFE.to_owned()), Just("#!/usr/bin/python3\n".to_owned()),
                Just("#!/bin/sh\nexit 0\n".to_owned()), "[a-z#!/ \n]{0,30}",
            ],
            present in any::<bool>(),
            inside in any::<bool>(),
        ) {
            let hook = if present { HookFile::Present(text) } else { HookFile::Missing };
            let first = plain_hook_plan("p", inside, false, &hook, new_hook());
            let after = match &first {
                HookStep::Write { text, .. } => HookFile::Present(text.clone()),
                HookStep::Append { line, .. } => match &hook {
                    HookFile::Present(old) => HookFile::Present(format!("{old}\n{line}\n")),
                    HookFile::Missing => HookFile::Missing,
                },
                HookStep::Print { .. } | HookStep::Nothing(_) => hook.clone(),
            };
            let second = plain_hook_plan("p", inside, false, &after, new_hook());
            match first {
                HookStep::Write { .. } | HookStep::Append { .. } => {
                    prop_assert_eq!(second, HookStep::Nothing(ALREADY.to_owned()));
                }
                HookStep::Print { .. } | HookStep::Nothing(_) => {
                    prop_assert_eq!(&after, &hook);
                    let writes = matches!(
                        second,
                        HookStep::Write { .. } | HookStep::Append { .. }
                    );
                    prop_assert!(!writes);
                }
            }
        }
    }

    #[test]
    fn the_guarded_line_is_the_owner_line_fixed() {
        assert_eq!(
            GUARDED_LINE,
            r#"if command -v shipit >/dev/null 2>&1; then exec shipit pre-push "$@"; fi"#
        );
        assert!(already_installed(GUARDED_LINE));
        assert!(new_hook().ends_with(&format!("{GUARDED_LINE}\n")));
    }
}
