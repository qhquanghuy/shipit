#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that runs git"
)]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::pure::failure::Failure;
use crate::pure::receipt::Oid;

#[derive(Debug)]
pub(crate) struct GitFailed;

#[derive(Debug)]
pub(crate) struct Git {
    top: PathBuf,
}

fn text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_suffix('\n').unwrap_or(&text).to_owned()
}

fn oid_of(bytes: &[u8]) -> Option<Oid> {
    Oid::parse(&text(bytes))
}

// `<rev>^{kind}`, built without turning a rev that is not UTF-8 into text.
fn peeled(rev: &OsStr, kind: &str) -> OsString {
    let mut spec = OsString::from(rev);
    spec.push("^{");
    spec.push(kind);
    spec.push("}");
    spec
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
        Ok(Self {
            top: PathBuf::from(text(&output.stdout)),
        })
    }

    pub(crate) fn top(&self) -> &Path {
        &self.top
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

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, GitFailed> {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        self.output(&args)
    }

    pub(crate) fn show_file(&self, rev: &OsStr, path: &str) -> Option<Vec<u8>> {
        let mut spec = OsString::from(rev);
        spec.push(":");
        spec.push(path);
        self.output(&[OsStr::new("show"), OsStr::new("--end-of-options"), &spec])
            .ok()
    }

    pub(crate) fn file_exists(&self, rev: &OsStr, path: &str) -> bool {
        let mut spec = OsString::from(rev);
        spec.push(":");
        spec.push(path);
        self.output(&[
            OsStr::new("cat-file"),
            OsStr::new("-e"),
            OsStr::new("--end-of-options"),
            &spec,
        ])
        .is_ok()
    }

    pub(crate) fn commit_of(&self, rev: &OsStr) -> Option<Oid> {
        let spec = peeled(rev, "commit");
        let bytes = self
            .output(&[
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new("--quiet"),
                OsStr::new("--end-of-options"),
                &spec,
            ])
            .ok()?;
        oid_of(&bytes)
    }

    pub(crate) fn tree_of(&self, rev: &OsStr) -> Result<Oid, Failure> {
        let spec = peeled(rev, "tree");
        self.output(&[
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new("--end-of-options"),
            &spec,
        ])
        .ok()
        .and_then(|bytes| oid_of(&bytes))
        .ok_or_else(|| Failure::usage(format!("cannot read the tree of {}", rev.to_string_lossy())))
    }

    pub(crate) fn short(&self, oid: &Oid) -> Result<String, Failure> {
        self.run(&["rev-parse", "--short", oid.as_str()])
            .map(|bytes| text(&bytes))
            .map_err(|_why| Failure::usage(format!("cannot shorten {oid}")))
    }

    pub(crate) fn common_dir(&self) -> Result<PathBuf, Failure> {
        self.run(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .map(|bytes| PathBuf::from(text(&bytes)))
            .map_err(|_why| {
                Failure::usage("not inside a git repository; run shipit from inside the project")
            })
    }

    pub(crate) fn status_porcelain(&self) -> Result<Vec<u8>, Failure> {
        self.run(&["status", "--porcelain", "-z", "--untracked-files=normal"])
            .map_err(|_why| Failure::usage("cannot read the git status"))
    }

    pub(crate) fn ls_files_v_z(&self) -> Result<Vec<u8>, Failure> {
        self.run(&["ls-files", "-v", "-z"])
            .map_err(|_why| Failure::usage("cannot list the tracked files"))
    }

    pub(crate) fn sparse_checkout(&self) -> bool {
        self.run(&["config", "--get", "--type=bool", "core.sparseCheckout"])
            .is_ok_and(|bytes| text(&bytes) == "true")
    }

    pub(crate) fn current_branch(&self) -> Option<String> {
        self.run(&["symbolic-ref", "--quiet", "--short", "HEAD"])
            .ok()
            .map(|bytes| text(&bytes))
            .filter(|name| !name.is_empty())
    }

    pub(crate) fn is_ancestor(&self, ancestor: &OsStr, descendant: &OsStr) -> bool {
        self.output(&[
            OsStr::new("merge-base"),
            OsStr::new("--is-ancestor"),
            OsStr::new("--end-of-options"),
            ancestor,
            descendant,
        ])
        .is_ok()
    }

    pub(crate) fn merge_base_with_head(&self, rev: &OsStr) -> Option<Oid> {
        let bytes = self
            .output(&[
                OsStr::new("merge-base"),
                OsStr::new("--end-of-options"),
                rev,
                OsStr::new("HEAD"),
            ])
            .ok()?;
        oid_of(&bytes)
    }

    // Counted from the NUL-separated names, so any file name counts as one; -M makes a rename
    // one file whatever the user's diff.renames says.
    pub(crate) fn changed_files_count(&self, since: &Oid) -> Result<u64, Failure> {
        let bytes = self
            .run(&[
                "diff",
                "-z",
                "--name-only",
                "-M",
                "--end-of-options",
                since.as_str(),
                "HEAD",
            ])
            .map_err(|_why| {
                Failure::usage(format!("cannot count the changed files since {since}"))
            })?;
        let count = bytes.split(|byte| *byte == 0).count().saturating_sub(1);
        u64::try_from(count)
            .map_err(|_why| Failure::usage(format!("cannot count the changed files since {since}")))
    }

    pub(crate) fn changed_paths(&self, before: &Oid, after: &Oid) -> Result<String, Failure> {
        self.run(&[
            "diff",
            "--name-only",
            "--end-of-options",
            before.as_str(),
            after.as_str(),
        ])
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|_why| Failure::usage("cannot list the changed paths"))
    }
}
