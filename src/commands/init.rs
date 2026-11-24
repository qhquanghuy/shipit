use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::commands::check::base_argument;
use crate::commands::setup::load_setup_table;
use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::edge::home::{home, machine_notes, machine_notes_path};
use crate::edge::machine::probe;
use crate::edge::projectfiles::{config_names, inside_project, is_executable, path_exists};
use crate::edge::terminal::{interactive, read_answer};
use crate::edge::write::{append_block, append_line, writability, write_new};
use crate::pure::base::BranchName;
use crate::pure::failure::Failure;
use crate::pure::hooks::{
    GitHook, HookFacts, HookFile, HookStep, Manager, ManagerFacts, Texts, follow_up, hook_plan,
    hooks_file, is_lefthook_local, manager, merge_steps,
};
use crate::pure::init::{
    CI_FILE, CI_KEY, HookMode, InitFacts, PACK_KEY, Start, choose_base, ci_job, embedded,
    hook_keys, init_plan, next_lines, starting_setup, test_check, tools_line,
};
use crate::pure::machine::{ToolState, Writability};
use crate::pure::machine_notes::{
    AT_LINE, BUILT_IN_NEVER, MachineFacts, MachineStep, NotesFile, ask_text, has_at_line,
    home_manager_snippet, init_recipe, machine_plan, plain_snippet, render_notes, says_yes,
};
use crate::pure::rules::invalid;
use crate::pure::setup::{HEAD, SETUP_PATH, parse_setup, toml_table};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit init [--base <branch>] [--no-hook | --hook-only] | shipit init --ci-job [--base <branch>]";
pub(crate) const SUMMARY: &str = "Writes the starting setup and adds the pre-push and pre-commit hooks without ever overwriting a file; --ci-job writes the CI job and reads the setup's base when --base is not given.";
pub(crate) const EXITS: [&str; 4] = [
    "written, or printed what to add",
    "a file exists that init never overwrites",
    "usage error, or the base cannot be told",
    "",
];

const SETUP_EXISTS: &str = "a setup file already exists; init never overwrites it (in a new clone, run shipit init --hook-only)";
const TOOLS: [&str; 6] = ["git", "bash", "gh", "jq", "rg", "nix"];

#[derive(Debug, PartialEq, Eq)]
struct Init {
    base: Option<BranchName>,
    mode: HookMode,
    ci_job: bool,
}

fn parse_args(args: &[OsString]) -> Result<Init, Failure> {
    let mut base: Option<BranchName> = None;
    let (mut no_hook, mut hook_only, mut ci_job) = (false, false, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--base" {
            if base.replace(base_argument(rest.next(), USAGE)?).is_some() {
                return Err(Failure::usage(format!("--base given twice; {USAGE}")));
            }
        } else if arg == "--no-hook" {
            no_hook = true;
        } else if arg == "--hook-only" {
            hook_only = true;
        } else if arg == "--ci-job" {
            ci_job = true;
        } else {
            return Err(Failure::usage(format!(
                "unknown argument '{}'; {USAGE}",
                arg.to_string_lossy()
            )));
        }
    }
    if no_hook && hook_only {
        return Err(Failure::usage(format!(
            "--no-hook and --hook-only cannot be given together; {USAGE}"
        )));
    }
    if ci_job && (no_hook || hook_only) {
        return Err(Failure::usage(format!(
            "--ci-job cannot be given with --no-hook or --hook-only; {USAGE}"
        )));
    }
    let mode = if no_hook {
        HookMode::NoHook
    } else if hook_only {
        HookMode::HookOnly
    } else {
        HookMode::Full
    };
    Ok(Init { base, mode, ci_job })
}

