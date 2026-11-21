// What `init` does about the pre-push hook. Nothing here replaces or rewrites a file: a hook is
// written new, or one line is appended to it, or the line is only printed with the reason.

// Guarded, so that a teammate without ShipIt pushes as before; the `&&` form would fail their push.
pub(crate) const GUARDED_LINE: &str =
    "if command -v shipit >/dev/null 2>&1; then exec shipit pre-push \"$@\"; fi";

pub(crate) const ALREADY_INSTALLED: &str = "pre-push hook: already installed";

// A file that is there but cannot be read as text is its own case, so that no write is planned
// for it: every refusal is decided before the first file is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookFile {
    Missing,
    Present(String),
    NotExecutable(String),
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unsafe {
    OutsideProject,
    Unreadable,
    NotExecutable,
    NotShell,
    EndsEarly,
    ReadsInput,
}

impl Unsafe {
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Self::OutsideProject => "it is outside this project",
            Self::Unreadable => "it cannot be read as text, so ShipIt cannot tell what it does",
            Self::NotExecutable => "it is not executable, so git would skip it",
            Self::NotShell => "it is not a sh or bash script",
            Self::EndsEarly => "it has exit or exec, so a line after it may never run",
            Self::ReadsInput => {
                "it uses git lfs or reads git's input, which ShipIt's line needs too"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookStep {
    Write {
        path: String,
        text: String,
    },
    Append {
        path: String,
        line: String,
    },
    Print {
        path: String,
        reason: String,
        text: String,
    },
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

// The words of a line, split at anything that is not part of a name.
fn has_word(line: &str, words: &[&str]) -> bool {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|token| words.contains(&token))
}

// A word anywhere, not only at the start of a line: `npm test; exit 0` ends the hook as well.
fn ends_early(line: &str) -> bool {
    !is_comment(line) && has_word(line, &["exit", "exec"])
}

// Whatever may drain git's input before ShipIt's line gets it; over-refusing only prints.
const READERS: [&str; 10] = [
    "read", "cat", "head", "tail", "grep", "sed", "awk", "while", "xargs", "lfs",
];

fn reads_input(line: &str) -> bool {
    !is_comment(line)
        && (has_word(line, &READERS)
            || line.contains("<&0")
            || line.split_whitespace().any(|token| token == "-"))
}

// In this order; the first that holds is the reason. A husky file may have no shebang, because
// husky runs its hooks with sh, and needs no executable bit for the same reason.
pub(crate) fn append_safety(
    inside_project: bool,
    husky: bool,
    executable: bool,
    text: &str,
) -> Result<(), Unsafe> {
    if !inside_project {
        return Err(Unsafe::OutsideProject);
    }
    if !executable && !husky {
        return Err(Unsafe::NotExecutable);
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
    text.lines()
        .any(|line| !is_comment(line) && line.contains("shipit pre-push"))
}

pub(crate) fn plain_hook_plan(
    path: &str,
    inside_project: bool,
    husky: bool,
    hook: &HookFile,
    new_hook: &str,
) -> HookStep {
    let print = |reason: Unsafe| HookStep::Print {
        path: path.to_owned(),
        reason: reason.reason().to_owned(),
        text: GUARDED_LINE.to_owned(),
    };
    match hook {
        HookFile::Missing if inside_project => HookStep::Write {
            path: path.to_owned(),
            text: new_hook.to_owned(),
        },
        HookFile::Missing => print(Unsafe::OutsideProject),
        HookFile::Unreadable => print(Unsafe::Unreadable),
        HookFile::Present(text) | HookFile::NotExecutable(text) => {
            let executable = matches!(hook, HookFile::Present(_));
            if (executable || husky) && already_installed(text) {
                return HookStep::Nothing(ALREADY_INSTALLED.to_owned());
            }
            match append_safety(inside_project, husky, executable, text) {
                Ok(()) => HookStep::Append {
                    path: path.to_owned(),
                    line: GUARDED_LINE.to_owned(),
                },
                Err(reason) => print(reason),
            }
        }
    }
}

// Every documented main config of lefthook that `init` has to know about.
pub(crate) const LEFTHOOK_MAIN: &[&str] = &[
    "lefthook.yml",
    ".lefthook.yml",
    "lefthook.yaml",
    ".lefthook.yaml",
    ".config/lefthook.yml",
    ".config/lefthook.yaml",
    "lefthook.toml",
    ".lefthook.toml",
    "lefthook.json",
    ".lefthook.json",
    "lefthook.jsonc",
    ".lefthook.jsonc",
];

pub(crate) const PRE_COMMIT_MAIN: &str = ".pre-commit-config.yaml";

const LEFTHOOK_NOTE: &str = "only one command receives git's input when several use use_stdin";
const PRE_COMMIT_NOTE: &str = "pre-commit runs it once per push, for one ref";

// The hook file `core.hooksPath` leads to: husky 9 points the path at its generated `.husky/_`,
// and the hook a person writes is `.husky/pre-push`.
pub(crate) fn hooks_file(hooks_path: &str) -> String {
    let dir = hooks_path.trim_end_matches('/');
    let husky = dir == ".husky/_" || dir.ends_with("/.husky/_");
    let folder = if husky {
        dir.strip_suffix("/_").unwrap_or(dir)
    } else {
        dir
    };
    format!("{folder}/pre-push")
}

fn is_husky_file(file: &str) -> bool {
    file.strip_suffix("/pre-push")
        .is_some_and(|dir| dir.rsplit('/').next() == Some(".husky"))
}

// Only the two YAML pairs are known well enough to write.
pub(crate) fn lefthook_local_name(main: &str) -> Option<&'static str> {
    match main {
        "lefthook.yml" => Some("lefthook-local.yml"),
        ".lefthook.yml" => Some(".lefthook-local.yml"),
        _ => None,
    }
}

// The per-person files lefthook merges over the main config, whatever their format.
pub(crate) fn is_lefthook_local(name: &str) -> bool {
    name.starts_with("lefthook-local.") || name.starts_with(".lefthook-local.")
}

// A top-level `pre-push` key in any spelling: bare or quoted, with a comment or a flow mapping
// after the colon. A second one would make lefthook refuse the whole file.
pub(crate) fn has_pre_push_key(text: &str) -> bool {
    text.lines().any(|line| {
        let key = line
            .strip_prefix("pre-push")
            .or_else(|| line.strip_prefix("\"pre-push\""))
            .or_else(|| line.strip_prefix("'pre-push'"));
        key.is_some_and(|rest| rest.trim_start().starts_with(':'))
    })
}

// The first entry of a list, as `manager` finds them, in this order of strength.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Manager {
    HooksPath {
        file: String,
        inside_project: bool,
        tracked: bool,
        hook: HookFile,
    },
    Lefthook {
        main: String,
        local: Option<(String, HookFile)>,
        other_local: bool,
        ignored: bool,
    },
    PreCommit,
    Plain {
        file: String,
        inside_project: bool,
        hook: HookFile,
    },
}

// The facts `manager` reads: where core.hooksPath leads, which of the names that matter are at
// the top of the project (and under `.config/`), and the text of each.
#[derive(Debug)]
pub(crate) struct ManagerFacts<'a> {
    pub(crate) hooks_path: Option<HookFacts<'a>>,
    pub(crate) plain: HookFacts<'a>,
    pub(crate) top_files: &'a [String],
    pub(crate) lefthook_locals: &'a [(String, HookFile)],
    pub(crate) ignored: &'a [String],
}

