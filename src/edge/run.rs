#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that runs the check commands"
)]

use std::ffi::OsStr;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use crate::edge::git::LOCAL_ENV;

const NOT_FOUND: i32 = 127;
const NOT_STARTED: i32 = 126;
const KILLED: i32 = 128;

// The command could not be started: the code a shell gives, and what the caller says about it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CannotRun {
    pub(crate) code: i32,
    pub(crate) message: String,
}

fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| KILLED + signal))
        .unwrap_or(NOT_STARTED)
}

// After the program has exited the output is read until it ends, or until this long passes with
// no data at all. A background process that kept the pipe open must not hold the caller up;
// what it writes after that is lost.
const SILENCE_AFTER_EXIT: Duration = Duration::from_millis(200);

// Called for each silent period. The first one that finds the program gone is not the end: the
// program may have written its last lines just before it exited, and they are still on their
// way. Only a whole silent period after the exit stops the reading.
fn stop_reading(seen_exit: &mut bool, exited: bool) -> bool {
    let stop = *seen_exit;
    *seen_exit = exited;
    stop
}

// An interrupted read is read again; any other error is handed on and ends the copy.
fn copy_chunks(mut reader: impl Read, mut send: impl FnMut(std::io::Result<Vec<u8>>) -> bool) {
    let mut chunk = [0_u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                let bytes = chunk.get(..read).unwrap_or_default().to_vec();
                if !send(Ok(bytes)) {
                    break;
                }
            }
            Err(why) if why.kind() == ErrorKind::Interrupted => {}
            Err(why) => {
                send(Err(why));
                break;
            }
        }
    }
}

// With a sink, stdout and stderr share one pipe, so their order is the program's; each chunk is
// written to the sink and kept. Without one both streams are the caller's and nothing is kept.
// The parent's write ends are dropped before reading, or the read would never end.
pub(crate) fn run_words(
    top: &Path,
    program: &OsStr,
    args: &[&OsStr],
    vars: &[(&str, &OsStr)],
    sink: Option<&mut dyn Write>,
) -> Result<(i32, Vec<u8>), CannotRun> {
    let cannot_run = |why: &std::io::Error| CannotRun {
        code: if why.kind() == ErrorKind::NotFound {
            NOT_FOUND
        } else {
            NOT_STARTED
        },
        message: format!("cannot run {}: {why}", program.to_string_lossy()),
    };
    let mut command = Command::new(program);
    for variable in LOCAL_ENV {
        command.env_remove(variable);
    }
    command
        .args(args)
        .current_dir(top)
        .envs(vars.iter().copied())
        .env("PWD", top)
        .stdin(Stdio::null());
    let Some(sink) = sink else {
        let status = command.status().map_err(|why| cannot_run(&why))?;
        return Ok((exit_code(status), Vec::new()));
    };
    let (reader, writer) = std::io::pipe().map_err(|why| cannot_run(&why))?;
    let stderr = writer.try_clone().map_err(|why| cannot_run(&why))?;
    command.stdout(writer).stderr(stderr);
    let mut child = command.spawn().map_err(|why| cannot_run(&why))?;
    drop(command);
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || copy_chunks(reader, |item| sender.send(item).is_ok()));
    let mut kept = Vec::new();
    let mut unread = None;
    let mut exited = None;
    let mut seen_exit = false;
    loop {
        match receiver.recv_timeout(SILENCE_AFTER_EXIT) {
            Ok(Ok(bytes)) => {
                sink.write_all(&bytes).ok();
                sink.flush().ok();
                kept.extend_from_slice(&bytes);
            }
            Ok(Err(why)) => unread = Some(why),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if exited.is_none() {
                    exited = child.try_wait().map_err(|why| cannot_run(&why))?;
                }
                if stop_reading(&mut seen_exit, exited.is_some()) {
                    break;
                }
            }
        }
    }
    let status = match exited {
        Some(status) => status,
        None => child.wait().map_err(|why| cannot_run(&why))?,
    };
    match unread {
        Some(why) => Err(cannot_run(&why)),
        None => Ok((exit_code(status), kept)),
    }
}