fn git_text(git: &Git, args: &[&str]) -> Option<String> {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let bytes = git.output(&args).ok()?;
    Some(
        String::from_utf8_lossy(&bytes)
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn embedded_file(key: &str) -> Result<&'static str, Failure> {
    embedded(key).ok_or_else(|| Failure::usage(format!("the embedded file {key} is missing")))
}

fn write_ci_job(git: &Git, base: Option<BranchName>, out: &mut dyn Write) -> Result<(), Failure> {
    let base = match base {
        Some(base) => base,
        None => load_setup_table(git, OsStr::new(HEAD))?.0.gate.base,
    };
    let text = ci_job(embedded_file(CI_KEY)?, &base);
    let exists = format!("{CI_FILE} already exists; init never overwrites it");
    write_new(&git.top().join(CI_FILE), &text, false, &exists)?;
    writeln!(out, "wrote {CI_FILE}").ok();
    Ok(())
}

fn base_of(git: &Git, given: Option<BranchName>, born: bool) -> Result<BranchName, Failure> {
    let origin_head = git_text(
        git,
        &[
            "symbolic-ref",
            "--short",
            "--quiet",
            "refs/remotes/origin/HEAD",
        ],
    );
    let locals: Vec<String> = git_text(
        git,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .map(|text| text.lines().map(str::to_owned).collect())
    .unwrap_or_default();
    let unborn = if born {
        None
    } else {
        git_text(git, &["symbolic-ref", "--short", "--quiet", HEAD])
    };
    choose_base(given, origin_head.as_deref(), &locals, unborn.as_deref()).ok_or_else(|| {
        Failure::usage("cannot tell the base branch; run shipit init --base <branch>")
    })
}

fn start_of(git: &Git, base: &BranchName, born: bool) -> Start {
    if !born {
        return Start::NoCommits;
    }
    let origin = format!("refs/remotes/origin/{base}");
    if git.commit_of(OsStr::new(&origin)).is_some() {
        Start::Pushed
    } else {
        Start::NoOrigin
    }
}

fn checked_setup(base: &BranchName, check: Option<&str>) -> Result<String, Failure> {
    let text = starting_setup(base, embedded_file(PACK_KEY)?, check);
    let table = toml_table(text.as_bytes())
        .map_err(|why| Failure::usage(format!("the starting setup is not TOML: {why}")))?;
    parse_setup(&table)
        .map_err(|problems| Failure::usage(invalid("the starting setup", &problems)))?;
    Ok(text)
}

fn git_succeeds(git: &Git, args: &[&str]) -> bool {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    git.output(&args).is_ok()
}

// A file that is there and cannot be read is not a missing one: nothing is planned to replace it.
// A hook git runs directly needs its executable bit (`must_run`).
fn read_hook(path: &Path, must_run: bool) -> HookFile {
    match read_text(path) {
        Ok(text) if must_run && !is_executable(path) => HookFile::NotExecutable(text),
        Ok(text) => HookFile::Present(text),
        Err(_why) if path_exists(path) => HookFile::Unreadable,
        Err(_why) => HookFile::Missing,
    }
}

fn hook_facts<'a>(git: &Git, roots: &[&Path], file: &'a str) -> HookFacts<'a> {
    let path = Path::new(file);
    HookFacts {
        file,
        inside_project: inside_project(path, roots),
        tracked: git_succeeds(git, &["ls-files", "--error-unmatch", "--", file]),
        hook: read_hook(path, true),
    }
}

// Every fact is read here, before the first file is written.
fn plan_hook(git: &Git, hook: GitHook) -> Result<(Manager, HookStep), Failure> {
    let common = git.common_dir()?;
    let roots = [git.top(), common.as_path()];
    let name = hook.name();
    let plain_file = git_text(
        git,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &format!("hooks/{name}"),
        ],
    )
    .ok_or_else(|| Failure::usage(format!("cannot find the {name} hook of this repository")))?;
    let hooks_file_path = git_text(git, &["config", "--type=path", "--get", "core.hooksPath"])
        .filter(|value| !value.is_empty())
        .map(|value| {
            git.top()
                .join(hooks_file(&value, hook))
                .to_string_lossy()
                .into_owned()
        });
    let top_files = config_names(git.top());
    let locals: Vec<(String, HookFile)> = top_files
        .iter()
        .filter(|name| is_lefthook_local(name))
        .map(|name| {
            let hook = read_hook(&git.top().join(name), false);
            (name.clone(), hook)
        })
        .collect();
    let ignored: Vec<String> = ["lefthook-local.yml", ".lefthook-local.yml"]
        .iter()
        .filter(|name| git_succeeds(git, &["check-ignore", "--quiet", "--", name]))
        .map(|name| (*name).to_owned())
        .collect();
    let found = manager(&ManagerFacts {
        hooks_path: hooks_file_path
            .as_deref()
            .map(|file| hook_facts(git, &roots, file)),
        plain: hook_facts(git, &roots, &plain_file),
        top_files: &top_files,
        lefthook_locals: &locals,
        ignored: &ignored,
    });
    let [hook_key, lefthook_key, pre_commit_key] = hook_keys(hook);
    let texts = Texts {
        hook: embedded_file(hook_key)?,
        lefthook: embedded_file(lefthook_key)?,
        pre_commit: embedded_file(pre_commit_key)?,
    };
    let step = hook_plan(hook, &found, &texts);
    Ok((found, step))
}