// A hook file: its path, whether it is inside the project, whether git tracks it, its text.
#[derive(Debug, Clone)]
pub(crate) struct HookFacts<'a> {
    pub(crate) file: &'a str,
    pub(crate) inside_project: bool,
    pub(crate) tracked: bool,
    pub(crate) hook: HookFile,
}

fn hooks_path_manager(facts: &HookFacts<'_>) -> Manager {
    Manager::HooksPath {
        file: facts.file.to_owned(),
        inside_project: facts.inside_project,
        tracked: facts.tracked,
        hook: facts.hook.clone(),
    }
}

// The hooks path beats lefthook beats pre-commit beats none.
pub(crate) fn manager(facts: &ManagerFacts<'_>) -> Manager {
    if let Some(path) = &facts.hooks_path {
        return hooks_path_manager(path);
    }
    if let Some(main) = LEFTHOOK_MAIN
        .iter()
        .find(|name| facts.top_files.iter().any(|have| have == **name))
    {
        let local_name = lefthook_local_name(main);
        let local = local_name.map(|name| {
            let hook = facts
                .lefthook_locals
                .iter()
                .find(|(have, _text)| have == name)
                .map_or(HookFile::Missing, |(_name, text)| text.clone());
            (name.to_owned(), hook)
        });
        let other_local = facts
            .lefthook_locals
            .iter()
            .any(|(have, _text)| Some(have.as_str()) != local_name);
        return Manager::Lefthook {
            main: (*main).to_owned(),
            local,
            other_local,
            ignored: facts
                .ignored
                .iter()
                .any(|name| Some(name.as_str()) == local_name),
        };
    }
    if facts.top_files.iter().any(|have| have == PRE_COMMIT_MAIN) {
        return Manager::PreCommit;
    }
    Manager::Plain {
        file: facts.plain.file.to_owned(),
        inside_project: facts.plain.inside_project,
        hook: facts.plain.hook.clone(),
    }
}

