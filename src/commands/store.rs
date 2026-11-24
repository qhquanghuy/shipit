use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::commands::setup::load_setup_table;
use crate::commands::status::judge_goals;
use crate::commands::validate::{Goal, valid_text};
use crate::edge::clock::now_utc;
use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::edge::goal_file::newest_goal;
use crate::edge::records::{
    Put, Written, append_line, copy_text, exists, load_goal, logbook_path, newest_roadmap,
    put_text, read_store_text, store_dir,
};
use crate::edge::slice_git::{file_hash, load_policy};
use crate::edge::store::versions;
use crate::pure::failure::Failure;
use crate::pure::flags::{goal_flag, once, unknown_argument};
use crate::pure::goal::{GoalName, goal_items};
use crate::pure::goal_pick::element_named;
use crate::pure::logbook::{StoredVersion, stored_line};
use crate::pure::review::tool;
use crate::pure::roadmap::Element;
use crate::pure::setup::{HEAD, Setup};
use crate::pure::state::State;
use crate::pure::store_path::{OwnerFile, Place, PutKind, Shown, put_kind, roadmap_goal, shown};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit store put goal|roadmap <draft> --goal <name> | shipit store put <kind> <draft> --goal <name> --slice <name> [--round <n>] | shipit store show <kind> --goal <name> [--slice <name>] [--round <n>] [--copy]";
pub(crate) const SUMMARY: &str = "Puts a checked build plan, plan review or review into the store at the place docs/store.md names. A build plan and its plan review are replaced only while the slice is not started; a review is never replaced; a link on the way is refused. A goal or roadmap version is put as the next version and recorded in the goal's logbook; shipit store show prints a goal, roadmap, build plan, plan review or review from the store, or with --copy puts a copy in the drafts folder, from any working copy.";
pub(crate) const EXITS: [&str; 4] = [
    "stored, shown or copied",
    "refused: the draft is not valid, a roadmap of another goal, its place in the store is taken, or the file to show is not in the store",
    "usage, setup, goal, roadmap or slice error, a link on the way into the store, or a write that failed",
    "",
];

struct Flags {
    goal: GoalName,
    slice: Option<String>,
    round: Option<String>,
    copy: bool,
}

fn parse_flags(flags: &[OsString], with_copy: bool) -> Result<Flags, Failure> {
    let (mut goal, mut slice, mut round, mut copy) = (None, None, None, false);
    let mut rest = flags.iter();
    while let Some(flag) = rest.next() {
        match flag.to_str() {
            Some("--goal") => goal_flag(&mut goal, rest.next(), USAGE)?,
            Some("--slice") => once(&mut slice, "--slice", rest.next(), USAGE)?,
            Some("--round") => once(&mut round, "--round", rest.next(), USAGE)?,
            Some("--copy") if with_copy => {
                if std::mem::replace(&mut copy, true) {
                    return Err(Failure::usage(format!("--copy given twice; {USAGE}")));
                }
            }
            Some(_) | None => return Err(unknown_argument(flag, USAGE)),
        }
    }
    let goal = goal.ok_or_else(|| Failure::usage(USAGE))?;
    Ok(Flags {
        goal,
        slice,
        round,
        copy,
    })
}

fn parse_put(args: &[OsString]) -> Result<(&OsString, &OsString, Flags), Failure> {
    let [verb, kind, draft, flags @ ..] = args else {
        return Err(Failure::usage(USAGE));
    };
    if verb != OsStr::new("put") {
        return Err(Failure::usage(USAGE));
    }
    Ok((kind, draft, parse_flags(flags, false)?))
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    match args.first().and_then(|verb| verb.to_str()) {
        Some("show") => run_show(&git, args, out),
        Some(_) | None => run_put(&git, args, out, err),
    }
}

