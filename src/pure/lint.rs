use std::cmp::Reverse;
use std::fmt;

use crate::pure::markdown::{Kind, lines};
use crate::pure::markdown_parts::without_code_spans;
use crate::pure::setup::Setup;

pub(crate) const SCAN_ROOTS: [&str; 3] = ["agents", "data", "skills"];
// The only folder where model names may be said: it is the pack that says them.
pub(crate) const MODEL_PACKS: &str = "data/packs/model/";
pub(crate) const HARNESS_FILE: &str = "harness/claude-code.txt";
pub(crate) const VOCABULARY: &str = "docs/vocab.md";
const TERMS_LINE: &str = "- Not framework terms:";
const HARNESS: [&str; 4] = ["claude code", "claude-code", "claude.md", ".claude"];

// The line of the vocabulary that lists the rejected words: names are cut at commas, a
// parenthesis is a remark, and the last full stop ends the list.
pub(crate) fn rejected_terms(vocab: &str) -> Option<Vec<String>> {
    let rest = vocab
        .lines()
        .find_map(|line| line.strip_prefix(TERMS_LINE))?;
    let mut text = String::new();
    let mut depth = 0_usize;
    for c in rest.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => text.push(c),
            _ => {}
        }
    }
    Some(
        text.trim()
            .trim_end_matches('.')
            .split(',')
            .map(|term| term.trim().to_lowercase())
            .filter(|term| !term.is_empty())
            .collect(),
    )
}

pub(crate) fn model_names(setup: &Setup) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let tiers = setup.agents.tiers.iter();
    let given = tiers.flat_map(|tier| {
        [
            Some(tier.model.as_str()),
            tier.stand_in
                .as_ref()
                .map(|stand_in| stand_in.model.as_str()),
        ]
    });
    for name in given
        .flatten()
        .chain([setup.packs.model.as_str()])
        .map(str::to_lowercase)
    {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

// One glob per line; a blank line and a line starting with `#` say nothing.
pub(crate) fn harness_globs(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

// `*` stands for any text inside one folder, never across a `/`.
pub(crate) fn matches_glob(glob: &str, path: &str) -> bool {
    let Some((head, tail)) = glob.split_once('*') else {
        return glob == path;
    };
    let Some(rest) = path.strip_prefix(head) else {
        return false;
    };
    let reach = rest.find('/').unwrap_or(rest.len());
    (0..=reach)
        .filter(|at| rest.is_char_boundary(*at))
        .filter_map(|at| rest.get(at..))
        .any(|after| matches_glob(tail, after))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    HarnessName,
    ModelName,
    RejectedTerm,
    BarePlan,
    Unreadable,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::HarnessName => "harness name",
            Self::ModelName => "model name",
            Self::RejectedTerm => "rejected term",
            Self::BarePlan => "bare plan",
            Self::Unreadable => "unreadable file",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) rule: Rule,
    pub(crate) matched: String,
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

// A match counts only when no word character touches a word character at its edge; a version
// number after a model name is part of the name, so `opus4` is `opus`.
fn bounded(text: &str, start: usize, end: usize, needle: &str, version: bool) -> bool {
    let before = text.get(..start).and_then(|head| head.chars().next_back());
    let tail = text.get(end..).unwrap_or_default();
    let tail = if version {
        tail.trim_start_matches(|c: char| c.is_ascii_digit())
    } else {
        tail
    };
    let after = tail.chars().next();
    let joined = |edge: Option<char>, outer: Option<char>| {
        edge.is_some_and(is_word) && outer.is_some_and(is_word)
    };
    !joined(needle.chars().next(), before) && !joined(needle.chars().next_back(), after)
}

fn after_build(text: &str, start: usize) -> bool {
    text.get(..start)
        .map(str::trim_end)
        .and_then(|head| head.strip_suffix("build"))
        .is_some_and(|before| !before.chars().next_back().is_some_and(is_word))
}

// How a needle is looked for: as written, as written with a version number allowed after it, or
// with at most one space between its letters (`work flow` is `workflow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Exact,
    Version,
    Spaced,
}

fn spaced_len(text: &str, needle: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    let mut end = 0;
    for (index, want) in needle.chars().enumerate() {
        let mut next = chars.next()?;
        if index > 0 && next.1 == ' ' {
            next = chars.next()?;
        }
        if next.1 != want {
            return None;
        }
        end = next.0 + next.1.len_utf8();
    }
    Some(end)
}

fn find_from(work: &str, needle: &str, from: usize, mode: Mode) -> Option<(usize, usize)> {
    let tail = work.get(from..)?;
    match mode {
        Mode::Exact | Mode::Version => tail
            .find(needle)
            .map(|at| (from + at, from + at + needle.len())),
        Mode::Spaced => tail.char_indices().find_map(|(offset, _)| {
            let len = spaced_len(tail.get(offset..)?, needle)?;
            Some((from + offset, from + offset + len))
        }),
    }
}

// The matches of one needle, each blanked so that no later rule reports the same text again;
// `keep` can still refuse a match, which is then left in place.
fn take_matches(
    work: &mut String,
    needle: &str,
    mode: Mode,
    keep: impl Fn(&str, usize) -> bool,
) -> Vec<(usize, String)> {
    let step = needle.chars().next().map_or(1, char::len_utf8);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some((start, end)) = find_from(work, needle, from, mode) {
        if bounded(work, start, end, needle, mode == Mode::Version) && keep(work, start) {
            found.push((start, needle.to_owned()));
            let blanked = (
                work.get(..start).map(str::to_owned),
                work.get(end..).map(str::to_owned),
            );
            if let (Some(head), Some(tail)) = blanked {
                *work = format!("{head}{}{tail}", " ".repeat(end - start));
            }
            from = end;
        } else {
            from = start + step;
        }
    }
    found
}

fn by_length(mut needles: Vec<String>) -> Vec<String> {
    needles.sort_by_key(|needle| Reverse(needle.len()));
    needles.dedup();
    needles
}

// Lower case, with `_`, `-` and any run of white space as one space: `Claude_Code`, `claude-code`
// and `Claude  Code` are one name.
fn normalized(text: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c == '_' || c == '-' || c.is_whitespace() {
            gap = true;
        } else {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        }
    }
    out
}

