use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::check::settle_base;
use crate::commands::setup::load_setup_table;
use crate::edge::clones::find_clones;
use crate::edge::complexity::functions;
use crate::edge::git::Git;
use crate::edge::records::store_dir;
use crate::edge::slice_git::refuse_unclean_work;
use crate::pure::failure::Failure;
use crate::pure::measure::{
    Marked, changed_rust_files, clone_ratio, highest, marked_blocks, touching, tracked_rust_files,
};
use crate::pure::setup::HEAD;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str =
    "usage: shipit measure <clones|clone-ratio|cognitive-complexity|merge-base>";
pub(crate) const SUMMARY: &str = "Measures the files this slice changed since the merge base with the base branch: clones lists the copies that touch them, clone-ratio and cognitive-complexity print one number each, merge-base prints the commit. Only .rs files are measured. PMD cannot read some valid Rust files: such a file that this slice changed is an error naming it, one it did not change is a notice. Clones are near-exact copies only: a copy with one renamed variable, or a piece under 100 tokens, is not found. A wanted clone is wrapped in '// CPD-OFF: <why>' and '// CPD-ON': it is printed in the output and counted as cloned in the ratio. Cognitive complexity is recorded, never gated.";
pub(crate) const EXITS: [&str; 4] = [
    "measured (clones: none touch the slice)",
    "clones touch the slice, or a CPD-OFF marker has no reason or no CPD-ON",
    "usage or setup error (bad arguments, missing or invalid setup file, no shared history with the base, git failed, pmd or lizard could not run or its output could not be read, a changed file that pmd cannot read)",
    "uncommitted changes, or a tracked file hidden from git",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum What {
    Clones,
    CloneRatio,
    CognitiveComplexity,
    MergeBase,
}

fn parse(args: &[OsString]) -> Result<What, Failure> {
    match args {
        [word] if word == "clones" => Ok(What::Clones),
        [word] if word == "clone-ratio" => Ok(What::CloneRatio),
        [word] if word == "cognitive-complexity" => Ok(What::CognitiveComplexity),
        [word] if word == "merge-base" => Ok(What::MergeBase),
        _ => Err(Failure::usage(USAGE)),
    }
}

// The changed files as committed: the tools read the work tree, which is clean, so both agree.
fn texts_at_head(git: &Git, files: &[String]) -> Result<Vec<(String, String)>, Failure> {
    files
        .iter()
        .map(|file| {
            git.show_file(OsStr::new(HEAD), file)
                .map(|bytes| (file.clone(), String::from_utf8_lossy(&bytes).into_owned()))
                .ok_or_else(|| Failure::usage(format!("cannot read {file} at HEAD")))
        })
        .collect()
}

fn marked_in(texts: &[(String, String)]) -> Result<Vec<Marked>, Failure> {
    let mut blocks = Vec::new();
    let mut problems = Vec::new();
    for (file, text) in texts {
        match marked_blocks(file, text) {
            Ok(found) => blocks.extend(found),
            Err(found) => problems.extend(found),
        }
    }
    if problems.is_empty() {
        return Ok(blocks);
    }
    let listed: Vec<String> = problems
        .iter()
        .map(|problem| format!("  {problem}"))
        .collect();
    Err(Failure::no(format!(
        "CPD-OFF markers are broken:\n{}",
        listed.join("\n")
    )))
}

// A changed file that PMD cannot read has clones nobody can find: that is a failure of the tool.
// An unchanged one only loses a possible partner for the changed files: a notice.
fn refuse_unreadable(
    unreadable: &[String],
    changed: &[String],
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let (mine, others): (Vec<&String>, Vec<&String>) =
        unreadable.iter().partition(|file| changed.contains(file));
    for file in others {
        writeln!(
            err,
            "shipit: pmd cannot read {file}; clones with it are not found"
        )
        .ok();
    }
    if mine.is_empty() {
        return Ok(());
    }
    let named: Vec<&str> = mine.iter().map(|file| file.as_str()).collect();
    Err(Failure::usage(format!(
        "pmd cannot read {}; its clones cannot be checked (rewrite what it stops at)",
        named.join(", ")
    )))
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (setup, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let what = parse(args)?;
    refuse_unclean_work(&git)?;
    let (_base, base_ref, _flow) = settle_base(&git, setup.gate.base.clone(), false, err)?;
    let merge_base = git
        .merge_base_with_head(OsStr::new(&base_ref.full()))
        .ok_or_else(|| Failure::usage(format!("no shared history with {base_ref}")))?;
    if what == What::MergeBase {
        writeln!(out, "{merge_base}").ok();
        return Ok(());
    }
    let tracked = tracked_rust_files(&git.ls_files_v_z()?);
    let changed = changed_rust_files(&tracked, &git.changed_names(&merge_base)?);
    let texts = texts_at_head(&git, &changed)?;
    if what == What::CognitiveComplexity {
        let found = functions(git.top(), &changed)?;
        writeln!(out, "{}", highest(&found, &changed)).ok();
        return Ok(());
    }
    let marked = marked_in(&texts)?;
    let found = find_clones(
        git.top(),
        &store_dir(&git.common_dir()?),
        if changed.is_empty() { &[] } else { &tracked },
    )?;
    refuse_unreadable(&found.unreadable, &changed, err)?;
    let found = found.clones;
    if what == What::CloneRatio {
        let lines: Vec<(String, u64)> = texts
            .iter()
            .map(|(file, text)| {
                (
                    file.clone(),
                    u64::try_from(text.lines().count()).unwrap_or(u64::MAX),
                )
            })
            .collect();
        writeln!(out, "{:.2}", clone_ratio(&found, &marked, &lines)).ok();
        return Ok(());
    }
    for block in &marked {
        writeln!(
            out,
            "hidden from the clone check: {}:{}-{}: {}",
            block.file, block.first, block.last, block.why
        )
        .ok();
    }
    let touched = touching(&found, &changed);
    if touched.is_empty() {
        writeln!(out, "clones: none touch the slice").ok();
        return Ok(());
    }
    let listed: Vec<String> = touched
        .iter()
        .map(|clone| {
            let places: Vec<String> = clone
                .iter()
                .map(|place| format!("{}:{}-{}", place.file, place.first, place.last))
                .collect();
            format!("  {}", places.join(", "))
        })
        .collect();
    Err(Failure::no(format!(
        "{} clones touch the slice:\n{}",
        touched.len(),
        listed.join("\n")
    )))
}
