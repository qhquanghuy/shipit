use toml::{Table, Value};

use crate::pure::block::toml_block;
use crate::pure::coverage_map::parse_map;
use crate::pure::finding::{
    Class, Outcome as FindingOutcome, ResultKind, outcome_problems, parse_finding,
};
use crate::pure::goal::goal_items;
use crate::pure::kinds::form;
use crate::pure::markdown::{Kind, Line, lines};
use crate::pure::markdown_parts::{bold_marker, heading, list_item};
use crate::pure::messages::{FILE_PROBLEM, NO_BLOCK, NOT_TOML, say};
use crate::pure::roadmap::{Element, meets_problems, parse_roadmap};
use crate::pure::setup::toml_table;
use crate::pure::text::comma_list;

const FORM: &str = "## Form";
const HEADER_KEYS: [&str; 3] = ["kind", "outcome", "input"];
const NEXT: &str = "Next";
const PLACEHOLDER_LABEL: &str = "[project · slice · phase]";
const SLICES: &str = "## Slices";
const COVERAGE: &str = "## Done-when coverage";
const SPIKE: &str = "settled by spike ";

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
    pub(crate) checks: &'static [(&'static str, Check)],
    // The part whose items are findings, and the kind of result they go with.
    pub(crate) findings: Option<(&'static str, ResultKind)>,
    // Parts where one item `none` says there is nothing to say.
    pub(crate) none_allowed: &'static [&'static str],
    pub(crate) embedded: Embedded,
    // Parts that an outcome refuses: (outcome, part).
    pub(crate) forbidden: &'static [(&'static str, &'static str)],
    pub(crate) contradicts: &'static [Contradiction],
}

// An item that the outcome cannot have: it starts with `start` in `part`; with `needs_finding`
// the item is only refused when the output has no finding at all.
#[derive(Debug)]
pub(crate) struct Contradiction {
    pub(crate) outcome: &'static str,
    pub(crate) part: &'static str,
    pub(crate) start: &'static str,
    pub(crate) needs_finding: bool,
}

// A machine format inside the output, read by the formats and found under a heading of its own.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Embedded {
    Nothing,
    Roadmap,
    CoverageMap,
}

#[derive(Debug)]
pub(crate) enum Check {
    Answer(&'static [&'static str]),
    Contains(&'static [&'static str]),
    FileLines,
    // The item contains one of the phrases and then the name of a slice or proof step.
    NamesElement(&'static [&'static str]),
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
    // The text of the goal file a roadmap is held against.
    pub(crate) goal: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) outcome: String,
    // Must fix, nice to fix and must answer, for a kind that has findings.
    pub(crate) counts: Option<[usize; 3]>,
}

pub(crate) struct Header {
    pub(crate) kind: String,
    pub(crate) outcome: String,
}

// The formats' own messages for a block that is missing or cannot be read.
fn block_table(file: &str, text: &str, heading: &str) -> Result<Table, String> {
    match toml_block(text, heading) {
        Ok(Some(block)) => toml_table(block.as_bytes())
            .map_err(|why| say(NOT_TOML, &[("file", &file), ("why", &why)])),
        Ok(None) => Err(say(NO_BLOCK, &[("file", &file), ("heading", &heading)])),
        Err(why) => Err(say(FILE_PROBLEM, &[("file", &file), ("why", &why)])),
    }
}

// The names of the slices and proof steps, when the block is a valid roadmap, and the problems
// the formats find in it and in its goal. Without the goal text nothing is held against it.
fn roadmap_problems(
    file: &str,
    text: &str,
    goal: Option<&str>,
) -> (Option<Vec<String>>, Vec<String>) {
    let table = match block_table(file, text, SLICES) {
        Ok(table) => table,
        Err(why) => return (None, vec![why]),
    };
    let roadmap = match parse_roadmap(&table) {
        Ok(roadmap) => roadmap,
        Err(problems) => return (None, problems.iter().map(ToString::to_string).collect()),
    };
    let names = roadmap
        .elements
        .iter()
        .map(|element| match element {
            Element::Slice(slice) => slice.name.to_string(),
            Element::Proof(proof) => proof.name.to_string(),
        })
        .collect();
    let problems = match goal.map(goal_items) {
        None => Vec::new(),
        Some(Err(why)) => vec![why],
        Some(Ok(items)) => meets_problems(&roadmap, &items)
            .iter()
            .map(ToString::to_string)
            .collect(),
    };
    (Some(names), problems)
}

fn map_problems(file: &str, text: &str) -> Vec<String> {
    match block_table(file, text, COVERAGE) {
        Err(why) => vec![why],
        Ok(table) => parse_map(&table).err().map_or_else(Vec::new, |problems| {
            problems.iter().map(ToString::to_string).collect()
        }),
    }
}

pub(crate) fn header(file: &str, text: &str) -> Result<Header, Vec<String>> {
    let table = block_table(file, text, FORM).map_err(|why| vec![why])?;
    // Text above the Form section would escape the owner's line cap, which counts what follows it.
    let first = text
        .trim_start_matches('\u{feff}')
        .lines()
        .find(|line| !line.trim().is_empty());
    if first != Some(FORM) {
        return Err(vec![format!(
            "header: the output must start with the \"{FORM}\" section"
        )]);
    }
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
    // The line number of each of the lines, counted from 1.
    numbers: Vec<usize>,
}

