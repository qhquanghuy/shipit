#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that asks a question at the terminal"
)]

use std::io::{BufRead as _, IsTerminal as _};

// Both streams are the person's: a question on stderr is answered on stdin.
pub(crate) fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

// None when the input ended or could not be read.
pub(crate) fn read_answer() -> Option<String> {
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_read) => Some(line),
    }
}