fn print_text(hook: GitHook, path: &str, reason: &str, text: &str, out: &mut dyn Write) {
    if text.contains('\n') {
        writeln!(out, "not added to {path} ({reason}); add it yourself:").ok();
        for line in text.lines() {
            writeln!(out, "  {line}").ok();
        }
    } else if hook == GitHook::Push {
        writeln!(
            out,
            "not added to {path} ({reason}); add this line yourself, before anything reads git's input: {text}"
        )
        .ok();
    } else {
        writeln!(
            out,
            "not added to {path} ({reason}); add this line yourself: {text}"
        )
        .ok();
    }
}

fn apply_hook(
    git: &Git,
    hook: GitHook,
    found: &Manager,
    step: &HookStep,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let executable = matches!(found, Manager::HooksPath { .. } | Manager::Plain { .. });
    let name = hook.name();
    match step {
        HookStep::Write { path, text } => {
            let exists = format!(
                "a {name} hook already exists at {path} and could not be read; init never overwrites it"
            );
            write_new(&git.top().join(path), text, executable, &exists)?;
            writeln!(out, "wrote {path}").ok();
        }
        HookStep::Append { path, line } if line.contains('\n') => {
            append_block(&git.top().join(path), line)?;
            writeln!(out, "added the {name} entry to {path}").ok();
        }
        HookStep::Append { path, line } => {
            if append_line(&git.top().join(path), line)? {
                writeln!(out, "added to {path}: {line}").ok();
            } else {
                writeln!(out, "{}", hook.already_installed_text()).ok();
            }
        }
        HookStep::Print { path, reason, text } => print_text(hook, path, reason, text, out),
        HookStep::Nothing(text) => {
            writeln!(out, "{text}").ok();
        }
    }
    Ok(())
}

// Both hooks are planned before the first write. A lefthook-local file is one write for both, so
// the steps are paired with the hooks in order: a merged write is told as the push hook's.
fn apply_hooks(
    git: &Git,
    (push_manager, push_step): (Manager, HookStep),
    (commit_manager, commit_step): (Manager, HookStep),
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let mut told: Vec<String> = Vec::new();
    let hooks = [
        (GitHook::Push, push_manager),
        (GitHook::Commit, commit_manager),
    ];
    for (step, (hook, found)) in merge_steps(push_step, commit_step).iter().zip(&hooks) {
        apply_hook(git, *hook, found, step, out)?;
        for line in follow_up(found, step) {
            if !told.contains(&line) {
                writeln!(out, "{line}").ok();
                told.push(line);
            }
        }
    }
    Ok(())
}

fn notes_file(notes_path: &Path, exists: bool) -> NotesFile {
    if exists {
        return NotesFile::Exists;
    }
    match writability(notes_path) {
        Writability::CanCreate => NotesFile::CanCreate,
        Writability::Writable | Writability::ReadOnly | Writability::CannotCreate => {
            NotesFile::CannotCreate
        }
    }
}

