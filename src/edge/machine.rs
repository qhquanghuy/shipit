#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that probes the machine: the environment, PATH and the tools"
)]

use std::ffi::OsStr;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::pure::machine::{Signal, ToolState};

const POLL: Duration = Duration::from_millis(50);
const POLLS: u32 = 60;
// After a tool has exited, its pipes are read this long for the last of its output.
const AFTER_EXIT: Duration = Duration::from_millis(200);

fn set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

pub(crate) fn signals() -> Vec<Signal> {
    let mut found = Vec::new();
    if set("CI") {
        found.push(Signal::Ci);
    }
    if set("SSH_CONNECTION") || set("SSH_TTY") {
        found.push(Signal::Ssh);
    }
    if set("DISPLAY") {
        found.push(Signal::Display);
    }
    if set("WAYLAND_DISPLAY") {
        found.push(Signal::Wayland);
    }
    found
}

// The first executable file of that name on the search path.
fn find_on(search: &OsStr, tool: &str) -> Option<PathBuf> {
    std::env::split_paths(search)
        .map(|dir| dir.join(tool))
        .find(|candidate| {
            std::fs::metadata(candidate)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

struct Probe {
    tool: String,
    child: Child,
    out: Receiver<Vec<u8>>,
    err: Receiver<Vec<u8>>,
}

fn reader(mut pipe: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).ok();
        sender.send(bytes).ok();
    });
    receiver
}

fn start(tool: &str, program: &Path) -> Result<Probe, ToolState> {
    let mut child = Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|why| {
            if why.kind() == ErrorKind::NotFound {
                ToolState::Missing
            } else {
                ToolState::NoAnswer
            }
        })?;
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(ToolState::NoAnswer);
    };
    Ok(Probe {
        tool: tool.to_owned(),
        child,
        out: reader(out),
        err: reader(err),
    })
}

fn first_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

// What the tool wrote, stdout first; a grandchild that keeps a pipe open is not waited for.
fn answer(probe: &Probe) -> ToolState {
    let read = |pipe: &Receiver<Vec<u8>>| pipe.recv_timeout(AFTER_EXIT).ok();
    let stdout = read(&probe.out).and_then(|bytes| first_line(&bytes));
    let line = stdout.or_else(|| {
        let bytes = read(&probe.err)?;
        first_line(&bytes)
    });
    line.map_or(ToolState::NoAnswer, ToolState::Have)
}

// Every tool is started at once and polled together: at most POLLS * POLL in all, however many
// hang. A hung tool is killed and its output is not waited for.
fn probe_in(search: &OsStr, tools: &[&str]) -> Vec<(String, ToolState)> {
    let mut known: Vec<(String, ToolState)> = Vec::new();
    let mut running: Vec<Probe> = Vec::new();
    for tool in tools {
        match find_on(search, tool) {
            None => known.push(((*tool).to_owned(), ToolState::Missing)),
            Some(program) => match start(tool, &program) {
                Ok(started) => running.push(started),
                Err(state) => known.push(((*tool).to_owned(), state)),
            },
        }
    }
    for _poll in 0..POLLS {
        if running.is_empty() {
            break;
        }
        std::thread::sleep(POLL);
        let mut waiting = Vec::new();
        for mut probe in running {
            match probe.child.try_wait() {
                Ok(Some(_status)) => {
                    let state = answer(&probe);
                    known.push((probe.tool, state));
                }
                Ok(None) => waiting.push(probe),
                Err(_why) => known.push((probe.tool, ToolState::NoAnswer)),
            }
        }
        running = waiting;
    }
    for mut hung in running {
        hung.child.kill().ok();
        hung.child.wait().ok();
        known.push((hung.tool, ToolState::NoAnswer));
    }
    tools
        .iter()
        .filter_map(|tool| known.iter().find(|(name, _state)| name == tool).cloned())
        .collect()
}

pub(crate) fn probe(tools: &[&str]) -> Vec<(String, ToolState)> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    probe_in(&search, tools)
}

#[cfg(test)]
#[expect(clippy::disallowed_methods, reason = "the unit test writes fake tools")]
mod tests {
    use std::fs;
    use std::time::Instant;

    use super::*;
    use crate::edge::testing::scratch;

    fn tool(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("a fake tool");
        let mut mode = fs::metadata(&path).expect("metadata").permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&path, mode).expect("executable");
    }

    #[test]
    fn a_tool_that_hangs_is_no_answer_within_four_seconds() {
        let dir = scratch("probe");
        tool(&dir, "quick", "echo quick 1.0");
        tool(&dir, "onlyerr", "echo err 2.0 >&2");
        tool(&dir, "silent", "exit 0");
        tool(&dir, "hangs", "sleep 8 &\nsleep 8");
        fs::write(dir.join("notexec"), "x").expect("a plain file");
        let started = Instant::now();
        let found = probe_in(
            dir.as_os_str(),
            &["quick", "onlyerr", "silent", "hangs", "notexec", "absent"],
        );
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(
            found,
            vec![
                ("quick".to_owned(), ToolState::Have("quick 1.0".to_owned())),
                ("onlyerr".to_owned(), ToolState::Have("err 2.0".to_owned())),
                ("silent".to_owned(), ToolState::NoAnswer),
                ("hangs".to_owned(), ToolState::NoAnswer),
                ("notexec".to_owned(), ToolState::Missing),
                ("absent".to_owned(), ToolState::Missing),
            ]
        );
        fs::remove_dir_all(&dir).ok();
    }
}