// `<prefix…> bash -c <command>` in the repository top, as the shell's `cd` and `"${prefix[@]}"`
// ran it; `$0` is `bash`, the same with or without a prefix.
pub(crate) fn run_check(
    top: &Path,
    prefix: &[String],
    command: &str,
    vars: &[(&str, &OsStr)],
) -> Result<i32, CannotRun> {
    let mut words = prefix
        .iter()
        .map(String::as_str)
        .chain(["bash", "-c", command])
        .map(OsStr::new);
    let program = words.next().unwrap_or(OsStr::new("bash"));
    let args: Vec<&OsStr> = words.collect();
    run_words(top, program, &args, vars, None).map(|(code, _kept)| code)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Instant;

    use proptest::prelude::*;

    use super::*;
    use crate::edge::testing::scratch;

    // The output of `command`, which writes to the file `out` in the top.
    fn written(name: &str, command: &str) -> (Result<i32, CannotRun>, String) {
        let top = scratch(name);
        let result = run_check(&top, &[], &format!("{command} > out"), &[]);
        let text = fs::read_to_string(top.join("out")).unwrap_or_default();
        fs::remove_dir_all(&top).ok();
        (result, text)
    }

    // (exit code, what was kept, what the sink got) of `sh -c <script>` run with a sink.
    fn sunk(name: &str, script: &str, vars: &[(&str, &OsStr)]) -> (i32, Vec<u8>, Vec<u8>) {
        let top = scratch(name);
        let mut sink: Vec<u8> = Vec::new();
        let args = [OsStr::new("-c"), OsStr::new(script)];
        let (code, kept) =
            run_words(&top, OsStr::new("sh"), &args, vars, Some(&mut sink)).expect("sh starts");
        fs::remove_dir_all(&top).ok();
        (code, kept, sink)
    }

    #[test]
    fn a_sink_keeps_both_streams_in_order_and_echoes_them() {
        let (code, kept, sink) = sunk("run-sink", "echo a; echo b >&2; echo c", &[]);
        assert_eq!(code, 0);
        assert_eq!(kept, b"a\nb\nc\n");
        assert_eq!(sink, b"a\nb\nc\n");
    }

    #[test]
    fn a_large_output_through_the_sink_does_not_block() {
        let (code, kept, sink) = sunk("run-large", "head -c 1500000 /dev/zero; exit 3", &[]);
        assert_eq!(code, 3);
        assert_eq!(kept.len(), 1_500_000);
        assert_eq!(sink, kept);
    }

    // Reads the scripted results one by one, then ends.
    struct Script(Vec<std::io::Result<Vec<u8>>>);

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            match self.0.remove(0) {
                Ok(bytes) => {
                    let n = bytes.len().min(buf.len());
                    buf.get_mut(..n)
                        .unwrap_or_default()
                        .copy_from_slice(bytes.get(..n).unwrap_or_default());
                    Ok(n)
                }
                Err(why) => Err(why),
            }
        }
    }

    // A program that is silent for 200 ms, writes its last line and exits just before the tick
    // that asks whether it has exited: that tick finds it gone, and the line is still on its way.
    #[test]
    fn the_silence_that_finds_the_program_gone_does_not_stop_the_reading() {
        let mut seen_exit = false;
        assert!(!stop_reading(&mut seen_exit, false));
        assert!(!stop_reading(&mut seen_exit, true));
        assert!(stop_reading(&mut seen_exit, true));
    }

    #[test]
    fn a_program_that_has_not_exited_never_stops_the_reading() {
        let mut seen_exit = false;
        for _tick in 0..5 {
            assert!(!stop_reading(&mut seen_exit, false));
        }
    }

    #[test]
    fn an_interrupted_read_is_read_again_and_no_byte_is_lost() {
        let reader = Script(vec![
            Ok(b"a".to_vec()),
            Err(ErrorKind::Interrupted.into()),
            Ok(b"b".to_vec()),
        ]);
        let mut got = Vec::new();
        copy_chunks(reader, |item| {
            got.extend(item.expect("no error"));
            true
        });
        assert_eq!(got, b"ab");
    }

    #[test]
    fn any_other_read_error_is_handed_on_and_ends_the_copy() {
        let reader = Script(vec![
            Ok(b"a".to_vec()),
            Err(ErrorKind::PermissionDenied.into()),
            Ok(b"never".to_vec()),
        ]);
        let mut seen = Vec::new();
        copy_chunks(reader, |item| {
            seen.push(item.map_err(|why| why.kind()));
            true
        });
        assert_eq!(seen, [Ok(b"a".to_vec()), Err(ErrorKind::PermissionDenied)]);
    }

    #[test]
    fn a_background_process_holding_the_pipe_does_not_hold_the_caller_up() {
        let top = scratch("run-background");
        let args = [OsStr::new("-c"), OsStr::new("sleep 8 & echo done")];
        let mut sink: Vec<u8> = Vec::new();
        let started = Instant::now();
        let result = run_words(&top, OsStr::new("sh"), &args, &[], Some(&mut sink));
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(result, Ok((0, b"done\n".to_vec())));
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn without_a_sink_nothing_is_kept() {
        let top = scratch("run-no-sink");
        let args = [OsStr::new("-c"), OsStr::new("echo a; exit 5")];
        let result = run_words(&top, OsStr::new("sh"), &args, &[], None);
        assert_eq!(result, Ok((5, Vec::new())));
        fs::remove_dir_all(&top).ok();
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 16,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn the_variables_reach_the_command(value in "[ -~]{0,30}") {
            let (code, kept, _sink) = sunk(
                "run-vars",
                "printf %s \"$SHIPIT_PROBE\"",
                &[("SHIPIT_PROBE", OsStr::new(&value))],
            );
            prop_assert_eq!(code, 0);
            prop_assert_eq!(kept, value.into_bytes());
        }
    }

    #[test]
    fn a_missing_prefix_program_gives_127_and_says_so_to_the_caller() {
        let top = scratch("run-missing");
        let prefix = ["shipit-no-such-program".to_owned()];
        let failure = run_check(&top, &prefix, "true", &[]).expect_err("cannot start");
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
        assert_eq!(run_check(&top, &[], "exit 7", &[]), Ok(7));
        assert_eq!(run_check(&top, &[], "true", &[]), Ok(0));
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn a_killed_command_gives_128_plus_the_signal() {
        let top = scratch("run-kill");
        assert_eq!(run_check(&top, &[], "kill -9 $$", &[]), Ok(137));
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
        assert_eq!(run_check(&top, &prefix, "echo \"$0\" > out", &[]), Ok(0));
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
            run_check(&top, &[], "echo \"$PWD\" > out; pwd -P >> out", &[]),
            Ok(0)
        );
        let text = fs::read_to_string(top.join("out")).unwrap_or_default();
        let expected = format!("{}\n{}\n", top.display(), canonical.display());
        assert_eq!(text, expected);
        fs::remove_dir_all(&top).ok();
    }
}
