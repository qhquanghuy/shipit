#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that writes the files of the project and the machine"
)]

use std::fs::{self, OpenOptions};
use std::io::Write as _;
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
