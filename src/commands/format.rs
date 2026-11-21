use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::edge::files::read_text;
use crate::pure::block::toml_block;
use crate::pure::coverage_map::parse_map;
use crate::pure::failure::Failure;
use crate::pure::finding::{Finding, Outcome, ResultKind, outcome_problems, parse_finding};
use crate::pure::goal::{QualifiedDoneWhen, goal_items};
use crate::pure::messages::{
    FILE_PROBLEM, FINDINGS_USAGE, MAP_USAGE, NO_BLOCK, NOT_A_RESULT, NOT_TOML, ROADMAP_USAGE,
    USAGE, say,
};
use crate::pure::roadmap::{Roadmap, SliceName, meets_problems, parse_roadmap, waves};
use crate::pure::rules::invalid;
use crate::pure::setup::toml_table;

pub(crate) const HIDDEN: bool = true;

const SLICES: &str = "## Slices";
const COVERAGE: &str = "## Done-when coverage";

#[derive(Serialize)]
struct Printed<'a> {
    roadmap: &'a Roadmap,
    waves: Vec<Vec<&'a SliceName>>,
    goal: Option<Vec<QualifiedDoneWhen>>,
}

#[derive(Serialize)]
struct PrintedFindings<'a> {
    result: Outcome,
    findings: &'a [Finding],
}

fn print(out: &mut dyn Write, value: &impl Serialize) -> Result<(), Failure> {
    let text = serde_json::to_string(value)
        .map_err(|why| Failure::usage(format!("cannot print the result: {why}")))?;
    writeln!(out, "{text}").ok();
    Ok(())
}

fn block_table(label: &str, text: &str, heading: &str) -> Result<toml::Table, Failure> {
    let block = match toml_block(text, heading) {
        Ok(Some(block)) => block,
        Ok(None) => {
            return Err(Failure::usage(say(
                NO_BLOCK,
                &[("file", &label), ("heading", &heading)],
            )));
        }
        Err(why) => {
            return Err(Failure::usage(say(
                FILE_PROBLEM,
                &[("file", &label), ("why", &why)],
            )));
        }
    };
    file_table(label, block)
}

fn file_table(label: &str, text: &str) -> Result<toml::Table, Failure> {
    toml_table(text.as_bytes())
        .map_err(|why| Failure::usage(say(NOT_TOML, &[("file", &label), ("why", &why)])))
}

fn roadmap_command(args: &[OsString], out: &mut dyn Write) -> Result<(), Failure> {
    let (file, goal_file) = match args {
        [file] => (file, None),
        [file, flag, goal] if flag == OsStr::new("--goal") => (file, Some(goal)),
        _ => return Err(Failure::usage(ROADMAP_USAGE)),
    };
    let label = file.to_string_lossy();
    let text = read_text(Path::new(file))?;
    let table = block_table(&label, &text, SLICES)?;
    let roadmap =
        parse_roadmap(&table).map_err(|problems| Failure::usage(invalid(&label, &problems)))?;
    let goal = match goal_file {
        None => None,
        Some(goal_file) => {
            let items = goal_items(&read_text(Path::new(goal_file))?).map_err(Failure::usage)?;
            let problems = meets_problems(&roadmap, &items);
            if !problems.is_empty() {
                return Err(Failure::usage(invalid(&label, &problems)));
            }
            Some(
                items
                    .into_iter()
                    .map(|id| QualifiedDoneWhen::of(&roadmap.goal, id))
                    .collect(),
            )
        }
    };
    print(
        out,
        &Printed {
            roadmap: &roadmap,
            waves: waves(&roadmap),
            goal,
        },
    )
}

fn map_command(args: &[OsString], out: &mut dyn Write) -> Result<(), Failure> {
    let [file] = args else {
        return Err(Failure::usage(MAP_USAGE));
    };
    let label = file.to_string_lossy();
    let text = read_text(Path::new(file))?;
    let is_markdown = Path::new(file)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
    let table = if is_markdown {
        block_table(&label, &text, COVERAGE)?
    } else {
        file_table(&label, &text)?
    };
    let map = parse_map(&table).map_err(|problems| Failure::usage(invalid(&label, &problems)))?;
    print(out, &map)
}

fn findings_command(args: &[OsString], out: &mut dyn Write) -> Result<(), Failure> {
    let [kind, result, texts @ ..] = args else {
        return Err(Failure::usage(FINDINGS_USAGE));
    };
    let kind = match kind.to_str() {
        Some("review") => ResultKind::Review,
        Some("test-drive") => ResultKind::TestDrive,
        Some(_) | None => return Err(Failure::usage(FINDINGS_USAGE)),
    };
    let result = result.to_string_lossy();
    let outcome = Outcome::parse(&result)
        .ok_or_else(|| Failure::usage(say(NOT_A_RESULT, &[("word", &result)])))?;
    let mut findings = Vec::new();
    let mut refused = Vec::new();
    for (index, text) in texts.iter().enumerate() {
        match parse_finding(index, &text.to_string_lossy()) {
            Ok(finding) => findings.push(finding),
            Err(why) => refused.push(why),
        }
    }
    if !refused.is_empty() {
        return Err(Failure::usage(refused.join("\n")));
    }
    let problems = outcome_problems(kind, outcome, &findings);
    if !problems.is_empty() {
        return Err(Failure::usage(problems.join("\n")));
    }
    print(
        out,
        &PrintedFindings {
            result: outcome,
            findings: &findings,
        },
    )
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    match args.split_first() {
        Some((form, rest)) if form == OsStr::new("roadmap") => roadmap_command(rest, out),
        Some((form, rest)) if form == OsStr::new("map") => map_command(rest, out),
        Some((form, rest)) if form == OsStr::new("findings") => findings_command(rest, out),
        Some(_) | None => Err(Failure::usage(USAGE)),
    }
}
