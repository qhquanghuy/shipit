use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::setup::load_setup_table;
use crate::commands::status::judge_goals;
use crate::commands::validate::{Goal, valid_text};
use crate::edge::git::Git;
use crate::edge::records::{Put, Written, exists, load_goal, put_text, store_dir};
use crate::edge::slice_git::load_policy;
use crate::pure::failure::Failure;
use crate::pure::flags::{goal_flag, once, unknown_argument};
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::element_named;
use crate::pure::roadmap::Element;
use crate::pure::setup::HEAD;
use crate::pure::state::State;
use crate::pure::store_path::place;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str =
    "usage: shipit store put <kind> <draft> --goal <name> --slice <name> [--round <n>]";
pub(crate) const SUMMARY: &str = "Puts a checked build plan, plan review or review into the store at the place docs/store.md names. A build plan and its plan review are replaced only while the slice is not started; a review is never replaced; a link on the way is refused. A goal or roadmap version is the owner's and is copied in by hand.";
pub(crate) const EXITS: [&str; 4] = [
    "stored",
    "refused: the draft is not valid, or its place in the store is taken",
    "usage, setup, goal, roadmap or slice error, a link on the way into the store, or a write that failed",
    "",
];

struct Args<'a> {
    kind: &'a OsString,
    draft: &'a OsString,
    goal: GoalName,
    slice: Option<String>,
    round: Option<String>,
}

fn parse_args(args: &[OsString]) -> Result<Args<'_>, Failure> {
    let [verb, kind, draft, flags @ ..] = args else {
        return Err(Failure::usage(USAGE));
    };
    if verb != OsStr::new("put") {
        return Err(Failure::usage(USAGE));
    }
    let (mut goal, mut slice, mut round) = (None, None, None);
    let mut rest = flags.iter();
    while let Some(flag) = rest.next() {
        match flag.to_str() {
            Some("--goal") => goal_flag(&mut goal, rest.next(), USAGE)?,
            Some("--slice") => once(&mut slice, "--slice", rest.next(), USAGE)?,
            Some("--round") => once(&mut round, "--round", rest.next(), USAGE)?,
            Some(_) | None => return Err(unknown_argument(flag, USAGE)),
        }
    }
    let goal = goal.ok_or_else(|| Failure::usage(USAGE))?;
    Ok(Args {
        kind,
        draft,
        goal,
        slice,
        round,
    })
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (setup, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let given = parse_args(args)?;
    let kind = given.kind.to_string_lossy();
    let place =
        place(&kind, given.slice.as_deref(), given.round.as_deref()).map_err(Failure::usage)?;
    let common = git.common_dir()?;
    let goal = given.goal;
    let roadmap = load_goal(&common, &goal)?;
    let slice = place.slice();
    match element_named(&roadmap, slice) {
        Some(Element::Slice(_)) => {}
        Some(Element::Proof(_)) => {
            return Err(Failure::usage(format!(
                "{slice} is a proof step: it has no build plan or reviews"
            )));
        }
        None => {
            return Err(Failure::usage(format!(
                r#"no slice named "{slice}" in the roadmap of goal "{goal}""#
            )));
        }
    }
    let (text, _verdict) = valid_text(&git, &setup, &kind, given.draft, &Goal::Nothing)?;
    let store = store_dir(&common);
    let parts: Vec<String> = ["goals".to_owned(), goal.to_string()]
        .into_iter()
        .chain(place.parts())
        .collect();
    let path = parts
        .iter()
        .fold(store.clone(), |path, part| path.join(part));
    let how = if place.replaceable() && exists(&path) {
        let (policy, base_ref) = load_policy(&git, setup, err)?;
        let loaded = [(goal.clone(), roadmap)];
        let goals = judge_goals(&git, &common, (&policy, &base_ref), &loaded)?;
        let started = goals
            .iter()
            .flat_map(|view| view.views.iter())
            .any(|view| view.name == *slice && (view.started || view.state == State::Shipped));
        if started {
            return Err(Failure::no(format!(
                "{} exists and the slice {slice} is started: a changed build plan or plan review is then the lead's copy by hand (docs/store.md)",
                path.display()
            )));
        }
        Put::Replace
    } else {
        Put::New
    };
    match put_text(&store, &parts, &text, how)? {
        Written::Stored(path) => writeln!(out, "stored {}", path.display()).ok(),
        Written::Replaced(path) => writeln!(out, "replaced {}", path.display()).ok(),
    };
    Ok(())
}
