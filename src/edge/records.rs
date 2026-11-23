#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the module that reads and writes the goals, receipts and logbooks of the store"
)]

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::edge::files::read_text;
use crate::pure::block::toml_block;
use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::roadmap_version;
use crate::pure::logbook::{Logged, read_entries};
use crate::pure::messages::{FILE_PROBLEM, NO_BLOCK, NOT_TOML, say};
use crate::pure::metric::{KeptMetrics, MetricName, parse_kept};
use crate::pure::receipt::{Oid, json_values};
use crate::pure::review::ReceiptKind;
use crate::pure::roadmap::{Roadmap, SliceName, parse_roadmap};
use crate::pure::rules::invalid;
use crate::pure::setup::toml_table;

const SLICES: &str = "## Slices";

pub(crate) fn store_dir(common: &Path) -> PathBuf {
    common.join("shipit")
}

pub(crate) fn goal_dir(store: &Path, goal: &GoalName) -> PathBuf {
    store.join("goals").join(goal.to_string())
}

pub(crate) fn logbook_path(store: &Path, goal: &GoalName) -> PathBuf {
    goal_dir(store, goal).join("logbook.jsonl")
}

pub(crate) fn coverage_record_path(store: &Path, tree: &Oid) -> PathBuf {
    store
        .join("receipts")
        .join("coverage")
        .join(format!("{tree}.json"))
}

pub(crate) fn metric_record_path(store: &Path, tree: &Oid, name: &MetricName) -> PathBuf {
    store
        .join("receipts")
        .join("metric")
        .join(tree.to_string())
        .join(format!("{name}.json"))
}

// Every metric record of the store; one that cannot be read is skipped.
pub(crate) fn metric_records(store: &Path) -> Vec<Value> {
    let Ok(trees) = fs::read_dir(store.join("receipts").join("metric")) else {
        return Vec::new();
    };
    trees
        .filter_map(Result::ok)
        .filter_map(|tree| fs::read_dir(tree.path()).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|file| json_values(&fs::read(file.path()).ok()?))
        .filter_map(|docs| match <[Value; 1]>::try_from(docs) {
            Ok([doc]) => Some(doc),
            Err(_several) => None,
        })
        .collect()
}

// The kept metrics files: the goal each is named for, and its text when it can be read.
pub(crate) fn kept_files(store: &Path) -> Vec<(GoalName, Option<String>)> {
    let Ok(files) = fs::read_dir(store.join("metrics")) else {
        return Vec::new();
    };
    let mut kept: Vec<(GoalName, Option<String>)> = files
        .filter_map(Result::ok)
        .filter_map(|file| {
            let name = file.file_name().into_string().ok()?;
            let goal = GoalName::parse(name.strip_suffix(".toml")?)?;
            Some((goal, fs::read_to_string(file.path()).ok()))
        })
        .collect();
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    kept
}

// The kept metrics of the completed goals, and the goals whose file is not a metrics file.
pub(crate) fn kept_metrics(store: &Path) -> (Vec<KeptMetrics>, Vec<GoalName>) {
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for (goal, text) in kept_files(store) {
        let parsed = text
            .and_then(|text| toml_table(text.as_bytes()).ok())
            .and_then(|table| parse_kept(goal.clone(), &table));
        match parsed {
            Some(found) => kept.push(found),
            None => skipped.push(goal),
        }
    }
    (kept, skipped)
}

// The receipt file of a kind and a key; None when it is missing or is not one document.
pub(crate) fn receipt_doc(store: &Path, kind: &str, key: &str) -> Option<Value> {
    let docs = json_values(
        &fs::read(receipt_path(
            store,
            ReceiptKind::parse(kind)?,
            &Oid::parse(key)?,
        ))
        .ok()?,
    )?;
    <[Value; 1]>::try_from(docs).ok().map(|[doc]| doc)
}

pub(crate) fn metrics_path(store: &Path, goal: &GoalName) -> PathBuf {
    store.join("metrics").join(format!("{goal}.toml"))
}

