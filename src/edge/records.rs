#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the module that reads and writes the goals, receipts and logbooks of the store"
)]

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::edge::files::read_text;
use crate::pure::block::toml_block;
use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::roadmap_version;
use crate::pure::logbook::{Logged, read_entries};
use crate::pure::messages::{FILE_PROBLEM, NO_BLOCK, NOT_TOML, say};
use crate::pure::receipt::Oid;
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WriteNew {
    Exists,
    Cannot(Failure),
}

// A temporary file in the final folder is hard-linked to the final name, which fails when the
// name is taken: the first receipt is never replaced.
pub(crate) fn write_new(path: &Path, value: &impl Serialize) -> Result<(), WriteNew> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let cannot = || WriteNew::Cannot(Failure::usage(format!("cannot write in {}", dir.display())));
    let mut text = serde_json::to_vec_pretty(value).map_err(|_why| cannot())?;
    text.push(b'\n');
    fs::create_dir_all(dir).map_err(|_why| cannot())?;
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
        .ok_or_else(cannot)?
        .map_err(|_why| cannot())?;
    let written = file.write_all(&text).and_then(|()| file.flush());
    drop(file);
    let linked = written.and_then(|()| fs::hard_link(&temporary, path));
    fs::remove_file(&temporary).ok();
    match linked {
        Ok(()) => Ok(()),
        Err(why) if why.kind() == ErrorKind::AlreadyExists => Err(WriteNew::Exists),
        Err(_why) => Err(cannot()),
    }
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
}
