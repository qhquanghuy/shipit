use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::commands::check::settle_base;
use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::edge::hook_process::{exists, home_dir, make_folder, project_dir, run_gate};
use crate::edge::records::store_dir;
use crate::pure::branch::is_slice_branch;
use crate::pure::claude_hook::{HookInput, mcp_ship, resolve};
use crate::pure::failure::Failure;
use crate::pure::made_by::{Caller, Change, Moves, answer, author_verdict, moves};
use crate::pure::setup::{HEAD, SETUP_PATH};
use crate::pure::ship_gate::{
    Answer, Facts, judged_dirs, named_folders, rewires_push, ship_answer, ship_reasons, triggered,
};

pub(crate) const HIDDEN: bool = true;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit claude-ship-gate (the hook input is read from stdin)";
pub(crate) const SUMMARY: &str = "A Claude Code hook; reads one JSON event from stdin and prints nothing or one ask or deny decision; it allows everything outside a ShipIt project.";
pub(crate) const EXITS: [&str; 4] = [
    "no decision, or one decision printed",
    "",
    "internal failure: the hook blocks",
    "",
];

// The tip of the default branch: what origin says its HEAD is, else origin's or a local main or
// master. A folder checked out at a commit from before the setup file is still in the same repository.
fn default_branch_has_setup(git: &Git) -> bool {
    let origin = lines_of(
        git,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    );
    let tips: Vec<String> = if origin.is_empty() {
        [
            "refs/remotes/origin/main",
            "refs/remotes/origin/master",
            "refs/heads/main",
            "refs/heads/master",
        ]
        .map(str::to_owned)
        .to_vec()
    } else {
        origin
            .iter()
            .filter_map(|line| line.strip_prefix("origin/"))
            .map(|name| format!("refs/remotes/origin/{name}"))
            .collect()
    };
    tips.iter()
        .any(|tip| git.file_exists(OsStr::new(tip), SETUP_PATH))
}

// A project is a repository, not a checkout: the setup file is in the working tree, at HEAD or
// at the tip of the default branch, or the store folder of the repository exists. A repository
// whose only setup file is on some other branch is no project.
pub(crate) fn shipit_project(git: &Git) -> bool {
    exists(&git.top().join(SETUP_PATH))
        || git.file_exists(OsStr::new(HEAD), SETUP_PATH)
        || default_branch_has_setup(git)
        || git
            .common_dir()
            .is_ok_and(|common| exists(&store_dir(&common)))
}

// A session that starts in a project makes the store folder of its repository, so that the
// repository stays one when a commit, a checkout or a branch no longer shows the setup file. Only
// the working tree and HEAD count here: a repository whose setup is elsewhere is not made one.
pub(crate) fn settle_project(dir: &Path) -> Result<(), String> {
    let Ok(git) = Git::at(dir) else {
        return Ok(());
    };
    if !exists(&git.top().join(SETUP_PATH)) && !git.file_exists(OsStr::new(HEAD), SETUP_PATH) {
        return Ok(());
    }
    let common = git
        .common_dir()
        .map_err(|_why| "cannot find the git folder of the project".to_owned())?;
    make_folder(&store_dir(&common))
}

fn lines_of(git: &Git, args: &[&str]) -> Vec<String> {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    git.output(&args)
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

// The base branch as its own setup names it (the slice's value never counts), then `main`,
// `master` and what origin says its default is.
pub(crate) fn base_names(git: &Git) -> Vec<String> {
    let mut names: Vec<String> = ["main", "master"].map(str::to_owned).to_vec();
    if let Ok((setup, _table)) = load_setup_table(git, OsStr::new(HEAD)) {
        let mut ignored = Vec::new();
        if let Ok((base, _ref, _flow)) = settle_base(git, setup.gate.base, false, &mut ignored) {
            names.push(base.to_string());
        }
    }
    let origin = lines_of(
        git,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    );
    names.extend(
        origin
            .iter()
            .filter_map(|line| line.strip_prefix("origin/"))
            .map(str::to_owned),
    );
    names.sort();
    names.dedup();
    names
}

fn refs(git: &Git, prefix: &str) -> Vec<String> {
    lines_of(git, &["for-each-ref", "--format=%(refname)", prefix])
        .iter()
        .filter_map(|line| line.strip_prefix(prefix))
        .map(str::to_owned)
        .collect()
}

// A folder that is no ShipIt project has no facts: a push there can name no slice branch.
fn facts_at(dir: &Path) -> Facts {
    let Ok(git) = Git::at(dir) else {
        return Facts::default();
    };
    if !shipit_project(&git) {
        return Facts::default();
    }
    let follow_tags = git
        .output(&[
            OsStr::new("config"),
            OsStr::new("--type=bool"),
            OsStr::new("push.followTags"),
        ])
        .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).trim() == "true");
    // The whole config, as git reads it here: a push setting anywhere in it can move a push.
    let rewires = lines_of(&git, &["config", "--list"]).join("\n");
    Facts {
        rewires_push: rewires_push(&rewires),
        current: git.current_branch(),
        base: base_names(&git),
        heads: refs(&git, "refs/heads/"),
        tags: refs(&git, "refs/tags/"),
        follow_tags,
    }
}

fn in_project(dir: &Path) -> bool {
    Git::at(dir).is_ok_and(|git| shipit_project(&git))
}

// The folder a hook judges: the input's own, or for an input that cannot be read the
// session's project folder, then the folder the process runs in.
pub(crate) fn input_folder(input: &HookInput) -> Option<Git> {
    Git::at(&input.cwd).ok().filter(shipit_project)
}

