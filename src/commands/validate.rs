use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::commands::setup::load_setup_table;
use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::edge::goal_file::newest_goal;
use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::kinds::{kind, kind_names};
use crate::pure::messages::GOAL_NAME;
use crate::pure::setup::HEAD;
use crate::pure::template::{Context, validate};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str =
    "usage: shipit validate <kind> <file> [--goal <name> | --goal-file <path>]";
pub(crate) const SUMMARY: &str = "Checks a filled output against its template and lists every problem; it checks form, not truth. A roadmap is also held against its goal, given by --goal (the goal's newest version in the store) or --goal-file (a goal outside the store). Kinds are listed by shipit template.";
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

enum Goal<'a> {
    Nothing,
    Stored(&'a OsStr),
    File(&'a OsStr),
}

fn goal_flag(flags: &[OsString]) -> Result<Option<Goal<'_>>, Failure> {
    match flags {
        [] => Ok(None),
        [flag, value] if flag == OsStr::new("--goal") => Ok(Some(Goal::Stored(value))),
        [flag, value] if flag == OsStr::new("--goal-file") => Ok(Some(Goal::File(value))),
        [_] | [_, _] | [_, _, _, ..] => Err(Failure::usage(USAGE)),
    }
}

// The text of the goal a roadmap is held against; any other kind takes no goal.
fn goal_text(git: &Git, name: &str, goal: &Goal<'_>) -> Result<Option<String>, Failure> {
    let path = match goal {
        Goal::Nothing if name == ROADMAP => return Err(Failure::usage(NEEDS_GOAL)),
        Goal::Nothing => return Ok(None),
        Goal::Stored(_) | Goal::File(_) if name != ROADMAP => {
            return Err(Failure::usage(GOAL_ONLY_FOR_ROADMAP));
        }
        Goal::File(path) => Path::new(path).to_path_buf(),
        Goal::Stored(given) => {
            let given = given.to_string_lossy();
            let goal = GoalName::parse(&given)
                .ok_or_else(|| Failure::usage(format!("--goal \"{given}\": {GOAL_NAME}")))?;
            newest_goal(&git.common_dir()?, &goal)?
        }
    };
    read_text(&path).map(Some)
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
    let spec = kind(&name).ok_or_else(|| {
        Failure::usage(format!("unknown kind \"{name}\"; kinds: {}", kind_names()))
    })?;
    let goal = goal_text(&git, &name, &goal)?;
    let label = file.to_string_lossy();
    let text = read_text(Path::new(file))?;
    let ctx = Context {
        file: &label,
        max_lines: u64::from(&setup.owner.max_lines),
        goal: goal.as_deref(),
    };
    match validate(spec, &ctx, &text) {
        Ok(verdict) => {
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
        Err(problems) => {
            let mut said = format!("{label} is not a valid {name}:");
            for problem in problems {
                said.push_str("\n  ");
                said.push_str(&problem);
            }
            Err(Failure::no(said))
        }
    }
}
