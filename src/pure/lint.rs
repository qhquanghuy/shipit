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

// A match counts only when no word character touches a word character at its edge.
fn bounded(text: &str, start: usize, end: usize, needle: &str) -> bool {
    let before = text.get(..start).and_then(|head| head.chars().next_back());
    let after = text.get(end..).and_then(|tail| tail.chars().next());
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

// The matches of one needle, each blanked so that no later rule reports the same text again;
// `keep` can still refuse a match, which is then left in place.
fn take_matches(
    work: &mut String,
    needle: &str,
    keep: impl Fn(&str, usize) -> bool,
) -> Vec<(usize, String)> {
    let step = needle.chars().next().map_or(1, char::len_utf8);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = work.get(from..).and_then(|tail| tail.find(needle)) {
        let start = from + at;
        let end = start + needle.len();
        if bounded(work, start, end, needle) && keep(work, start) {
            found.push((start, needle.to_owned()));
            let blanked = (
                work.get(..start).map(str::to_owned),
                work.get(end..).map(str::to_owned),
            );
            if let (Some(head), Some(tail)) = blanked {
                *work = format!("{head}{}{tail}", " ".repeat(needle.len()));
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

struct Scan<'a> {
    rejected: Vec<String>,
    models: &'a [String],
    harness: bool,
    model: bool,
}

// The rules of a line, in the order they run; a harness name is not reported again as a model name.
fn line_hits(text: &str, scan: &Scan<'_>) -> Vec<(usize, Rule, String)> {
    let mut work = without_code_spans(text).to_lowercase();
    let mut hits = Vec::new();
    let always = |_: &str, _: usize| true;
    // Always blanked, so that a harness name in an exempt file is not a model name either.
    let names = by_length(HARNESS.iter().map(|name| (*name).to_owned()).collect());
    for needle in &names {
        for (at, found) in take_matches(&mut work, needle, always) {
            if scan.harness {
                hits.push((at, Rule::HarnessName, found));
            }
        }
    }
    if scan.model {
        for needle in &by_length(scan.models.to_vec()) {
            for (at, found) in take_matches(&mut work, needle, always) {
                hits.push((at, Rule::ModelName, found));
            }
        }
    }
    for needle in &by_length(scan.rejected.clone()) {
        for (at, found) in take_matches(&mut work, needle, always) {
            hits.push((at, Rule::RejectedTerm, found));
        }
    }
    for needle in ["plans", "plan"] {
        let bare = |text: &str, start: usize| !after_build(text, start);
        for (at, found) in take_matches(&mut work, needle, bare) {
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
    let rejected = with_plurals(rejected);
    let mut all: Vec<(String, usize, usize, Rule, String)> = Vec::new();
    for (path, text) in files {
        let in_model_pack = path.starts_with(MODEL_PACKS);
        let scan = Scan {
            rejected: rejected.clone(),
            models,
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
            for (column, rule, matched) in line_hits(line.text, &scan) {
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
            left in "[a-z0-9_]{0,3}",
            right in "[a-z0-9_]{0,3}",
        ) {
            prop_assume!(!left.is_empty() || !right.is_empty());
            let text = format!("see {left}{term}{right} here\n");
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
