#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that reads HOME and the machine notes"
)]

use std::fs;
use std::path::PathBuf;

// `$HOME` only, never `XDG_CONFIG_HOME`: the `@~/.config/...` line must point at the file.
pub(crate) fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn machine_notes_path(home: &std::path::Path) -> PathBuf {
    home.join(".config").join("shipit").join("machine.md")
}

// Any failure reads as no notes: the built-in list and recipe then apply.
pub(crate) fn machine_notes() -> Option<String> {
    fs::read_to_string(machine_notes_path(&home()?)).ok()
}
