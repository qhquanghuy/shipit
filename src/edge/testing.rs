#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "the unit tests of the edge make their own folders"
)]

use std::fs;
use std::path::PathBuf;

// A new empty folder for one test, named by the test.
pub(crate) fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shipit-test-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).expect("a temp folder");
    dir
}
