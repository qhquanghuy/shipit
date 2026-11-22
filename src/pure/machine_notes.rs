// What the machine notes say that a tool reads: the recipe for a missing tool and the commands
// never to run. The file is `docs/machine-notes.md`'s example; every other line is free text.

use crate::pure::machine::Writability;

// Each entry is words: the first is the program, the others must be among its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Never(pub(crate) Vec<Vec<String>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Notes {
    pub(crate) never: Never,
    pub(crate) recipe: String,
}

pub(crate) const BUILT_IN_NEVER: &[&str] = &[
    "pip install",
    "pip3 install",
    "pipx install",
    "uv tool install",
    "npm install -g",
    "npm i -g",
    "npm install --global",
    "pnpm add -g",
    "yarn global add",
    "cargo install",
    "go install",
    "gem install",
    "brew install",
    "apt install",
    "apt-get install",
    "dnf install",
    "yum install",
    "pacman -S",
    "nix profile install",
    "nix-env -i",
    "nix-env --install",
];

pub(crate) const BUILT_IN_RECIPE: &str = "Use what the project provides (for example its nix develop shell). If a tool is still missing, ask the owner; never install it globally.";

const RECIPE_PREFIX: &str = "Missing tool: ";
const NEVER_PREFIX: &str = "Never: ";

fn built_in_never() -> Never {
    Never(
        BUILT_IN_NEVER
            .iter()
            .map(|entry| entry.split_whitespace().map(str::to_owned).collect())
            .collect(),
    )
}

// The text between each pair of backticks; an unclosed last span still counts.
fn code_spans(line: &str) -> Vec<Vec<String>> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .map(|span| {
            span.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|words| !words.is_empty())
        .collect()
}

pub(crate) fn parse_notes(text: Option<&str>) -> Notes {
    let lines: Vec<&str> = text.map(|text| text.lines().collect()).unwrap_or_default();
    let recipe = lines
        .iter()
        .find_map(|line| line.strip_prefix(RECIPE_PREFIX))
        .map(str::trim)
        .filter(|recipe| !recipe.is_empty())
        .map_or_else(|| BUILT_IN_RECIPE.to_owned(), str::to_owned);
    let never = lines
        .iter()
        .find_map(|line| line.strip_prefix(NEVER_PREFIX))
        .map(code_spans)
        .filter(|entries| !entries.is_empty())
        .map_or_else(built_in_never, Never);
    Notes { never, recipe }
}

pub(crate) const AT_LINE: &str = "@~/.config/shipit/machine.md";
const NOTES_PATH: &str = "~/.config/shipit/machine.md";
const CLAUDE_MD: &str = "~/.claude/CLAUDE.md";
const NOT_WRITTEN: &str = "machine notes: not written (init asks only in a terminal, and only where the files can be written); add them yourself:";

// The file `init` offers; `parse_notes` reads back the two lines tools read.
pub(crate) fn render_notes(missing: &[&str], recipe: &str, never: &[&str]) -> String {
    let missing = if missing.is_empty() {
        "nothing".to_owned()
    } else {
        missing.join(", ")
    };
    let never: Vec<String> = never.iter().map(|entry| format!("`{entry}`")).collect();
    format!(
        "# Machine notes (written by shipit init; run shipit doctor for the full list)\nMissing here: {missing}.\n{RECIPE_PREFIX}{recipe}\n{NEVER_PREFIX}{}\n",
        never.join(", ")
    )
}

pub(crate) fn init_recipe(nix: bool) -> String {
    if nix {
        "use `nix shell nixpkgs#<tool> --command <command>` for one run; to keep a tool, add it to the project's dev shell.".to_owned()
    } else {
        BUILT_IN_RECIPE.to_owned()
    }
}

// A plain Nix string: the characters that end it, escape in it or start a splice are escaped.
pub(crate) fn nix_string(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("${", "\\${")
        .replace('\n', "\\n")
}

pub(crate) fn home_manager_snippet(notes: Option<&str>, add_at_line: bool) -> String {
    let mut lines = vec![NOT_WRITTEN.to_owned()];
    if let Some(text) = notes {
        lines.push(format!(
            "home.file.\".config/shipit/machine.md\".text = \"{}\";",
            nix_string(text)
        ));
    }
    if add_at_line {
        lines.push(format!(
            "add this line to the text of {CLAUDE_MD}: {AT_LINE}"
        ));
    }
    lines.join("\n")
}

