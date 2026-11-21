#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that runs git"
)]

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::pure::failure::Failure;
use crate::pure::receipt::Oid;
use crate::pure::setup::HEAD;

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

// The newline git ends the line with is dropped; the rest is the path as the file system has it,
// whether or not it is UTF-8.
fn path_of(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(
        bytes.strip_suffix(b"\n").unwrap_or(bytes).to_vec(),
    ))
}

fn oid_of(bytes: &[u8]) -> Option<Oid> {
    Oid::parse(&text(bytes))
}

// `<rev><parts…>`, built without turning a rev that is not UTF-8 into text.
fn spec(rev: &OsStr, parts: &[&str]) -> OsString {
    let mut spec = OsString::from(rev);
    for part in parts {
        spec.push(part);
    }
    spec
}

fn peeled(rev: &OsStr, kind: &str) -> OsString {
    spec(rev, &["^{", kind, "}"])
}

fn rev_path(rev: &OsStr, path: &str) -> OsString {
    spec(rev, &[":", path])
}

// Git obeys these before it looks at the folder (it sets some itself for hooks run in a linked
// worktree), so a `Git` for one folder could read another repository's objects, config or
// store. Every git call discovers the repository from its folder instead, and the check
// commands see the same environment, so they look at the repository that gets the receipt.
// The list is `git rev-parse --local-env-vars`, the variables git itself clears when it enters
// a submodule; a test compares it with the installed git.
pub(crate) const LOCAL_ENV: [&str; 15] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

fn git_in(dir: &Path) -> Command {
    let mut command = Command::new("git");
    for variable in LOCAL_ENV {
        command.env_remove(variable);
    }
    command.current_dir(dir);
    command
}

impl Git {
    pub(crate) fn find() -> Result<Self, Failure> {
        Self::at(Path::new("."))
    }