fn run_show(git: &Git, args: &[OsString], out: &mut dyn Write) -> Result<(), Failure> {
    let [_verb, kind, flags @ ..] = args else {
        return Err(Failure::usage(USAGE));
    };
    let given = parse_flags(flags, true)?;
    let kind = kind.to_string_lossy();
    let shown =
        shown(&kind, given.slice.as_deref(), given.round.as_deref()).map_err(Failure::usage)?;
    let common = git.common_dir()?;
    let store = store_dir(&common);
    let goal = given.goal;
    let named = |path: PathBuf| {
        path.file_name()
            .and_then(OsStr::to_str)
            .map(str::to_owned)
            .ok_or_else(|| Failure::usage(format!("{} has no file name", path.display())))
    };
    let (below_goal, copy_name) = match &shown {
        Shown::Goal => {
            let name = named(newest_goal(&common, &goal)?)?;
            (vec![name.clone()], name)
        }
        Shown::Roadmap => {
            let name = named(newest_roadmap(&store, &goal)?)?;
            (vec![name.clone()], name)
        }
        Shown::Placed(place) => (place.parts(), place.copy_name()),
    };
    let parts: Vec<String> = ["goals".to_owned(), goal.to_string()]
        .into_iter()
        .chain(below_goal)
        .collect();
    let text = read_store_text(&store, &parts)?;
    if given.copy {
        let at: Vec<String> = [
            ".shipit".to_owned(),
            "td".to_owned(),
            shown.folder_name(&goal),
            "store".to_owned(),
            copy_name,
        ]
        .into();
        let path = copy_text(git.top(), &at, &text)?;
        writeln!(out, "copied {}", path.display()).ok();
    } else {
        write!(out, "{text}").ok();
    }
    Ok(())
}

fn run_put(
    git: &Git,
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let (setup, _table) = load_setup_table(git, OsStr::new(HEAD))?;
    let (kind, draft, given) = parse_put(args)?;
    let kind = kind.to_string_lossy();
    match put_kind(&kind, given.slice.as_deref(), given.round.as_deref()).map_err(Failure::usage)? {
        PutKind::Owner(owned) => put_owned(git, &setup, owned, draft, &given.goal, out),
        PutKind::Placed(place) => put_placed(
            git,
            setup,
            (&*kind, draft.as_os_str()),
            (given.goal, place),
            (out, err),
        ),
    }
}

// The file is written first: a failed hash or append leaves a version that status then shows
// as not recorded, which is the truth.
fn put_owned(
    git: &Git,
    setup: &Setup,
    owned: OwnerFile,
    draft: &OsStr,
    goal: &GoalName,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let common = git.common_dir()?;
    let store = store_dir(&common);
    let label = draft.to_string_lossy();
    let text = match owned {
        OwnerFile::Goal => {
            let text = read_text(Path::new(draft))?;
            goal_items(&text)
                .map_err(|why| Failure::no(format!("{label} is not a valid goal:\n  {why}")))?;
            text
        }
        OwnerFile::Roadmap => {
            let name = goal.to_string();
            let (text, _verdict) = valid_text(
                git,
                setup,
                "roadmap",
                draft,
                &Goal::Stored(OsStr::new(&name)),
            )?;
            if roadmap_goal(&text).as_ref() != Some(goal) {
                return Err(Failure::no(format!(
                    "{label} names another goal than \"{goal}\": a roadmap is put under the goal it names"
                )));
            }
            text
        }
    };
    let newest = versions(&store, goal, owned).last().copied();
    let version = OwnerFile::next(newest);
    let parts = ["goals".to_owned(), goal.to_string(), owned.name(version)];
    let (Written::Stored(path) | Written::Replaced(path)) =
        put_text(&store, &parts, &text, Put::New)?;
    let hash = file_hash(git, &path)?;
    let stored = StoredVersion {
        kind: owned,
        version,
        hash,
    };
    append_line(
        &logbook_path(&store, goal),
        &stored_line(&stored, &now_utc(), &tool()),
    )?;
    writeln!(out, "stored {}", path.display()).ok();
    Ok(())
}

fn put_placed(
    git: &Git,
    setup: Setup,
    (kind, draft): (&str, &OsStr),
    (goal, place): (GoalName, Place),
    (out, err): (&mut dyn Write, &mut dyn Write),
) -> Result<(), Failure> {
    let common = git.common_dir()?;
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
    let (text, _verdict) = valid_text(git, &setup, kind, draft, &Goal::Nothing)?;
    let store = store_dir(&common);
    let parts: Vec<String> = ["goals".to_owned(), goal.to_string()]
        .into_iter()
        .chain(place.parts())
        .collect();
    let path = parts
        .iter()
        .fold(store.clone(), |path, part| path.join(part));
    let how = if place.replaceable() && exists(&path) {
        let (policy, base_ref) = load_policy(git, setup, err)?;
        let loaded = [(goal.clone(), roadmap)];
        let goals = judge_goals(git, &common, (&policy, &base_ref), &loaded)?;
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
