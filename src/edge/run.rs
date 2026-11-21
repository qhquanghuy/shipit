#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that runs the check commands"
)]

use std::io::{ErrorKind, Write as _};
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

const NOT_FOUND: i32 = 127;
const NOT_STARTED: i32 = 126;
const KILLED: i32 = 128;

// `<prefix…> bash -c <command>` in the repository top, as the shell's `cd` and `"${prefix[@]}"`
// ran it; `$0` is `bash`, the same with or without a prefix.
pub(crate) fn run_check(top: &Path, prefix: &[String], command: &str) -> i32 {
    let mut words = prefix
        .iter()
        .map(String::as_str)
        .chain(["bash", "-c", command]);
    let Some(program) = words.next() else {
        return NOT_STARTED;
    };
    let status = Command::new(program)
        .args(words)
        .current_dir(top)
        .env("PWD", top)
        .stdin(Stdio::null())
        .status();
    match status {
        Ok(status) => status
            .code()
            .or_else(|| status.signal().map(|signal| KILLED + signal))
            .unwrap_or(NOT_STARTED),
        Err(why) => {
            writeln!(std::io::stderr(), "shipit: cannot run {program}: {why}").ok();
            if why.kind() == ErrorKind::NotFound {
                NOT_FOUND
            } else {
                NOT_STARTED
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shipit-run-{}-{name}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("a temp folder");
        dir
    }

    // The output of `command`, which writes to the file `out` in the top.
    fn written(name: &str, command: &str) -> (i32, String) {
        let top = scratch(name);
        let code = run_check(&top, &[], &format!("{command} > out"));
        let text = fs::read_to_string(top.join("out")).unwrap_or_default();
        fs::remove_dir_all(&top).ok();
        (code, text)
    }

    #[test]
    fn a_missing_prefix_program_gives_127() {
        let top = scratch("missing");
        let prefix = ["shipit-no-such-program".to_owned()];
        assert_eq!(run_check(&top, &prefix, "true"), 127);
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn the_exit_code_passes_through() {
        let top = scratch("code");
        assert_eq!(run_check(&top, &[], "exit 7"), 7);
        assert_eq!(run_check(&top, &[], "true"), 0);
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn a_killed_command_gives_128_plus_the_signal() {
        let top = scratch("kill");
        assert_eq!(run_check(&top, &[], "kill -9 $$"), 137);
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn stdin_is_null() {
        let top = scratch("stdin");
        assert_eq!(run_check(&top, &[], "read x"), 1);
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn dollar_zero_is_bash_with_or_without_a_prefix() {
        assert_eq!(written("zero", "echo \"$0\""), (0, "bash\n".to_owned()));
        let top = scratch("zero-prefix");
        let prefix = ["env".to_owned()];
        assert_eq!(run_check(&top, &prefix, "echo \"$0\" > out"), 0);
        assert_eq!(
            fs::read_to_string(top.join("out")).unwrap_or_default(),
            "bash\n"
        );
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn the_working_directory_and_pwd_are_the_top() {
        let top = scratch("pwd");
        let canonical = fs::canonicalize(&top).expect("canonical");
        assert_eq!(
            run_check(&top, &[], "echo \"$PWD\" > out; pwd -P >> out"),
            0
        );
        let text = fs::read_to_string(top.join("out")).unwrap_or_default();
        let expected = format!("{}\n{}\n", top.display(), canonical.display());
        assert_eq!(text, expected);
        fs::remove_dir_all(&top).ok();
    }
}
