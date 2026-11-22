use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::commands::setup::load_setup_table;
use crate::edge::check_env::{branch_name, check_env};
use crate::edge::clock::now_utc;
use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::edge::records::{
    build_plan_path, coverage_record_path, goal_names, load_goal, store_dir,
};
use crate::edge::run::run_words;
use crate::edge::slice_git::{changed_with_status, map_paths_at, refuse_unclean_work};
use crate::edge::store::write_json_atomic;
use crate::pure::baseline::parse_baseline;
use crate::pure::block::toml_block;
use crate::pure::branch::{is_slice_branch, parse_slice_branch};
use crate::pure::coverage_map::{CoverageMap, parse_map};
use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::element_named;
use crate::pure::messages::{NOT_TOML, say};
use crate::pure::repo_path::RepoPath;
use crate::pure::review::tool;
use crate::pure::roadmap::{Element, SliceName};
use crate::pure::rules::invalid;
use crate::pure::setup::{HEAD, SETUP_PATH, toml_table};
use crate::pure::tests_ran::{
    CoverageRecord, MapRule, local_problems, map_path, map_rule, parse_map_path, passed_tests,
    verdict,
};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit coverage -- <program> [args…]";
pub(crate) const SUMMARY: &str = "Runs the test command and confirms that every test the committed done-when map names ran; it runs only as a check command of shipit check, one coverage line must wrap every test run, renaming a test means editing the older map, a test that prints a fake ok line is not caught, and SHIPIT_BRANCH set by hand forges only a local receipt.";
pub(crate) const EXITS: [&str; 4] = [
    "tests passed and coverage confirmed (or the branch is not slice/*)",
    "the test command failed or coverage is incomplete",
    "usage or setup error, or not run by shipit check",
    "uncommitted changes",
];

// The files the base lists, as the slice forked from it; none when that setup cannot be read.
fn setup_files_at(git: &Git, rev: &OsStr) -> Vec<RepoPath> {
    git.show_file(rev, SETUP_PATH)
        .and_then(|bytes| toml_table(&bytes).ok())
        .and_then(|table| parse_baseline(&table).ok())
        .map(|flow| flow.setup_files())
        .unwrap_or_default()
}

const COVERAGE_HEADING: &str = "## Done-when coverage";

fn words(args: &[OsString]) -> Result<(&OsString, &[OsString]), Failure> {
    match args {
        [dashes, program, rest @ ..] if dashes == "--" => Ok((program, rest)),
        _ => Err(Failure::usage(USAGE)),
    }
}

// Every committed map at HEAD, from git and never from the working tree or the store.
fn committed_maps(git: &Git) -> Result<Vec<(GoalName, SliceName, CoverageMap)>, Failure> {
    let mut maps = Vec::new();
    for path in map_paths_at(git, OsStr::new(HEAD))? {
        let (goal, slice) = parse_map_path(&path).ok_or_else(|| {
            Failure::usage(format!(
                "{path} is invalid: a coverage map is .shipit/coverage/<goal>/<slice>.toml"
            ))
        })?;
        let bytes = git
            .show_file(OsStr::new(HEAD), &path)
            .ok_or_else(|| Failure::usage(format!("cannot read {path} at HEAD")))?;
        let table = toml_table(&bytes)
            .map_err(|why| Failure::usage(say(NOT_TOML, &[("file", &path), ("why", &why)])))?;
        let map =
            parse_map(&table).map_err(|problems| Failure::usage(invalid(&path, &problems)))?;
        maps.push((goal, slice, map));
    }
    Ok(maps)
}