pub(crate) fn plain_snippet(notes: Option<&str>, add_at_line: bool) -> String {
    let mut lines = vec![NOT_WRITTEN.to_owned()];
    if let Some(text) = notes {
        lines.push(format!("write this to {NOTES_PATH}:"));
        lines.extend(text.lines().map(|line| format!("  {line}")));
    }
    if add_at_line {
        lines.push(format!("add this line to {CLAUDE_MD}: {AT_LINE}"));
    }
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotesFile {
    Exists,
    CanCreate,
    CannotCreate,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MachineFacts {
    pub(crate) interactive: bool,
    pub(crate) nix: bool,
    pub(crate) notes: NotesFile,
    pub(crate) claude_md: Writability,
    pub(crate) has_at_line: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MachineStep {
    Nothing,
    Ask,
    Print,
}

// A machine file is written only on a yes in a terminal, and only where it can be: otherwise the
// owner is told what to add (a home-manager setup keeps these files read-only).
pub(crate) fn machine_plan(facts: MachineFacts) -> MachineStep {
    let notes_todo = facts.notes != NotesFile::Exists;
    let at_todo = !facts.has_at_line;
    if !notes_todo && !at_todo {
        return MachineStep::Nothing;
    }
    let notes_ok = !notes_todo || facts.notes == NotesFile::CanCreate;
    let at_ok = !at_todo
        || matches!(
            facts.claude_md,
            Writability::Writable | Writability::CanCreate
        );
    if facts.interactive && notes_ok && at_ok {
        MachineStep::Ask
    } else {
        MachineStep::Print
    }
}

// What the question says: the parts that are to be done.
pub(crate) fn ask_text(write_notes: bool, add_at_line: bool) -> String {
    let mut parts = Vec::new();
    if write_notes {
        parts.push(format!("Write {NOTES_PATH}"));
    }
    if add_at_line {
        parts.push(format!("add {AT_LINE} to {CLAUDE_MD}"));
    }
    format!("{}? [y/N] ", parts.join(" and "))
}

pub(crate) fn has_at_line(text: &str) -> bool {
    text.lines().any(|line| line == AT_LINE)
}

pub(crate) fn says_yes(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn written(recipe: &str, never: &[String]) -> String {
        let spans: Vec<String> = never.iter().map(|entry| format!("`{entry}`")).collect();
        format!(
            "# Machine notes\nMissing here: jq.\n{RECIPE_PREFIX}{recipe}\n{NEVER_PREFIX}{}, \u{2026}\n",
            spans.join(", ")
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn notes_round_trip(
            recipe in "[a-z][a-z .]{0,20}[a-z]",
            never in proptest::collection::vec("[a-z-]{1,6}( [a-z-]{1,6}){0,2}", 1..4),
        ) {
            let notes = parse_notes(Some(&written(&recipe, &never)));
            prop_assert_eq!(notes.recipe, recipe);
            let words: Vec<Vec<String>> = never
                .iter()
                .map(|entry| entry.split(' ').map(str::to_owned).collect())
                .collect();
            prop_assert_eq!(notes.never, Never(words));
        }

        #[test]
        fn no_lines_means_built_in(text in "[a-z ,.`\n]{0,40}") {
            let built_in = parse_notes(None);
            prop_assert_eq!(&built_in.recipe, BUILT_IN_RECIPE);
            prop_assert_eq!(built_in.never.0.len(), BUILT_IN_NEVER.len());
            prop_assert_eq!(parse_notes(Some(&text)), built_in);
        }

        #[test]
        fn rendered_notes_parse_back(
            nix in any::<bool>(),
            missing in proptest::collection::vec("[a-z]{1,5}", 0..3),
        ) {
            let missing: Vec<&str> = missing.iter().map(String::as_str).collect();
            let text = render_notes(&missing, &init_recipe(nix), BUILT_IN_NEVER);
            let notes = parse_notes(Some(&text));
            prop_assert_eq!(notes.recipe, init_recipe(nix));
            prop_assert_eq!(notes.never, parse_notes(None).never);
            prop_assert!(text.lines().count() <= 4);
        }

        #[test]
        fn machine_plan_writes_only_on_interactive_and_writable(
            interactive in any::<bool>(),
            nix in any::<bool>(),
            notes in prop_oneof![
                Just(NotesFile::Exists), Just(NotesFile::CanCreate), Just(NotesFile::CannotCreate),
            ],
            claude_md in prop_oneof![
                Just(Writability::Writable), Just(Writability::ReadOnly),
                Just(Writability::CanCreate), Just(Writability::CannotCreate),
            ],
            has_at_line in any::<bool>(),
        ) {
            let facts = MachineFacts { interactive, nix, notes, claude_md, has_at_line };
            let step = machine_plan(facts);
            prop_assert_eq!(
                step == MachineStep::Nothing,
                notes == NotesFile::Exists && has_at_line
            );
            if step == MachineStep::Ask {
                prop_assert!(interactive);
                prop_assert!(notes != NotesFile::CannotCreate);
                prop_assert!(
                    has_at_line
                        || matches!(claude_md, Writability::Writable | Writability::CanCreate)
                );
            }
            if step == MachineStep::Print {
                let notes_blocked = notes == NotesFile::CannotCreate;
                let at_blocked = !has_at_line
                    && matches!(claude_md, Writability::ReadOnly | Writability::CannotCreate);
                prop_assert!(!interactive || notes_blocked || at_blocked);
            }
        }

        #[test]
        fn the_at_line_counts_only_as_a_whole_line(before in "[a-z ]{0,6}", after in "[a-z ]{1,6}") {
            let alone = format!("{before}\n{AT_LINE}\n{before}");
            let glued_after = format!("{AT_LINE}{after}\n");
            let glued_before = format!("{after}{AT_LINE}\n");
            prop_assert!(has_at_line(&alone));
            prop_assert!(!has_at_line(&glued_after));
            prop_assert!(!has_at_line(&glued_before));
        }

        #[test]
        fn says_yes_rule(text in "[ \tyYeEsSnNo]{0,6}") {
            let word = text.trim().to_lowercase();
            prop_assert_eq!(says_yes(&text), word == "y" || word == "yes");
            prop_assert!(says_yes("  YES "));
            prop_assert!(!says_yes(""));
        }

        #[test]
        fn nix_string_escapes(text in "[a-z\\\\\"$ {}\n]{0,16}") {
            let escaped = nix_string(&text);
            let bytes = escaped.as_bytes();
            for (at, byte) in bytes.iter().enumerate() {
                let before = at.checked_sub(1).and_then(|prior| bytes.get(prior));
                if *byte == b'"' {
                    prop_assert_eq!(before, Some(&b'\\'));
                }
                if *byte == b'$' && bytes.get(at + 1) == Some(&b'{') {
                    prop_assert_eq!(before, Some(&b'\\'));
                }
            }
            prop_assert!(!escaped.contains('\n'));
            let mut back = String::new();
            let mut chars = escaped.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.next() {
                        Some('n') => back.push('\n'),
                        Some(other) => back.push(other),
                        None => prop_assert!(false, "a dangling backslash"),
                    }
                } else {
                    back.push(c);
                }
            }
            prop_assert_eq!(back, text);
        }

        #[test]
        fn the_snippets_name_only_what_is_to_do(
            notes in proptest::option::of("[a-z ]{1,8}"),
            at in any::<bool>(),
        ) {
            for snippet in [
                home_manager_snippet(notes.as_deref(), at),
                plain_snippet(notes.as_deref(), at),
            ] {
                prop_assert_eq!(snippet.contains(AT_LINE), at);
                prop_assert_eq!(snippet.lines().count() > 1, notes.is_some() || at);
            }
            let asked = ask_text(notes.is_some(), at);
            prop_assert_eq!(asked.contains("Write ~/.config/shipit/machine.md"), notes.is_some());
            prop_assert_eq!(asked.contains(AT_LINE), at);
            prop_assert!(asked.ends_with("? [y/N] "));
        }
    }

    #[test]
    fn an_empty_recipe_or_list_falls_back_to_the_built_in_one() {
        let notes = parse_notes(Some("Missing tool: \nNever: nothing in backticks\n"));
        assert_eq!(notes, parse_notes(None));
    }
}
