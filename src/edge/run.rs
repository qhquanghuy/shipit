#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that runs the check commands"
)]

use std::io::ErrorKind;
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

const NOT_FOUND: i32 = 127;
const NOT_STARTED: i32 = 126;
const KILLED: i32 = 128;

// The command could not be started: the code a shell gives, and what the caller says about it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CannotRun {
    pub(crate) code: i32,
    pub(crate) message: String,
}

// `<prefix…> bash -c <command>` in the repository top, as the shell's `cd` and `"${prefix[@]}"`
// ran it; `$0` is `bash`, the same with or without a prefix.
pub(crate) fn run_check(top: &Path, prefix: &[String], command: &str) -> Result<i32, CannotRun> {
    let mut words = prefix
        .iter()
        .map(String::as_str)
        .chain(["bash", "-c", command]);
    let Some(program) = words.next() else {
        return Err(CannotRun {
            code: NOT_STARTED,
            message: "cannot run: no program".to_owned(),
        });
    };
    let status = Command::new(program)
        .args(words)
        .current_dir(top)
        .env("PWD", top)
        .stdin(Stdio::null())
        .status()
        .map_err(|why| CannotRun {
            code: if why.kind() == ErrorKind::NotFound {
                NOT_FOUND
            } else {
                NOT_STARTED
            },
            message: format!("cannot run {program}: {why}"),
        })?;
    Ok(status
        .code()
        .or_else(|| status.signal().map(|signal| KILLED + signal))
        .unwrap_or(NOT_STARTED))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::edge::testing::scratch;

    // The output of `command`, which writes to the file `out` in the top.
    fn written(name: &str, command: &str) -> (Result<i32, CannotRun>, String) {
        let top = scratch(name);
        let result = run_check(&top, &[], &format!("{command} > out"));
        let text = fs::read_to_string(top.join("out")).unwrap_or_default();
        fs::remove_dir_all(&top).ok();
        (result, text)
    }

    #[test]
    fn a_missing_prefix_program_gives_127_and_says_so_to_the_caller() {
        let top = scratch("run-missing");
        let prefix = ["shipit-no-such-program".to_owned()];
        let failure = run_check(&top, &prefix, "true").expect_err("cannot start");
        assert_eq!(failure.code, 127);
        assert!(
            failure
                .message
                .starts_with("cannot run shipit-no-such-program: "),
            "{}",
            failure.message
        );
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn the_exit_code_passes_through() {
        let top = scratch("run-code");
        assert_eq!(run_check(&top, &[], "exit 7"), Ok(7));
        assert_eq!(run_check(&top, &[], "true"), Ok(0));
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn a_killed_command_gives_128_plus_the_signal() {
        let top = scratch("run-kill");
        assert_eq!(run_check(&top, &[], "kill -9 $$"), Ok(137));
        fs::remove_dir_all(&top).ok();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stdin_is_dev_null() {
        assert_eq!(
            written("run-stdin", "readlink /proc/self/fd/0"),
            (Ok(0), "/dev/null\n".to_owned())
        );
    }

    #[test]
    fn dollar_zero_is_bash_with_or_without_a_prefix() {
        assert_eq!(
            written("run-zero", "echo \"$0\""),
            (Ok(0), "bash\n".to_owned())
        );
        let top = scratch("run-zero-prefix");
        let prefix = ["env".to_owned()];
        assert_eq!(run_check(&top, &prefix, "echo \"$0\" > out"), Ok(0));
        assert_eq!(
            fs::read_to_string(top.join("out")).unwrap_or_default(),
            "bash\n"
        );
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn the_working_directory_and_pwd_are_the_top() {
        let top = scratch("run-pwd");
        let canonical = fs::canonicalize(&top).expect("canonical");
        assert_eq!(
            run_check(&top, &[], "echo \"$PWD\" > out; pwd -P >> out"),
            Ok(0)
        );
        let text = fs::read_to_string(top.join("out")).unwrap_or_default();
        let expected = format!("{}\n{}\n", top.display(), canonical.display());
        assert_eq!(text, expected);
        fs::remove_dir_all(&top).ok();
    }
}
