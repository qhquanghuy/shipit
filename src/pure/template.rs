use toml::{Table, Value};

use crate::pure::block::toml_block;
use crate::pure::coverage_map::parse_map;
use crate::pure::finding::{
    Class, Outcome as FindingOutcome, ResultKind, outcome_problems, parse_finding,
};
use crate::pure::goal::goal_items;
use crate::pure::kinds::form;
use crate::pure::markdown::{Kind, Line, lines};
use crate::pure::markdown_parts::{bold_marker, code_spans, heading, list_item};
use crate::pure::messages::{FILE_PROBLEM, NO_BLOCK, NOT_TOML, say};
use crate::pure::roadmap::{Element, meets_problems, parse_roadmap};
use crate::pure::setup::toml_table;
use crate::pure::text::comma_list;

const FORM: &str = "## Form";
const HEADER_KEYS: [&str; 3] = ["kind", "outcome", "input"];
const NEXT: &str = "Next";
const FINDING_LABELS: [&str; 4] = ["issue", "suggestion", "question", "nitpick"];
const PLACEHOLDER_LABEL: &str = "[project · slice · phase]";
const EXAMPLE: &str = "example";
const SLICES: &str = "## Slices";
const COVERAGE: &str = "## Done-when coverage";
const SPIKE: &str = "settled by spike ";
const FLOW: &str = "Flow:";
const FLOW_LEVEL: usize = 3;
const DIAGRAM: &str = "mermaid";

#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) name: &'static str,
    pub(crate) parts: &'static [&'static str],
}

#[derive(Debug)]
pub(crate) struct Spec {
    pub(crate) kind: &'static str,
    pub(crate) outcomes: &'static [Outcome],
    // Parts that tell the way through a change: unless a part is the single item `none`, each of
    // its flows is a `### Flow:` heading with a Mermaid diagram.
    pub(crate) flows: &'static [&'static str],
    // The parts the output ends with, in this order.
    pub(crate) last: &'static [&'static str],
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

// An item that the outcome cannot have, whatever else the output holds: it starts with `start` in `part`.
#[derive(Debug)]
pub(crate) struct Contradiction {
    pub(crate) outcome: &'static str,
    pub(crate) part: &'static str,
    pub(crate) start: &'static str,
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
    // An answer whose why starts with a done-when id such as `DW3`.
    AnswerForDoneWhen(&'static [&'static str]),
    Contains(&'static [&'static str]),
    FileLines,
    // The item contains one of the phrases and then the name of a slice or proof step.
    NamesElement(&'static [&'static str]),
    // The item names what ran, in a code span with something in it.
    CodeSpan,
    // The item points at a line of a file: a diff or line link, or `<file>:<line>` somewhere in it.
    Place,
    // Any item will do; the part only has to say something, or `none`.
    Item,
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
    pub(crate) example_max_lines: u64,
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