// The words to look for, made once for the whole run.
struct Words {
    harness: Vec<String>,
    models: Vec<String>,
    rejected: Vec<String>,
}

impl Words {
    fn new(rejected: &[String], models: &[String]) -> Self {
        let spelled =
            |names: Vec<String>| by_length(names.iter().map(|name| normalized(name)).collect());
        Self {
            harness: spelled(HARNESS.iter().map(|name| (*name).to_owned()).collect()),
            models: spelled(models.to_vec()),
            rejected: by_length(with_plurals(rejected)),
        }
    }
}

// What a file may say: the model pack says models, and a harness file says harness names.
struct Report {
    harness: bool,
    model: bool,
}

fn hits_of(
    work: &mut String,
    needles: &[String],
    mode: Mode,
    rule: Rule,
) -> Vec<(usize, Rule, String)> {
    let mut hits = Vec::new();
    for needle in needles {
        for (at, found) in take_matches(work, needle, mode, |_, _| true) {
            hits.push((at, rule, found));
        }
    }
    hits
}

// A harness name split by a line break: the end of one line and the start of the next.
fn split_harness(first: &str, second: &str, needles: &[String]) -> Vec<(usize, Rule, String)> {
    let (a, b) = (
        normalized(&without_code_spans(first)),
        normalized(&without_code_spans(second)),
    );
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let window = format!("{a} {b}");
    let split = a.len();
    needles
        .iter()
        .filter(|needle| {
            window.match_indices(needle.as_str()).any(|(start, _)| {
                let end = start + needle.len();
                start < split && end > split + 1 && bounded(&window, start, end, needle, false)
            })
        })
        .map(|needle| (split, Rule::HarnessName, needle.clone()))
        .collect()
}

// The start of a name that goes on in the next line is part of the name, not a model.
fn blank_tail(work: &mut String, name: &str) {
    let words: Vec<&str> = name.split(' ').collect();
    for count in (1..words.len()).rev() {
        let prefix = words.get(..count).unwrap_or_default().join(" ");
        let kept = work.trim_end().len();
        if work.trim_end().ends_with(&prefix) {
            work.truncate(kept - prefix.len());
            work.push_str(&" ".repeat(prefix.len()));
            return;
        }
    }
}