// The machine files are the owner's, not the project's: they are written only on a yes in a
// terminal, and otherwise printed.
fn machine_step(
    tools: &[(String, ToolState)],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let Some(home) = home() else {
        writeln!(out, "machine notes: HOME is not set; skipped").ok();
        return Ok(());
    };
    let notes_path = machine_notes_path(&home);
    let claude_md = home.join(".claude").join("CLAUDE.md");
    let facts = MachineFacts {
        interactive: interactive(),
        nix: tools
            .iter()
            .any(|(tool, state)| tool == "nix" && *state != ToolState::Missing),
        notes: notes_file(&notes_path, machine_notes().is_some()),
        claude_md: writability(&claude_md),
        has_at_line: read_text(&claude_md).is_ok_and(|text| has_at_line(&text)),
    };
    let write_notes = facts.notes != NotesFile::Exists;
    let add_at_line = !facts.has_at_line;
    let missing: Vec<&str> = tools
        .iter()
        .filter(|(_tool, state)| *state == ToolState::Missing)
        .map(|(tool, _state)| tool.as_str())
        .collect();
    let notes =
        write_notes.then(|| render_notes(&missing, &init_recipe(facts.nix), BUILT_IN_NEVER));
    match machine_plan(facts) {
        MachineStep::Nothing => {}
        MachineStep::Print => {
            let snippet = if facts.nix {
                home_manager_snippet(notes.as_deref(), add_at_line)
            } else {
                plain_snippet(notes.as_deref(), add_at_line)
            };
            writeln!(out, "{snippet}").ok();
        }
        MachineStep::Ask => {
            write!(err, "{}", ask_text(write_notes, add_at_line)).ok();
            err.flush().ok();
            if !read_answer().is_some_and(|answer| says_yes(&answer)) {
                writeln!(out, "machine notes: not written").ok();
                return Ok(());
            }
            if let Some(text) = &notes {
                write_new(
                    &notes_path,
                    text,
                    false,
                    "~/.config/shipit/machine.md already exists; init never overwrites it",
                )?;
                writeln!(out, "machine notes: wrote ~/.config/shipit/machine.md").ok();
            }
            if add_at_line && append_line(&claude_md, AT_LINE)? {
                writeln!(out, "machine notes: added {AT_LINE} to ~/.claude/CLAUDE.md").ok();
            }
        }
    }
    Ok(())
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let init = parse_args(args)?;
    let git = Git::find()?;
    if init.ci_job {
        return write_ci_job(&git, init.base, out);
    }
    let setup_path = git.top().join(SETUP_PATH);
    let facts = InitFacts {
        setup_exists: path_exists(&setup_path),
    };
    let steps = init_plan(&facts, init.mode)?;
    let born = git.commit_of(OsStr::new(HEAD)).is_some();
    let check = test_check(&config_names(git.top()));
    let setup = if steps.write_setup {
        let base = base_of(&git, init.base, born)?;
        Some((checked_setup(&base, check)?, base))
    } else {
        None
    };
    let hooks = if steps.hook {
        Some((
            plan_hook(&git, GitHook::Push)?,
            plan_hook(&git, GitHook::Commit)?,
        ))
    } else {
        None
    };

    if let Some((text, base)) = &setup {
        write_new(&setup_path, text, false, SETUP_EXISTS)?;
        writeln!(out, "wrote {SETUP_PATH} (base {base})").ok();
    }
    if let Some((push, commit)) = hooks {
        apply_hooks(&git, push, commit, out)?;
    }
    let tools = probe(&TOOLS);
    // The setup is the start of an adoption; the tools and the next steps belong to it.
    if let Some((_text, base)) = &setup {
        if let Some(line) = tools_line(&tools) {
            writeln!(out, "{line}").ok();
        }
        for line in next_lines(base, start_of(&git, base, born), check.is_some()) {
            writeln!(out, "{line}").ok();
        }
    }
    machine_step(&tools, out, err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pure::testing::{name, os};

    #[test]
    fn init_args() {
        assert_eq!(
            parse_args(&[]),
            Ok(Init {
                base: None,
                mode: HookMode::Full,
                ci_job: false
            })
        );
        assert_eq!(
            parse_args(&os(&["--hook-only", "--base", "main"])),
            Ok(Init {
                base: Some(name("main")),
                mode: HookMode::HookOnly,
                ci_job: false
            })
        );
        assert_eq!(
            parse_args(&os(&["--ci-job", "--base", "dev"])),
            Ok(Init {
                base: Some(name("dev")),
                mode: HookMode::Full,
                ci_job: true
            })
        );
        for (words, message) in [
            (vec!["--nope"], "unknown argument '--nope'"),
            (vec!["--base"], "--base needs a branch name"),
            (vec!["--base", "a", "--base", "b"], "--base given twice"),
            (
                vec!["--no-hook", "--hook-only"],
                "--no-hook and --hook-only cannot be given together",
            ),
            (
                vec!["--ci-job", "--no-hook"],
                "--ci-job cannot be given with --no-hook or --hook-only",
            ),
            (
                vec!["--hook-only", "--ci-job"],
                "--ci-job cannot be given with --no-hook or --hook-only",
            ),
        ] {
            assert_eq!(
                parse_args(&os(&words)),
                Err(Failure::usage(format!("{message}; {USAGE}"))),
                "{words:?}"
            );
        }
    }
}
