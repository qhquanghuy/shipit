use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::commands::setup::load_setup_table;
use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::edge::goal_file::newest_goal;
use crate::edge::records::{goal_dir, read_logbook, read_store_text, store_dir};
use crate::edge::store::versions;
use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::kinds::{kind, kind_names};
use crate::pure::messages::GOAL_NAME;
use crate::pure::roadmap_delta::{entries, refusals};
use crate::pure::setup::{HEAD, Setup};
use crate::pure::store_path::OwnerFile;
use crate::pure::template::{Context, Verdict, validate};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str =
    "usage: shipit validate <kind> <file> [--goal <name> | --goal-file <path>]";
pub(crate) const SUMMARY: &str = "Checks a filled output against its template and lists every problem; it checks form, not truth. A roadmap is also held against its goal, given by --goal (the goal's newest version in the store) or --goal-file (a goal outside the store). With --goal, a roadmap is also compared with the goal's newest stored roadmap: a shipped slice's entry cannot change, and a started slice's may only gain split_into. Kinds are listed by shipit template.";
pub(crate) const EXITS: [&str; 4] = [
    "valid",
    "the output is not valid",
    "usage, setup or file error",
    "",
];

const ROADMAP: &str = "roadmap";
const NEEDS_GOAL: &str = "the roadmap needs its goal: shipit validate roadmap <file> --goal <name> or --goal-file <path>";
const GOAL_ONLY_FOR_ROADMAP: &str =
    "--goal and --goal-file are only for the roadmap, and only one of them";

pub(crate) enum Goal<'a> {
    Nothing,
    Both,
    Stored(&'a OsStr),
    File(&'a OsStr),
}

fn is_goal_flag(flag: &OsString) -> bool {
    flag == OsStr::new("--goal") || flag == OsStr::new("--goal-file")
}

fn goal_flag(flags: &[OsString]) -> Result<Option<Goal<'_>>, Failure> {
    match flags {
        [] => Ok(None),
        [flag, value] if flag == OsStr::new("--goal") => Ok(Some(Goal::Stored(value))),
        [flag, value] if flag == OsStr::new("--goal-file") => Ok(Some(Goal::File(value))),
        [first, _, second, _] if is_goal_flag(first) && is_goal_flag(second) && first != second => {
            Ok(Some(Goal::Both))
        }
        [_] | [_, _] | [_, _, _, ..] => Err(Failure::usage(USAGE)),
    }
}

// The text of the goal a roadmap is held against, and the goal's name when it is in the store (a
// goal file outside the store has no stored roadmap to compare with); any other kind takes no goal.
fn goal_text(
    git: &Git,
    name: &str,
    goal: &Goal<'_>,
) -> Result<(Option<String>, Option<GoalName>), Failure> {
    let (path, stored) = match goal {
        Goal::Nothing if name == ROADMAP => return Err(Failure::usage(NEEDS_GOAL)),
        Goal::Nothing => return Ok((None, None)),
        Goal::Both => return Err(Failure::usage(GOAL_ONLY_FOR_ROADMAP)),
        Goal::Stored(_) | Goal::File(_) if name != ROADMAP => {
            return Err(Failure::usage(GOAL_ONLY_FOR_ROADMAP));
        }
        Goal::File(path) => (Path::new(path).to_path_buf(), None),
        Goal::Stored(given) => {
            let given = given.to_string_lossy();
            let goal = GoalName::parse(&given)
                .ok_or_else(|| Failure::usage(format!("--goal \"{given}\": {GOAL_NAME}")))?;
            (newest_goal(&git.common_dir()?, &goal)?, Some(goal))
        }
    };
    read_text(&path).map(|text| (Some(text), stored))
}

fn not_valid(label: &str, name: &str, problems: Vec<String>) -> Failure {
    let mut said = format!("{label} is not a valid {name}:");
    for problem in problems {
        said.push_str("\n  ");
        said.push_str(&problem);
    }
    Failure::no(said)
}

// What a valid draft changes in the newest stored roadmap that the logbook says it may not. With
// nothing stored there is nothing to compare, but a `split_into` is still held to the logbook.
fn frozen_problems(
    git: &Git,
    goal: &GoalName,
    label: &str,
    text: &str,
) -> Result<Vec<String>, Failure> {
    let store = store_dir(&git.common_dir()?);
    let stored = match versions(&store, goal, OwnerFile::Roadmap).last() {
        None => Vec::new(),
        Some(newest) => {
            let file = OwnerFile::Roadmap.name(*newest);
            let shown = goal_dir(&store, goal).join(&file);
            let kept = read_store_text(&store, &["goals".to_owned(), goal.to_string(), file])?;
            entries(&shown.display().to_string(), &kept).map_err(Failure::usage)?
        }
    };
    let draft = match entries(label, text) {
        Ok(draft) => draft,
        Err(why) => return Ok(vec![why]),
    };
    let logbook = read_logbook(&store, goal)?;
    Ok(refusals(&stored, &draft, &logbook)
        .iter()
        .map(ToString::to_string)
        .collect())
}

// The text of a draft held against its kind; the problems are the refusal.
pub(crate) fn valid_text(
    git: &Git,
    setup: &Setup,
    name: &str,
    file: &OsStr,
    goal: &Goal<'_>,
) -> Result<(String, Verdict), Failure> {
    let spec = kind(name).ok_or_else(|| {
        Failure::usage(format!("unknown kind \"{name}\"; kinds: {}", kind_names()))
    })?;
    let (goal, stored_goal) = goal_text(git, name, goal)?;
    let label = file.to_string_lossy();
    let text = read_text(Path::new(file))?;
    let ctx = Context {
        file: &label,
        max_lines: u64::from(&setup.owner.max_lines),
        example_max_lines: u64::from(&setup.owner.example_max_lines),
        goal: goal.as_deref(),
    };
    match validate(spec, &ctx, &text) {
        Ok(verdict) => {
            if let Some(stored) = &stored_goal {
                let problems = frozen_problems(git, stored, &label, &text)?;
                if !problems.is_empty() {
                    return Err(not_valid(&label, name, problems));
                }
            }
            Ok((text, verdict))
        }
        Err(problems) => Err(not_valid(&label, name, problems)),
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    // The setup comes first: a command that needs one stops without it whatever the arguments.
    let git = Git::find()?;
    let (setup, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let [name, file, flags @ ..] = args else {
        return Err(Failure::usage(USAGE));
    };
    let goal = goal_flag(flags)?.unwrap_or(Goal::Nothing);
    let name = name.to_string_lossy();
    let (_text, verdict) = valid_text(&git, &setup, &name, file, &goal)?;
    write!(out, "valid {name}: {}", verdict.outcome).ok();
    if let Some([must_fix, nice_to_fix, must_answer]) = verdict.counts {
        write!(
            out,
            " ({must_fix} must fix, {nice_to_fix} nice to fix, {must_answer} must answer)"
        )
        .ok();
    }
    writeln!(out).ok();
    Ok(())
}
