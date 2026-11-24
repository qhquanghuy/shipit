// What `init` does about the pre-push and pre-commit hooks. Nothing here replaces or rewrites a
// file: a hook is written new, or one line is appended to it, or the line is only printed with
// the reason.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitHook {
    Push,
    Commit,
}

impl GitHook {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Push => "pre-push",
            Self::Commit => "pre-commit",
        }
    }

    pub(crate) const fn guarded_line(self) -> &'static str {
        match self {
            Self::Push => GUARDED_LINE,
            Self::Commit => COMMIT_LINE,
        }
    }

    pub(crate) fn already_installed_text(self) -> String {
        format!("{} hook: already installed", self.name())
    }
}

// Guarded, so that a teammate without ShipIt pushes as before; the `&&` form would fail their push.
pub(crate) const GUARDED_LINE: &str =
    "if command -v shipit >/dev/null 2>&1; then exec shipit pre-push \"$@\"; fi";

// Guarded twice, so a teammate whose shipit predates the command commits as before.
pub(crate) const COMMIT_LINE: &str = "if command -v shipit >/dev/null 2>&1 && shipit help pre-commit >/dev/null 2>&1; then exec shipit pre-commit; fi";

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
// husky runs its hooks with sh, and needs no executable bit for the same reason. Git gives a
// pre-commit hook no input, so only the push hook can have it drained.
pub(crate) fn append_safety(
    hook: GitHook,
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
    if hook == GitHook::Push && text.lines().any(reads_input) {
        return Err(Unsafe::ReadsInput);
    }
    Ok(())
}

pub(crate) fn already_installed(hook: GitHook, text: &str) -> bool {
    let needle = format!("shipit {}", hook.name());
    text.lines()
        .any(|line| !is_comment(line) && line.contains(&needle))
}

