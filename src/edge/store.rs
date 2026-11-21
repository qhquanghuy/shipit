#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that writes the store"
)]

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::pure::failure::Failure;
use crate::pure::receipt::Oid;

pub(crate) fn check_receipt_path(common_dir: &Path, tree: &Oid) -> PathBuf {
    common_dir
        .join("shipit")
        .join("receipts")
        .join("check")
        .join(format!("{tree}.json"))
}

// A stale temporary file from a crashed run is skipped, never reused.
fn create_temporary(dir: &Path) -> std::io::Result<(PathBuf, File)> {
    let pid = std::process::id();
    let mut n = 0_u32;
    loop {
        let path = dir.join(format!(".tmp.{pid}.{n}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(why) if why.kind() == ErrorKind::AlreadyExists => n += 1,
            Err(why) => return Err(why),
        }
    }
}

fn write_then_move(dir: &Path, path: &Path, text: &[u8]) -> std::io::Result<()> {
    let (temporary, mut file) = create_temporary(dir)?;
    let written = file
        .write_all(text)
        .and_then(|()| file.flush())
        .and_then(|()| {
            drop(file);
            fs::rename(&temporary, path)
        });
    if written.is_err() {
        fs::remove_file(&temporary).ok();
    }
    written
}

pub(crate) fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), Failure> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let cannot = || Failure::usage(format!("cannot write in {}", dir.display()));
    let mut text = serde_json::to_vec_pretty(value).map_err(|_why| cannot())?;
    text.push(b'\n');
    fs::create_dir_all(dir).map_err(|_why| cannot())?;
    write_then_move(dir, path, &text).map_err(|_why| cannot())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shipit-store-{}-{name}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("a temp folder");
        dir
    }

    #[test]
    fn the_path_is_below_the_common_dir() {
        let tree = Oid::parse(&"a".repeat(40)).expect("40 hex digits");
        assert_eq!(
            check_receipt_path(Path::new("/r/.git"), &tree),
            PathBuf::from(format!(
                "/r/.git/shipit/receipts/check/{}.json",
                "a".repeat(40)
            ))
        );
    }

    #[test]
    fn the_file_is_pretty_json_with_a_newline_and_nothing_is_left_behind() {
        let dir = scratch("write");
        let path = dir.join("a").join("b").join("x.json");
        write_json_atomic(&path, &json!({"k": 1})).expect("writes");
        assert_eq!(
            fs::read_to_string(&path).expect("reads"),
            "{\n  \"k\": 1\n}\n"
        );
        let left: Vec<_> = fs::read_dir(path.parent().expect("a parent"))
            .expect("lists")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(left, ["x.json"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_stale_temporary_file_is_skipped_not_reused() {
        let dir = scratch("stale");
        let stale = dir.join(format!(".tmp.{}.0", std::process::id()));
        fs::write(&stale, "stale").expect("writes");
        let path = dir.join("x.json");
        write_json_atomic(&path, &json!(1)).expect("writes");
        assert_eq!(fs::read_to_string(&stale).expect("reads"), "stale");
        assert_eq!(fs::read_to_string(&path).expect("reads"), "1\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_folder_that_cannot_be_made_gives_exit_2_with_the_folder() {
        let dir = scratch("blocked");
        let blocker = dir.join("file");
        fs::write(&blocker, "x").expect("writes");
        let path = blocker.join("sub").join("x.json");
        let failure = write_json_atomic(&path, &json!(1)).expect_err("fails");
        assert_eq!(
            failure,
            Failure::usage(format!("cannot write in {}", blocker.join("sub").display()))
        );
        fs::remove_dir_all(&dir).ok();
    }
}