pub(crate) fn exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

pub(crate) fn build_plan_path(store: &Path, goal: &GoalName, slice: &SliceName) -> PathBuf {
    goal_dir(store, goal)
        .join("slices")
        .join(slice.to_string())
        .join("build-plan.md")
}

// Reviews of work are keyed by the patch id and kept apart by kind; the others by their own key.
pub(crate) fn receipt_path(store: &Path, kind: ReceiptKind, key: &Oid) -> PathBuf {
    let receipts = store.join("receipts");
    match kind {
        ReceiptKind::PlanReview | ReceiptKind::TestDrive => {
            receipts.join(kind.name()).join(format!("{key}.json"))
        }
        ReceiptKind::InformedReview | ReceiptKind::SecondOpinion => receipts
            .join("review")
            .join(key.to_string())
            .join(format!("{}.json", kind.name())),
    }
}

// The folders of `goals/` that are goal names; a missing store has none.
pub(crate) fn goal_names(store: &Path) -> Vec<GoalName> {
    let Ok(entries) = fs::read_dir(store.join("goals")) else {
        return Vec::new();
    };
    let mut names: Vec<GoalName> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| GoalName::parse(entry.file_name().to_str()?))
        .collect();
    names.sort();
    names
}

pub(crate) fn newest_roadmap(store: &Path, goal: &GoalName) -> Result<PathBuf, Failure> {
    let dir = goal_dir(store, goal);
    if fs::metadata(&dir).is_err() {
        return Err(Failure::usage(format!(
            "no goal named \"{goal}\" in the store"
        )));
    }
    let newest = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| roadmap_version(entry.file_name().to_str()?))
        .max();
    newest
        .map(|version| dir.join(format!("roadmap.v{version}.md")))
        .ok_or_else(|| Failure::usage(format!("no roadmap in {}", dir.display())))
}

// The newest `roadmap.vN.md` of the goal, which must name the goal it is filed under.
pub(crate) fn load_goal(common: &Path, goal: &GoalName) -> Result<Roadmap, Failure> {
    let path = newest_roadmap(&store_dir(common), goal)?;
    let label = path.display().to_string();
    let text = read_text(&path)?;
    let block = match toml_block(&text, SLICES) {
        Ok(Some(block)) => block,
        Ok(None) => {
            return Err(Failure::usage(say(
                NO_BLOCK,
                &[("file", &label), ("heading", &SLICES)],
            )));
        }
        Err(why) => {
            return Err(Failure::usage(say(
                FILE_PROBLEM,
                &[("file", &label), ("why", &why)],
            )));
        }
    };
    let table = toml_table(block.as_bytes())
        .map_err(|why| Failure::usage(say(NOT_TOML, &[("file", &label), ("why", &why)])))?;
    let roadmap =
        parse_roadmap(&table).map_err(|problems| Failure::usage(invalid(&label, &problems)))?;
    if roadmap.goal != *goal {
        return Err(Failure::usage(format!(
            "{label}: goal \"{}\" does not match its folder \"{goal}\"",
            roadmap.goal
        )));
    }
    Ok(roadmap)
}

// Every goal of the store with its newest roadmap, by goal name.
pub(crate) fn load_goals(common: &Path) -> Result<Vec<(GoalName, Roadmap)>, Failure> {
    let names = goal_names(&store_dir(common));
    if names.is_empty() {
        return Err(Failure::usage("no goal in the store"));
    }
    names
        .into_iter()
        .map(|goal| load_goal(common, &goal).map(|roadmap| (goal, roadmap)))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WriteNew {
    Exists,
    Cannot(Failure),
}

fn cannot_write_in(dir: &Path) -> Failure {
    Failure::usage(format!("cannot write in {}", dir.display()))
}

// A temporary file in the final folder is hard-linked to the final name, which fails when the
// name is taken, a dangling link included: what is there is never replaced.
fn link_new(path: &Path, bytes: &[u8]) -> Result<(), WriteNew> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let cannot = || WriteNew::Cannot(cannot_write_in(dir));
    let temporary = write_temporary(dir, bytes).map_err(|_why| cannot())?;
    let linked = fs::hard_link(&temporary, path);
    fs::remove_file(&temporary).ok();
    match linked {
        Ok(()) => Ok(()),
        Err(why) if why.kind() == ErrorKind::AlreadyExists => Err(WriteNew::Exists),
        Err(_why) => Err(cannot()),
    }
}