// The texts a plan writes or prints, as `data/` embeds them.
#[derive(Debug)]
pub(crate) struct Texts<'a> {
    pub(crate) hook: &'a str,
    pub(crate) lefthook: &'a str,
    pub(crate) pre_commit: &'a str,
}

fn print_entry(path: &str, reason: &str, snippet: &str, note: &str) -> HookStep {
    HookStep::Print {
        path: path.to_owned(),
        reason: reason.to_owned(),
        text: format!("{snippet}# {note}\n"),
    }
}

fn lefthook_plan(
    main: &str,
    local: Option<&(String, HookFile)>,
    other_local: bool,
    texts: &Texts<'_>,
) -> HookStep {
    let print = |path: &str, reason: &str| print_entry(path, reason, texts.lefthook, LEFTHOOK_NOTE);
    let Some((name, hook)) = local else {
        return print(
            main,
            "init writes lefthook-local.yml only next to lefthook.yml or .lefthook.yml",
        );
    };
    if other_local {
        return print(name, "another lefthook-local file exists");
    }
    match hook {
        HookFile::Missing => HookStep::Write {
            path: name.clone(),
            text: texts.lefthook.to_owned(),
        },
        HookFile::Unreadable => print(name, "it cannot be read as text"),
        HookFile::Present(text) | HookFile::NotExecutable(text) if already_installed(text) => {
            HookStep::Nothing(ALREADY_INSTALLED.to_owned())
        }
        HookFile::Present(text) | HookFile::NotExecutable(text) if has_pre_push_key(text) => {
            print(name, &format!("{name} already has a pre-push entry"))
        }
        HookFile::Present(_) | HookFile::NotExecutable(_) => HookStep::Append {
            path: name.clone(),
            line: texts.lefthook.to_owned(),
        },
    }
}

pub(crate) fn hook_plan(manager: &Manager, texts: &Texts<'_>) -> HookStep {
    match manager {
        Manager::HooksPath {
            file,
            inside_project,
            tracked: _,
            hook,
        } => plain_hook_plan(file, *inside_project, is_husky_file(file), hook, texts.hook),
        Manager::Plain {
            file,
            inside_project,
            hook,
        } => plain_hook_plan(file, *inside_project, false, hook, texts.hook),
        Manager::Lefthook {
            main,
            local,
            other_local,
            ignored: _,
        } => lefthook_plan(main, local.as_ref(), *other_local, texts),
        Manager::PreCommit => print_entry(
            PRE_COMMIT_MAIN,
            "pre-commit hooks are listed in the config file, which init never edits; add this entry under repos:",
            texts.pre_commit,
            PRE_COMMIT_NOTE,
        ),
    }
}

