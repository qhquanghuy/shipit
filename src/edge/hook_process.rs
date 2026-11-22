#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that tests for a file and reads CLAUDE_PROJECT_DIR for the hooks"
)]

use std::path::{Path, PathBuf};

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