pub(crate) fn write_new(path: &Path, value: &impl Serialize) -> Result<(), WriteNew> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut text =
        serde_json::to_vec_pretty(value).map_err(|_why| WriteNew::Cannot(cannot_write_in(dir)))?;
    text.push(b'\n');
    link_new(path, &text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Put {
    New,
    Replace,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Written {
    Stored(PathBuf),
    Replaced(PathBuf),
}

// One level of the way into the store: a real folder, made when missing. `create_dir` never
// follows a link at the new name, and `create_dir_all` would.
fn level_is_folder(level: &Path) -> Result<(), Failure> {
    match fs::symlink_metadata(level) {
        Ok(found) if found.file_type().is_symlink() => Err(link_refused(level)),
        Ok(found) if found.is_dir() => Ok(()),
        Ok(_other) => Err(cannot_write_in(level)),
        Err(why) if why.kind() == ErrorKind::NotFound => {
            fs::create_dir(level).map_err(|_why| cannot_write_in(level))
        }
        Err(_why) => Err(cannot_write_in(level)),
    }
}

fn link_refused(path: &Path) -> Failure {
    Failure::usage(format!(
        "{} is a symbolic link; the store holds no links",
        path.display()
    ))
}

// The text goes at `parts` below the store, every level a real folder from the store down. A new
// file is never put over a name; a replacing put swaps the whole file with a rename.
pub(crate) fn put_text(
    store: &Path,
    parts: &[String],
    text: &str,
    how: Put,
) -> Result<Written, Failure> {
    let (name, folders) = parts
        .split_last()
        .ok_or_else(|| Failure::usage("nothing to put"))?;
    let mut dir = store.to_path_buf();
    level_is_folder(&dir)?;
    for part in folders {
        dir.push(part);
        level_is_folder(&dir)?;
    }
    let path = dir.join(name);
    let taken = match fs::symlink_metadata(&path) {
        Ok(found) if found.file_type().is_symlink() => return Err(link_refused(&path)),
        Ok(_other) => true,
        Err(why) if why.kind() == ErrorKind::NotFound => false,
        Err(_why) => return Err(cannot_write_in(&dir)),
    };
    match (how, taken) {
        (Put::Replace, true) => {
            let temporary =
                write_temporary(&dir, text.as_bytes()).map_err(|_why| cannot_write_in(&dir))?;
            fs::rename(&temporary, &path).map_err(|_why| {
                fs::remove_file(&temporary).ok();
                cannot_write_in(&dir)
            })?;
            Ok(Written::Replaced(path))
        }
        (Put::New | Put::Replace, _) => match link_new(&path, text.as_bytes()) {
            Ok(()) => Ok(Written::Stored(path)),
            Err(WriteNew::Exists) => Err(Failure::no(format!(
                "{} exists; a store file is never replaced",
                path.display()
            ))),
            Err(WriteNew::Cannot(failure)) => Err(failure),
        },
    }
}

// The text under a name no other writer uses, in the folder it is meant for; a stale file of a
// crashed run is skipped, never reused.
fn write_temporary(dir: &Path, text: &[u8]) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let (temporary, mut file) = (0_u32..)
        .find_map(|n| {
            let temporary = dir.join(format!(".tmp.{}.{n}", std::process::id()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => Some(Ok((temporary, file))),
                Err(why) if why.kind() == ErrorKind::AlreadyExists => None,
                Err(why) => Some(Err(why)),
            }
        })
        .ok_or_else(|| std::io::Error::from(ErrorKind::AlreadyExists))??;
    let written = file.write_all(text).and_then(|()| file.flush());
    drop(file);
    if let Err(why) = written {
        fs::remove_file(&temporary).ok();
        return Err(why);
    }
    Ok(temporary)
}

// A reader sees the old text or the whole new text.
pub(crate) fn write_text_atomic(path: &Path, text: &str) -> Result<(), Failure> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let cannot = || Failure::usage(format!("cannot write in {}", dir.display()));
    let temporary = write_temporary(dir, text.as_bytes()).map_err(|_why| cannot())?;
    fs::rename(&temporary, path).map_err(|_why| {
        fs::remove_file(&temporary).ok();
        cannot()
    })
}

pub(crate) fn remove_goal(store: &Path, goal: &GoalName) -> Result<(), Failure> {
    let dir = goal_dir(store, goal);
    fs::remove_dir_all(&dir)
        .map_err(|_why| Failure::usage(format!("cannot delete {}", dir.display())))
}

// A goal without a logbook yet has no entries; a damaged line stops the reader, naming it.
pub(crate) fn read_logbook(store: &Path, goal: &GoalName) -> Result<Vec<Logged>, Failure> {
    let path = logbook_path(store, goal);
    let shown = path.display();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(why) if why.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_why) => return Err(Failure::usage(format!("cannot read {shown}"))),
    };
    let text = String::from_utf8(bytes)
        .map_err(|_why| Failure::usage(format!("logbook {shown} is not UTF-8 text")))?;
    read_entries(&text)
        .map_err(|why| Failure::usage(format!("logbook {shown} {why}; fix that line by hand")))
}

