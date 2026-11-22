use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::commands::check::settle_base;
use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::edge::hook_process::{exists, project_dir, read_hook_input};
use crate::pure::claude_hook::{HookInput, answer_json, mcp_ship, parse_input, resolve};
use crate::pure::failure::Failure;
use crate::pure::setup::{HEAD, SETUP_PATH};
use crate::pure::shell_words::{Parse, parse};
use crate::pure::ship_gate::{Answer, Facts, ship_answer, ship_reasons, triggered};

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

// Larger than any command a person types, and as much as the hook is asked to hold.
const INPUT_LIMIT: usize = 8 * 1024 * 1024;

// A project is a repository with the setup file in the working tree or at HEAD.
pub(crate) fn shipit_project(git: &Git) -> bool {
    exists(&git.top().join(SETUP_PATH)) || git.file_exists(OsStr::new(HEAD), SETUP_PATH)
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
    Facts {
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

fn judge_command(input: &HookInput, command: &str) -> Answer {
    if !triggered(command) {
        return Answer::Pass;
    }
    let mut dirs: BTreeSet<Option<String>> = BTreeSet::from([None]);
    if let Parse::Plain { segments, .. } = parse(command) {
        dirs.extend(segments.into_iter().map(|segment| segment.dir));
    }
    let anywhere = dirs
        .iter()
        .any(|dir| in_project(&resolve(&input.cwd, dir.as_deref())));
    if !anywhere {
        return Answer::Pass;
    }
    let reasons = ship_reasons(command, &|dir| facts_at(&resolve(&input.cwd, dir)));
    ship_answer(input.mode, &reasons)
}

fn judge(input: &HookInput) -> Answer {
    let Some(name) = input.tool_name.as_deref() else {
        return unreadable(input, "the hook input has no tool_name");
    };
    if name.starts_with("mcp__") {
        return match mcp_ship(name) {
            Some(reason) if in_project(&input.cwd) => ship_answer(input.mode, &[reason]),
            Some(_) | None => Answer::Pass,
        };
    }
    let command = input
        .tool_input
        .as_ref()
        .and_then(|tool_input| tool_input.get("command"))
        .and_then(Value::as_str);
    match command {
        Some(command) => judge_command(input, command),
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
    if !args.is_empty() {
        return Err(Failure::usage(USAGE));
    }
    let input = read_hook_input(INPUT_LIMIT).and_then(|bytes| parse_input(&bytes));
    if let Some(json) = answer_json(&decide(&input)) {
        writeln!(out, "{json}").ok();
    }
    Ok(())
}
