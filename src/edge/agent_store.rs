#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that keeps the agent ledger: it reads CLAUDE_PLUGIN_DATA and owns the ledger files and their lock"
)]

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use crate::edge::clock::now_utc;
use crate::pure::agent_ledger::{
    Change, Entry, file_name, is_stale, parse_entry, seconds, utc_text,
};

const LOCK: &str = "lock";
const STARTED: &str = "started";
const NOT_SET: &str = "CLAUDE_PLUGIN_DATA is not set";

// One folder per session below the plugin's data folder, which an update of the plugin keeps.
fn agents_root() -> Result<PathBuf, String> {
    std::env::var_os("CLAUDE_PLUGIN_DATA")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("agents"))
        .ok_or_else(|| NOT_SET.to_owned())
}

fn cannot(what: &str, why: &std::io::Error) -> String {
    format!("{what}: {why}")
}

#[derive(Debug)]
pub(crate) struct Ledger {
    dir: PathBuf,
}

#[derive(Debug)]
pub(crate) struct Locked<'a> {
    ledger: &'a Ledger,
    _lock: File,
}

impl Ledger {
    // Without `create` a session that has no folder has nothing to count, and gets None.
    pub(crate) fn open(session_id: &str, create: bool) -> Result<Option<Self>, String> {
        Self::at(&agents_root()?, session_id, create)
    }

    fn at(root: &Path, session_id: &str, create: bool) -> Result<Option<Self>, String> {
        let dir = root.join(session_id);
        let ledger = Self { dir };
        if ledger.dir.is_dir() {
            return Ok(Some(ledger));
        }
        if !create {
            return Ok(None);
        }
        fs::create_dir_all(&ledger.dir).map_err(|why| cannot("cannot make the ledger", &why))?;
        // The start time is written once, by whichever process makes the folder first.
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(ledger.dir.join(STARTED))
        {
            Ok(mut file) => file
                .write_all(now_utc().as_bytes())
                .map_err(|why| cannot("cannot write the ledger", &why))?,
            Err(why) if why.kind() == ErrorKind::AlreadyExists => {}
            Err(why) => return Err(cannot("cannot write the ledger", &why)),
        }
        Ok(Some(ledger))
    }

    // The kernel releases the lock when the process ends, so a crashed hook leaves none behind.
    pub(crate) fn locked(&self) -> Result<Locked<'_>, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join(LOCK))
            .map_err(|why| cannot("cannot open the ledger lock", &why))?;
        file.lock()
            .map_err(|why| cannot("cannot lock the ledger", &why))?;
        Ok(Locked {
            ledger: self,
            _lock: file,
        })
    }

    pub(crate) fn clear(&self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

impl Locked<'_> {
    pub(crate) fn entries(&self) -> Vec<Entry> {
        let Ok(read) = fs::read_dir(&self.ledger.dir) else {
            return Vec::new();
        };
        read.filter_map(Result::ok)
            .filter_map(|file| {
                let name = file.file_name().into_string().ok()?;
                let text = fs::read_to_string(file.path()).ok()?;
                parse_entry(&name, &text)
            })
            .collect()
    }

    pub(crate) fn apply(&self, changes: &[Change]) -> Result<(), String> {
        for change in changes {
            match change {
                Change::Write(entry) => self.write(entry)?,
                Change::Remove(entry) => {
                    fs::remove_file(self.ledger.dir.join(file_name(entry))).ok();
                }
            }
        }
        Ok(())
    }

    // Written under a temporary name and moved in: no reader sees half an entry.
    fn write(&self, entry: &Entry) -> Result<(), String> {
        let at = match entry {
            Entry::Reserved { at, .. } | Entry::Running { at, .. } => *at,
        };
        let name = file_name(entry);
        let temporary = self
            .ledger
            .dir
            .join(format!(".tmp-{}-{name}", std::process::id()));
        fs::write(&temporary, utc_text(at))
            .and_then(|()| fs::rename(&temporary, self.ledger.dir.join(&name)))
            .map_err(|why| {
                fs::remove_file(&temporary).ok();
                cannot("cannot write the ledger", &why)
            })
    }
}

fn remove_stale_at(root: &Path, own: &str, now: i64) {
    let Ok(read) = fs::read_dir(root) else {
        return;
    };
    for folder in read.filter_map(Result::ok) {
        if folder.file_name().to_str() == Some(own) {
            continue;
        }
        let started = fs::read_to_string(folder.path().join(STARTED)).ok();
        if is_stale(started.as_deref(), now) {
            fs::remove_dir_all(folder.path()).ok();
        }
    }
}

// The ledgers of sessions that ended long ago, which no `startup` of their own will clear.
pub(crate) fn remove_stale(own: &str) -> Result<(), String> {
    let now = seconds(&now_utc()).ok_or("cannot read the clock")?;
    remove_stale_at(&agents_root()?, own, now);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::testing::scratch;

    #[test]
    fn written_entries_are_read_back_and_removed() {
        let root = scratch("ledger-round-trip");
        assert!(Ledger::at(&root, "s1", false).expect("opens").is_none());
        let ledger = Ledger::at(&root, "s1", true)
            .expect("opens")
            .expect("a ledger");
        let held = ledger.locked().expect("locks");
        let reserved = Entry::Reserved {
            tool_use_id: "t1".to_owned(),
            at: 1_700_000_000,
        };
        let running = Entry::Running {
            agent_id: "a1".to_owned(),
            at: 1_700_000_005,
        };
        held.apply(&[
            Change::Write(reserved.clone()),
            Change::Write(running.clone()),
        ])
        .expect("writes");
        let mut found = held.entries();
        found.sort_by_key(file_name);
        assert_eq!(found, [running.clone(), reserved.clone()]);
        held.apply(&[Change::Remove(reserved)]).expect("removes");
        assert_eq!(held.entries(), [running]);
        drop(held);
        ledger.clear();
        assert!(Ledger::at(&root, "s1", false).expect("opens").is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn only_the_ledgers_that_are_old_and_not_the_own_are_removed() {
        let root = scratch("ledger-stale");
        let now = 1_800_000_000;
        for (name, started) in [
            ("old", Some(utc_text(now - 8 * 24 * 3600))),
            ("young", Some(utc_text(now - 3600))),
            ("own", Some(utc_text(now - 9 * 24 * 3600))),
            ("blank", None),
        ] {
            let dir = root.join(name);
            fs::create_dir_all(&dir).expect("makes");
            if let Some(started) = started {
                fs::write(dir.join(STARTED), started).expect("writes");
            }
        }
        remove_stale_at(&root, "own", now);
        let mut left: Vec<String> = fs::read_dir(&root)
            .expect("reads")
            .filter_map(|folder| folder.ok()?.file_name().into_string().ok())
            .collect();
        left.sort();
        assert_eq!(left, ["blank", "own", "young"]);
        fs::remove_dir_all(&root).ok();
    }
}