// The rules of a line, in the order they run. A rejected term or a harness name in a code span
// is a mention; a model name is not, so it is read inside spans too. A harness name is blanked
// before the model names, so `Claude Code` is not also the model `claude`.
fn line_hits(
    text: &str,
    next: Option<&str>,
    words: &Words,
    report: &Report,
) -> Vec<(usize, Rule, String)> {
    let plain = without_code_spans(text);
    let mut hits = Vec::new();
    let mut harness_work = normalized(&plain);
    let found = hits_of(
        &mut harness_work,
        &words.harness,
        Mode::Exact,
        Rule::HarnessName,
    );
    let crossing = next
        .map(|next| split_harness(text, next, &words.harness))
        .unwrap_or_default();
    let mut model_work = normalized(&text.replace('`', " "));
    hits_of(
        &mut model_work,
        &words.harness,
        Mode::Exact,
        Rule::HarnessName,
    );
    for (_, _, name) in &crossing {
        blank_tail(&mut model_work, name);
    }
    if report.harness {
        hits.extend(found);
        hits.extend(crossing);
    }
    if report.model {
        hits.extend(hits_of(
            &mut model_work,
            &words.models,
            Mode::Version,
            Rule::ModelName,
        ));
    }
    let mut work = normalized(&plain);
    hits.extend(hits_of(
        &mut work,
        &words.rejected,
        Mode::Spaced,
        Rule::RejectedTerm,
    ));
    for needle in ["plans", "plan"] {
        let bare = |text: &str, start: usize| !after_build(text, start);
        for (at, found) in take_matches(&mut work, needle, Mode::Exact, bare) {
            hits.push((at, Rule::BarePlan, found));
        }
    }
    hits
}

// A term also stands for its plural: `task` is `tasks` and `release` is `releases`, `tes` too.
fn with_plurals(terms: &[String]) -> Vec<String> {
    terms
        .iter()
        .flat_map(|term| [term.clone(), format!("{term}s"), format!("{term}es")])
        .collect()
}

