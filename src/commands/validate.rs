use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::commands::setup::load_setup_table;
use crate::edge::files::read_text;
use crate::edge::git::Git;
use crate::pure::failure::Failure;
use crate::pure::kinds::{kind, kind_names};
use crate::pure::setup::HEAD;
use crate::pure::template::{Context, validate};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit validate <kind> <file>";
pub(crate) const SUMMARY: &str = "Checks a filled output against its template and lists every problem; it checks form, not truth. Kinds are listed by shipit template.";
pub(crate) const EXITS: [&str; 4] = [
    "valid",
    "the output is not valid",
    "usage, setup or file error",
    "",
];

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    // The setup comes first: a command that needs one stops without it whatever the arguments.
    let git = Git::find()?;
    let (setup, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let [name, file] = args else {
        return Err(Failure::usage(USAGE));
    };
    let name = name.to_string_lossy();
    let spec = kind(&name).ok_or_else(|| {
        Failure::usage(format!("unknown kind \"{name}\"; kinds: {}", kind_names()))
    })?;
    let label = file.to_string_lossy();
    let text = read_text(Path::new(file))?;
    let ctx = Context {
        file: &label,
        max_lines: u64::from(&setup.owner.max_lines),
    };
    match validate(spec, &ctx, &text) {
        Ok(verdict) => {
            write!(out, "valid {name}: {}", verdict.outcome).ok();
            if let Some([must_fix, nice_to_fix, must_answer]) = verdict.counts {
                write!(
                    out,
                    " ({must_fix} must fix, {nice_to_fix} nice to fix, {must_answer} must answer)"
                )
                .ok();
            }
            writeln!(out).ok();
            Ok(())
        }
        Err(problems) => {
            let mut said = format!("{label} is not a valid {name}:");
            for problem in problems {
                said.push_str("\n  ");
                said.push_str(&problem);
            }
            Err(Failure::no(said))
        }
    }
}
