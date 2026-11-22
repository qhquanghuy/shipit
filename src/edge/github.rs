use std::ffi::OsStr;
use std::path::Path;

use crate::edge::run::run_words;

// The only place that runs `gh`. The issue's address is the last line it prints; the failure is
// that line too, for the message that names the issue that could not be made.
pub(crate) fn create_issue(top: &Path, title: &str, body: &str) -> Result<String, String> {
    let args = [
        OsStr::new("issue"),
        OsStr::new("create"),
        OsStr::new("--title"),
        OsStr::new(title),
        OsStr::new("--body"),
        OsStr::new(body),
    ];
    let mut echoed: Vec<u8> = Vec::new();
    let (code, kept) = run_words(top, OsStr::new("gh"), &args, &[], Some(&mut echoed))
        .map_err(|not_started| not_started.message)?;
    let text = String::from_utf8_lossy(&kept);
    let last = text
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned();
    if code != 0 {
        return Err(format!("gh issue create failed (exit {code}): {last}"));
    }
    if last.is_empty() {
        return Err("gh issue create printed no address".to_owned());
    }
    Ok(last)
}