// One write of the whole line, so that lines of parallel writers never interleave.
pub(crate) fn append_line(path: &Path, text: &str) -> Result<(), Failure> {
    let cannot = || Failure::usage(format!("cannot append to {}", path.display()));
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|_why| cannot())?;
    }
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|_why| cannot())?;
    file.write_all(text.as_bytes()).map_err(|_why| cannot())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::edge::testing::scratch;

    #[test]
    fn write_new_never_replaces_a_file() {
        let dir = scratch("write-new");
        let path = dir.join("a").join("x.json");
        assert_eq!(write_new(&path, &1), Ok(()));
        assert_eq!(write_new(&path, &2), Err(WriteNew::Exists));
        assert_eq!(fs::read_to_string(&path).expect("reads"), "1\n");
        let left: Vec<_> = fs::read_dir(path.parent().expect("a folder"))
            .expect("lists")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(left, ["x.json"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_line_keeps_earlier_bytes() {
        let dir = scratch("append");
        let path = dir.join("goals").join("g").join("logbook.jsonl");
        append_line(&path, "one\n").expect("appends");
        append_line(&path, "two\n").expect("appends");
        assert_eq!(fs::read_to_string(&path).expect("reads"), "one\ntwo\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_newest_roadmap_version_is_read_and_a_missing_goal_is_named() {
        let store = scratch("newest");
        let goal = GoalName::parse("demo").expect("a goal name");
        assert_eq!(
            newest_roadmap(&store, &goal),
            Err(Failure::usage("no goal named \"demo\" in the store"))
        );
        let dir = goal_dir(&store, &goal);
        fs::create_dir_all(&dir).expect("makes the goal");
        assert_eq!(goal_names(&store), vec![goal.clone()]);
        assert!(newest_roadmap(&store, &goal).is_err());
        for name in ["roadmap.v2.md", "roadmap.v10.md", "roadmap.v3.txt"] {
            fs::write(dir.join(name), "x").expect("writes");
        }
        assert_eq!(
            newest_roadmap(&store, &goal),
            Ok(dir.join("roadmap.v10.md"))
        );
        fs::remove_dir_all(&store).ok();
    }

    fn parts(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn tmp_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("lists")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with(".tmp."))
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            cases: 16,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_put_never_replaces_a_name_and_leaves_no_temporary_file(
            first in "[a-z ]{0,12}",
            second in "[a-z ]{0,12}",
        ) {
            let store = scratch("put-new");
            let way = parts(&["goals", "g", "slices", "a", "reviews", "round-1", "review.md"]);
            let path = store.join("goals/g/slices/a/reviews/round-1/review.md");
            prop_assert_eq!(
                put_text(&store, &way, &first, Put::New),
                Ok(Written::Stored(path.clone()))
            );
            let taken = Failure::no(format!("{} exists; a store file is never replaced", path.display()));
            prop_assert_eq!(put_text(&store, &way, &second, Put::New), Err(taken));
            prop_assert_eq!(fs::read_to_string(&path).expect("reads"), first);
            let dangling = path.with_file_name("other.md");
            std::os::unix::fs::symlink(store.join("nowhere"), &dangling).expect("links");
            let way = parts(&["goals", "g", "slices", "a", "reviews", "round-1", "other.md"]);
            prop_assert!(put_text(&store, &way, &second, Put::New).is_err());
            prop_assert!(!store.join("nowhere").exists());
            prop_assert!(tmp_files(path.parent().expect("a folder")).is_empty());
            fs::remove_dir_all(&store).ok();
        }

        #[test]
        fn a_replacing_put_swaps_the_whole_file_and_never_follows_a_link(
            first in "[a-z ]{0,12}",
            second in "[a-z ]{0,12}",
        ) {
            let store = scratch("put-replace");
            let way = parts(&["goals", "g", "slices", "a", "build-plan.md"]);
            let path = store.join("goals/g/slices/a/build-plan.md");
            prop_assert_eq!(
                put_text(&store, &way, &first, Put::Replace),
                Ok(Written::Stored(path.clone()))
            );
            prop_assert_eq!(
                put_text(&store, &way, &second, Put::Replace),
                Ok(Written::Replaced(path.clone()))
            );
            prop_assert_eq!(fs::read_to_string(&path).expect("reads"), second);
            prop_assert!(tmp_files(path.parent().expect("a folder")).is_empty());
            let target = store.join("target.txt");
            fs::write(&target, "kept").expect("writes");
            let linked = path.with_file_name("plan-review.md");
            std::os::unix::fs::symlink(&target, &linked).expect("links");
            let way = parts(&["goals", "g", "slices", "a", "plan-review.md"]);
            prop_assert_eq!(
                put_text(&store, &way, &first, Put::Replace),
                Err(link_refused(&linked))
            );
            prop_assert_eq!(fs::read_to_string(&target).expect("reads"), "kept");
            fs::remove_dir_all(&store).ok();
        }
    }

    #[test]
    fn a_link_on_the_way_into_the_store_is_refused() {
        let way = parts(&[
            "goals",
            "g",
            "slices",
            "a",
            "reviews",
            "round-1",
            "review.md",
        ]);
        for at in 0..=6 {
            let root = scratch(&format!("put-link-{at}"));
            let elsewhere = root.join("elsewhere");
            fs::create_dir(&elsewhere).expect("makes");
            let store = root.join("shipit");
            let levels: Vec<PathBuf> = way
                .iter()
                .take(6)
                .scan(store.clone(), |path, name| {
                    path.push(name);
                    Some(path.clone())
                })
                .collect();
            fs::create_dir_all(levels.last().expect("a level")).expect("makes");
            let linked = if at == 0 { &store } else { &levels[at - 1] };
            fs::remove_dir_all(linked).expect("removes");
            std::os::unix::fs::symlink(&elsewhere, linked).expect("links");
            assert_eq!(
                put_text(&store, &way, "x", Put::New),
                Err(link_refused(linked)),
                "level {at}"
            );
            assert_eq!(fs::read_dir(&elsewhere).expect("lists").count(), 0);
            fs::remove_dir_all(&root).ok();
        }
    }
}
