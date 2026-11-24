#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that tests for a file, reads CLAUDE_PROJECT_DIR, makes the store folder and asks whether the input is a terminal for the hooks"
)]

use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal as _, Write};
use std::path::{Path, PathBuf};

use crate::commands::check::settle_base;
use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::edge::input::read_stdin;
use crate::edge::records::store_dir;
use crate::pure::branch::is_slice_branch;
use crate::pure::claude_hook::{Answer, HookInput, answer_json, parse_input};
use crate::pure::failure::Failure;
use crate::pure::setup::{HEAD, SETUP_PATH};

// Larger than any command a person types, and as much as the hook is asked to hold.
const INPUT_LIMIT: usize = 8 * 1024 * 1024;

// An unreadable path counts as a missing file: the gates then read "not a ShipIt project", and
// a project is also looked for at HEAD.
pub(crate) fn exists(path: &Path) -> bool {
    path.try_exists().unwrap_or(false)
}

// The folder Claude Code started the session in, when it says so.
pub(crate) fn project_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_PROJECT_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

// The store folder of a project, made when a session starts in it.
pub(crate) fn make_folder(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|why| format!("cannot make {}: {why}", path.display()))
}

// A hook is handed its event on a pipe. A person who runs one by hand at a terminal would wait
// for input that never ends, so a terminal is told as an unreadable input.
pub(crate) fn read_hook_input(limit: usize) -> Result<Vec<u8>, String> {
    if std::io::stdin().is_terminal() {
        return Err("the input is a terminal, not a hook event".to_owned());
    }
    read_stdin(limit)
}

// Every gate answers its event the same way: the input is read and parsed, the gate decides, and
// any answer but a pass is printed as one line of JSON. A failure to print is not reported: the
// hook has nothing left to tell.
pub(crate) fn run_gate(
    args: &[OsString],
    out: &mut dyn Write,
    usage: &str,
    decide: fn(&Result<HookInput, String>) -> Answer,
) -> Result<(), Failure> {
    if !args.is_empty() {
        return Err(Failure::usage(usage));
    }
    let input = read_hook_input(INPUT_LIMIT).and_then(|bytes| parse_input(&bytes));
    if let Some(json) = answer_json(&decide(&input)) {
        writeln!(out, "{json}").ok();
    }
    Ok(())
}

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

// The folder when it is a working copy of a ShipIt project on a slice branch: the work of a slice.
fn is_slice_working_copy(git: &Git) -> bool {
    git.current_branch()
        .is_some_and(|branch| is_slice_branch(&branch))
}

pub(crate) fn on_slice_branch(dir: &Path) -> Option<Git> {
    let git = Git::at(dir).ok().filter(shipit_project)?;
    is_slice_working_copy(&git).then_some(git)
}
