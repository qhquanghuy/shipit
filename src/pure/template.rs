use toml::Value;

use crate::pure::block::toml_block;
use crate::pure::markdown::{Kind, Line, lines};
use crate::pure::markdown_parts::{bold_marker, heading};
use crate::pure::messages::{FILE_PROBLEM, NO_BLOCK, NOT_TOML, say};
use crate::pure::setup::toml_table;
use crate::pure::text::comma_list;

const FORM: &str = "## Form";
const HEADER_KEYS: [&str; 3] = ["kind", "outcome", "input"];
const NEXT: &str = "Next";

#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) name: &'static str,
    pub(crate) parts: &'static [&'static str],
}

#[derive(Debug)]
pub(crate) struct Spec {
    pub(crate) kind: &'static str,
    pub(crate) outcomes: &'static [Outcome],
    pub(crate) owner_message: bool,
}

impl Spec {
    // Every part of any outcome, once, in the order the outcomes list them.
    pub(crate) fn part_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = Vec::new();
        for part in self.outcomes.iter().flat_map(|outcome| outcome.parts) {
            if !names.contains(part) {
                names.push(part);
            }
        }
        names
    }
}

#[derive(Debug)]
pub(crate) struct Context<'a> {
    pub(crate) file: &'a str,
    pub(crate) max_lines: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) outcome: String,
}

pub(crate) struct Header {
    pub(crate) kind: String,
    pub(crate) outcome: String,
}

pub(crate) fn header(file: &str, text: &str) -> Result<Header, Vec<String>> {
    let block = match toml_block(text, FORM) {
        Ok(Some(block)) => block,
        Ok(None) => {
            return Err(vec![say(NO_BLOCK, &[("file", &file), ("heading", &FORM)])]);
        }
        Err(why) => return Err(vec![say(FILE_PROBLEM, &[("file", &file), ("why", &why)])]),
    };
    let table = toml_table(block.as_bytes())
        .map_err(|why| vec![say(NOT_TOML, &[("file", &file), ("why", &why)])])?;
    let mut unknown: Vec<&String> = table
        .keys()
        .filter(|key| !HEADER_KEYS.contains(&key.as_str()))
        .collect();
    unknown.sort();
    let mut problems: Vec<String> = unknown
        .into_iter()
        .map(|key| format!("header: unknown key \"{key}\""))
        .collect();
    let mut value = |key: &str| {
        let found = table
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty());
        if found.is_none() {
            problems.push(format!("header: {key} must be a non-empty string"));
        }
        found
    };
    let (kind, outcome, input) = (value("kind"), value("outcome"), value("input"));
    match (kind, outcome, input) {
        (Some(kind), Some(outcome), Some(_input)) if problems.is_empty() => Ok(Header {
            kind: kind.to_owned(),
            outcome: outcome.to_owned(),
        }),
        _ => Err(problems),
    }
}

#[derive(Debug)]
pub(crate) struct Part<'a> {
    pub(crate) name: &'static str,
    // What follows the marker on its own line: a bold marker can carry the first words.
    lead: &'a str,
    pub(crate) lines: Vec<Line<'a>>,
}

impl Part<'_> {
    fn is_empty(&self) -> bool {
        self.lead.trim().is_empty() && self.lines.iter().all(|line| line.text.trim().is_empty())
    }
}

// Only a name in the spec starts a part, and only outside a fence; every other heading or bold
// line is content of the part it sits in. A heading part ends at a heading of its own level or a
// higher one, a bold part at any heading.
fn start_of<'a>(
    names: &[&'static str],
    text: &'a str,
) -> Option<(&'static str, Option<usize>, &'a str)> {
    let known = |word: &str| names.iter().find(|name| **name == word).copied();
    if let Some((level, name)) = heading(text) {
        return known(name).map(|name| (name, Some(level), ""));
    }
    let (name, lead) = bold_marker(text)?;
    known(name).map(|name| (name, None, lead))
}

pub(crate) fn parts<'a>(spec: &Spec, all: Vec<Line<'a>>) -> Vec<Part<'a>> {
    let names = spec.part_names();
    let mut done = Vec::new();
    let mut open: Option<(Part<'a>, Option<usize>)> = None;
    for line in all {
        if matches!(line.kind, Kind::Prose) {
            if let Some((name, level, lead)) = start_of(&names, line.text) {
                done.extend(open.take().map(|(part, _level)| part));
                open = Some((
                    Part {
                        name,
                        lead,
                        lines: Vec::new(),
                    },
                    level,
                ));
                continue;
            }
            let closes = heading(line.text).is_some_and(|(level, _name)| {
                open.as_ref()
                    .is_some_and(|(_, own)| own.is_none_or(|own| level <= own))
            });
            if closes {
                done.extend(open.take().map(|(part, _level)| part));
                continue;
            }
        }
        if let Some((part, _level)) = open.as_mut() {
            part.lines.push(line);
        }
    }
    done.extend(open.map(|(part, _level)| part));
    done
}

