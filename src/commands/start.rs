use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::setup::load_setup_table;
use crate::commands::status::{judge_goals, unknown_branches};
use crate::edge::clock::now_utc;
use crate::edge::git::Git;
use crate::edge::records::{
    append_line, exists, load_goals, logbook_path, metrics_path, read_logbook, store_dir,
};
use crate::edge::slice_git::{create_branch, load_policy, origin_branch_of};
use crate::edge::write;
use crate::pure::branch::slice_branch;
use crate::pure::failure::Failure;
use crate::pure::flags::{goal_flag, unknown_argument};
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::{element_name, element_named, which_goal};
use crate::pure::logbook::{Entry, line};
use crate::pure::review::tool;
use crate::pure::roadmap::{Element, SliceName};
use crate::pure::setup::HEAD;
use crate::pure::state::{StartFacts, State, start_refusal};

// Anchored and ending in a slash: it never matches `.shipit/coverage/`.
const WORKTREES: [&str; 2] = ["/.shipit/wt/", "/.claude/worktrees/"];

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit start <slice> [--goal <name>]";
pub(crate) const SUMMARY: &str = "Makes the slice's branch and logs it, refused at the parallel cap, which counts all goals; fetch first, since a goal that another clone has not pushed is not seen, and two starts at the same moment can both pass the cap. It adds /.shipit/wt/ and /.claude/worktrees/ to the git common dir's info/exclude, so the worktree it names and the worktrees of isolated agents stay out of git.";
pub(crate) const EXITS: [&str; 4] = [
    "started",
    "refused: build plan not ready, an earlier slice not shipped, the branch exists, or the cap is reached",
    "usage, setup, goal or roadmap error, or a goal name that is already used",
    "",
];

fn parse_args(args: &[OsString]) -> Result<(String, Option<GoalName>), Failure> {
    let mut goal = None;
    let mut slice = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.to_str() {
            Some("--goal") => goal_flag(&mut goal, rest.next(), USAGE)?,
            Some(flag) if flag.starts_with("--") => return Err(unknown_argument(arg, USAGE)),
            Some(word) => slice.push(word.to_owned()),
            None => return Err(unknown_argument(arg, USAGE)),
        }
    }
    match slice.as_slice() {
        [only] => Ok((only.clone(), goal)),
        _ => Err(Failure::usage(USAGE)),
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (head, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let (typed, flag_goal) = parse_args(args)?;
    let (policy, base_ref) = load_policy(&git, head, err)?;
    let common = git.common_dir()?;
    let store = store_dir(&common);
    let loaded = load_goals(&common)?;
    let names: Vec<GoalName> = loaded.iter().map(|(goal, _)| goal.clone()).collect();
    let goal = which_goal(flag_goal.as_ref(), git.current_branch().as_deref(), &names)?;
    let (_, roadmap) = loaded
        .iter()
        .find(|(name, _)| *name == goal)
        .ok_or_else(|| Failure::usage(format!("no goal named \"{goal}\" in the store")))?;
    let unknown_slice = || {
        Failure::usage(format!(
            "no slice named \"{typed}\" in the roadmap of goal \"{goal}\""
        ))
    };
    let slice = SliceName::parse(&typed).ok_or_else(unknown_slice)?;
    let after = match element_named(roadmap, &slice).ok_or_else(unknown_slice)? {
        Element::Slice(found) => &found.after,
        Element::Proof(_) => {
            return Err(Failure::usage(format!(
                "\"{typed}\" is a proof step: it has no branch to start"
            )));
        }
    };

    let goals = judge_goals(&git, &common, (&policy, &base_ref), &loaded)?;
    let unknown = unknown_branches(&git, &loaded)?;
    let views: Vec<_> = goals.iter().flat_map(|goal| goal.views.iter()).collect();
    let this = views
        .iter()
        .find(|view| view.goal == goal && view.name == slice)
        .ok_or_else(unknown_slice)?;
    let waiting: Vec<&SliceName> = roadmap
        .elements
        .iter()
        .map(element_name)
        .filter(|name| after.contains(name))
        .filter(|name| {
            views.iter().any(|view| {
                view.goal == goal && view.name == **name && view.state != State::Shipped
            })
        })
        .collect();
    let started_goals: Vec<GoalName> = views
        .iter()
        .filter(|view| view.started)
        .map(|view| view.goal.clone())
        .chain(unknown.iter().map(|item| item.goal.clone()))
        .collect();
    let cap = u64::from(&policy.agents.parallel.slices);
    let facts = StartFacts {
        view: this,
        waiting: &waiting,
        started_goals: &started_goals,
        cap,
    };
    if let Some(why) = start_refusal(&facts) {
        return Err(Failure::no(why));
    }

    let first = !read_logbook(&store, &goal)?
        .iter()
        .any(|logged| matches!(logged.entry, Entry::Started { .. }));
    if first {
        let used = match origin_branch_of(&git, &goal)? {
            Some(branch) => Some(branch),
            None if exists(&metrics_path(&store, &goal)) => Some(format!("metrics/{goal}.toml")),
            None => None,
        };
        if let Some(used) = used {
            return Err(Failure::usage(format!(
                "goal \"{goal}\" is already used ({used}): pick another goal name; if the remote \
                 branches are new, fetch first"
            )));
        }
    }

    for folder in WORKTREES {
        write::append_line(&common.join("info").join("exclude"), folder)?;
    }
    let branch = slice_branch(&goal, &slice);
    create_branch(&git, &branch, &base_ref)?;
    let entry = Entry::Started {
        branch: branch.clone(),
    };
    let text = line(&entry, &slice, &now_utc(), &tool()).map_err(Failure::usage)?;
    append_line(&logbook_path(&store, &goal), &text)?;
    // The main working copy holds the folder, whatever folder or worktree the start ran in.
    let main_top = match (common.file_name(), common.parent()) {
        (Some(name), Some(parent)) if name == ".git" => parent,
        (Some(_) | None, Some(_) | None) => git.top(),
    };
    let worktree = main_top.join(".shipit").join("wt").join(slice.to_string());
    writeln!(
        out,
        "started {slice} on {branch}; for one session per slice make its working copy: git worktree add \
         {} {branch}; an isolated maker needs none (it switches to {branch})",
        worktree.display()
    )
    .ok();
    Ok(())
}
