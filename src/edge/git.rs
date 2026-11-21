#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that runs git"
)]

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::pure::failure::Failure;

#[derive(Debug)]
pub(crate) struct GitFailed;

#[derive(Debug)]
pub(crate) struct Git {
    top: PathBuf,
}

impl Git {
    pub(crate) fn find() -> Result<Self, Failure> {
        let not_a_repository =
            || Failure::usage("not inside a git repository; run shipit from inside the project");
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_why| not_a_repository())?;
        if !output.status.success() {
            return Err(not_a_repository());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let top = text.strip_suffix('\n').unwrap_or(&text);
        Ok(Self {
            top: PathBuf::from(top),
        })
    }

    // git's own stderr is discarded; the caller's message says what failed.
    pub(crate) fn output(&self, args: &[&OsStr]) -> Result<Vec<u8>, GitFailed> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.top)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_why| GitFailed)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(GitFailed)
        }
    }

    pub(crate) fn show_file(&self, rev: &OsStr, path: &str) -> Option<Vec<u8>> {
        let mut spec = OsString::from(rev);
        spec.push(":");
        spec.push(path);
        self.output(&[OsStr::new("show"), OsStr::new("--end-of-options"), &spec])
            .ok()
    }
}
