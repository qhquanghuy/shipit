#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that writes the files of the project and the machine"
)]

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

use crate::pure::failure::Failure;
use crate::pure::machine::Writability;

fn cannot_write(path: &Path) -> Failure {
    Failure::usage(format!("cannot write {}", path.display()))
}

// Creates the file when it is missing; what is there is never changed, only added to.
pub(crate) fn append_text(path: &Path, text: &str) -> Result<(), Failure> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|_why| cannot_write(path))
}

// The file is made new: one that exists is the caller's refusal (`exists`, exit 1), and a write
// that fails leaves no half-written file.
pub(crate) fn write_new(
    path: &Path,
    text: &str,
    executable: bool,
    exists: &str,
) -> Result<(), Failure> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|_why| cannot_write(path))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    if executable {
        options.mode(0o755);
    }
    let mut file = options.open(path).map_err(|why| {
        if why.kind() == ErrorKind::AlreadyExists {
            Failure::no(exists)
        } else {
            cannot_write(path)
        }
    })?;
    if file.write_all(text.as_bytes()).is_err() {
        drop(file);
        fs::remove_file(path).ok();
        return Err(cannot_write(path));
    }
    Ok(())
}

fn existing(path: &Path) -> Result<Vec<u8>, Failure> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(why) if why.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(_why) => Err(cannot_write(path)),
    }
}

// Whatever the file holds is kept; the last line is ended first, so the block starts on its own.
pub(crate) fn append_block(path: &Path, block: &str) -> Result<(), Failure> {
    let before = existing(path)?;
    let ended = before.is_empty() || before.ends_with(b"\n");
    let text = if ended {
        block.to_owned()
    } else {
        format!("\n{block}")
    };
    append_text(path, &text)
}

// A line is added only when no line of the file equals it.
pub(crate) fn append_line(path: &Path, line: &str) -> Result<bool, Failure> {
    let present = existing(path)?
        .split(|byte| *byte == b'\n')
        .any(|have| have == line.as_bytes());
    if present {
        return Ok(false);
    }
    append_block(path, &format!("{line}\n"))?;
    Ok(true)
}

// Opening for append writes nothing and follows symlinks. A missing file can be created when
// the nearest folder that exists can be written to.
pub(crate) fn writability(path: &Path) -> Writability {
    if fs::metadata(path).is_ok() {
        return if OpenOptions::new().append(true).open(path).is_ok() {
            Writability::Writable
        } else {
            Writability::ReadOnly
        };
    }
    let creatable = path
        .ancestors()
        .skip(1)
        .find_map(|dir| fs::metadata(dir).ok())
        .is_some_and(|meta| meta.is_dir() && !meta.permissions().readonly());
    if creatable {
        Writability::CanCreate
    } else {
        Writability::CannotCreate
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    use proptest::prelude::*;

    use super::*;
    use crate::edge::testing::scratch;

    fn mode(path: &Path, bits: u32) {
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(bits);
        fs::set_permissions(path, permissions).expect("a mode");
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            cases: 16,
            ..ProptestConfig::default()
        })]

        #[test]
        fn appending_creates_then_keeps_what_was_there(
            first in "[a-z\n]{0,10}",
            second in "[a-z\n]{0,10}",
        ) {
            let dir = scratch("append-text");
            let path = dir.join("summary.md");
            prop_assert_eq!(append_text(&path, &first), Ok(()));
            prop_assert_eq!(append_text(&path, &second), Ok(()));
            prop_assert_eq!(fs::read_to_string(&path).ok(), Some(format!("{first}{second}")));
            fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn a_path_that_cannot_be_opened_is_named() {
        let dir = scratch("append-text-missing");
        let path = dir.join("no-folder").join("x");
        assert_eq!(
            append_text(&path, "x"),
            Err(Failure::usage(format!("cannot write {}", path.display())))
        );
        fs::remove_dir_all(&dir).ok();
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            cases: 16,
            ..ProptestConfig::default()
        })]

        #[test]
        fn write_new_never_replaces(first in "[a-z\n]{0,10}", second in "[a-z\n]{0,10}") {
            let dir = scratch("write-new");
            let path = dir.join("deep").join("file");
            prop_assert_eq!(write_new(&path, &first, false, "exists"), Ok(()));
            prop_assert_eq!(
                write_new(&path, &second, true, "it exists"),
                Err(Failure::no("it exists"))
            );
            prop_assert_eq!(fs::read_to_string(&path).ok(), Some(first));
            fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn append_line_only_when_absent(
            lines in proptest::collection::vec("[a-z]{0,4}", 0..4),
            line in "[a-z]{1,4}",
            ended in any::<bool>(),
        ) {
            let dir = scratch("append-line");
            let path = dir.join("file");
            let mut text = lines.join("\n");
            if ended && !lines.is_empty() {
                text.push('\n');
            }
            fs::write(&path, &text).expect("a file");
            let present = text.split('\n').any(|have| have == line);
            prop_assert_eq!(append_line(&path, &line), Ok(!present));
            let after = fs::read_to_string(&path).expect("readable");
            prop_assert!(after.starts_with(&text));
            if present {
                prop_assert_eq!(after, text);
            } else {
                // the last line is ended first, and the new line ends the file
                let added = after.get(text.len()..).unwrap_or_default();
                let wanted = if text.is_empty() || text.ends_with('\n') {
                    format!("{line}\n")
                } else {
                    format!("\n{line}\n")
                };
                prop_assert_eq!(added, wanted);
                prop_assert_eq!(append_line(&path, &line), Ok(false));
            }
            fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn write_new_makes_the_hook_executable() {
        let dir = scratch("write-new-mode");
        let hook = dir.join("hook");
        write_new(&hook, "x", true, "exists").expect("written");
        let plain = dir.join("plain");
        write_new(&plain, "x", false, "exists").expect("written");
        let mode_of = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode();
        assert_eq!(mode_of(&hook) & 0o111, 0o111);
        assert_eq!(mode_of(&plain) & 0o111, 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writability_of_a_file_a_symlink_and_a_missing_file() {
        let dir = scratch("writability");
        let file = dir.join("file");
        fs::write(&file, "x").expect("a file");
        assert_eq!(writability(&file), Writability::Writable);
        assert_eq!(fs::read_to_string(&file).ok(), Some("x".to_owned()));
        let link = dir.join("link");
        symlink(&file, &link).expect("a symlink");
        assert_eq!(writability(&link), Writability::Writable);
        mode(&file, 0o444);
        assert_eq!(writability(&file), Writability::ReadOnly);
        assert_eq!(writability(&link), Writability::ReadOnly);

        assert_eq!(
            writability(&dir.join("deep").join("missing")),
            Writability::CanCreate
        );
        assert_eq!(
            writability(&file.join("under-a-file")),
            Writability::CannotCreate
        );
        let locked = dir.join("locked");
        fs::create_dir(&locked).expect("a folder");
        mode(&locked, 0o555);
        assert_eq!(writability(&locked.join("x")), Writability::CannotCreate);
        mode(&locked, 0o755);
        fs::remove_dir_all(&dir).ok();
    }
}
