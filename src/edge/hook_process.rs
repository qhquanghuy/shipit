#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that tests for a file, reads CLAUDE_PROJECT_DIR and HOME, makes the store folder and asks whether the input is a terminal for the hooks"
)]

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use crate::edge::input::read_stdin;

// An unreadable path counts as a missing file: the gates then read "not a ShipIt project", and
// a project is also looked for at HEAD.
pub(crate) fn exists(path: &Path) -> bool {
    path.try_exists().unwrap_or(false)
}

// The folder Claude Code started the session in, when it says so.
pub(crate) fn project_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_PROJECT_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

// The store folder of a project, made when a session starts in it.
pub(crate) fn make_folder(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|why| format!("cannot make {}: {why}", path.display()))
}

// The home folder, for a `cd ~/x` that the gate resolves.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

// A hook is handed its event on a pipe. A person who runs one by hand at a terminal would wait
// for input that never ends, so a terminal is told as an unreadable input.
pub(crate) fn read_hook_input(limit: usize) -> Result<Vec<u8>, String> {
    if std::io::stdin().is_terminal() {
        return Err("the input is a terminal, not a hook event".to_owned());
    }
    read_stdin(limit)
}
