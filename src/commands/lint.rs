use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::setup::load_setup_table;
use crate::edge::git::Git;
use crate::pure::failure::Failure;
use crate::pure::lint::{
    HARNESS_FILE, SCAN_ROOTS, VOCABULARY, harness_globs, lint, model_names, rejected_terms,
};
use crate::pure::setup::HEAD;

pub(crate) const HIDDEN: bool = true;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit lint";
pub(crate) const SUMMARY: &str = "Refuses the rejected terms of docs/vocab.md, a bare plan, model names outside data/packs/model and harness names outside the globs of harness/claude-code.txt, in the files of agents, data and skills as committed at HEAD. A word in a code span or a fence is exempt, and only the model names of the setup are known. The terms are read from HEAD too, so a change that drops one shows in its diff. docs are not scanned.";
pub(crate) const EXITS: [&str; 4] = ["clean", "findings", "usage, setup or vocabulary error", ""];

fn text_at_head(git: &Git, path: &str) -> Option<String> {
    git.show_file(OsStr::new(HEAD), path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

// The files of the scanned folders as committed; an edit that is not committed is not read.
fn committed_files(git: &Git) -> Result<Vec<(String, String)>, Failure> {
    let mut args = vec![
        OsStr::new("ls-tree"),
        OsStr::new("-r"),
        OsStr::new("-z"),
        OsStr::new("--name-only"),
        OsStr::new(HEAD),
        OsStr::new("--"),
    ];
    args.extend(SCAN_ROOTS.iter().map(OsStr::new));
    let listed = git
        .output(&args)
        .map_err(|_why| Failure::usage("cannot list the files at HEAD"))?;
    let mut files = Vec::new();
    for name in listed
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let path = String::from_utf8_lossy(name).into_owned();
        let text = text_at_head(git, &path)
            .ok_or_else(|| Failure::usage(format!("cannot read {path} at HEAD")))?;
        files.push((path, text));
    }
    Ok(files)
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    // The setup comes first: a command that needs one stops without it whatever the arguments.
    let git = Git::find()?;
    let (setup, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    if !args.is_empty() {
        return Err(Failure::usage(USAGE));
    }
    let vocabulary = text_at_head(&git, VOCABULARY)
        .ok_or_else(|| Failure::usage(format!("no {VOCABULARY} at HEAD")))?;
    let rejected = rejected_terms(&vocabulary).ok_or_else(|| {
        Failure::usage(format!("{VOCABULARY} has no \"Not framework terms\" line"))
    })?;
    let globs = text_at_head(&git, HARNESS_FILE)
        .map(|text| harness_globs(&text))
        .unwrap_or_default();
    let files = committed_files(&git)?;
    let found = lint(&files, &rejected, &model_names(&setup), &globs);
    if found.is_empty() {
        writeln!(out, "lint: {} files clean", files.len()).ok();
        return Ok(());
    }
    for hit in &found {
        writeln!(
            err,
            "{}:{}: {}: {}",
            hit.path, hit.line, hit.rule, hit.matched
        )
        .ok();
    }
    Err(Failure::no(format!(
        "lint found {} problems; put names of tools in a code span or reword",
        found.len()
    )))
}
