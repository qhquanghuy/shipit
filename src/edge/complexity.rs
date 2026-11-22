#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that runs the complexity tool"
)]

use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::pure::failure::Failure;
use crate::pure::measure::{Function, csv_fields};

// Columns of `lizard --csv`: the file is the 7th, the cognitive complexity the 12th.
const FILE: usize = 6;
const COGNITIVE: usize = 11;

// The error is the number of the line that could not be read.
fn parse_rows(text: &str) -> Result<Vec<Function>, usize> {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            let row = || -> Option<Function> {
                let fields = csv_fields(line)?;
                let file = fields.get(FILE)?;
                Some(Function {
                    file: file.strip_prefix("./").unwrap_or(file).to_owned(),
                    cognitive: fields.get(COGNITIVE)?.parse().ok()?,
                })
            };
            row().ok_or(index + 1)
        })
        .collect()
}

pub(crate) fn functions(top: &Path, files: &[String]) -> Result<Vec<Function>, Failure> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let output = Command::new("lizard")
        .args(["-Ecognitive", "--csv"])
        .args(files.iter().map(|file| format!("./{file}")))
        .current_dir(top)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|why| Failure::usage(format!("cannot run lizard: {why}")))?;
    match output.status.code() {
        Some(0) => {}
        Some(code) => return Err(Failure::usage(format!("lizard failed (exit {code})"))),
        None => {
            let signal = output.status.signal().unwrap_or_default();
            return Err(Failure::usage(format!(
                "lizard failed (killed by signal {signal})"
            )));
        }
    }
    parse_rows(&String::from_utf8_lossy(&output.stdout))
        .map_err(|line| Failure::usage(format!("cannot read lizard's output: line {line}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_captured_output_with_commas_inside_quotes() {
        let text = "3,2,26,2,3,\"add@1-3@./c.rs\",\"./c.rs\",\"add\",\"add a : i64 , b : i64\",1,3,2\n\
                    27,7,119,1,27,\"tally@1-27@./a.rs\",\"./a.rs\",\"tally\",\"tally values : & [ i64 ]\",1,27,8\n";
        let function = |file: &str, cognitive| Function {
            file: file.to_owned(),
            cognitive,
        };
        assert_eq!(
            parse_rows(text),
            Ok(vec![function("c.rs", 2), function("a.rs", 8)])
        );
        assert_eq!(parse_rows("1,2,3\n"), Err(1));
    }
}