    pub(crate) fn at(dir: &Path) -> Result<Self, Failure> {
        let not_a_repository =
            || Failure::usage("not inside a git repository; run shipit from inside the project");
        let output = git_in(dir)
            .args(["rev-parse", "--show-toplevel"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_why| not_a_repository())?;
        if !output.status.success() {
            return Err(not_a_repository());
        }
        Ok(Self {
            top: path_of(&output.stdout),
        })
    }

    pub(crate) fn top(&self) -> &Path {
        &self.top
    }

    // git's own stderr is discarded; the caller's message says what failed.
    pub(crate) fn output(&self, args: &[&OsStr]) -> Result<Vec<u8>, GitFailed> {
        self.output_fed(args, None)
    }

    // The input is written whole on its own thread while the output is read, so no size of
    // either can fill a pipe and stop both sides; the pipe is closed when the input is done.
    pub(crate) fn output_fed(
        &self,
        args: &[&OsStr],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>, GitFailed> {
        let mut command = git_in(&self.top);
        command
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_why| GitFailed)?;
        let pipe = child.stdin.take();
        let output = match (pipe, input) {
            (Some(mut pipe), Some(bytes)) => std::thread::scope(|scope| {
                scope.spawn(move || {
                    // A git that stops reading early is judged by its exit status.
                    pipe.write_all(bytes).ok();
                });
                child.wait_with_output()
            }),
            (None, _) | (Some(_), None) => child.wait_with_output(),
        }
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
        self.output(&[
            OsStr::new("show"),
            OsStr::new("--end-of-options"),
            &rev_path(rev, path),
        ])
        .ok()
    }

    pub(crate) fn file_exists(&self, rev: &OsStr, path: &str) -> bool {
        self.output(&[
            OsStr::new("cat-file"),
            OsStr::new("-e"),
            OsStr::new("--end-of-options"),
            &rev_path(rev, path),
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
            .map(|bytes| path_of(&bytes))
            .map_err(|_why| {
                Failure::usage("not inside a git repository; run shipit from inside the project")
            })
    }

    pub(crate) fn status_porcelain(&self) -> Result<Vec<u8>, Failure> {
        self.run(&[
            "-c",
            "core.fsmonitor=false",
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=none",
        ])
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
        self.run(&["symbolic-ref", "--quiet", "--short", HEAD])
            .ok()
            .map(|bytes| text(&bytes))
            .filter(|name| !name.is_empty())
    }

    pub(crate) fn is_ancestor(&self, ancestor: &Oid, descendant: &Oid) -> bool {
        self.run(&[
            "merge-base",
            "--is-ancestor",
            "--end-of-options",
            ancestor.as_str(),
            descendant.as_str(),
        ])
        .is_ok()
    }

    pub(crate) fn merge_base_with_head(&self, rev: &OsStr) -> Option<Oid> {
        let bytes = self
            .output(&[
                OsStr::new("merge-base"),
                OsStr::new("--end-of-options"),
                rev,
                OsStr::new(HEAD),
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
                "--ignore-submodules=none",
                "-M",
                "--end-of-options",
                since.as_str(),
                HEAD,
            ])
            .map_err(|_why| {
                Failure::usage(format!("cannot count the changed files since {since}"))
            })?;
        let count = bytes.split(|byte| *byte == 0).count().saturating_sub(1);
        u64::try_from(count)
            .map_err(|_why| Failure::usage(format!("cannot count the changed files since {since}")))
    }

    // Every path a deletion or a rename touches, each once: no rename detection can hide one.
    pub(crate) fn changed_names(&self, since: &Oid) -> Result<Vec<Vec<u8>>, Failure> {
        let bytes = self
            .run(&[
                "diff",
                "-z",
                "--name-only",
                "--ignore-submodules=none",
                "--no-renames",
                "--end-of-options",
                since.as_str(),
                HEAD,
            ])
            .map_err(|_why| {
                Failure::usage(format!("cannot list the changed files since {since}"))
            })?;
        Ok(bytes
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(<[u8]>::to_vec)
            .collect())
    }

    pub(crate) fn changed_paths(&self, before: &Oid, after: &Oid) -> Result<String, Failure> {
        self.run(&[
            "diff",
            "--name-only",
            "--ignore-submodules=none",
            "--end-of-options",
            before.as_str(),
            after.as_str(),
        ])
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|_why| Failure::usage("cannot list the changed paths"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::edge::testing::scratch;

    fn init(dir: &Path) {
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir)
            .status()
            .expect("git runs");
        assert!(status.success());
    }

    #[test]
    fn a_megabyte_of_input_is_fed_whole_while_the_output_is_read() {
        let dir = scratch("git-fed");
        init(&dir);
        let ids: Vec<String> = (0..25_600_u32).map(|n| format!("{n:040x}")).collect();
        let input: Vec<u8> = ids
            .iter()
            .flat_map(|id| format!("{id}\n").into_bytes())
            .collect();
        assert!(input.len() >= 1_048_576);
        let mut expected = String::new();
        for id in &ids {
            expected.push_str(id);
            expected.push_str(" missing\n");
        }
        let git = Git { top: dir.clone() };
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let args = [OsStr::new("cat-file"), OsStr::new("--batch-check")];
            sender.send(git.output_fed(&args, Some(&input))).ok();
        });
        let output = receiver
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("git answered in time")
            .expect("git succeeded");
        assert_eq!(String::from_utf8_lossy(&output), expected);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_git_call_ignores_the_repository_variables_of_the_environment() {
        let command = git_in(Path::new("."));
        for variable in LOCAL_ENV {
            assert!(
                command
                    .get_envs()
                    .any(|(name, value)| name == variable && value.is_none()),
                "{variable}"
            );
        }
    }

    #[test]
    fn the_list_is_the_one_the_installed_git_prints() {
        let output = Command::new("git")
            .args(["rev-parse", "--local-env-vars"])
            .output()
            .expect("git runs");
        let printed = String::from_utf8_lossy(&output.stdout);
        let mut listed: Vec<&str> = printed.lines().collect();
        let mut ours = LOCAL_ENV.to_vec();
        listed.sort_unstable();
        ours.sort_unstable();
        assert_eq!(ours, listed);
    }

    #[test]
    fn at_a_subfolder_gives_the_top() {
        let top = scratch("git-at-sub");
        init(&top);
        let sub = top.join("a").join("b");
        fs::create_dir_all(&sub).expect("a subfolder");
        let git = Git::at(&sub).expect("inside a repository");
        assert_eq!(
            fs::canonicalize(git.top()).expect("canonical"),
            fs::canonicalize(&top).expect("canonical")
        );
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn at_a_folder_outside_a_repository_fails_like_find() {
        let dir = scratch("git-at-outside");
        let failure = Git::at(&dir).expect_err("not a repository");
        assert_eq!(
            failure,
            Failure::usage("not inside a git repository; run shipit from inside the project")
        );
        fs::remove_dir_all(&dir).ok();
    }
}
