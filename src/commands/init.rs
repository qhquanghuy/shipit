use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::commands::check::base_argument;
use crate::commands::setup::load_setup_table;
use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::edge::machine::probe;
use crate::edge::write::{append_line, write_new};
use crate::pure::base::BranchName;
use crate::pure::failure::Failure;
use crate::pure::hooks::{HookFile, HookStep, plain_hook_plan};
use crate::pure::init::{
    CI_FILE, CI_KEY, HOOK_KEY, HookMode, InitFacts, PACK_KEY, Start, choose_base, ci_job, embedded,
    init_plan, next_lines, starting_setup, tools_line,
};
use crate::pure::rules::invalid;
use crate::pure::setup::{HEAD, SETUP_PATH, parse_setup, toml_table};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = false;
pub(crate) const USAGE: &str = "usage: shipit init [--base <branch>] [--no-hook | --hook-only] | shipit init --ci-job [--base <branch>]";
pub(crate) const SUMMARY: &str = "Writes the starting setup and adds the pre-push hook without ever overwriting a file; --ci-job writes the CI job and reads the setup's base when --base is not given.";
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

fn checked_setup(base: &BranchName) -> Result<String, Failure> {
    let text = starting_setup(base, embedded_file(PACK_KEY)?);
    let table = toml_table(text.as_bytes())
        .map_err(|why| Failure::usage(format!("the starting setup is not TOML: {why}")))?;
    parse_setup(&table)
        .map_err(|problems| Failure::usage(invalid("the starting setup", &problems)))?;
    Ok(text)
}

fn hook_step(git: &Git) -> Result<(String, HookStep), Failure> {
    let path = git_text(
        git,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "hooks/pre-push",
        ],
    )
    .ok_or_else(|| Failure::usage("cannot find the pre-push hook of this repository"))?;
    let here = Path::new(&path);
    let inside = here.starts_with(git.top()) || here.starts_with(git.common_dir()?);
    let hook = read_text(here).map_or(HookFile::Missing, HookFile::Present);
    let step = plain_hook_plan(&path, inside, false, &hook, embedded_file(HOOK_KEY)?);
    Ok((path, step))
}

fn apply_hook(path: &str, step: &HookStep, out: &mut dyn Write) -> Result<(), Failure> {
    match step {
        HookStep::Write { path, text } => {
            let exists = format!(
                "a pre-push hook already exists at {path} and could not be read; init never overwrites it"
            );
            write_new(Path::new(path), text, true, &exists)?;
            writeln!(out, "wrote {path}").ok();
        }
        HookStep::Append { path, line } => {
            if append_line(Path::new(path), line)? {
                writeln!(out, "added to {path}: {line}").ok();
            } else {
                writeln!(out, "pre-push hook: already installed").ok();
            }
        }
        HookStep::Print { reason, text } => {
            writeln!(
                out,
                "not added to {path} ({reason}); add this line yourself, before anything reads git's input: {text}"
            )
            .ok();
        }
        HookStep::Nothing(text) => {
            writeln!(out, "{text}").ok();
        }
    }
    Ok(())
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    let init = parse_args(args)?;
    let git = Git::find()?;
    if init.ci_job {
        return write_ci_job(&git, init.base, out);
    }
    let setup_path = git.top().join(SETUP_PATH);
    let facts = InitFacts {
        setup_exists: read_text(&setup_path).is_ok(),
    };
    let steps = init_plan(&facts, init.mode)?;
    let born = git.commit_of(OsStr::new(HEAD)).is_some();
    let setup = if steps.write_setup {
        let base = base_of(&git, init.base, born)?;
        Some((checked_setup(&base)?, base))
    } else {
        None
    };
    let hook = if steps.hook {
        Some(hook_step(&git)?)
    } else {
        None
    };

    if let Some((text, base)) = &setup {
        write_new(&setup_path, text, false, SETUP_EXISTS)?;
        writeln!(out, "wrote {SETUP_PATH} (base {base})").ok();
    }
    if let Some((path, step)) = &hook {
        apply_hook(path, step, out)?;
    }
    // The setup is the start of an adoption; the tools and the next steps belong to it.
    if let Some((_text, base)) = &setup {
        if let Some(line) = tools_line(&probe(&TOOLS)) {
            writeln!(out, "{line}").ok();
        }
        for line in next_lines(base, start_of(&git, base, born)) {
            writeln!(out, "{line}").ok();
        }
    }
    Ok(())
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