// The store's roadmap and build plan, when this machine has them for the branch's slice.
fn local_check(
    git: &Git,
    branch: &str,
    maps: &[(GoalName, SliceName, CoverageMap)],
) -> Result<(), Failure> {
    let Some((goal, slice)) = parse_slice_branch(branch) else {
        return Ok(());
    };
    let common = git.common_dir()?;
    let store = store_dir(&common);
    if !goal_names(&store).contains(&goal) {
        return Ok(());
    }
    let Ok(roadmap) = load_goal(&common, &goal) else {
        return Ok(());
    };
    let Some(Element::Slice(found)) = element_named(&roadmap, &slice) else {
        return Ok(());
    };
    let own = maps
        .iter()
        .find(|(g, s, _)| *g == goal && *s == slice)
        .map(|(_, _, map)| map);
    let empty = CoverageMap {
        items: std::collections::BTreeMap::new(),
    };
    let plan_block = read_text(&build_plan_path(&store, &goal, &slice))
        .ok()
        .and_then(|text| {
            let block = toml_block(&text, COVERAGE_HEADING).ok().flatten()?;
            parse_map(&toml_table(block.as_bytes()).ok()?).ok()
        });
    let problems = local_problems(own.unwrap_or(&empty), &found.meets, plan_block.as_ref());
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Failure::no(problems.join("\n")))
    }
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (_head, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let env = check_env().ok_or_else(|| {
        Failure::usage(
            "shipit coverage runs as a check command of shipit check (SHIPIT_MERGE_BASE is not set)",
        )
    })?;
    let (program, rest) = words(args)?;
    refuse_unclean_work(&git)?;
    let rest: Vec<&OsStr> = rest.iter().map(OsString::as_os_str).collect();
    let (code, kept) = match run_words(git.top(), program, &rest, &[], Some(&mut *out)) {
        Ok(done) => done,
        Err(not_started) => {
            writeln!(err, "shipit: {}", not_started.message).ok();
            (not_started.code, Vec::new())
        }
    };
    if code != 0 {
        return Err(Failure::no(format!(
            "the test command failed (exit {code})"
        )));
    }

    let branch = branch_name(&git);
    let shown = branch.as_deref().unwrap_or(HEAD);
    if !branch.as_deref().is_some_and(is_slice_branch) {
        writeln!(
            err,
            "shipit: {shown} is not a slice/* branch; done-when coverage is not checked"
        )
        .ok();
        return Ok(());
    }

    let changes = changed_with_status(&git, &env.merge_base, OsStr::new(HEAD))?;
    let own = parse_slice_branch(shown);
    let listed = setup_files_at(&git, OsStr::new(env.merge_base.as_str()));
    match map_rule(
        &changes,
        own.as_ref().map(|(goal, slice)| (goal, slice)),
        &listed,
    ) {
        MapRule::NotNeeded | MapRule::Added => {}
        MapRule::Missing => {
            let path = own.as_ref().map_or_else(
                || ".shipit/coverage/<goal>/<slice>.toml".to_owned(),
                |(goal, slice)| map_path(goal, slice),
            );
            return Err(Failure::no(format!(
                "this change adds no done-when map: commit {path} (see shipit help coverage)"
            )));
        }
    }
    let maps = committed_maps(&git)?;
    let now: BTreeSet<String> = maps
        .iter()
        .map(|(goal, slice, _)| map_path(goal, slice))
        .collect();
    for gone in map_paths_at(&git, OsStr::new(env.merge_base.as_str()))? {
        if !now.contains(&gone) {
            writeln!(err, "shipit: coverage: map removed: {gone}").ok();
        }
    }
    let passed = passed_tests(&String::from_utf8_lossy(&kept));
    let covered = verdict(&maps, &passed).map_err(|problems| {
        Failure::no(format!(
            "test coverage is incomplete:\n{}\n(one coverage line must wrap every test run)",
            problems.join("\n")
        ))
    })?;
    local_check(&git, shown, &maps)?;

    let tree = git.tree_of(OsStr::new(HEAD))?;
    let store = store_dir(&git.common_dir()?);
    let record = CoverageRecord::new(tree.to_string(), &env.command, &maps, (now_utc(), tool()));
    write_json_atomic(&coverage_record_path(&store, &tree), &record)?;
    for line in covered.iter().map(crate::pure::tests_ran::Covered::line) {
        writeln!(out, "{line}").ok();
    }
    Ok(())
}