impl Part<'_> {
    fn is_empty(&self) -> bool {
        self.texts().is_empty()
    }

    // The lines with something on them, trimmed.
    fn texts(&self) -> Vec<&str> {
        let lead = self.lead.trim();
        let lines = self.lines.iter().map(|line| line.text.trim());
        std::iter::once(lead)
            .chain(lines)
            .filter(|text| !text.is_empty())
            .collect()
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
    for (index, line) in all.into_iter().enumerate() {
        if matches!(line.kind, Kind::Prose) {
            if let Some((name, level, lead)) = start_of(&names, line.text) {
                done.extend(open.take().map(|(part, _level)| part));
                open = Some((
                    Part {
                        name,
                        lead,
                        lines: Vec::new(),
                        numbers: Vec::new(),
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
            part.numbers.push(index + 1);
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
    let label_problem = if label.is_some_and(|line| line.text.trim() == PLACEHOLDER_LABEL) {
        Some(format!(
            "label: {PLACEHOLDER_LABEL} is still the template's; name the project, the slice and the phase"
        ))
    } else {
        (!label.is_some_and(|line| is_label(line.text)))
            .then(|| format!("label: the line after the Form section must be {PLACEHOLDER_LABEL}"))
    };
    let final_empty = body.last().is_some_and(|line| line.text.is_empty());
    let count = body.len() - usize::from(final_empty);
    let long = u64::try_from(count).is_ok_and(|count| count > max_lines);
    let length_problem =
        long.then(|| format!("too long: {count} lines (owner.max_lines is {max_lines})"));
    (label_problem, length_problem)
}

fn part_problems(
    names: &[&'static str],
    required: &[&str],
    found: &[Part<'_>],
    template: &[Part<'_>],
) -> Vec<String> {
    let mut problems = Vec::new();
    for name in names {
        let mine: Vec<&Part<'_>> = found.iter().filter(|part| part.name == *name).collect();
        match mine.as_slice() {
            [] if required.contains(name) => problems.push(format!("part \"{name}\" is missing")),
            [] => {}
            [only] if only.is_empty() => problems.push(format!("part \"{name}\" is empty")),
            [only] if is_template_text(only, template) => {
                problems.push(format!("part \"{name}\" is still the template's text"));
            }
            [_only] => {}
            [_, _, ..] => problems.push(format!("part \"{name}\" appears {} times", mine.len())),
        }
    }
    problems
}

// A part whose every line is a line of the same part in the blank form was never filled in.
fn is_template_text(part: &Part<'_>, template: &[Part<'_>]) -> bool {
    let own = part.texts();
    template
        .iter()
        .filter(|blank| blank.name == part.name)
        .any(|blank| {
            let given = blank.texts();
            own.iter().all(|text| given.contains(text))
        })
}

// A list item is read only outside fences, from the left margin.
fn items<'a>(part: &Part<'a>) -> Vec<&'a str> {
    part.lines
        .iter()
        .filter(|line| matches!(line.kind, Kind::Prose))
        .filter_map(|line| list_item(line.text))
        .collect()
}

// `path:line: text` or `path:first-last: text`, the path without spaces and first not after last.
fn is_file_lines(item: &str) -> bool {
    let Some((place, text)) = item.split_once(": ") else {
        return false;
    };
    let Some((path, lines)) = place.rsplit_once(':') else {
        return false;
    };
    let range = match lines.split_once('-') {
        Some((first, last)) => first.parse::<u64>().ok().zip(last.parse::<u64>().ok()),
        None => lines.parse::<u64>().ok().map(|line| (line, line)),
    };
    !path.is_empty()
        && !path.contains(char::is_whitespace)
        && range.is_some_and(|(first, last)| first <= last)
        && !text.trim().is_empty()
}

// A name after a phrase; a spike is not in the roadmap, so any name will do for it. Without the
// roadmap's names (it did not parse) nothing can be held against them.
fn names_an_element(item: &str, phrases: &[&str], names: Option<&[String]>) -> bool {
    let Some(names) = names else {
        return true;
    };
    phrases.iter().any(|phrase| {
        let Some((_, after)) = item.split_once(phrase) else {
            return false;
        };
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect();
        !name.is_empty() && (*phrase == SPIKE || names.contains(&name))
    })
}

impl Check {
    // None when the item passes; else what it must do, to follow the item's number.
    fn refuse(&self, item: &str, names: Option<&[String]>) -> Option<String> {
        match self {
            Self::Answer(starts) => {
                let answered = starts.iter().any(|start| {
                    item.strip_prefix(start)
                        .is_some_and(|why| !why.trim().is_empty())
                });
                (!answered).then(|| {
                    format!(
                        "must start with one of: {}, then say why",
                        comma_list(starts.iter().map(|start| start.trim_end()))
                    )
                })
            }
            Self::Contains(phrases) => (!phrases.iter().any(|phrase| item.contains(phrase)))
                .then(|| format!("must contain one of: {}", comma_list(phrases.iter()))),
            Self::FileLines => (!is_file_lines(item))
                .then(|| "must start with <file>:<line> or <file>:<first>-<last>".to_owned()),
            Self::NamesElement(phrases) => (!names_an_element(item, phrases, names)).then(|| {
                format!(
                    "must name a slice or proof step after one of: {}",
                    comma_list(phrases.iter().map(|phrase| phrase.trim_end()))
                )
            }),
        }
    }
}

fn check_problems(
    name: &str,
    check: &Check,
    items: &[&str],
    none_is_ok: bool,
    names: Option<&[String]>,
) -> Vec<String> {
    if items.is_empty() {
        return vec![format!("part \"{name}\" has no list items")];
    }
    if none_is_ok && items == ["none"] {
        return Vec::new();
    }
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            check
                .refuse(item, names)
                .map(|why| format!("part \"{name}\" item {} {why}", index + 1))
        })
        .collect()
}

// Formats judges each finding and then the result; a finding that cannot be read stops the judgement.
fn judge(
    name: &str,
    kind: ResultKind,
    outcome: &str,
    items: &[&str],
) -> Result<[usize; 3], Vec<String>> {
    let mut found = Vec::new();
    let mut refused = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match parse_finding(index, item) {
            Ok(finding) => found.push(finding),
            Err(why) => refused.push(format!("part \"{name}\" item {}: {why}", index + 1)),
        }
    }
    if !refused.is_empty() {
        return Err(refused);
    }
    let result = FindingOutcome::parse(outcome).ok_or_else(|| {
        vec![format!(
            "header: outcome \"{outcome}\" is not a review or test-drive result"
        )]
    })?;
    let problems = outcome_problems(kind, result, &found);
    if !problems.is_empty() {
        return Err(problems);
    }
    let count = |class: Class| {
        found
            .iter()
            .filter(|finding| finding.class == class)
            .count()
    };
    Ok([
        count(Class::MustFix),
        count(Class::NiceToFix),
        count(Class::MustAnswer),
    ])
}

// A part is checked only when it appears once: a missing or doubled part is already a problem.
fn once<'l, 'a>(found: &'l [Part<'a>], name: &str) -> Option<&'l Part<'a>> {
    let mut mine = found.iter().filter(|part| part.name == name);
    let first = mine.next()?;
    mine.next().is_none().then_some(first)
}

// An answer that says the opposite of the outcome is refused; the rules are in the spec.
fn contradictions(
    spec: &Spec,
    outcome: &str,
    found: &[Part<'_>],
    counts: Option<[usize; 3]>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for rule in spec
        .contradicts
        .iter()
        .filter(|rule| rule.outcome == outcome)
    {
        let Some(part) = once(found, rule.part) else {
            continue;
        };
        if rule.needs_finding && counts != Some([0, 0, 0]) {
            continue;
        }
        let start = rule.start.trim_end();
        for (index, item) in items(part).iter().enumerate() {
            if !item.starts_with(rule.start) {
                continue;
            }
            let place = format!(
                "part \"{}\" item {} starts with {start}",
                rule.part,
                index + 1
            );
            problems.push(if rule.needs_finding {
                format!("{place}, but the outcome {outcome} has no finding")
            } else {
                format!("{place}, which does not fit the outcome {outcome}")
            });
        }
    }
    problems
}

// The numbers of the lines that read as a finding once their indent and list marker are gone.
fn loose_findings(spec: &Spec, all: &[Line<'_>]) -> Vec<usize> {
    if spec.findings.is_none() {
        return Vec::new();
    }
    all.iter()
        .enumerate()
        .filter(|(_, line)| matches!(line.kind, Kind::Prose))
        .filter(|(_, line)| {
            let text = line.text.trim_start();
            parse_finding(0, list_item(text).unwrap_or(text)).is_ok()
        })
        .map(|(index, _)| index + 1)
        .collect()
}

// A finding that no item of the findings part counts would escape the result: it is nested, plain
// text, or under a heading that closed the part.
fn hidden_findings(name: &str, loose: &[usize], found: &[Part<'_>]) -> Vec<String> {
    let counted: Vec<usize> = found
        .iter()
        .filter(|part| part.name == name)
        .flat_map(|part| part.lines.iter().zip(&part.numbers))
        .filter(|(line, _)| matches!(line.kind, Kind::Prose) && list_item(line.text).is_some())
        .map(|(_, number)| *number)
        .collect();
    loose
        .iter()
        .filter(|number| !counted.contains(number))
        .map(|number| {
            format!("line {number}: a finding that no item of the part \"{name}\" counts; make it a list item there")
        })
        .collect()
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
    let (names, embedded) = match spec.embedded {
        Embedded::Nothing => (None, Vec::new()),
        Embedded::Roadmap => roadmap_problems(ctx.file, text, ctx.goal),
        Embedded::CoverageMap => (None, map_problems(ctx.file, text)),
    };
    let loose = loose_findings(spec, &all);
    let found = parts(spec, all);
    let blank = form(spec.kind)
        .and_then(|blank| lines(blank).ok())
        .map(|blank| parts(spec, blank))
        .unwrap_or_default();
    let mut problems: Vec<String> = label.into_iter().collect();
    problems.extend(part_problems(
        &spec.part_names(),
        outcome.parts,
        &found,
        &blank,
    ));
    for (shown, part) in spec.forbidden {
        if *shown == head.outcome && found.iter().any(|found| found.name == *part) {
            problems.push(format!(
                "part \"{part}\" does not belong to the outcome {shown}"
            ));
        }
    }
    for (name, check) in spec.checks {
        if let Some(part) = once(&found, name) {
            let none_is_ok = spec.none_allowed.contains(name);
            problems.extend(check_problems(
                name,
                check,
                &items(part),
                none_is_ok,
                names.as_deref(),
            ));
        }
    }
    let mut counts = None;
    if let Some((name, kind)) = spec.findings {
        let listed = once(&found, name).map(items).unwrap_or_default();
        if once(&found, name).is_some() && listed.is_empty() {
            problems.push(format!("part \"{name}\" has no list items"));
        } else {
            match judge(name, kind, &head.outcome, &listed) {
                Ok(done) => counts = Some(done),
                Err(more) => problems.extend(more),
            }
        }
    }
    problems.extend(contradictions(spec, &head.outcome, &found, counts));
    if let Some((name, _kind)) = spec.findings {
        problems.extend(hidden_findings(name, &loose, &found));
    }
    let next_is_last = found
        .iter()
        .rposition(|part| part.name == NEXT)
        .is_none_or(|last| last + 1 == found.len());
    if spec.owner_message && !next_is_last {
        problems.push(format!("part \"{NEXT}\" must be the last part"));
    }
    problems.extend(length);
    problems.extend(embedded);
    if problems.is_empty() {
        Ok(Verdict {
            outcome: head.outcome,
            counts,
        })
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use super::*;
    use crate::pure::kinds::KINDS;

    type Bodies = Vec<(&'static str, Vec<String>)>;

    fn is_must_fix_result(outcome: &str) -> bool {
        !matches!(outcome, "approve" | "pass")
    }

    fn chosen<'a>(list: &[&'a str], seed: u64) -> &'a str {
        let which = usize::try_from(seed % 7).expect("small") % list.len();
        list.get(which).copied().unwrap_or_default()
    }

    const GOAL: &str = "# Goal\n\n## Done-when\n\n1. one\n2. two\n";

    const ROADMAP_BLOCK: [&str; 24] = [
        "```toml",
        "goal = \"demo\"",
        "",
        "[[slice]]",
        "name = \"alpha\"",
        "after = []",
        "meets = [\"DW1\"]",
        "owns = [\"src/alpha.rs\"]",
        "test_drive = false",
        "",
        "[[slice]]",
        "name = \"beta\"",
        "after = [\"alpha\"]",
        "meets = [\"DW2\"]",
        "owns = [\"src/beta.rs\"]",
        "test_drive = true",
        "",
        "[[proof]]",
        "name = \"proof\"",
        "after = [\"beta\"]",
        "meets = []",
        "how = \"by the owner\"",
        "```",
        "",
    ];

    const COVERAGE_BLOCK: [&str; 3] = ["```toml", "DW1 = [\"a_test\", \"another test\"]", "```"];

    fn lines_of(block: &[&str]) -> Vec<String> {
        block.iter().map(|line| (*line).to_owned()).collect()
    }

    // The lines of a part: list items made so that the check of the part passes, or the block
    // the formats read.
    fn sample(spec: &Spec, outcome: &str, part: &str, seed: u64) -> Vec<String> {
        match (spec.embedded, part) {
            (Embedded::Roadmap, "Slices") => return lines_of(&ROADMAP_BLOCK),
            (Embedded::CoverageMap, "Done-when coverage") => return lines_of(&COVERAGE_BLOCK),
            _ => {}
        }
        let item = if let Some((name, _kind)) = spec.findings
            && name == part
        {
            if is_must_fix_result(outcome) {
                "issue (blocking, security): words".to_owned()
            } else {
                "nitpick: words".to_owned()
            }
        } else {
            let check = spec.checks.iter().find(|(name, _)| *name == part);
            match check.map(|(_, check)| check) {
                Some(Check::Answer(starts)) => {
                    // An answer that says the opposite of the outcome is not a valid sample.
                    let fits: Vec<&str> = starts
                        .iter()
                        .copied()
                        .filter(|start| {
                            !spec.contradicts.iter().any(|rule| {
                                rule.outcome == outcome && rule.part == part && rule.start == *start
                            })
                        })
                        .collect();
                    format!("{}because of {seed}", chosen(&fits, seed))
                }
                Some(Check::Contains(phrases)) => format!("a {}word", chosen(phrases, seed)),
                Some(Check::FileLines) if seed.is_multiple_of(2) => {
                    "src/a.rs:3-5: words".to_owned()
                }
                Some(Check::FileLines) => "src/a.rs:4: words".to_owned(),
                Some(Check::NamesElement(phrases)) => {
                    format!("a thing, {}alpha and more", chosen(phrases, seed))
                }
                None => format!("words {seed}"),
            }
        };
        vec![format!("- {item}")]
    }

    fn bodies(spec: &Spec, outcome: &str, seed: u64) -> Bodies {
        let parts = spec
            .outcomes
            .iter()
            .find(|listed| listed.name == outcome)
            .map(|listed| listed.parts)
            .unwrap_or_default();
        parts
            .iter()
            .map(|part| (*part, sample(spec, outcome, part, seed)))
            .collect()
    }

    // The seed picks the style of the part markers and whether nested content is added.
    fn render(spec: &Spec, shown: &str, seed: u64, parts: &Bodies) -> String {
        // A block is found under a heading, so a kind with one has heading parts only.
        let bold = seed & 1 == 0 && matches!(spec.embedded, Embedded::Nothing);
        let nested = seed & 2 == 0;
        let mut text = format!(
            "## Form\n\n```toml\nkind = \"{}\"\noutcome = \"{shown}\"\ninput = \"x\"\n```\n\n",
            spec.kind
        );
        if spec.owner_message {
            text.push_str("[shipit · slice · phase]\n\n");
        }
        for (name, items) in parts {
            if bold {
                writeln!(text, "**{name}:** some words").ok();
            } else {
                write!(text, "## {name}\n\nsome words\n").ok();
            }
            for item in items {
                writeln!(text, "{item}").ok();
            }
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
        }
        text
    }

    fn filled(spec: &Spec, outcome: &str, seed: u64) -> String {
        render(spec, outcome, seed, &bodies(spec, outcome, seed))
    }

    fn ctx(max_lines: u64) -> Context<'static> {
        Context {
            file: "out.md",
            max_lines,
            goal: Some(GOAL),
        }
    }

    // Some outcome of some kind, by index, so that every kind the specs list is covered.
    fn pick(kind: usize, outcome: usize) -> (&'static Spec, &'static Outcome) {
        let spec = &KINDS[kind % KINDS.len()];
        (spec, &spec.outcomes[outcome % spec.outcomes.len()])
    }

    fn spec_of(kind: &str) -> &'static Spec {
        KINDS
            .iter()
            .find(|spec| spec.kind == kind)
            .expect("a kind that exists")
    }

    fn problems_of(spec: &Spec, text: &str) -> Vec<String> {
        validate(spec, &ctx(1000), text).expect_err("refused")
    }

    fn with_body(spec: &Spec, outcome: &str, part: &str, items: &[&str]) -> String {
        let mut parts = bodies(spec, outcome, 0);
        let items = items.iter().map(|item| format!("- {item}")).collect();
        if let Some((_, body)) = parts.iter_mut().find(|(name, _)| *name == part) {
            *body = items;
        } else {
            let name = spec
                .part_names()
                .into_iter()
                .find(|name| *name == part)
                .expect("a part of the spec");
            parts.push((name, items));
        }
        render(spec, outcome, 0, &parts)
    }

    // A finding as the formats read it, or as it would be refused.
    fn finding_text() -> impl Strategy<Value = String> {
        prop_oneof![
            "(issue|suggestion|question|nitpick)( \\((blocking|non-blocking|security)(, security)?\\))?: [a-z ]{0,6}",
            "[a-z][a-z ]{0,8}",
        ]
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
            let verdict = validate(spec, &ctx(1000), &text);
            prop_assert_eq!(verdict.map(|done| done.outcome), Ok(outcome.name.to_owned()), "{}", text);
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
            let mut parts = bodies(spec, outcome.name, seed);
            parts.retain(|(part, _)| *part != name);
            let text = render(spec, outcome.name, seed, &parts);
            let problems = problems_of(spec, &text);
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
            let text = render(spec, &shown, seed, &bodies(spec, outcome.name, seed));
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
            let mut parts = bodies(spec, outcome.name, seed);
            let at = parts.iter().position(|(part, _)| *part == name).expect("a part of the outcome");
            let again = parts[at].clone();
            parts.insert(at, again);
            let text = render(spec, outcome.name, seed, &parts);
            let problems = problems_of(spec, &text);
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
            let found = parts(spec, lines(&text).expect("closed fences"));
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
                    Just("kind = \"second-opinion\"\n".to_owned()),
                    Just("outcome = \"rework\"\n".to_owned()),
                    Just("input = \"x\"\n".to_owned()),
                    Just("## Findings\n- issue (blocking): x\n".to_owned()),
                    "[ -~\n]{0,12}",
                ],
                0..20,
            ),
        ) {
            let text: String = pieces.concat();
            match validate(spec_of("second-opinion"), &ctx(30), &text) {
                Ok(_) => {}
                Err(problems) => prop_assert!(!problems.is_empty()),
            }
        }

        // What `validate` says about the findings is what the formats say, in the same words.
        #[test]
        fn findings_problems_are_formats_problems(
            result in prop::sample::select(vec!["approve", "approve-after-fixes", "rework"]),
            texts in proptest::collection::vec(finding_text(), 1..5),
        ) {
            let spec = spec_of("second-opinion");
            let items: Vec<&str> = texts.iter().map(|text| text.trim()).collect();
            let text = with_body(spec, result, "Findings", &items);
            let mut refused = Vec::new();
            let mut found = Vec::new();
            for (index, item) in items.iter().enumerate() {
                match parse_finding(index, item) {
                    Ok(finding) => found.push(finding),
                    Err(why) => refused.push(format!("part \"Findings\" item {}: {why}", index + 1)),
                }
            }
            let outcome = FindingOutcome::parse(result).expect("a review result");
            let expected = if refused.is_empty() {
                outcome_problems(ResultKind::Review, outcome, &found)
            } else {
                refused
            };
            // The result may name a part the outcome does not list, so Findings is always there.
            let got = validate(spec, &ctx(1000), &text);
            match got {
                Ok(done) => {
                    prop_assert!(expected.is_empty(), "{:?}", expected);
                    let count = |class: Class| found.iter().filter(|f| f.class == class).count();
                    prop_assert_eq!(
                        done.counts,
                        Some([count(Class::MustFix), count(Class::NiceToFix), count(Class::MustAnswer)])
                    );
                }
                Err(problems) => prop_assert_eq!(problems, expected),
            }
        }

        #[test]
        fn answers_need_a_why(
            part in prop::sample::select(vec!["Security", "Performance"]),
            start in prop::sample::select(vec!["none found: ", "concern: ", "not relevant: "]),
            why in "[a-z][a-z ]{0,8}",
        ) {
            let spec = spec_of("second-opinion");
            let bare = with_body(spec, "approve-after-fixes", part, &[start.trim_end()]);
            prop_assert_eq!(
                problems_of(spec, &bare),
                vec![format!(
                    "part \"{part}\" item 1 must start with one of: none found:, concern:, not relevant:, then say why"
                )]
            );
            let answered = with_body(spec, "approve-after-fixes", part, &[&format!("{start}{why}")]);
            prop_assert!(validate(spec, &ctx(1000), &answered).is_ok());
        }

        #[test]
        fn file_lines_rule(
            path in "[a-z/.]{1,6}",
            space in any::<bool>(),
            first in 0_u64..20,
            last in 0_u64..20,
            range in any::<bool>(),
        ) {
            let path = if space { format!("{path} x") } else { path };
            let place = if range { format!("{path}:{first}-{last}") } else { format!("{path}:{first}") };
            let spec = spec_of("explanation");
            let text = with_body(spec, "understood", "Parts", &[&format!("{place}: words")]);
            let accepted = !space && (!range || first <= last);
            prop_assert_eq!(validate(spec, &ctx(1000), &text).is_ok(), accepted, "{}", place);
        }

        #[test]
        fn none_only_where_the_spec_allows_it(
            kind in 0_usize..32,
            which in 0_usize..32,
            seed in any::<u64>(),
        ) {
            let spec = &KINDS[kind % KINDS.len()];
            prop_assume!(!spec.checks.is_empty());
            let (name, _check) = &spec.checks[which % spec.checks.len()];
            prop_assume!(!spec.none_allowed.contains(name));
            let outcome = spec.outcomes.iter().find(|listed| listed.parts.contains(name)).expect("an outcome");
            let mut parts = bodies(spec, outcome.name, seed);
            for (part, body) in &mut parts {
                if *part == *name {
                    *body = vec!["- none".to_owned()];
                }
            }
            let text = render(spec, outcome.name, seed, &parts);
            prop_assert!(validate(spec, &ctx(1000), &text).is_err());
        }
    }

    const FACTS_MAY_BE_NONE: Spec = Spec {
        kind: "x",
        outcomes: &[Outcome {
            name: "found",
            parts: &["Facts"],
        }],
        owner_message: false,
        checks: &[("Facts", Check::Contains(&["(source: "]))],
        findings: None,
        none_allowed: &["Facts"],
        embedded: Embedded::Nothing,
        forbidden: &[],
        contradicts: &[],
    };

    #[test]
    fn a_lone_none_passes_in_a_listed_part_only() {
        let alone = with_body(&FACTS_MAY_BE_NONE, "found", "Facts", &["none"]);
        assert!(validate(&FACTS_MAY_BE_NONE, &ctx(1000), &alone).is_ok());
        let with_more = with_body(
            &FACTS_MAY_BE_NONE,
            "found",
            "Facts",
            &["none", "(source: x)"],
        );
        assert!(validate(&FACTS_MAY_BE_NONE, &ctx(1000), &with_more).is_err());
        let scout = spec_of("scout-report");
        let text = with_body(scout, "found", "Facts", &["none"]);
        assert_eq!(
            problems_of(scout, &text),
            ["part \"Facts\" item 1 must contain one of: (source: "]
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // The roadmap block has alpha, beta and a proof step; a spike may be named freely.
        #[test]
        fn named_slices_must_exist(
            part in 0_usize..3,
            name in "[a-z][a-z0-9-]{0,6}",
            known in prop::sample::select(vec!["alpha", "beta", "proof"]),
            use_known in any::<bool>(),
        ) {
            let spec = spec_of("roadmap");
            let (part, phrase, shown) = [
                ("Shared contracts", "lands first in slice ", "lands first in slice"),
                ("Owner decisions", "before slice ", "before slice"),
                ("Risks", "settled by slice ", "settled by slice, settled by spike"),
            ][part];
            let name = if use_known { known.to_owned() } else { name };
            let text = with_body(spec, "ready-for-review", part, &[&format!("a thing, {phrase}{name} and more")]);
            let exists = ["alpha", "beta", "proof"].contains(&name.as_str());
            let got = validate(spec, &ctx(1000), &text);
            prop_assert_eq!(got.is_ok(), exists, "{:?}", got);
            if !exists {
                let message = format!(
                    "part \"{part}\" item 1 must name a slice or proof step after one of: {shown}"
                );
                prop_assert_eq!(got, Err(vec![message]));
            }
            let spike = with_body(spec, "ready-for-review", "Risks", &[&format!("a risk, settled by spike {name}")]);
            prop_assert!(validate(spec, &ctx(1000), &spike).is_ok());
        }
    }

    #[test]
    fn a_roadmap_may_say_none_in_the_three_parts_and_nowhere_else() {
        let spec = spec_of("roadmap");
        for part in ["Shared contracts", "Owner decisions", "Risks"] {
            let text = with_body(spec, "ready-for-review", part, &["none"]);
            assert!(validate(spec, &ctx(1000), &text).is_ok(), "{part}");
        }
        let text = with_body(
            spec,
            "ready-for-review",
            "Risks",
            &["none", "a risk, settled by slice alpha"],
        );
        assert_eq!(
            problems_of(spec, &text),
            [
                "part \"Risks\" item 1 must name a slice or proof step after one of: settled by slice, settled by spike"
            ]
        );
    }

    // The block's own problems are the formats' words, after the engine's.
    #[test]
    fn roadmap_problems_come_from_formats() {
        let spec = spec_of("roadmap");
        let good = filled(spec, "ready-for-review", 1);

        let unknown = good.replace("after = [\"alpha\"]", "after = [\"zzz\"]");
        let table = toml_table(
            unknown
                .split_once("## Slices")
                .and_then(|(_, rest)| rest.split_once("```toml\n"))
                .and_then(|(_, rest)| rest.split_once("```"))
                .map(|(block, _)| block)
                .expect("a block")
                .as_bytes(),
        )
        .expect("toml");
        let wanted: Vec<String> = parse_roadmap(&table)
            .expect_err("an unknown name")
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            wanted,
            [".slice[1].after[0]: no slice or proof step named \"zzz\""]
        );
        assert_eq!(problems_of(spec, &unknown), wanted);

        let unmet = good.replace("meets = [\"DW2\"]", "meets = []");
        assert_eq!(problems_of(spec, &unmet), ["DW2: no slice"]);

        let outside = good.replace("meets = [\"DW1\"]", "meets = [\"DW9\"]");
        assert_eq!(
            problems_of(spec, &outside),
            [
                ".slice[0].meets[0]: \"DW9\" is not a done-when item of the goal",
                "DW1: no slice"
            ]
        );

        let no_block = good.replace("```toml\ngoal", "```text\ngoal");
        assert_eq!(
            problems_of(spec, &no_block),
            ["out.md has no toml block under \"## Slices\""]
        );
    }

    #[test]
    fn a_build_plans_coverage_block_is_read_by_the_formats() {
        let spec = spec_of("build-plan");
        let good = filled(spec, "ready-for-review", 1);
        assert!(validate(spec, &ctx(1000), &good).is_ok());

        let bad = good.replace("DW1 = [\"a_test\"", "DW1 = [\"a_DW3_test\"");
        assert_eq!(
            problems_of(spec, &bad),
            [".DW1[0]: a test name must not carry a done-when id"]
        );
        let empty = good.replace("DW1 = [\"a_test\", \"another test\"]", "");
        assert!(
            validate(spec, &ctx(1000), &empty).is_ok(),
            "an empty map is valid"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // The line cap counts what follows the Form section, so nothing may come before it.
        #[test]
        fn text_above_the_form_is_refused(
            kind in 0_usize..32,
            outcome in 0_usize..32,
            seed in any::<u64>(),
            above in "[a-z][a-z ]{0,20}",
        ) {
            let (spec, outcome) = pick(kind, outcome);
            let text = format!("{above}\n{}", filled(spec, outcome.name, seed));
            let problems = problems_of(spec, &text);
            prop_assert!(
                problems.contains(&"header: the output must start with the \"## Form\" section".to_owned()),
                "{:?}", problems
            );
        }

        // A hidden must-fix finding would pass under `approve`: nested, plain, or after a heading.
        #[test]
        fn a_finding_outside_the_findings_list_is_refused(
            kind in prop::sample::select(vec!["second-opinion", "informed-review", "test-drive"]),
            place in 0_usize..3,
            seed in any::<u64>(),
        ) {
            let spec = spec_of(kind);
            let outcome = if kind == "test-drive" { "pass" } else { "approve" };
            let mut text = filled(spec, outcome, seed | 1);
            text.push_str(["  - issue (blocking, security): a\n", "issue (blocking): a\n", "# Notes\n- issue (blocking): a\n"][place]);
            let problems = problems_of(spec, &text);
            prop_assert!(
                problems.iter().any(|problem| problem.contains("a finding that no item of the part \"Findings\" counts")),
                "{:?}", problems
            );
        }
    }

    #[test]
    fn a_part_before_the_form_does_not_escape_the_line_cap() {
        let spec = spec_of("decision-point");
        let good = filled(spec, "waiting-for-the-owner", 0);
        let (form, rest) = good.split_once("[shipit").expect("a label");
        let moved = format!("**What:** one\n**Context:** two\n{form}[shipit{rest}");
        let problems = problems_of(spec, &moved);
        assert_eq!(
            problems,
            ["header: the output must start with the \"## Form\" section"]
        );
    }

    // A blank form with an outcome chosen is every kind's easiest way to say nothing.
    #[test]
    fn a_blank_form_is_refused_for_every_outcome() {
        for spec in KINDS {
            let blank = form(spec.kind).expect("a form for every kind");
            let listed = spec
                .outcomes
                .iter()
                .map(|outcome| outcome.name)
                .collect::<Vec<_>>()
                .join(" | ");
            for outcome in spec.outcomes {
                let text = blank.replace(
                    &format!("outcome = \"{listed}\""),
                    &format!("outcome = \"{}\"", outcome.name),
                );
                let problems = problems_of(spec, &text);
                for part in outcome.parts {
                    let said = format!("part \"{part}\" is still the template's text");
                    assert!(
                        problems.contains(&said),
                        "{} {part}: {problems:?}",
                        spec.kind
                    );
                }
                if spec.owner_message {
                    assert!(
                        problems
                            .iter()
                            .any(|problem| problem
                                .contains("is still the template's; name the project")),
                        "{}: {problems:?}",
                        spec.kind
                    );
                }
            }
        }
    }

    #[test]
    fn a_filled_part_with_one_template_line_is_content() {
        let spec = spec_of("known-issue");
        let blank = form(spec.kind).expect("a form");
        let filled_in = blank.replace(
            "Why it is left open.",
            "Why it is left open. And a real reason.",
        );
        let problems = problems_of(spec, &filled_in);
        assert!(!problems.contains(&"part \"Reason\" is still the template's text".to_owned()));
    }

    #[test]
    fn an_answer_that_says_the_opposite_of_the_outcome_is_refused() {
        let spec = spec_of("test-drive");
        let text = with_body(spec, "pass", "Done-when", &["[fail] nothing works"]);
        assert_eq!(
            problems_of(spec, &text),
            ["part \"Done-when\" item 1 starts with [fail], which does not fit the outcome pass"]
        );
        let text = with_body(spec, "findings", "Done-when", &["[fail] nothing works"]);
        assert!(validate(spec, &ctx(1000), &text).is_ok());

        let spec = spec_of("second-opinion");
        let text = with_body(
            spec,
            "approve",
            "Security",
            &["concern: injection everywhere"],
        );
        assert_eq!(
            problems_of(spec, &text),
            [
                "part \"Security\" item 1 starts with concern:, but the outcome approve has no finding"
            ]
        );
        let with_finding = {
            let mut parts = bodies(spec, "approve", 0);
            for (name, body) in &mut parts {
                if *name == "Security" {
                    *body = vec!["- concern: injection everywhere".to_owned()];
                }
            }
            parts.push(("Findings", vec!["- question: is it fixed?".to_owned()]));
            render(spec, "approve", 0, &parts)
        };
        assert!(validate(spec, &ctx(1000), &with_finding).is_ok());

        let spec = spec_of("informed-review");
        let text = with_body(
            spec,
            "approve",
            "Teach-back",
            &["[wrong] the reader misread it"],
        );
        assert_eq!(
            problems_of(spec, &text),
            [
                "part \"Teach-back\" item 1 starts with [wrong], but the outcome approve has no finding"
            ]
        );
    }

    #[test]
    fn a_part_of_another_outcome_is_refused() {
        let spec = spec_of("explanation");
        let text = with_body(spec, "understood", "Unclear", &["src/a.rs:3: why"]);
        assert_eq!(
            problems_of(spec, &text),
            ["part \"Unclear\" does not belong to the outcome understood"]
        );
        let spec = spec_of("scout-report");
        let text = with_body(spec, "found", "Missing", &["the owner of the repository"]);
        assert_eq!(
            problems_of(spec, &text),
            ["part \"Missing\" does not belong to the outcome found"]
        );
    }

    #[test]
    fn a_checked_part_without_items_is_named() {
        let spec = spec_of("second-opinion");
        let text = with_body(spec, "approve", "Security", &[]);
        assert_eq!(
            problems_of(spec, &text),
            ["part \"Security\" has no list items"]
        );
        let text = with_body(spec, "approve-after-fixes", "Findings", &[]);
        assert_eq!(
            problems_of(spec, &text),
            ["part \"Findings\" has no list items"]
        );
    }

    #[test]
    fn findings_are_optional_with_approve_but_never_a_must_fix() {
        let spec = spec_of("second-opinion");
        let mut parts = bodies(spec, "approve", 0);
        parts.push((
            "Findings",
            vec!["- nitpick: a word".to_owned(), "- question: why".to_owned()],
        ));
        let text = render(spec, "approve", 0, &parts);
        let done = validate(spec, &ctx(1000), &text).expect("valid");
        assert_eq!(done.counts, Some([0, 1, 1]));

        let mut worse = bodies(spec, "approve", 0);
        worse.push(("Findings", vec!["- issue (blocking): bad".to_owned()]));
        let text = render(spec, "approve", 0, &worse);
        assert_eq!(
            problems_of(spec, &text),
            ["approve allows no must-fix finding"]
        );
    }

    #[test]
    fn header_problems_are_the_formats_messages_and_the_headers_own() {
        let spec = spec_of("decision-point");
        let check = |text: &str| problems_of(spec, text);
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
        let spec = spec_of("decision-point");
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
        let spec = spec_of("decision-point");
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
