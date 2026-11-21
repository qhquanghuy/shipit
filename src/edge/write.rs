#![expect(
    clippy::disallowed_types,
    reason = "this is the only module that writes the files of the project and the machine"
)]

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;

use crate::pure::failure::Failure;

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

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the unit tests read back what the module wrote"
)]
mod tests {
    use std::fs;

    use proptest::prelude::*;

    use super::*;
    use crate::edge::testing::scratch;

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
}