pub(crate) fn lint(
    files: &[(String, String)],
    rejected: &[String],
    models: &[String],
    harness_globs: &[String],
) -> Vec<Finding> {
    let words = Words::new(rejected, models);
    let mut all: Vec<(String, usize, usize, Rule, String)> = Vec::new();
    for (path, text) in files {
        let in_model_pack = path.starts_with(MODEL_PACKS);
        let report = Report {
            harness: !in_model_pack && !harness_globs.iter().any(|glob| matches_glob(glob, path)),
            model: !in_model_pack,
        };
        let read = match lines(text) {
            Ok(read) => read,
            Err(why) => {
                all.push((path.clone(), 1, 0, Rule::Unreadable, why));
                continue;
            }
        };
        for (index, line) in read.iter().enumerate() {
            if !matches!(line.kind, Kind::Prose) {
                continue;
            }
            let next = read
                .get(index + 1)
                .filter(|next| matches!(next.kind, Kind::Prose))
                .map(|next| next.text);
            for (column, rule, matched) in line_hits(line.text, next, &words, &report) {
                all.push((path.clone(), index + 1, column, rule, matched));
            }
        }
    }
    all.sort_by(|a, b| (&a.0, a.1, a.2).cmp(&(&b.0, b.1, b.2)));
    all.into_iter()
        .map(|(path, line, _column, rule, matched)| Finding {
            path,
            line,
            rule,
            matched,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::setup_with;

    const VOCAB_LINE: &str = "- Not framework terms: task, unit, artifact, workflow, pipeline, milestone (GitHub's), release.";

    fn terms() -> Vec<String> {
        rejected_terms(VOCAB_LINE).expect("the line is there")
    }

    fn found(path: &str, text: &str) -> Vec<(usize, Rule, String)> {
        let models = ["opus".to_owned(), "claude".to_owned()];
        lint(
            &[(path.to_owned(), text.to_owned())],
            &terms(),
            &models,
            &[],
        )
        .into_iter()
        .map(|hit| (hit.line, hit.rule, hit.matched))
        .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // A term between spaces is found on its line; in a span, a fence or inside a longer word
        // it is not; findings come sorted.
        #[test]
        fn rejected_terms_are_found(
            term in "[a-z]{3,8}",
            lines_with in proptest::collection::vec(any::<bool>(), 1..6),
        ) {
            prop_assume!(term != "see" && term != "here");
            let rejected = vec![term.clone()];
            let text: String = lines_with
                .iter()
                .map(|with| if *with { format!("see {term} here\n") } else { format!("see `{term}` here\n") })
                .collect();
            let got = lint(&[("a.md".to_owned(), text)], &rejected, &[], &[]);
            let wanted: Vec<usize> = lines_with
                .iter()
                .enumerate()
                .filter(|(_, with)| **with)
                .map(|(index, _)| index + 1)
                .collect();
            prop_assert_eq!(got.iter().map(|f| f.line).collect::<Vec<_>>(), wanted);
            prop_assert!(got.iter().all(|f| f.rule == Rule::RejectedTerm && f.matched == term));
            let fenced = format!("```\nsee {term} here\n```\n");
            prop_assert!(lint(&[("a.md".to_owned(), fenced)], &rejected, &[], &[]).is_empty());
        }

        #[test]
        fn word_boundaries(
            term in "[a-z]{3,8}",
            (left, right) in prop_oneof![
                ("[a-z0-9]{1,3}", "[a-z0-9]{0,3}"),
                (Just(String::new()), "[a-z0-9]{1,3}"),
            ],
        ) {
            let text = format!("x {left}{term}{right} y\n");
            let got = lint(&[("a.md".to_owned(), text)], std::slice::from_ref(&term), &[], &[]);
            prop_assert!(got.iter().all(|f| f.matched != term));
        }

        #[test]
        fn a_glob_without_a_star_is_the_path_and_a_star_stays_in_its_folder(
            path in "[a-z]{1,4}(/[a-z]{1,4}){0,3}",
            other in "[a-z]{1,4}(/[a-z]{1,4}){0,3}",
        ) {
            prop_assert_eq!(matches_glob(&path, &other), path == other);
            let (folder, name) = path.rsplit_once('/').map_or(("", path.as_str()), |(f, n)| (f, n));
            let starred = if folder.is_empty() { "*".to_owned() } else { format!("{folder}/*") };
            prop_assert!(matches_glob(&starred, &path), "{} {}", starred, name);
            let deeper = format!("{path}/deeper");
            prop_assert!(!matches_glob(&starred, &deeper));
        }
    }

    #[test]
    fn the_vocabulary_line_gives_the_terms() {
        assert_eq!(
            terms(),
            [
                "task",
                "unit",
                "artifact",
                "workflow",
                "pipeline",
                "milestone",
                "release"
            ]
        );
        assert_eq!(
            rejected_terms("# Title\n- Not framework terms:\n"),
            Some(Vec::new())
        );
        assert_eq!(rejected_terms("- Not the line: x, y.\n"), None);
    }

    #[test]
    fn a_term_and_its_plural_are_found_across_a_hyphen_and_in_any_case() {
        let hits = |text: &str| {
            found("a.md", text)
                .into_iter()
                .map(|(_, _, m)| m)
                .collect::<Vec<_>>()
        };
        assert_eq!(hits("a sub-task"), ["task"]);
        assert_eq!(hits("Tasks and RELEASES"), ["tasks", "releases"]);
        assert_eq!(hits("a subtask, tasked, a release2"), Vec::<String>::new());
    }

    // `_`, `-` and a space are one gap, as they are for model names: a term spelled with a gap, or
    // joined across one, is the term; a longer word is not.
    #[test]
    fn a_rejected_term_is_found_in_any_spelling() {
        let hits = |text: &str| {
            found("a.md", text)
                .into_iter()
                .map(|(_, _, matched)| matched)
                .collect::<Vec<_>>()
        };
        assert_eq!(hits("a Work-flow"), ["workflow"]);
        assert_eq!(hits("a work flow, a pipe_line"), ["workflow", "pipeline"]);
        assert_eq!(hits("a sub_task and a SUB-TASK"), ["task", "task"]);
        assert_eq!(
            hits("a subtask, a worker flow, a tasked"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn plan_is_bare_unless_it_is_a_build_plan() {
        let hits = |text: &str| {
            found("a.md", text)
                .into_iter()
                .map(|(_, _, m)| m)
                .collect::<Vec<_>>()
        };
        assert_eq!(hits("the plan"), ["plan"]);
        assert_eq!(hits("two plans, Plans"), ["plans", "plans"]);
        assert_eq!(
            hits("the build plan, the Build  plans"),
            Vec::<String>::new()
        );
        assert_eq!(hits("planned by the planner; rebuild plan"), ["plan"]);
    }

    #[test]
    fn model_names_outside_the_model_pack_are_found() {
        let said = "use opus here\n";
        assert_eq!(
            found("data/packs/skill/x.md", said),
            [(1, Rule::ModelName, "opus".to_owned())]
        );
        assert_eq!(found("data/packs/model/claude/x.md", said), Vec::new());
        assert_eq!(
            found("data/packs/model/claude/x.md", "a release\n"),
            [(1, Rule::RejectedTerm, "release".to_owned())]
        );
    }

    #[test]
    fn harness_name_once() {
        let got = found(
            "skills/x/SKILL.md",
            "Run Claude Code, not .claude or CLAUDE.md.\n",
        );
        assert_eq!(
            got,
            [
                (1, Rule::HarnessName, "claude code".to_owned()),
                (1, Rule::HarnessName, ".claude".to_owned()),
                (1, Rule::HarnessName, "claude.md".to_owned()),
            ]
        );
        let exempt = lint(
            &[("harness/x.md".to_owned(), "Claude Code\n".to_owned())],
            &terms(),
            &["claude".to_owned()],
            &["harness/*.md".to_owned()],
        );
        assert_eq!(exempt, Vec::new());
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // A model name is a model name in any case, in a code span, before a version number and
        // beside any of `_`, `-` and a space; a letter after it makes it another word.
        #[test]
        fn a_model_name_is_found_in_any_spelling(
            name in prop::sample::select(vec!["opus", "Opus", "OPUS", "claude", "Claude"]),
            span in any::<bool>(),
            version in "[0-9]{0,2}(\\.[0-9])?",
            gap in prop::sample::select(vec![" ", "_", "-", "  "]),
            letter in "[a-z]",
        ) {
            let said = format!("{name}{version}");
            let said = if span { format!("`{said}`") } else { said };
            let text = format!("use{gap}{said}{gap}here\n");
            let wanted = vec![(1, Rule::ModelName, name.to_lowercase())];
            prop_assert_eq!(found("a.md", &text), wanted);
            let longer = format!("use {name}{letter} here\n");
            prop_assert_eq!(found("a.md", &longer), Vec::new());
        }
    }

    #[test]
    fn a_harness_name_is_found_in_any_spelling_and_across_a_line_break() {
        let harness = |text: &str| found("a.md", text);
        let one = vec![(1, Rule::HarnessName, "claude code".to_owned())];
        assert_eq!(harness("run claude_code now\n"), one);
        assert_eq!(harness("run Claude  Code now\n"), one);
        assert_eq!(harness("run claude-\ncode now\n"), one);
        assert_eq!(harness("run Claude\nCode now\n"), one);
        assert_eq!(harness("run `claude code` now\n"), Vec::new());
        assert_eq!(
            harness("run Claude\n```\nCode\n```\n"),
            vec![(1, Rule::ModelName, "claude".to_owned())]
        );
    }

    #[test]
    fn a_name_outside_the_setup_is_not_known() {
        assert_eq!(found("a.md", "ask GPT-5 or Gemini\n"), Vec::new());
    }

    #[test]
    fn fences_hide_text_and_lines_count_from_the_top() {
        let text = "one\n```\ntask\ntask\n```\n`x` and task\ntask\n";
        assert_eq!(
            found("a.md", text),
            [
                (6, Rule::RejectedTerm, "task".to_owned()),
                (7, Rule::RejectedTerm, "task".to_owned())
            ]
        );
    }

    #[test]
    fn a_file_the_reader_refuses_is_a_finding_of_its_own() {
        assert_eq!(
            found("a.md", "ok\n<!-- hidden -->\n"),
            [(
                1,
                Rule::Unreadable,
                "raw HTML opens at line 2, which hides text from the reader".to_owned()
            )]
        );
    }

    #[test]
    fn the_models_are_the_setups_tiers_stand_ins_and_the_model_pack() {
        assert_eq!(
            model_names(&setup_with(&[])),
            ["haiku", "sonnet", "opus", "fable", "claude"]
        );
    }

    #[test]
    fn harness_globs_skip_blank_lines_and_comments() {
        assert_eq!(
            harness_globs("# harness files\n\nharness/*\n  skills/x/*.md  \n"),
            ["harness/*", "skills/x/*.md"]
        );
    }

    // Every file the tool embeds is held to its own rules, so a form or a pack cannot be written
    // in words the lint would refuse.
    #[test]
    fn every_embedded_file_is_clean() {
        let files: Vec<(String, String)> = crate::DATA
            .iter()
            .map(|(key, text)| (format!("data/{key}"), (*text).to_owned()))
            .collect();
        assert_ne!(files.len(), 0);
        let got = lint(&files, &terms(), &model_names(&setup_with(&[])), &[]);
        assert!(got.is_empty(), "{got:?}");
    }
}