pub(crate) fn plain_hook_plan(
    git_hook: GitHook,
    path: &str,
    inside_project: bool,
    husky: bool,
    file: &HookFile,
    new_hook: &str,
) -> HookStep {
    let print = |reason: Unsafe| HookStep::Print {
        path: path.to_owned(),
        reason: reason.reason().to_owned(),
        text: git_hook.guarded_line().to_owned(),
    };
    match file {
        HookFile::Missing if inside_project => HookStep::Write {
            path: path.to_owned(),
            text: new_hook.to_owned(),
        },
        HookFile::Missing => print(Unsafe::OutsideProject),
        HookFile::Unreadable => print(Unsafe::Unreadable),
        HookFile::Present(text) | HookFile::NotExecutable(text) => {
            let executable = matches!(file, HookFile::Present(_));
            if (executable || husky) && already_installed(git_hook, text) {
                return HookStep::Nothing(git_hook.already_installed_text());
            }
            match append_safety(git_hook, inside_project, husky, executable, text) {
                Ok(()) => HookStep::Append {
                    path: path.to_owned(),
                    line: git_hook.guarded_line().to_owned(),
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
pub(crate) fn hooks_file(hooks_path: &str, hook: GitHook) -> String {
    let dir = hooks_path.trim_end_matches('/');
    let husky = dir == ".husky/_" || dir.ends_with("/.husky/_");
    let folder = if husky {
        dir.strip_suffix("/_").unwrap_or(dir)
    } else {
        dir
    };
    format!("{folder}/{}", hook.name())
}

fn is_husky_file(file: &str, hook: GitHook) -> bool {
    file.strip_suffix(hook.name())
        .and_then(|dir| dir.strip_suffix('/'))
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

// A top-level key of the hook in any spelling: bare or quoted, with a comment or a flow mapping
// after the colon. A second one would make lefthook refuse the whole file.
pub(crate) fn has_hook_key(hook: GitHook, text: &str) -> bool {
    let name = hook.name();
    let double = format!("\"{name}\"");
    let single = format!("'{name}'");
    text.trim_start_matches('\u{feff}').lines().any(|line| {
        let key = line
            .strip_prefix(name)
            .or_else(|| line.strip_prefix(double.as_str()))
            .or_else(|| line.strip_prefix(single.as_str()));
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

// The notes are about git's input, which only the push hook gets.
fn print_entry(path: &str, reason: &str, snippet: &str, note: Option<&str>) -> HookStep {
    let text = match note {
        Some(note) => format!("{snippet}# {note}\n"),
        None => snippet.to_owned(),
    };
    HookStep::Print {
        path: path.to_owned(),
        reason: reason.to_owned(),
        text,
    }
}

fn lefthook_plan(
    hook: GitHook,
    main: &str,
    local: Option<&(String, HookFile)>,
    other_local: bool,
    texts: &Texts<'_>,
) -> HookStep {
    let note = (hook == GitHook::Push).then_some(LEFTHOOK_NOTE);
    let print = |path: &str, reason: &str| print_entry(path, reason, texts.lefthook, note);
    let Some((name, file)) = local else {
        return print(
            main,
            "init writes lefthook-local.yml only next to lefthook.yml or .lefthook.yml",
        );
    };
    if other_local {
        return print(name, "another lefthook-local file exists");
    }
    match file {
        HookFile::Missing => HookStep::Write {
            path: name.clone(),
            text: texts.lefthook.to_owned(),
        },
        HookFile::Unreadable => print(name, "it cannot be read as text"),
        HookFile::Present(text) | HookFile::NotExecutable(text)
            if already_installed(hook, text) =>
        {
            HookStep::Nothing(hook.already_installed_text())
        }
        HookFile::Present(text) | HookFile::NotExecutable(text) if has_hook_key(hook, text) => {
            print(name, &format!("{name} already has a {} entry", hook.name()))
        }
        HookFile::Present(_) | HookFile::NotExecutable(_) => HookStep::Append {
            path: name.clone(),
            line: texts.lefthook.to_owned(),
        },
    }
}

pub(crate) fn hook_plan(hook: GitHook, manager: &Manager, texts: &Texts<'_>) -> HookStep {
    match manager {
        Manager::HooksPath {
            file,
            inside_project,
            tracked: _,
            hook: found,
        } => plain_hook_plan(
            hook,
            file,
            *inside_project,
            is_husky_file(file, hook),
            found,
            texts.hook,
        ),
        Manager::Plain {
            file,
            inside_project,
            hook: found,
        } => plain_hook_plan(hook, file, *inside_project, false, found, texts.hook),
        Manager::Lefthook {
            main,
            local,
            other_local,
            ignored: _,
        } => lefthook_plan(hook, main, local.as_ref(), *other_local, texts),
        Manager::PreCommit => print_entry(
            PRE_COMMIT_MAIN,
            "pre-commit hooks are listed in the config file, which init never edits; add this entry under repos:",
            texts.pre_commit,
            (hook == GitHook::Push).then_some(PRE_COMMIT_NOTE),
        ),
    }
}

// Both hooks are planned from the same facts; two new files of one name are one write.
pub(crate) fn merge_steps(push: HookStep, commit: HookStep) -> Vec<HookStep> {
    match (push, commit) {
        (
            HookStep::Write { path, text },
            HookStep::Write {
                path: other,
                text: more,
            },
        ) if path == other => vec![HookStep::Write {
            path,
            text: format!("{text}{more}"),
        }],
        (push, commit) => vec![push, commit],
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
    use crate::pure::init::{embedded, hook_keys};

    const SAFE: &str = "#!/bin/sh\necho hello\n";

    fn new_hook(hook: GitHook) -> &'static str {
        embedded(hook_keys(hook)[0]).expect("the hook is embedded")
    }

    fn any_hook() -> impl Strategy<Value = GitHook> {
        prop_oneof![Just(GitHook::Push), Just(GitHook::Commit)]
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
            prop_assert_eq!(append_safety(GitHook::Push, true, false, true, &script), Ok(()));
            let with = format!("{script}{line}\n");
            prop_assert_eq!(append_safety(GitHook::Push, true, false, true, &with), Err(reason));
            // a comment never counts
            let commented = format!("{script}# {line}\n");
            prop_assert_eq!(append_safety(GitHook::Push, true, false, true, &commented), Ok(()));
            // the reasons are tried in the order: project, shell, ends early, input
            prop_assert_eq!(append_safety(GitHook::Push, false, false, true, &with), Err(Unsafe::OutsideProject));
            let python = with.replacen(shebang, "#!/usr/bin/python3", 1);
            prop_assert_eq!(append_safety(GitHook::Push, true, false, true, &python), Err(Unsafe::NotShell));
            let both = format!("{script}git lfs pre-push\nexit 0\n");
            prop_assert_eq!(append_safety(GitHook::Push, true, false, true, &both), Err(Unsafe::EndsEarly));
            // a hook git would skip is refused before anything is said about its text, and a
            // husky file needs no executable bit
            prop_assert_eq!(append_safety(GitHook::Push, true, false, false, &script), Err(Unsafe::NotExecutable));
            prop_assert_eq!(append_safety(GitHook::Push, true, true, false, &script), Ok(()));
            prop_assert_eq!(append_safety(GitHook::Push, false, false, false, &script), Err(Unsafe::OutsideProject));
        }

        #[test]
        fn a_husky_file_without_a_shebang_is_sh(
            // Those bodies are refused for another reason, proven by `append_safety_reasons`.
            body in "[a-z ]{0,10}".prop_filter("no word the detectors refuse", |b| {
                !has_word(b, &READERS) && !has_word(b, &["exit", "exec"])
            }),
        ) {
            let text = format!("echo {body}\n");
            prop_assert_eq!(append_safety(GitHook::Push, true, true, true, &text), Ok(()));
            prop_assert_eq!(append_safety(GitHook::Push, true, false, true, &text), Err(Unsafe::NotShell));
            let python = format!("#!/usr/bin/python3\n{text}");
            prop_assert_eq!(append_safety(GitHook::Push, true, true, true, &python), Err(Unsafe::NotShell));
        }
    }

    fn texts(hook: GitHook) -> Texts<'static> {
        let [file, lefthook, pre_commit] = hook_keys(hook);
        Texts {
            hook: embedded(file).expect("embedded"),
            lefthook: embedded(lefthook).expect("embedded"),
            pre_commit: embedded(pre_commit).expect("embedded"),
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
        fn hook_plan_never_replaces(manager in manager_strategy(), hook in any_hook()) {
            let step = hook_plan(hook, &manager, &texts(hook));
            match step {
                HookStep::Write { .. } => prop_assert_eq!(missing_file(&manager), Some(true)),
                HookStep::Append { .. } => prop_assert_eq!(missing_file(&manager), Some(false)),
                HookStep::Print { .. } | HookStep::Nothing(_) => {}
            }
        }

        #[test]
        fn a_second_plan_never_writes(manager in manager_strategy(), hook in any_hook()) {
            let first = hook_plan(hook, &manager, &texts(hook));
            let after = applied(&manager, &first);
            let second = hook_plan(hook, &after, &texts(hook));
            match first {
                HookStep::Write { .. } | HookStep::Append { .. } => {
                    prop_assert_eq!(second, HookStep::Nothing(hook.already_installed_text()));
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
            let step = hook_plan(GitHook::Push, &found, &texts(GitHook::Push));
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
            let printed = matches!(
                hook_plan(GitHook::Push, &found, &texts(GitHook::Push)),
                HookStep::Print { .. }
            );
            prop_assert!(printed);
        }

        #[test]
        fn lefthook_key_detection(
            hook in any_hook(),
            before in "[a-z ]{0,8}",
            after in "[a-z ]{0,8}",
        ) {
            let name = hook.name();
            let key_at_column_zero = format!("{before}\n{name}:  \n{after}");
            let key_indented = format!("  {name}:\n{before}{after}x\n");
            let plain = format!("{name}:\n  commands:\n");
            prop_assert!(has_hook_key(hook, &plain));
            prop_assert!(has_hook_key(hook, &key_at_column_zero));
            // a file that starts with a byte order mark, as some editors write it
            let with_mark = format!("\u{feff}{name}:\n  commands: {{}}\n");
            prop_assert!(has_hook_key(hook, &with_mark));
            // every spelling of the same key: a comment, quotes, a flow mapping
            for spelled in [
                format!("{name}: # mine\n  commands: {{}}\n"),
                format!("\"{name}\":\n  commands: {{}}\n"),
                format!("'{name}':\n  commands: {{}}\n"),
                format!("{name}: {{commands: {{}}}}\n"),
                format!("{name} :\n"),
            ] {
                let text = format!("{before}\n{spelled}{after}");
                prop_assert!(has_hook_key(hook, &text));
            }
            prop_assert!(!has_hook_key(hook, &key_indented));
            let other = format!("# {name}:\n{name}-other:\n");
            prop_assert!(!has_hook_key(hook, &other));
        }

        #[test]
        fn a_hooks_path_leads_to_the_hook_a_person_writes(
            hook in any_hook(),
            dir in "[a-z]{1,6}(/[a-z]{1,6}){0,2}",
        ) {
            let name = hook.name();
            prop_assert_eq!(hooks_file(&dir, hook), format!("{dir}/{name}"));
            prop_assert_eq!(hooks_file(&format!("{dir}/"), hook), format!("{dir}/{name}"));
            prop_assert_eq!(hooks_file(&format!("{dir}/.husky/_"), hook), format!("{dir}/.husky/{name}"));
            prop_assert_eq!(hooks_file(".husky/_", hook), format!(".husky/{name}"));
            prop_assert_eq!(hooks_file(&format!("{dir}/_"), hook), format!("{dir}/_/{name}"));
        }

        #[test]
        fn two_new_writes_to_one_file_are_one_write(manager in manager_strategy()) {
            let push = hook_plan(GitHook::Push, &manager, &texts(GitHook::Push));
            let commit = hook_plan(GitHook::Commit, &manager, &texts(GitHook::Commit));
            let merged = merge_steps(push.clone(), commit.clone());
            let paths: Vec<&str> = merged
                .iter()
                .filter_map(|step| match step {
                    HookStep::Write { path, .. } => Some(path.as_str()),
                    HookStep::Append { .. } | HookStep::Print { .. } | HookStep::Nothing(_) => None,
                })
                .collect();
            let distinct: std::collections::BTreeSet<&str> = paths.iter().copied().collect();
            prop_assert_eq!(paths.len(), distinct.len());
            if let (
                HookStep::Write { path, text },
                HookStep::Write { path: other, text: more },
            ) = (&push, &commit)
            {
                if path == other {
                    prop_assert_eq!(merged.len(), 1);
                    prop_assert_eq!(merged.first(), Some(&HookStep::Write {
                        path: path.clone(),
                        text: format!("{text}{more}"),
                    }));
                }
            } else {
                prop_assert_eq!(merged, vec![push, commit]);
            }
        }
    }

    #[test]
    fn reads_input_is_a_reason_only_for_the_push_hook() {
        let reading = format!("{SAFE}while read line; do :; done\n");
        assert_eq!(
            append_safety(GitHook::Push, true, false, true, &reading),
            Err(Unsafe::ReadsInput)
        );
        assert_eq!(
            append_safety(GitHook::Commit, true, false, true, &reading),
            Ok(())
        );
    }

    #[test]
    fn the_guarded_line_is_the_owner_line_fixed() {
        assert_eq!(
            GUARDED_LINE,
            r#"if command -v shipit >/dev/null 2>&1; then exec shipit pre-push "$@"; fi"#
        );
        assert!(already_installed(GitHook::Push, GUARDED_LINE));
        assert!(!already_installed(
            GitHook::Push,
            "# TODO: shipit pre-push\n"
        ));
        assert!(new_hook(GitHook::Push).ends_with(&format!("{GUARDED_LINE}\n")));
        assert_eq!(
            COMMIT_LINE,
            "if command -v shipit >/dev/null 2>&1 && shipit help pre-commit >/dev/null 2>&1; then exec shipit pre-commit; fi"
        );
        assert!(already_installed(GitHook::Commit, COMMIT_LINE));
        assert!(!already_installed(GitHook::Commit, GUARDED_LINE));
        assert!(new_hook(GitHook::Commit).ends_with(&format!("{COMMIT_LINE}\n")));
    }
}