// What the person does after a write or an append: a hook in a tracked folder is committed, and
// lefthook installs a hook only after a sync.
pub(crate) fn follow_up(manager: &Manager, step: &HookStep) -> Vec<String> {
    if !matches!(step, HookStep::Write { .. } | HookStep::Append { .. }) {
        return Vec::new();
    }
    match manager {
        Manager::HooksPath {
            tracked: true,
            file,
            ..
        } => vec![format!("commit it: {file} is tracked")],
        Manager::HooksPath { tracked: false, .. } | Manager::PreCommit | Manager::Plain { .. } => {
            Vec::new()
        }
        Manager::Lefthook { local, ignored, .. } => {
            let mut lines = vec!["run lefthook install".to_owned()];
            if let (false, Some((name, _hook))) = (ignored, local) {
                lines.push(format!("add {name} to .gitignore (it is per person)"));
            }
            lines
        }
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
            word().prop_map(|w| (Unsafe::EndsEarly, format!("npm {w}; exit 0"))),
            word().prop_map(|w| (Unsafe::EndsEarly, format!("[ -n \"${w}\" ] && exit 0"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("head -c 10 {w}"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("{w} | grep -q x"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("tail {w}"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("{w} | sed s/a/b/"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("{w} | awk 1"))),
            word().prop_map(|w| (Unsafe::ReadsInput, format!("{w} | xargs echo"))),
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
            prop_assert_eq!(append_safety(true, false, true, &script), Ok(()));
            let with = format!("{script}{line}\n");
            prop_assert_eq!(append_safety(true, false, true, &with), Err(reason));
            // a comment never counts
            let commented = format!("{script}# {line}\n");
            prop_assert_eq!(append_safety(true, false, true, &commented), Ok(()));
            // the reasons are tried in the order: project, shell, ends early, input
            prop_assert_eq!(append_safety(false, false, true, &with), Err(Unsafe::OutsideProject));
            let python = with.replacen(shebang, "#!/usr/bin/python3", 1);
            prop_assert_eq!(append_safety(true, false, true, &python), Err(Unsafe::NotShell));
            let both = format!("{script}git lfs pre-push\nexit 0\n");
            prop_assert_eq!(append_safety(true, false, true, &both), Err(Unsafe::EndsEarly));
            // a hook git would skip is refused before anything is said about its text, and a
            // husky file needs no executable bit
            prop_assert_eq!(append_safety(true, false, false, &script), Err(Unsafe::NotExecutable));
            prop_assert_eq!(append_safety(true, true, false, &script), Ok(()));
            prop_assert_eq!(append_safety(false, false, false, &script), Err(Unsafe::OutsideProject));
        }

        #[test]
        fn a_husky_file_without_a_shebang_is_sh(body in "[a-z ]{0,10}") {
            let text = format!("echo {body}\n");
            prop_assert_eq!(append_safety(true, true, true, &text), Ok(()));
            prop_assert_eq!(append_safety(true, false, true, &text), Err(Unsafe::NotShell));
            let python = format!("#!/usr/bin/python3\n{text}");
            prop_assert_eq!(append_safety(true, true, true, &python), Err(Unsafe::NotShell));
        }
    }

    fn texts() -> Texts<'static> {
        Texts {
            hook: new_hook(),
            lefthook: embedded("hooks-snippets/lefthook.yml").expect("embedded"),
            pre_commit: embedded("hooks-snippets/pre-commit.yml").expect("embedded"),
        }
    }

    fn hook_file() -> impl Strategy<Value = HookFile> {
        prop_oneof![
            Just(HookFile::Missing),
            Just(HookFile::Unreadable),
            Just(HookFile::NotExecutable(SAFE.to_owned())),
            Just(HookFile::Present(
                "#!/bin/sh\nnpm test; exit 0\n".to_owned()
            )),
            Just(HookFile::Present(
                "#!/bin/sh\nhead -c 10 >/dev/null\n".to_owned()
            )),
            Just(HookFile::Present(SAFE.to_owned())),
            Just(HookFile::Present("#!/usr/bin/python3\n".to_owned())),
            Just(HookFile::Present("#!/bin/sh\nexit 0\n".to_owned())),
            Just(HookFile::Present("pre-push:\n  commands: {}\n".to_owned())),
            Just(HookFile::Present("other:\n".to_owned())),
            "[a-z:#!/ \n-]{0,30}".prop_map(HookFile::Present),
        ]
    }

    fn manager_strategy() -> impl Strategy<Value = Manager> {
        let file = prop_oneof![Just(".husky/pre-push".to_owned()), "[a-z/.]{1,10}"];
        prop_oneof![
            (file.clone(), any::<bool>(), any::<bool>(), hook_file()).prop_map(
                |(file, inside_project, tracked, hook)| Manager::HooksPath {
                    file,
                    inside_project,
                    tracked,
                    hook,
                }
            ),
            (
                prop_oneof![
                    Just("lefthook.yml".to_owned()),
                    Just(".lefthook.yml".to_owned()),
                    Just("lefthook.toml".to_owned()),
                ],
                proptest::option::of(hook_file()),
                any::<bool>(),
                any::<bool>(),
            )
                .prop_map(|(main, local, other_local, ignored)| {
                    let local = lefthook_local_name(&main)
                        .zip(local)
                        .map(|(name, hook)| (name.to_owned(), hook));
                    Manager::Lefthook {
                        main,
                        local,
                        other_local,
                        ignored,
                    }
                }),
            Just(Manager::PreCommit),
            (file, any::<bool>(), hook_file()).prop_map(|(file, inside_project, hook)| {
                Manager::Plain {
                    file,
                    inside_project,
                    hook,
                }
            }),
        ]
    }

    // The manager as it is after the step was carried out on its file.
    fn applied(manager: &Manager, step: &HookStep) -> Manager {
        let after = |hook: &HookFile| match (step, hook) {
            (HookStep::Write { text, .. }, _) => HookFile::Present(text.clone()),
            (
                HookStep::Append { line, .. },
                HookFile::Present(old) | HookFile::NotExecutable(old),
            ) => HookFile::Present(format!("{old}\n{line}\n")),
            (HookStep::Append { .. }, HookFile::Missing | HookFile::Unreadable)
            | (HookStep::Print { .. } | HookStep::Nothing(_), _) => hook.clone(),
        };
        match manager.clone() {
            Manager::HooksPath {
                file,
                inside_project,
                tracked,
                hook,
            } => Manager::HooksPath {
                hook: after(&hook),
                file,
                inside_project,
                tracked,
            },
            Manager::Plain {
                file,
                inside_project,
                hook,
            } => Manager::Plain {
                hook: after(&hook),
                file,
                inside_project,
            },
            Manager::Lefthook {
                main,
                local,
                other_local,
                ignored,
            } => Manager::Lefthook {
                local: local.map(|(name, hook)| {
                    let hook = after(&hook);
                    (name, hook)
                }),
                main,
                other_local,
                ignored,
            },
            Manager::PreCommit => Manager::PreCommit,
        }
    }

    // Some(true) for a file that is not there, Some(false) for one that is, None for a file
    // that is there and cannot be read (and for no file at all): nothing may be planned for it.
    fn missing_file(manager: &Manager) -> Option<bool> {
        let state = |hook: &HookFile| match hook {
            HookFile::Missing => Some(true),
            HookFile::Present(_) | HookFile::NotExecutable(_) => Some(false),
            HookFile::Unreadable => None,
        };
        match manager {
            Manager::HooksPath { hook, .. } | Manager::Plain { hook, .. } => state(hook),
            Manager::Lefthook { local, .. } => {
                let (_name, hook) = local.as_ref()?;
                state(hook)
            }
            Manager::PreCommit => None,
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn hook_plan_never_replaces(manager in manager_strategy()) {
            let step = hook_plan(&manager, &texts());
            match step {
                HookStep::Write { .. } => prop_assert_eq!(missing_file(&manager), Some(true)),
                HookStep::Append { .. } => prop_assert_eq!(missing_file(&manager), Some(false)),
                HookStep::Print { .. } | HookStep::Nothing(_) => {}
            }
        }

        #[test]
        fn a_second_plan_never_writes(manager in manager_strategy()) {
            let first = hook_plan(&manager, &texts());
            let after = applied(&manager, &first);
            let second = hook_plan(&after, &texts());
            match first {
                HookStep::Write { .. } | HookStep::Append { .. } => {
                    prop_assert_eq!(second, HookStep::Nothing(ALREADY_INSTALLED.to_owned()));
                }
                HookStep::Print { .. } | HookStep::Nothing(_) => {
                    prop_assert_eq!(&after, &manager);
                    let writes = matches!(
                        second,
                        HookStep::Write { .. } | HookStep::Append { .. }
                    );
                    prop_assert!(!writes);
                }
            }
        }

        #[test]
        fn manager_order(
            hooks_path in any::<bool>(),
            lefthook in any::<bool>(),
            pre_commit in any::<bool>(),
        ) {
            let mut top_files = Vec::new();
            if lefthook {
                top_files.push("lefthook.yml".to_owned());
            }
            if pre_commit {
                top_files.push(PRE_COMMIT_MAIN.to_owned());
            }
            let plain = HookFacts {
                file: ".git/hooks/pre-push",
                inside_project: true,
                tracked: false,
                hook: HookFile::Missing,
            };
            let facts = ManagerFacts {
                hooks_path: hooks_path.then(|| HookFacts { file: "h/pre-push", ..plain.clone() }),
                plain,
                top_files: &top_files,
                lefthook_locals: &[],
                ignored: &[],
            };
            let found = manager(&facts);
            let expected = if hooks_path {
                "hooks path"
            } else if lefthook {
                "lefthook"
            } else if pre_commit {
                "pre-commit"
            } else {
                "none"
            };
            let name = match found {
                Manager::HooksPath { .. } => "hooks path",
                Manager::Lefthook { .. } => "lefthook",
                Manager::PreCommit => "pre-commit",
                Manager::Plain { .. } => "none",
            };
            prop_assert_eq!(name, expected);
        }

        #[test]
        fn lefthook_local_name_follows_the_main_name(at in 0..LEFTHOOK_MAIN.len()) {
            let main = LEFTHOOK_MAIN.get(at).copied().unwrap_or_default();
            let expected = match main {
                "lefthook.yml" => Some("lefthook-local.yml"),
                ".lefthook.yml" => Some(".lefthook-local.yml"),
                _ => None,
            };
            prop_assert_eq!(lefthook_local_name(main), expected);
            let top_files = vec![main.to_owned()];
            let plain = HookFacts {
                file: "f",
                inside_project: true,
                tracked: false,
                hook: HookFile::Missing,
            };
            let found = manager(&ManagerFacts {
                hooks_path: None,
                plain,
                top_files: &top_files,
                lefthook_locals: &[],
                ignored: &[],
            });
            let step = hook_plan(&found, &texts());
            prop_assert_eq!(matches!(step, HookStep::Write { .. }), expected.is_some());
            if let HookStep::Write { path, .. } = step {
                prop_assert_eq!(Some(path.as_str()), expected);
            }
        }

        #[test]
        fn another_local_file_means_print(other in "[a-z]{1,5}", main_yml in any::<bool>()) {
            let main = if main_yml { "lefthook.yml" } else { ".lefthook.yml" };
            let locals = vec![(format!("lefthook-local.{other}x"), HookFile::Present(String::new()))];
            let top_files = vec![main.to_owned()];
            let plain = HookFacts {
                file: "f",
                inside_project: true,
                tracked: false,
                hook: HookFile::Missing,
            };
            let found = manager(&ManagerFacts {
                hooks_path: None,
                plain,
                top_files: &top_files,
                lefthook_locals: &locals,
                ignored: &[],
            });
            let other_local = matches!(found, Manager::Lefthook { other_local: true, .. });
            prop_assert!(other_local);
            let printed = matches!(hook_plan(&found, &texts()), HookStep::Print { .. });
            prop_assert!(printed);
        }

        #[test]
        fn lefthook_key_detection(before in "[a-z ]{0,8}", after in "[a-z ]{0,8}") {
            let key_at_column_zero = format!("{before}\npre-push:  \n{after}");
            let key_indented = format!("  pre-push:\n{before}{after}x\n");
            prop_assert!(has_pre_push_key("pre-push:\n  commands:\n"));
            prop_assert!(has_pre_push_key(&key_at_column_zero));
            // every spelling of the same key: a comment, quotes, a flow mapping
            for spelled in [
                "pre-push: # mine\n  commands: {}\n",
                "\"pre-push\":\n  commands: {}\n",
                "'pre-push':\n  commands: {}\n",
                "pre-push: {commands: {}}\n",
                "pre-push :\n",
            ] {
                let text = format!("{before}\n{spelled}{after}");
                prop_assert!(has_pre_push_key(&text));
            }
            prop_assert!(!has_pre_push_key(&key_indented));
            prop_assert!(!has_pre_push_key("# pre-push:\npre-push-other:\n"));
        }

        #[test]
        fn a_hooks_path_leads_to_the_hook_a_person_writes(dir in "[a-z]{1,6}(/[a-z]{1,6}){0,2}") {
            prop_assert_eq!(hooks_file(&dir), format!("{dir}/pre-push"));
            prop_assert_eq!(hooks_file(&format!("{dir}/")), format!("{dir}/pre-push"));
            prop_assert_eq!(hooks_file(&format!("{dir}/.husky/_")), format!("{dir}/.husky/pre-push"));
            prop_assert_eq!(hooks_file(".husky/_"), ".husky/pre-push".to_owned());
            prop_assert_eq!(hooks_file(&format!("{dir}/_")), format!("{dir}/_/pre-push"));
        }
    }

    #[test]
    fn the_guarded_line_is_the_owner_line_fixed() {
        assert_eq!(
            GUARDED_LINE,
            r#"if command -v shipit >/dev/null 2>&1; then exec shipit pre-push "$@"; fi"#
        );
        assert!(already_installed(GUARDED_LINE));
        assert!(!already_installed("# TODO: shipit pre-push\n"));
        assert!(new_hook().ends_with(&format!("{GUARDED_LINE}\n")));
    }
}