pub(crate) fn gate_folder(input: &Result<HookInput, String>) -> Option<Git> {
    let git = match input {
        Ok(input) => return input_folder(input),
        Err(_why) => match project_dir() {
            Some(dir) => Git::at(&dir).ok()?,
            None => Git::find().ok()?,
        },
    };
    shipit_project(&git).then_some(git)
}

fn unreadable(input: &HookInput, why: &str) -> Answer {
    if in_project(&input.cwd) {
        ship_answer(
            input.mode,
            &[format!("cannot be read by ShipIt ({why}); it may ship")],
        )
    } else {
        Answer::Pass
    }
}

// Every folder the command works in: the working folder, each `cd`, and a folder it names for git
// itself (`-C`, `--git-dir`, GIT_DIR).
fn judged_folders(input: &HookInput, home: Option<&Path>, command: &str) -> Vec<PathBuf> {
    let named = named_folders(command).into_iter().map(|(dir, folder)| {
        resolve(
            &resolve(&input.cwd, home, dir.as_deref()),
            None,
            Some(folder.as_str()),
        )
    });
    let dirs = judged_dirs(command)
        .into_iter()
        .map(|dir| resolve(&input.cwd, home, dir.as_deref()));
    named.chain(dirs).collect()
}

fn judge_command(input: &HookInput, command: &str) -> Answer {
    if !triggered(command) {
        return Answer::Pass;
    }
    let home = home_dir();
    let home = home.as_deref();
    let anywhere = judged_folders(input, home, command)
        .iter()
        .any(|folder| in_project(folder));
    if !anywhere {
        return Answer::Pass;
    }
    let reasons = ship_reasons(command, &|dir| facts_at(&resolve(&input.cwd, home, dir)));
    ship_answer(input.mode, &reasons)
}

fn caller(input: &HookInput) -> Caller<'_> {
    match &input.agent_id {
        Some(_) => Caller::Agent(input.agent_type.as_deref().unwrap_or("")),
        None => Caller::Main,
    }
}

// The folder when it is a working copy of a ShipIt project on a slice branch: the work of a slice.
pub(crate) fn on_slice_branch(dir: &Path) -> Option<Git> {
    let git = Git::at(dir).ok().filter(shipit_project)?;
    git.current_branch()
        .is_some_and(|branch| is_slice_branch(&branch))
        .then_some(git)
}

// A commit or move in a working copy on a slice branch is the maker's. A command that names git
// and a move but cannot be read is judged as a commit with no trailer, in every folder it passes.
fn judge_moves(input: &HookInput, command: &str) -> Answer {
    let home = home_dir();
    let home = home.as_deref();
    let who = caller(input);
    let targets: Vec<(PathBuf, String, Option<String>)> = match moves(command) {
        Moves::Plain(found) => found
            .into_iter()
            .map(|one| {
                let folder = resolve(
                    &resolve(&input.cwd, home, one.dir.as_deref()),
                    None,
                    one.folder.as_deref(),
                );
                (folder, one.sub, one.trailer)
            })
            .collect(),
        Moves::Unreadable => judged_folders(input, home, command)
            .into_iter()
            .map(|folder| (folder, "commit".to_owned(), None))
            .collect(),
    };
    for (folder, sub, trailer) in &targets {
        if on_slice_branch(folder).is_none() {
            continue;
        }
        let change = Change::Move {
            sub,
            trailer: trailer.as_deref(),
        };
        let answered = answer(input.mode, author_verdict(&who, &change));
        if answered != Answer::Pass {
            return answered;
        }
    }
    Answer::Pass
}

// What git ignores and does not track is not the work: build output, drafts and notes pass.
fn judge_edit(input: &HookInput, path: &str) -> Answer {
    let home = home_dir();
    let path = resolve(&input.cwd, home.as_deref(), Some(path));
    let Some(git) = path
        .ancestors()
        .skip(1)
        .find(|dir| exists(dir))
        .and_then(on_slice_branch)
    else {
        return Answer::Pass;
    };
    let ignored = git
        .output(&[
            OsStr::new("check-ignore"),
            OsStr::new("-q"),
            OsStr::new("--"),
            path.as_os_str(),
        ])
        .is_ok();
    if !path.starts_with(git.top()) || ignored {
        return Answer::Pass;
    }
    answer(input.mode, author_verdict(&caller(input), &Change::Edit))
}

fn judge(input: &HookInput) -> Answer {
    let Some(name) = input.tool_name.as_deref() else {
        return unreadable(input, "the hook input has no tool_name");
    };
    if name.starts_with("mcp__") {
        return match mcp_ship(name, input.tool_input.as_ref()) {
            Some(reason) if in_project(&input.cwd) => ship_answer(input.mode, &[reason]),
            Some(_) | None => Answer::Pass,
        };
    }
    let text = |key: &str| {
        input
            .tool_input
            .as_ref()
            .and_then(|tool_input| tool_input.get(key))
            .and_then(Value::as_str)
    };
    if matches!(name, "Edit" | "Write" | "NotebookEdit") {
        return text("file_path")
            .or_else(|| text("notebook_path"))
            .map_or(Answer::Pass, |path| judge_edit(input, path));
    }
    match text("command") {
        Some(command) => match judge_moves(input, command) {
            Answer::Pass => judge_command(input, command),
            decided @ (Answer::Ask(_) | Answer::Deny(_)) => decided,
        },
        None => unreadable(input, "the hook input has no tool_input.command"),
    }
}

fn decide(input: &Result<HookInput, String>) -> Answer {
    match input {
        Ok(input) => judge(input),
        Err(why) if gate_folder(input).is_some() => Answer::Deny(format!(
            "ShipIt cannot read the hook input ({why}); only the owner may run a ship command."
        )),
        Err(_why) => Answer::Pass,
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    run_gate(args, out, USAGE, decide)
}
