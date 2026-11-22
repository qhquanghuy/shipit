#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that runs the clone tool"
)]

use std::fs;
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::pure::failure::Failure;
use crate::pure::measure::{Place, csv_fields};

// Below this many tokens a piece is not a clone: small repeated pieces are idiom, not copying.
pub(crate) const MIN_TOKENS: u32 = 100;

const HEADER: &str = "lines,tokens,occurrences";
const CLONES_FOUND: i32 = 4;

// CPD names a file as it was given or by its absolute path; either way it is made relative to
// the top, and a path outside the top is not a clone of this repository.
fn relative(top: &Path, canonical_top: Option<&Path>, file: &str) -> Option<String> {
    let path = Path::new(file);
    let inside = path
        .strip_prefix(top)
        .ok()
        .or_else(|| path.strip_prefix(canonical_top?).ok())
        .or_else(|| path.is_relative().then_some(path))?;
    Some(
        inside
            .strip_prefix(".")
            .unwrap_or(inside)
            .to_str()?
            .to_owned(),
    )
}

// Rows are `<lines>,<tokens>,<k>` then `<first line>,<file>` for each of the k places. The error
// is the number of the line that could not be read.
fn parse_rows(
    text: &str,
    top: &Path,
    canonical_top: Option<&Path>,
) -> Result<Vec<Vec<Place>>, usize> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(1);
    }
    let mut clones = Vec::new();
    for (index, line) in lines.enumerate() {
        let number = index + 2;
        let row = || -> Option<Vec<Place>> {
            let fields = csv_fields(line)?;
            let (head, rest) = fields.split_at_checked(3)?;
            let [length, _tokens, count] = head else {
                return None;
            };
            let length: u64 = length.parse().ok()?;
            let count: usize = count.parse().ok()?;
            if length == 0 || rest.len() != count.checked_mul(2)? || count == 0 {
                return None;
            }
            rest.as_chunks::<2>()
                .0
                .iter()
                .map(|[first, file]| {
                    let first: u64 = first.parse().ok()?;
                    Some(Place {
                        file: relative(top, canonical_top, file)?,
                        first,
                        last: first.checked_add(length - 1)?,
                    })
                })
                .collect()
        };
        clones.push(row().ok_or(number)?);
    }
    Ok(clones)
}

// PMD's Rust reader stops at some valid files; it skips them and exits 5, still reporting the
// clones of the others. Those files are named here so the caller can tell which matter.
const RECOVERABLE_ERRORS: i32 = 5;

pub(crate) struct Found {
    pub(crate) clones: Vec<Vec<Place>>,
    pub(crate) unreadable: Vec<String>,
}

// None when a line of the errors is not the one for a skipped file with a name.
fn skipped_files(errors: &str, top: &Path, canonical_top: Option<&Path>) -> Option<Vec<String>> {
    errors
        .lines()
        .filter(|line| line.starts_with("[ERROR]"))
        .map(|line| {
            let (_before, rest) = line.split_once("Skipping file: Lexical error in file '")?;
            let (file, _after) = rest.split_once("' at line")?;
            relative(top, canonical_top, file)
        })
        .collect()
}

// The list goes in a temporary file in the store folder, never in the work, which stays as it
// was committed.
pub(crate) fn find_clones(top: &Path, store: &Path, files: &[String]) -> Result<Found, Failure> {
    if files.is_empty() {
        return Ok(Found {
            clones: Vec::new(),
            unreadable: Vec::new(),
        });
    }
    let cannot = |why: std::io::Error| Failure::usage(format!("cannot run pmd: {why}"));
    fs::create_dir_all(store).map_err(cannot)?;
    let list = store.join(format!(".tmp.cpd-list.{}", std::process::id()));
    let mut names = files.join("\n");
    names.push('\n');
    fs::write(&list, names).map_err(cannot)?;
    let ran = Command::new("pmd")
        .args(["cpd", "--language", "rust", "--minimum-tokens"])
        .arg(MIN_TOKENS.to_string())
        .args(["--format", "csv", "--file-list"])
        .arg(&list)
        .current_dir(top)
        .stdin(Stdio::null())
        .output();
    fs::remove_file(&list).ok();
    let output = ran.map_err(cannot)?;
    let failed = |what: String| Failure::usage(format!("pmd cpd failed ({what})"));
    let canonical = top.canonicalize().ok();
    let unreadable = match output.status.code() {
        Some(0 | CLONES_FOUND) => Vec::new(),
        Some(RECOVERABLE_ERRORS) => skipped_files(
            &String::from_utf8_lossy(&output.stderr),
            top,
            canonical.as_deref(),
        )
        .filter(|skipped| !skipped.is_empty())
        .ok_or_else(|| failed(format!("exit {RECOVERABLE_ERRORS}")))?,
        Some(code) => return Err(failed(format!("exit {code}"))),
        None => {
            let signal = output.status.signal().unwrap_or_default();
            return Err(failed(format!("killed by signal {signal}")));
        }
    };
    let clones = parse_rows(
        &String::from_utf8_lossy(&output.stdout),
        top,
        canonical.as_deref(),
    )
    .map_err(|line| Failure::usage(format!("cannot read pmd's output: line {line}")))?;
    Ok(Found { clones, unreadable })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_captured_row_with_absolute_paths_is_made_relative() {
        let text = "lines,tokens,occurrences\n\
                    27,117,2,1,/work/repo/src/a.rs,1,/work/repo/src/b.rs\n";
        let place = |file: &str| Place {
            file: file.to_owned(),
            first: 1,
            last: 27,
        };
        assert_eq!(
            parse_rows(text, Path::new("/work/repo"), None),
            Ok(vec![vec![place("src/a.rs"), place("src/b.rs")]])
        );
    }

    #[test]
    fn skipped_files_are_named_from_the_captured_errors() {
        let errors = "[ERROR] Skipping file: Lexical error in file 'src/pure/base.rs' at line 226, column 71: token recognition error at: '\\'\n\
                      [ERROR] Skipping file: Lexical error in file '/work/repo/b.rs' at line 1, column 2: x\n";
        assert_eq!(
            skipped_files(errors, Path::new("/work/repo"), None),
            Some(vec!["src/pure/base.rs".to_owned(), "b.rs".to_owned()])
        );
        assert_eq!(
            skipped_files("[ERROR] something else\n", Path::new("/work/repo"), None),
            None
        );
        assert_eq!(
            skipped_files("", Path::new("/work/repo"), None),
            Some(Vec::new())
        );
    }

    #[test]
    fn a_relative_path_and_a_bad_row_are_told_apart() {
        let header = "lines,tokens,occurrences\n";
        let top = Path::new("/work/repo");
        assert_eq!(
            parse_rows(&format!("{header}3,9,1,5,./a.rs\n"), top, None),
            Ok(vec![vec![Place {
                file: "a.rs".to_owned(),
                first: 5,
                last: 7,
            }]])
        );
        assert_eq!(
            parse_rows(&format!("{header}3,9,2,5,a.rs\n"), top, None),
            Err(2)
        );
        assert_eq!(
            parse_rows(&format!("{header}3,9,1,5,/else/a.rs\n"), top, None),
            Err(2)
        );
        assert_eq!(parse_rows("no header\n", top, None), Err(1));
        assert_eq!(parse_rows(header, top, None), Ok(Vec::new()));
    }
}