// The lines of an owner message outside example blocks, then the lines inside them. A fence whose
// info string is exactly `example` is an example block; its two fence lines count with its lines,
// so an empty block is not free. An unclosed fence never gets here (`lines` refuses it), so an
// example cannot hide a line.
fn counted(all: &[Line<'_>]) -> (usize, usize) {
    let mut in_example = false;
    let (mut normal, mut example) = (0, 0);
    for line in all {
        let inside = match line.kind {
            Kind::Open { info } => {
                in_example = info == EXAMPLE;
                in_example
            }
            Kind::Inside | Kind::Close => in_example,
            Kind::Prose => false,
        };
        if inside {
            example += 1;
        } else {
            normal += 1;
        }
    }
    (normal, example)
}

// The label problem, and the length problems, of an owner message. Every line outside example
// blocks counts, wherever it sits: above, inside or below the Form section.
fn owner_problems(
    all: &[Line<'_>],
    max_lines: u64,
    example_max_lines: u64,
) -> (Option<String>, Vec<String>) {
    let label = body(all).iter().find(|line| !line.text.trim().is_empty());
    let label_problem = if label.is_some_and(|line| line.text.trim() == PLACEHOLDER_LABEL) {
        Some(format!(
            "label: {PLACEHOLDER_LABEL} is still the template's; name the project, the slice and the phase"
        ))
    } else {
        (!label.is_some_and(|line| is_label(line.text)))
            .then(|| format!("label: the line after the Form section must be {PLACEHOLDER_LABEL}"))
    };
    let final_empty = all.last().is_some_and(|line| line.text.is_empty());
    let (normal, example) = counted(all);
    let count = normal.saturating_sub(usize::from(final_empty));
    let mut length = Vec::new();
    if u64::try_from(count).is_ok_and(|count| count > max_lines) {
        length.push(format!(
            "too long: {count} lines (owner.max_lines is {max_lines})"
        ));
    }
    if u64::try_from(example).is_ok_and(|example| example > example_max_lines) {
        length.push(format!(
            "too many example lines: {example} (owner.example_max_lines is {example_max_lines})"
        ));
    }
    (label_problem, length)
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

// `<first>` or `<first>-<last>`, the first not after the last.
fn line_range(lines: &str) -> Option<(u64, u64)> {
    let range = match lines.split_once('-') {
        Some((first, last)) => first.parse::<u64>().ok().zip(last.parse::<u64>().ok()),
        None => lines.parse::<u64>().ok().map(|line| (line, line)),
    };
    range.filter(|(first, last)| first <= last)
}

// `path:line: text` or `path:first-last: text`, the path without spaces.
fn is_file_lines(item: &str) -> bool {
    let Some((place, text)) = item.split_once(": ") else {
        return false;
    };
    let Some((path, lines)) = place.rsplit_once(':') else {
        return false;
    };
    !path.is_empty()
        && !path.contains(char::is_whitespace)
        && line_range(lines).is_some()
        && !text.trim().is_empty()
}

const BARE_FILES: [&str; 5] = ["Makefile", "Dockerfile", "Justfile", "LICENSE", "Gemfile"];

// A path a reader can open: it has a folder or an extension, or it is a well-known bare name; a
// time, an address or a template's placeholder is not.
fn is_file(path: &str) -> bool {
    !path.contains("://")
        && !path.contains(['<', '>'])
        && (path.contains(['/', '.']) || BARE_FILES.contains(&path))
}

// A link to a diff or to a line, with every placeholder of the form filled in.
fn is_line_link(link: &str) -> bool {
    let link = link.trim_end_matches(['>', '.', ',', ';']);
    !link.contains(['<', '>'])
        && (link.contains("#diff-")
            || link
                .split("#L")
                .skip(1)
                .any(|after| after.starts_with(|c: char| c.is_ascii_digit())))
}

// A link, or a word that is `<file>:<line>` or `<file>:<first>-<last>` once the marks around it are
// gone, so a step can say where it is the way a reader's tool opens it. Lines count from 1.
fn names_a_place(item: &str) -> bool {
    let linked = item.match_indices("https://").any(|(at, _)| {
        let link = item.get(at..).unwrap_or_default();
        let end = link
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '`' | '"'))
            .unwrap_or(link.len());
        is_line_link(link.get(..end).unwrap_or_default())
    });
    linked
        || item.split_whitespace().any(|word| {
            let word = word
                .trim_matches(|c: char| matches!(c, '`' | '(' | ')' | '[' | ']' | ',' | ';' | '.'))
                .trim_end_matches(':');
            word.rsplit_once(':').is_some_and(|(path, lines)| {
                is_file(path) && line_range(lines).is_some_and(|(first, _last)| first >= 1)
            })
        })
}

// A done-when id such as `DW3` at the start, not the start of a longer word.
fn starts_with_done_when(text: &str) -> bool {
    text.strip_prefix("DW").is_some_and(|rest| {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        digits > 0
            && !rest
                .get(digits..)
                .and_then(|tail| tail.chars().next())
                .is_some_and(char::is_alphanumeric)
    })
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
            Self::CodeSpan => (!code_spans(item).iter().any(|span| !span.trim().is_empty()))
                .then(|| {
                    "must name the command or test that ran, in a code span (the only item may be none)"
                        .to_owned()
                }),
            Self::AnswerForDoneWhen(starts) => {
                let right = starts
                    .iter()
                    .any(|start| item.strip_prefix(start).is_some_and(starts_with_done_when));
                (!right).then(|| {
                    format!(
                        "must start with one of: {}, then a done-when id such as DW3 and why",
                        comma_list(starts.iter().map(|start| start.trim_end()))
                    )
                })
            }
            Self::Place => (!names_a_place(item)).then(|| {
                "must hold a diff link (https://...#diff-...) or <file>:<line>".to_owned()
            }),
            Self::Item => None,
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
    if none_is_ok && matches!(items, [only] if only.eq_ignore_ascii_case("none")) {
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

// A line as a reader takes it: the Markdown that is skipped over (quotes, list markers, bold and
// indent) gone, in lower case.
fn bare(line: &str) -> String {
    let mut text = line.trim_start();
    loop {
        let stripped = strip_marker(text);
        if stripped == text {
            break;
        }
        text = stripped;
    }
    text.replace("**", "").to_lowercase()
}

// An answer that says the opposite of the outcome is refused; the rules are in the spec. Every line
// of the part is read, so nesting an answer under another or writing it as a paragraph hides nothing.
fn contradictions(spec: &Spec, outcome: &str, found: &[Part<'_>]) -> Vec<String> {
    let mut problems = Vec::new();
    for rule in spec
        .contradicts
        .iter()
        .filter(|rule| rule.outcome == outcome)
    {
        let Some(part) = once(found, rule.part) else {
            continue;
        };
        let start = rule.start.trim_end();
        let wanted = rule.start.to_lowercase();
        let named = part
            .lines
            .iter()
            .zip(&part.numbers)
            .filter(|(line, _)| matches!(line.kind, Kind::Prose))
            .map(|(line, number)| (line.text, format!(" line {number}")))
            .chain(std::iter::once((part.lead, String::new())));
        for (text, place) in named {
            if bare(text).starts_with(&wanted) {
                problems.push(format!(
                    "part \"{}\"{place} starts with {start}, which does not fit the outcome {outcome}",
                    rule.part
                ));
            }
        }
    }
    problems
}

// A Conventional Comments label, optional decorations and a colon, once the Markdown that a
// reader skips over is gone.
fn finding_shaped(line: &str) -> bool {
    let lower = bare(line);
    FINDING_LABELS.iter().any(|label| {
        lower.strip_prefix(label).is_some_and(|rest| {
            let rest = rest
                .strip_prefix(" (")
                .and_then(|inner| inner.split_once(')'))
                .map_or(rest, |(_, tail)| tail);
            rest.starts_with(':')
        })
    })
}

// One Markdown prefix: `>`, `-`, `+`, `*`, `1.` or `1)`, with the space after it.
fn strip_marker(text: &str) -> &str {
    let text = text.trim_start();
    if let Some(rest) = text.strip_prefix('>') {
        return rest;
    }
    for marker in ["- ", "+ ", "* "] {
        if let Some(rest) = text.strip_prefix(marker) {
            return rest;
        }
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    let after = text.get(digits..).unwrap_or_default();
    if digits > 0
        && let Some(rest) = after
            .strip_prefix(". ")
            .or_else(|| after.strip_prefix(") "))
    {
        return rest;
    }
    text
}

// The numbers of the lines that read as a finding once their indent and list marker are gone.
fn loose_findings(spec: &Spec, all: &[Line<'_>]) -> Vec<usize> {
    if spec.findings.is_none() {
        return Vec::new();
    }
    all.iter()
        .enumerate()
        .filter(|(_, line)| matches!(line.kind, Kind::Prose))
        .filter(|(_, line)| finding_shaped(line.text))
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

// A part that tells the way through the change has a flow for each entry point, and a diagram for
// each flow; the single item `none` says the change has no entry point.
fn flow_problems(name: &str, part: &Part<'_>) -> Vec<String> {
    let listed = items(part);
    if part.is_empty() || matches!(listed.as_slice(), [only] if only.eq_ignore_ascii_case("none")) {
        return Vec::new();
    }
    let mut flows: Vec<(&str, bool)> = Vec::new();
    for line in &part.lines {
        match line.kind {
            Kind::Prose => {
                if let Some((FLOW_LEVEL, title)) = heading(line.text)
                    && let Some(entry) = title.strip_prefix(FLOW)
                {
                    flows.push((entry.trim(), false));
                }
            }
            Kind::Open { info: DIAGRAM } => {
                if let Some(last) = flows.last_mut() {
                    last.1 = true;
                }
            }
            Kind::Open { .. } | Kind::Inside | Kind::Close => {}
        }
    }
    if flows.is_empty() {
        return vec![format!(
            "part \"{name}\" needs a \"### {FLOW} <entry point>\" heading for each entry point, or the single item none"
        )];
    }
    flows
        .iter()
        .filter(|(_, drawn)| !drawn)
        .map(|(entry, _)| format!("part \"{name}\" flow \"{entry}\" has no {DIAGRAM} diagram"))
        .collect()
}

// Parts the spec puts last must be the last, in its order; a part that is missing or doubled is
// already a problem of its own.
fn order_problems(spec: &Spec, found: &[Part<'_>]) -> Vec<String> {
    if spec.last.iter().any(|name| once(found, name).is_none()) {
        return Vec::new();
    }
    let names: Vec<&str> = found.iter().map(|part| part.name).collect();
    let tail = names
        .len()
        .checked_sub(spec.last.len())
        .and_then(|from| names.get(from..));
    if tail == Some(spec.last) {
        Vec::new()
    } else {
        vec![format!(
            "the last parts must be {}, in this order",
            comma_list(spec.last.iter())
        )]
    }
}

// The findings of the part the spec names, judged against the outcome: their counts, or the problems.
fn findings_verdict(
    spec: &Spec,
    outcome: &str,
    found: &[Part<'_>],
) -> Result<Option<[usize; 3]>, Vec<String>> {
    let Some((name, kind)) = spec.findings else {
        return Ok(None);
    };
    let part = once(found, name);
    // An empty part is reported once, by the part check.
    if part.is_some_and(Part::is_empty) {
        return Ok(None);
    }
    let listed = part.map(items).unwrap_or_default();
    if part.is_some() && listed.is_empty() {
        return Err(vec![format!("part \"{name}\" has no list items")]);
    }
    judge(name, kind, outcome, &listed).map(Some)
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
        owner_problems(&all, ctx.max_lines, ctx.example_max_lines)
    } else {
        (None, Vec::new())
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
        if let Some(part) = once(&found, name).filter(|part| !part.is_empty()) {
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
    let counts = match findings_verdict(spec, &head.outcome, &found) {
        Ok(counts) => counts,
        Err(more) => {
            problems.extend(more);
            None
        }
    };
    for name in spec.flows {
        if let Some(part) = once(&found, name) {
            problems.extend(flow_problems(name, part));
        }
    }
    problems.extend(order_problems(spec, &found));
    problems.extend(contradictions(spec, &head.outcome, &found));
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
    use crate::pure::messages::FENCE_UNCLOSED;

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

    // One flow with a step and its diagram. A heading would end a bold part, so a kind with flows
    // is rendered with heading parts only.
    fn flow_lines() -> Vec<String> {
        lines_of(&[
            "### Flow: shipit run",
            "",
            "- `run` takes args: src/a.rs:4",
            "",
            "```mermaid",
            "sequenceDiagram",
            "  A->>B: call",
            "```",
        ])
    }

    // The lines of a part: list items made so that the check of the part passes, or the block
    // the formats read.
    fn sample(spec: &Spec, outcome: &str, part: &str, seed: u64) -> Vec<String> {
        match (spec.embedded, part) {
            (Embedded::Roadmap, "Slices") => return lines_of(&ROADMAP_BLOCK),
            (Embedded::CoverageMap, "Done-when coverage") => return lines_of(&COVERAGE_BLOCK),
            _ => {}
        }
        if spec.flows.contains(&part) {
            return flow_lines();
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
                Some(Check::Answer(starts) | Check::AnswerForDoneWhen(starts)) => {
                    let id = if matches!(check, Some((_, Check::AnswerForDoneWhen(_)))) {
                        format!("DW{}: ", seed % 90 + 1)
                    } else {
                        String::new()
                    };
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
                    format!("{}{id}because of {seed}", chosen(&fits, seed))
                }
                Some(Check::Contains(phrases)) => format!("a {}word", chosen(phrases, seed)),
                Some(Check::FileLines | Check::Place) if seed.is_multiple_of(2) => {
                    "src/a.rs:3-5: words".to_owned()
                }
                Some(Check::FileLines | Check::Place) => "src/a.rs:4: words".to_owned(),
                Some(Check::CodeSpan) => format!("ran `cargo test` {seed}"),
                Some(Check::Item) | None => format!("words {seed}"),
                Some(Check::NamesElement(phrases)) => {
                    format!("a thing, {}alpha and more", chosen(phrases, seed))
                }
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
        let bold =
            seed & 1 == 0 && matches!(spec.embedded, Embedded::Nothing) && spec.flows.is_empty();
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
            example_max_lines: 15,
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

    // The parts that must say something: a check, and no leave to be `none`.
    fn parts_without_none() -> Vec<(&'static Spec, &'static str)> {
        KINDS
            .iter()
            .flat_map(|spec| {
                spec.checks
                    .iter()
                    .map(|(name, _check)| *name)
                    .filter(|name| !spec.none_allowed.contains(name))
                    .map(move |name| (spec, name))
            })
            .collect()
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
            // The parts the output ends with stay last.
            let at = parts
                .iter()
                .position(|(part, _)| spec.last.first() == Some(part))
                .unwrap_or(parts.len());
            parts.insert(at, (name, items));
        }
        render(spec, outcome, 0, &parts)
    }

    // The problem that says a line of the part starts with `start` against the outcome.
    fn does_not_fit(part: &str, start: &str, outcome: &str) -> impl Fn(&String) -> bool {
        let (head, tail) = (
            format!("part \"{part}\""),
            format!(" starts with {start}, which does not fit the outcome {outcome}"),
        );
        move |problem| problem.starts_with(&head) && problem.ends_with(&tail)
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
            (spec, name) in proptest::sample::select(parts_without_none()),
            seed in any::<u64>(),
        ) {
            let name = &name;
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
        flows: &[],
        last: &[],
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
            kind in prop::sample::select(vec!["second-opinion", "informed-review", "review", "plan-review", "test-drive"]),
            place in 0_usize..9,
            seed in any::<u64>(),
        ) {
            let spec = spec_of(kind);
            let outcome = if kind == "test-drive" { "pass" } else { "approve" };
            let mut text = filled(spec, outcome, seed | 1);
            text.push_str([
                "  - issue (blocking, security): a\n",
                "issue (blocking): a\n",
                "# Notes\n- issue (blocking): a\n",
                "+ issue (blocking): a\n",
                "1) issue (blocking): a\n",
                "> issue (blocking): a\n",
                "**issue (blocking):** a\n",
                "Issue (Blocking): a\n",
                "  > - **Question:** a\n",
            ][place]);
            let problems = problems_of(spec, &text);
            prop_assert!(
                problems.iter().any(|problem| problem.contains("a finding that no item of the part \"Findings\" counts")),
                "{:?}", problems
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // Every line counts, wherever it sits: between the heading and the block, inside the
        // `input` value, or after the label.
        #[test]
        fn every_line_counts_against_the_cap(extra in 1_usize..40, place in 0_usize..3) {
            let spec = spec_of("decision-point");
            let good = filled(spec, "waiting-for-the-owner", 0);
            let max = u64::try_from(good.lines().count()).expect("small");
            prop_assert!(validate(spec, &ctx(max), &good).is_ok());
            let lines = "more words\n".repeat(extra);
            let text = match place {
                0 => good.replacen("## Form\n\n", &format!("## Form\n\n{lines}"), 1),
                1 => good.replacen("input = \"x\"", &format!("input = \"\"\"\n{lines}\"\"\""), 1),
                _ => format!("{good}{lines}"),
            };
            let total = text.lines().count();
            prop_assert_eq!(
                validate(spec, &ctx(max), &text),
                Err(vec![format!("too long: {total} lines (owner.max_lines is {max})")])
            );
        }
    }

    fn fenced(mark: &str, info: &str, count: usize) -> String {
        format!(
            "{mark}{info}\n{}{mark}\n",
            "an example line\n".repeat(count)
        )
    }

    // Puts a block above the label, or after the last part.
    fn with_block(good: &str, block: &str, place: bool) -> String {
        if place {
            good.replacen("## Form\n\n", &format!("## Form\n\n{block}"), 1)
        } else {
            format!("{good}{block}")
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // Each cap judges only its own lines: the fence lines count toward neither, the example
        // lines of every block add up, and the two messages come in the same order.
        #[test]
        fn each_cap_judges_only_its_own_lines(
            blocks in proptest::collection::vec(0_usize..12, 1..4),
            plain in 0_usize..4,
            example_cap in 0_u64..30,
            mark in proptest::sample::select(vec!["```", "~~~", "`````"]),
            place in any::<bool>(),
        ) {
            let spec = spec_of("decision-point");
            let good = filled(spec, "waiting-for-the-owner", 0);
            let max = u64::try_from(good.lines().count()).expect("small");
            let mut text = with_block(&good, &"more words\n".repeat(plain), place);
            for count in &blocks {
                text = with_block(&text, &fenced(mark, "example", *count), place);
            }
            let example: usize = blocks.iter().map(|count| count + 2).sum();
            let mut expected = Vec::new();
            if plain > 0 {
                expected.push(format!(
                    "too long: {} lines (owner.max_lines is {max})",
                    good.lines().count() + plain
                ));
            }
            if u64::try_from(example).expect("small") > example_cap {
                expected.push(format!(
                    "too many example lines: {example} (owner.example_max_lines is {example_cap})"
                ));
            }
            let ctx = Context { example_max_lines: example_cap, ..ctx(max) };
            let verdict = validate(spec, &ctx, &text);
            prop_assert_eq!(verdict.err().unwrap_or_default(), expected);
        }

        // Only the info string `example`, exactly, makes an example block.
        #[test]
        fn a_fence_with_any_other_info_string_counts_as_plain_lines(
            info in proptest::sample::select(vec!["", "text", "json", "examples", "Example", "example x"]),
            count in 0_usize..12,
            place in any::<bool>(),
        ) {
            let spec = spec_of("decision-point");
            let good = filled(spec, "waiting-for-the-owner", 0);
            let max = u64::try_from(good.lines().count()).expect("small");
            let text = with_block(&good, &fenced("```", info, count), place);
            let total = text.lines().count();
            prop_assert_eq!(
                validate(spec, &ctx(max), &text),
                Err(vec![format!("too long: {total} lines (owner.max_lines is {max})")])
            );
        }
    }

    #[test]
    fn an_example_fence_that_never_closes_is_the_unclosed_fence_error() {
        let spec = spec_of("decision-point");
        let good = filled(spec, "waiting-for-the-owner", 0);
        let line = good.lines().count() + 1;
        let text = format!("{good}```example\nan example line\n");
        assert_eq!(
            validate(spec, &ctx(1000), &text),
            Err(vec![format!(
                "out.md: {}",
                say(FENCE_UNCLOSED, &[("line", &line)])
            )])
        );
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
        let problems = problems_of(spec, &text);
        assert!(
            problems.len() == 1 && does_not_fit("Done-when", "[fail]", "pass")(&problems[0]),
            "{problems:?}"
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
        let problems = problems_of(spec, &text);
        assert!(
            problems.len() == 1 && does_not_fit("Security", "concern:", "approve")(&problems[0]),
            "{problems:?}"
        );
        let with_finding = {
            let mut parts = bodies(spec, "approve", 0);
            for (name, body) in &mut parts {
                if *name == "Security" {
                    *body = vec!["- concern: injection everywhere".to_owned()];
                }
            }
            let at = parts.len() - 3;
            parts.insert(
                at,
                (
                    "Findings",
                    vec![
                        "- question: is it fixed?".to_owned(),
                        "- nitpick: a comma".to_owned(),
                    ],
                ),
            );
            render(spec, "approve", 0, &parts)
        };
        // A concern makes an approval wrong whatever findings come with it.
        let problems = problems_of(spec, &with_finding);
        assert!(
            problems.len() == 1 && does_not_fit("Security", "concern:", "approve")(&problems[0]),
            "{problems:?}"
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
        let at = parts.len() - 3;
        parts.insert(
            at,
            (
                "Findings",
                vec!["- nitpick: a word".to_owned(), "- question: why".to_owned()],
            ),
        );
        let text = render(spec, "approve", 0, &parts);
        let done = validate(spec, &ctx(1000), &text).expect("valid");
        assert_eq!(done.counts, Some([0, 1, 1]));

        let mut worse = bodies(spec, "approve", 0);
        let at = worse.len() - 3;
        worse.insert(at, ("Findings", vec!["- issue (blocking): bad".to_owned()]));
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
        let count = text.lines().count();
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

    const CLAIM_KINDS: [&str; 7] = [
        "review",
        "informed-review",
        "second-opinion",
        "plan-review",
        "scout-report",
        "test-drive",
        "explanation",
    ];
    const CLAIMS: [&str; 3] = ["Verified", "Inferred", "Unverifiable"];

    // An outcome of a kind, by index.
    fn outcome_of(kind: &str, outcome: usize) -> (&'static Spec, &'static str) {
        let spec = spec_of(kind);
        (spec, spec.outcomes[outcome % spec.outcomes.len()].name)
    }

    fn reading_path(spec: &Spec, outcome: &str, lines: &[String]) -> String {
        let mut parts = bodies(spec, outcome, 0);
        if let Some((_, body)) = parts.iter_mut().find(|(name, _)| *name == "Reading path") {
            *body = lines.to_vec();
        }
        render(spec, outcome, 0, &parts)
    }

    // Every (spec, rule) that refuses an outcome because of what a part says.
    fn contradiction_cases() -> Vec<(&'static Spec, &'static Contradiction)> {
        KINDS
            .iter()
            .flat_map(|spec| spec.contradicts.iter().map(move |rule| (spec, rule)))
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // Every outcome of a review or a report asks for all three lists; none is optional.
        #[test]
        fn every_outcome_needs_the_three_claim_lists(
            kind in prop::sample::select(CLAIM_KINDS.to_vec()),
            outcome in 0_usize..3,
            claim in prop::sample::select(CLAIMS.to_vec()),
            seed in any::<u64>(),
        ) {
            let (spec, outcome) = outcome_of(kind, outcome);
            let mut parts = bodies(spec, outcome, seed);
            parts.retain(|(part, _)| *part != claim);
            let text = render(spec, outcome, seed, &parts);
            prop_assert_eq!(problems_of(spec, &text), vec![format!("part \"{claim}\" is missing")]);
        }

        #[test]
        fn a_claim_list_may_be_the_single_item_none_in_any_case(
            kind in prop::sample::select(CLAIM_KINDS.to_vec()),
            outcome in 0_usize..3,
            claim in prop::sample::select(CLAIMS.to_vec()),
            none in prop::sample::select(vec!["none", "None", "NONE"]),
        ) {
            let (spec, outcome) = outcome_of(kind, outcome);
            let text = with_body(spec, outcome, claim, &[none]);
            prop_assert!(validate(spec, &ctx(1000), &text).is_ok());
        }

        // The three lists are the last parts, in order: any other part after one is refused.
        #[test]
        fn the_claim_lists_come_last_in_order(
            kind in prop::sample::select(CLAIM_KINDS.to_vec()),
            outcome in 0_usize..3,
            by in 1_usize..32,
            seed in any::<u64>(),
        ) {
            let (spec, outcome) = outcome_of(kind, outcome);
            let mut parts = bodies(spec, outcome, seed);
            let by = by % (parts.len() - 1) + 1;
            parts.rotate_left(by);
            let text = render(spec, outcome, seed, &parts);
            prop_assert_eq!(
                problems_of(spec, &text),
                vec!["the last parts must be Verified, Inferred, Unverifiable, in this order".to_owned()]
            );
        }

        // Verified needs a code span with something in it; the other two take any item.
        #[test]
        fn verified_items_name_what_ran_in_a_code_span(
            kind in prop::sample::select(CLAIM_KINDS.to_vec()),
            words in "[a-z][a-z ]{0,10}",
            span in "[a-z][a-z ]{0,10}",
            blank in prop::sample::select(vec!["``", "` `", "`  `"]),
        ) {
            let (spec, outcome) = outcome_of(kind, 0);
            let message = "part \"Verified\" item 1 must name the command or test that ran, in a code span (the only item may be none)";
            for bare in [words.clone(), format!("{words} {blank} {words}")] {
                let text = with_body(spec, outcome, "Verified", &[&bare]);
                prop_assert_eq!(problems_of(spec, &text), vec![message.to_owned()]);
            }
            let named = with_body(spec, outcome, "Verified", &[&format!("`{span}`: {words}")]);
            prop_assert!(validate(spec, &ctx(1000), &named).is_ok());
            for claim in ["Inferred", "Unverifiable"] {
                let any = with_body(spec, outcome, claim, &[&words]);
                prop_assert!(validate(spec, &ctx(1000), &any).is_ok());
            }
        }

        // A place is a link to a diff or a line with nothing left to fill in, or a file with a
        // folder or an extension and a line from 1.
        #[test]
        fn a_step_says_where_it_is(
            kind in prop::sample::select(vec!["review", "informed-review"]),
            part in prop::sample::select(vec!["Look here", "Reading path"]),
            path in "[a-z]{1,5}(/[a-z]{1,5}){0,2}\\.[a-z]{1,3}",
            line in 1_u64..500,
            span in 0_u64..20,
            style in 0_usize..7,
        ) {
            let (spec, outcome) = outcome_of(kind, 0);
            let last = line + span;
            let place = [
                format!("`f` at {path}:{line}"),
                format!("`f` ({path}:{line}-{last})"),
                format!("`f` [here](https://github.com/o/r/pull/1/files#diff-ab{line}R{last})"),
                format!("`f` https://github.com/o/r/blob/main/{path}#L{line}"),
                "`f` at Makefile:3".to_owned(),
                format!("`f` at {path}:{last}-{line}x"),
                format!("`f` at {path}"),
            ][style].clone();
            let accepted = style < 5;
            let text = if part == "Reading path" {
                let flow = ["### Flow: x".to_owned(), format!("- {place}"), "```mermaid".to_owned(), "```".to_owned()];
                reading_path(spec, outcome, &flow)
            } else {
                with_body(spec, outcome, part, &[&place])
            };
            let got = validate(spec, &ctx(1000), &text);
            prop_assert_eq!(got.is_ok(), accepted, "{} {:?}", place, got);
            if !accepted {
                prop_assert_eq!(
                    got,
                    Err(vec![format!(
                        "part \"{part}\" item 1 must hold a diff link (https://...#diff-...) or <file>:<line>"
                    )])
                );
            }
        }

        // What is not a place: a time, an address, line 0, a bare word, a link that is not to a
        // line, and the form's own placeholder.
        #[test]
        fn what_only_looks_like_a_place_is_refused(
            kind in prop::sample::select(vec!["review", "informed-review"]),
            hour in 0_u32..24,
            minute in 0_u32..60,
            port in 1_u32..9999,
            word in "[a-z]{1,6}",
            style in 0_usize..8,
        ) {
            let (spec, outcome) = outcome_of(kind, 0);
            let place = [
                format!("it fires at {hour}:{minute:02}"),
                format!("none at {hour}:{minute}"),
                format!("calls http://localhost:{port}"),
                format!("{word}.rs:0"),
                format!("https://example.com/#L{word}"),
                format!("{word}:{port}"),
                "https://github.com/<owner>/<repo>/pull/<n>/files#diff-<sha256 of the path>R<line>".to_owned(),
                format!("https://example.com/{word}"),
            ][style].clone();
            let text = with_body(spec, outcome, "Look here", &[&place]);
            prop_assert!(validate(spec, &ctx(1000), &text).is_err(), "{}", place);
        }

        // Unless it is `none`, the reading path has a flow for each entry point and a diagram for
        // each flow.
        #[test]
        fn each_flow_has_its_diagram(
            kind in prop::sample::select(vec!["review", "informed-review"]),
            drawn in proptest::collection::vec(any::<bool>(), 1..5),
        ) {
            let (spec, outcome) = outcome_of(kind, 0);
            let mut lines = Vec::new();
            for (index, drawn) in drawn.iter().enumerate() {
                lines.push(format!("### Flow: entry{index}"));
                lines.push(format!("- `f` at src/a.rs:{}", index + 1));
                if *drawn {
                    lines.extend(["```mermaid".to_owned(), "sequenceDiagram".to_owned(), "```".to_owned()]);
                }
            }
            let got = validate(spec, &ctx(1000), &reading_path(spec, outcome, &lines));
            let missing: Vec<String> = drawn
                .iter()
                .enumerate()
                .filter(|(_, drawn)| !**drawn)
                .map(|(index, _)| format!("part \"Reading path\" flow \"entry{index}\" has no mermaid diagram"))
                .collect();
            prop_assert_eq!(got.map(|_| ()), if missing.is_empty() { Ok(()) } else { Err(missing) });
        }

        #[test]
        fn steps_without_a_flow_heading_are_refused_and_none_is_not(
            kind in prop::sample::select(vec!["review", "informed-review"]),
            none in prop::sample::select(vec!["none", "None"]),
        ) {
            let (spec, outcome) = outcome_of(kind, 0);
            let steps = reading_path(spec, outcome, &["- `f` at src/a.rs:1".to_owned()]);
            prop_assert_eq!(
                problems_of(spec, &steps),
                vec!["part \"Reading path\" needs a \"### Flow: <entry point>\" heading for each entry point, or the single item none".to_owned()]
            );
            let nothing = reading_path(spec, outcome, &[format!("- {none}")]);
            prop_assert!(validate(spec, &ctx(1000), &nothing).is_ok());
        }

        // Look here cannot be none; the reading path and other files can.
        #[test]
        fn none_in_look_here_is_refused_and_in_the_other_two_accepted(
            kind in prop::sample::select(vec!["review", "informed-review"]),
        ) {
            let (spec, outcome) = outcome_of(kind, 0);
            for part in ["Reading path", "Other files"] {
                let text = with_body(spec, outcome, part, &["none"]);
                prop_assert!(validate(spec, &ctx(1000), &text).is_ok());
            }
            let text = with_body(spec, outcome, "Look here", &["none"]);
            prop_assert!(validate(spec, &ctx(1000), &text).is_err());
        }

        #[test]
        fn other_files_name_a_risk(
            kind in prop::sample::select(vec!["review", "informed-review"]),
            risk in prop::sample::select(vec!["high", "medium", "low", "none", ""]),
        ) {
            let (spec, outcome) = outcome_of(kind, 0);
            let text = with_body(spec, outcome, "Other files", &[&format!("tests: risk: {risk} words")]);
            prop_assert_eq!(
                validate(spec, &ctx(1000), &text).is_ok(),
                ["high", "medium", "low"].contains(&risk)
            );
        }

        // The reader's teach-back is a signal, not a gate: no answer in it refuses an approval.
        #[test]
        fn a_wrong_teach_back_does_not_refuse_an_approval(
            kind in prop::sample::select(vec!["review", "informed-review"]),
            start in prop::sample::select(vec!["[matches] ", "[wrong] ", "[unclear] "]),
            why in "[a-z][a-z ]{0,8}",
        ) {
            let (spec, outcome) = outcome_of(kind, 0);
            let text = with_body(spec, outcome, "Teach-back", &[&format!("{start}{why}")]);
            prop_assert!(validate(spec, &ctx(1000), &text).is_ok());
        }

        // Each item of a review of a roadmap or a build plan names the done-when item it judges.
        #[test]
        fn a_done_when_item_names_its_id(
            mark in prop::sample::select(vec!["[covered] ", "[gap] "]),
            id in 1_u32..200,
            tail in prop::sample::select(vec![": slice alpha", " is met", "x: slice alpha"]),
            bare in "[a-z][a-z ]{0,10}",
        ) {
            let spec = spec_of("plan-review");
            let outcome = "approve-after-fixes";
            let named = with_body(spec, outcome, "Done-when", &[&format!("{mark}DW{id}{tail}")]);
            prop_assert_eq!(validate(spec, &ctx(1000), &named).is_ok(), !tail.starts_with('x'));
            let unnamed = with_body(spec, outcome, "Done-when", &[&format!("{mark}{bare}")]);
            prop_assert_eq!(
                problems_of(spec, &unnamed),
                vec!["part \"Done-when\" item 1 must start with one of: [covered], [gap], then a done-when id such as DW3 and why".to_owned()]
            );
        }

        // An answer that says the opposite of the outcome is refused wherever it sits in the part:
        // nested, as a paragraph, quoted, bold or in another case. The other outcome takes it.
        #[test]
        fn a_contradiction_is_found_on_any_line_of_the_part(
            case in prop::sample::select(contradiction_cases()),
            shape in 0_usize..6,
            seed in any::<u64>(),
        ) {
            let (spec, rule) = case;
            let start = rule.start.trim_end();
            let line = [
                format!("  - {start} hidden"),
                format!("{} hidden", rule.start),
                format!("> - {start} hidden"),
                format!("**{start}** hidden"),
                format!("1) {start} hidden"),
                format!("    - {} hidden", rule.start.to_uppercase()),
            ][shape].clone();
            let mut parts = bodies(spec, rule.outcome, seed);
            parts
                .iter_mut()
                .find(|(name, _)| *name == rule.part)
                .expect("the part of the rule")
                .1
                .push(line.clone());
            let text = render(spec, rule.outcome, seed, &parts);
            let problems = problems_of(spec, &text);
            prop_assert!(
                problems.iter().any(does_not_fit(rule.part, start, rule.outcome)),
                "{:?} for {}", problems, line
            );
        }
    }

    // An empty part is one problem, not two.
    #[test]
    fn an_empty_part_is_reported_once() {
        let spec = spec_of("second-opinion");
        for (outcome, part) in [
            ("approve", "Security"),
            ("rework", "Findings"),
            ("approve", "Verified"),
        ] {
            let mut parts = bodies(spec, outcome, 3);
            if let Some((_, body)) = parts.iter_mut().find(|(name, _)| *name == part) {
                body.clear();
            }
            let text = render(spec, outcome, 3, &parts).replace(
                &format!("## {part}\n\nsome words\n"),
                &format!("## {part}\n"),
            );
            assert_eq!(
                problems_of(spec, &text),
                [format!("part \"{part}\" is empty")]
            );
        }
    }
}
