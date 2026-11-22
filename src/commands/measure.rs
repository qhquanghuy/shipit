use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::check::settle_base;
use crate::commands::setup::load_setup_table;
use crate::edge::check_env::check_env;
use crate::edge::clones::find_clones;
use crate::edge::complexity::functions;
use crate::edge::git::Git;
use crate::edge::records::store_dir;
use crate::edge::slice_git::refuse_unclean_work;
use crate::pure::failure::Failure;
use crate::pure::measure::{
    Marked, OFF, changed_rust_files, clone_ratio, highest, marked_blocks, touching,
    tracked_rust_files,
};
use crate::pure::receipt::Oid;
use crate::pure::setup::HEAD;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str =
    "usage: shipit measure <clones|clone-ratio|cognitive-complexity|merge-base>";
// The marker words are spelled in pieces: this file is measured too, and it may not hold them.
pub(crate) const SUMMARY: &str = concat!(
    "Measures the files this slice changed since the merge base with the base branch (inside \
     shipit check, the merge base the check was given): clones lists the copies that touch them, \
     clone-ratio and cognitive-complexity print one number each, merge-base prints the commit. \
     Only .rs files are measured. Clones are near-exact copies only: a copy with one renamed \
     variable, or a piece under 100 tokens, is not found. A wanted clone is wrapped in '// CPD",
    "-OFF: <why>' and '// CPD",
    "-ON', each alone on its line (any other line of a changed file that holds either word is an \
     error): the block is printed in the output and counted as cloned in the ratio. The markers \
     of a file the slice did not change are not read, so a copy of a block it marks off is not \
     found. PMD cannot read some valid Rust files (a string that ends in a backslash, for one): \
     a tracked file it cannot read is an error naming it, and so is a name with a comma, a line \
     break or bytes that are not UTF-8. Cognitive complexity is recorded, never gated."
);
pub(crate) const EXITS: [&str; 4] = [
    "measured (clones: none touch the slice)",
    concat!(
        "clones touch the slice, or a CPD",
        "-OFF marker is broken (no reason, no CPD",
        "-ON, or not alone on its line)"
    ),
    "usage or setup error (bad arguments, missing or invalid setup file, no shared history with the base, git failed, pmd or lizard could not run, its output could not be read, or pmd cannot read a tracked file)",
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
        "{OFF} markers are broken:\n{}",
        listed.join("\n")
    )))
}

// Inside `shipit check` the merge base is the one the check was given, which `--base` may have
// set; by hand it is found the way `check` finds it from the setup.
fn merge_base(
    git: &Git,
    base: &crate::pure::base::BranchName,
    err: &mut dyn Write,
) -> Result<Oid, Failure> {
    if let Some(env) = check_env() {
        return Ok(env.merge_base);
    }
    let (_base, base_ref, _flow) = settle_base(git, base.clone(), false, err)?;
    git.merge_base_with_head(OsStr::new(&base_ref.full()))
        .ok_or_else(|| Failure::usage(format!("no shared history with {base_ref}")))
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
    let merge_base = merge_base(&git, &setup.gate.base, err)?;
    if what == What::MergeBase {
        writeln!(out, "{merge_base}").ok();
        return Ok(());
    }
    let tracked = tracked_rust_files(&git.ls_files_v_z()?).map_err(|names| {
        Failure::usage(format!(
            "cannot measure {}: a .rs file name with a comma, a line break or bytes that are not UTF-8",
            names.join(", ")
        ))
    })?;
    let changed = changed_rust_files(&tracked, &git.changed_names(&merge_base)?);
    if what == What::CognitiveComplexity {
        let found = functions(git.top(), &changed)?;
        writeln!(out, "{}", highest(&found, &changed)).ok();
        return Ok(());
    }
    let texts = texts_at_head(&git, &changed)?;
    let marked = marked_in(&texts)?;
    let found = find_clones(
        git.top(),
        &store_dir(&git.common_dir()?),
        if changed.is_empty() { &[] } else { &tracked },
    )?;
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
    let head = match touched.len() {
        1 => "1 clone touches the slice:".to_owned(),
        count => format!("{count} clones touch the slice:"),
    };
    Err(Failure::no(format!("{head}\n{}", listed.join("\n"))))
}