// The lines after the Form section's block: a form's own header is not part of the message.
fn body<'l, 'a>(all: &'l [Line<'a>]) -> &'l [Line<'a>] {
    let after = all
        .iter()
        .position(|line| matches!(line.kind, Kind::Prose) && line.text == FORM)
        .and_then(|form| all.get(form + 1..))
        .unwrap_or_default();
    let open = after
        .iter()
        .position(|line| matches!(line.kind, Kind::Open { info: "toml" }));
    let inside = open
        .and_then(|open| after.get(open + 1..))
        .unwrap_or_default();
    inside
        .iter()
        .position(|line| matches!(line.kind, Kind::Close))
        .and_then(|close| inside.get(close + 1..))
        .unwrap_or_default()
}

fn is_label(text: &str) -> bool {
    text.trim()
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .is_some_and(|inner| {
            let fields: Vec<&str> = inner.split('·').map(str::trim).collect();
            fields.len() == 3 && fields.iter().all(|field| !field.is_empty())
        })
}

// The label and the length of an owner message, each as a problem or nothing.
fn owner_problems(body: &[Line<'_>], max_lines: u64) -> (Option<String>, Option<String>) {
    let label = body.iter().find(|line| !line.text.trim().is_empty());
    let label_problem = (!label.is_some_and(|line| is_label(line.text))).then(|| {
        "label: the line after the Form section must be [project · slice · phase]".to_owned()
    });
    let final_empty = body.last().is_some_and(|line| line.text.is_empty());
    let count = body.len() - usize::from(final_empty);
    let long = u64::try_from(count).is_ok_and(|count| count > max_lines);
    let length_problem =
        long.then(|| format!("too long: {count} lines (owner.max_lines is {max_lines})"));
    (label_problem, length_problem)
}

fn part_problems(names: &[&'static str], required: &[&str], found: &[Part<'_>]) -> Vec<String> {
    let mut problems = Vec::new();
    for name in names {
        let mine: Vec<&Part<'_>> = found.iter().filter(|part| part.name == *name).collect();
        match mine.as_slice() {
            [] if required.contains(name) => problems.push(format!("part \"{name}\" is missing")),
            [] => {}
            [only] if only.is_empty() => problems.push(format!("part \"{name}\" is empty")),
            [_only] => {}
            [_, _, ..] => problems.push(format!("part \"{name}\" appears {} times", mine.len())),
        }
    }
    problems
}

pub(crate) fn validate(spec: &Spec, ctx: &Context<'_>, text: &str) -> Result<Verdict, Vec<String>> {
    let head = header(ctx.file, text)?;
    if head.kind != spec.kind {
        return Err(vec![format!(
            "header: kind is \"{}\", expected \"{}\"",
            head.kind, spec.kind
        )]);
    }
    let outcome = spec
        .outcomes
        .iter()
        .find(|outcome| outcome.name == head.outcome)
        .ok_or_else(|| {
            vec![format!(
                "header: outcome \"{}\" is not one of: {}",
                head.outcome,
                comma_list(spec.outcomes.iter().map(|outcome| outcome.name))
            )]
        })?;
    let all = lines(text).map_err(|why| vec![why])?;
    let (label, length) = if spec.owner_message {
        owner_problems(body(&all), ctx.max_lines)
    } else {
        (None, None)
    };
    let found = parts(spec, all);
    let mut problems: Vec<String> = label.into_iter().collect();
    problems.extend(part_problems(&spec.part_names(), outcome.parts, &found));
    let next_is_last = found
        .iter()
        .rposition(|part| part.name == NEXT)
        .is_none_or(|last| last + 1 == found.len());
    if spec.owner_message && !next_is_last {
        problems.push(format!("part \"{NEXT}\" must be the last part"));
    }
    problems.extend(length);
    if problems.is_empty() {
        Ok(Verdict {
            outcome: head.outcome,
        })
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::kinds::KINDS;

    struct Draft<'a> {
        spec: &'a Spec,
        outcome: &'a str,
        shown: &'a str,
        seed: u64,
        skip: Option<&'a str>,
        twice: Option<&'a str>,
    }

    fn section(name: &str, bold: bool, nested: bool) -> String {
        let mut text = if bold {
            format!("**{name}:** some words\n")
        } else {
            format!("## {name}\n\nsome words\n")
        };
        if nested {
            text.push_str(if bold {
                "**Note:** an unknown marker\n"
            } else {
                "### Sub\n\n**Note:** an unknown marker\n"
            });
        }
        if !bold {
            text.push('\n');
        }
        text
    }

    // The seed picks the style of the part markers and whether nested content is added.
    fn draft(d: &Draft<'_>) -> String {
        let mut text = format!(
            "## Form\n\n```toml\nkind = \"{}\"\noutcome = \"{}\"\ninput = \"x\"\n```\n\n",
            d.spec.kind, d.shown
        );
        if d.spec.owner_message {
            text.push_str("[shipit · slice · phase]\n\n");
        }
        let bold = d.seed & 1 == 0;
        let nested = d.seed & 2 == 0;
        let parts = d
            .spec
            .outcomes
            .iter()
            .find(|outcome| outcome.name == d.outcome)
            .map(|outcome| outcome.parts)
            .unwrap_or_default();
        for name in parts {
            if d.skip == Some(name) {
                continue;
            }
            let times = if d.twice == Some(name) { 2 } else { 1 };
            for _ in 0..times {
                text.push_str(&section(name, bold, nested));
            }
        }
        text
    }

    fn filled(spec: &Spec, outcome: &str, seed: u64) -> String {
        draft(&Draft {
            spec,
            outcome,
            shown: outcome,
            seed,
            skip: None,
            twice: None,
        })
    }

    fn ctx(max_lines: u64) -> Context<'static> {
        Context {
            file: "out.md",
            max_lines,
        }
    }

    // Some outcome of some kind, by index, so that every kind the specs list is covered.
    fn pick(kind: usize, outcome: usize) -> (&'static Spec, &'static Outcome) {
        let spec = &KINDS[kind % KINDS.len()];
        (spec, &spec.outcomes[outcome % spec.outcomes.len()])
    }

    fn decision_point() -> &'static Spec {
        KINDS
            .iter()
            .find(|spec| spec.kind == "decision-point")
            .expect("the decision point is a kind")
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn generated_outputs_pass(kind in 0_usize..32, outcome in 0_usize..32, seed in any::<u64>()) {
            let (spec, outcome) = pick(kind, outcome);
            let text = filled(spec, outcome.name, seed);
            prop_assert_eq!(
                validate(spec, &ctx(1000), &text),
                Ok(Verdict { outcome: outcome.name.to_owned() }),
                "{}", text
            );
        }

        #[test]
        fn a_missing_part_is_named(
            kind in 0_usize..32,
            outcome in 0_usize..32,
            which in 0_usize..32,
            seed in any::<u64>(),
        ) {
            let (spec, outcome) = pick(kind, outcome);
            let name = outcome.parts[which % outcome.parts.len()];
            let text = draft(&Draft {
                spec,
                outcome: outcome.name,
                shown: outcome.name,
                seed,
                skip: Some(name),
                twice: None,
            });
            let problems = validate(spec, &ctx(1000), &text).expect_err("a part is missing");
            prop_assert!(problems.contains(&format!("part \"{name}\" is missing")), "{:?}", problems);
        }

        #[test]
        fn an_unlisted_outcome_is_refused(
            kind in 0_usize..32,
            outcome in 0_usize..32,
            shown in "[a-z][a-z-]{0,12}",
            seed in any::<u64>(),
        ) {
            let (spec, outcome) = pick(kind, outcome);
            prop_assume!(spec.outcomes.iter().all(|listed| listed.name != shown));
            let text = draft(&Draft {
                spec,
                outcome: outcome.name,
                shown: &shown,
                seed,
                skip: None,
                twice: None,
            });
            let listed = comma_list(spec.outcomes.iter().map(|listed| listed.name));
            prop_assert_eq!(
                validate(spec, &ctx(1000), &text),
                Err(vec![format!("header: outcome \"{shown}\" is not one of: {listed}")])
            );
        }

        #[test]
        fn a_doubled_part_is_counted(
            kind in 0_usize..32,
            outcome in 0_usize..32,
            which in 0_usize..32,
            seed in any::<u64>(),
        ) {
            let (spec, outcome) = pick(kind, outcome);
            let name = outcome.parts[which % outcome.parts.len()];
            let text = draft(&Draft {
                spec,
                outcome: outcome.name,
                shown: outcome.name,
                seed,
                skip: None,
                twice: Some(name),
            });
            let problems = validate(spec, &ctx(1000), &text).expect_err("a part is doubled");
            prop_assert!(problems.contains(&format!("part \"{name}\" appears 2 times")), "{:?}", problems);
        }

        // The generator puts a nested heading and an unknown bold line into every part when the
        // seed asks for it; they must stay inside the part, and no part may be split or added.
        #[test]
        fn nested_headings_and_unknown_markers_stay_in_their_part(
            kind in 0_usize..32,
            outcome in 0_usize..32,
            seed in any::<u64>(),
        ) {
            let (spec, outcome) = pick(kind, outcome);
            let text = filled(spec, outcome.name, seed & !2);
            let all = lines(&text).expect("closed fences");
            let found = parts(spec, all);
            let names: Vec<&str> = found.iter().map(|part| part.name).collect();
            prop_assert_eq!(names, outcome.parts.to_vec());
            let noted = found.iter().all(|part| {
                part.lines.iter().any(|line| line.text.starts_with("**Note:**"))
            });
            prop_assert!(noted);
        }

        #[test]
        fn part_markers_inside_a_fence_are_not_parts(
            kind in 0_usize..32,
            which in 0_usize..32,
            bold in any::<bool>(),
            fence in 3_usize..6,
        ) {
            let spec = &KINDS[kind % KINDS.len()];
            let names = spec.part_names();
            let marker = names[which % names.len()];
            let marker = if bold { format!("**{marker}:** x") } else { format!("## {marker}") };
            let fence = "`".repeat(fence);
            let first = names[0];
            let text = format!("## {first}\n{fence}\n{marker}\n{fence}\nafter\n");
            let found = parts(spec, lines(&text).expect("closed"));
            prop_assert_eq!(found.len(), 1);
            prop_assert_eq!(found.first().map(|part| part.name), Some(first));
            prop_assert_eq!(found.first().map(|part| part.lines.len()), Some(4));
        }

        #[test]
        fn any_text_gives_a_verdict_or_header_problems(
            pieces in proptest::collection::vec(
                prop_oneof![
                    Just("## Form\n".to_owned()),
                    Just("```toml\n".to_owned()),
                    Just("```\n".to_owned()),
                    Just("kind = \"decision-point\"\n".to_owned()),
                    Just("outcome = \"waiting-for-the-owner\"\n".to_owned()),
                    Just("input = \"x\"\n".to_owned()),
                    "[ -~\n]{0,12}",
                ],
                0..20,
            ),
        ) {
            let text: String = pieces.concat();
            match validate(decision_point(), &ctx(30), &text) {
                Ok(_) => {}
                Err(problems) => prop_assert!(!problems.is_empty()),
            }
        }
    }

    #[test]
    fn header_problems_are_the_formats_messages_and_the_headers_own() {
        let check = |text: &str| validate(decision_point(), &ctx(30), text).expect_err("refused");
        assert_eq!(
            check("no block\n"),
            ["out.md has no toml block under \"## Form\""]
        );
        let not_toml = check("## Form\n```toml\nkind = \n```\n");
        assert_eq!(not_toml.len(), 1);
        assert!(not_toml[0].starts_with("out.md is not valid TOML: "));
        assert_eq!(
            check("## Form\n```toml\nz = 1\na = 2\nkind = \"\"\noutcome = 3\n```\n"),
            [
                "header: unknown key \"a\"",
                "header: unknown key \"z\"",
                "header: kind must be a non-empty string",
                "header: outcome must be a non-empty string",
                "header: input must be a non-empty string",
            ]
        );
        assert_eq!(
            check("## Form\n```toml\nkind = \"x\"\noutcome = \"y\"\ninput = \"z\"\n```\n"),
            ["header: kind is \"x\", expected \"decision-point\""]
        );
    }

    #[test]
    fn an_owner_message_needs_its_label_and_ends_with_next() {
        let spec = decision_point();
        let good = filled(spec, "waiting-for-the-owner", 0);
        assert!(validate(spec, &ctx(30), &good).is_ok());

        let unlabelled = good.replace("[shipit · slice · phase]", "[shipit · slice]");
        assert_eq!(
            validate(spec, &ctx(30), &unlabelled),
            Err(vec![
                "label: the line after the Form section must be [project · slice · phase]"
                    .to_owned()
            ])
        );

        let misordered = format!("{good}**What:** again\n");
        assert_eq!(
            validate(spec, &ctx(30), &misordered),
            Err(vec![
                "part \"What\" appears 2 times".to_owned(),
                "part \"Next\" must be the last part".to_owned()
            ])
        );
    }

    #[test]
    fn exactly_max_lines_pass_and_one_more_fails_with_the_count() {
        let spec = decision_point();
        let text = filled(spec, "waiting-for-the-owner", 0);
        let count = text
            .lines()
            .skip_while(|line| *line != "```")
            .skip(1)
            .count();
        let max = u64::try_from(count).expect("a small count");
        assert!(validate(spec, &ctx(max), &text).is_ok());
        assert_eq!(
            validate(spec, &ctx(max - 1), &text),
            Err(vec![format!(
                "too long: {count} lines (owner.max_lines is {})",
                max - 1
            )])
        );
    }
}
