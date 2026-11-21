use std::ffi::{OsStr, OsString};
use std::io::Write;

use crate::edge::git::Git;
use crate::edge::store::{check_receipt_path, read_json};
use crate::pure::failure::Failure;
use crate::pure::receipt::{Validity, receipt_validity};
use crate::pure::setup::HEAD;
use crate::pure::work::{UNCOMMITTED, hidden_paths, hidden_refusal};

pub(crate) const HIDDEN: bool = false;

const USAGE: &str = "usage: shipit verify [commit]";

// None means HEAD, and then the work must be clean too. Two or more arguments are counted before
// the emptiness of one.
fn parse_args(args: &[OsString]) -> Result<Option<&OsStr>, Failure> {
    match args {
        [] => Ok(None),
        [commit] if commit.is_empty() => Err(Failure::usage(format!("empty commit; {USAGE}"))),
        [commit] => Ok(Some(commit)),
        [_, _, ..] => Err(Failure::usage(format!("too many arguments; {USAGE}"))),
    }
}

fn refuse_unclean_work(git: &Git) -> Result<(), Failure> {
    let hidden = hidden_paths(&git.ls_files_v_z()?);
    if !hidden.is_empty() {
        return Err(Failure::no(hidden_refusal(&hidden)));
    }
    if !git.status_porcelain()?.is_empty() {
        return Err(Failure::no(UNCOMMITTED));
    }
    Ok(())
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<(), Failure> {
    let explicit = parse_args(args)?;
    let git = Git::find()?;
    let target = explicit.unwrap_or(OsStr::new(HEAD));
    if git.commit_of(target).is_none() {
        return Err(Failure::usage(format!(
            "'{}' is not a commit; give a branch, tag or commit",
            target.to_string_lossy()
        )));
    }
    if explicit.is_none() {
        refuse_unclean_work(&git)?;
    }

    let tree = git.tree_of(target)?;
    let path = check_receipt_path(&git.common_dir()?, &tree);
    let docs = read_json(&path);
    let Some(validity) = receipt_validity(docs.as_deref(), &tree) else {
        return Err(Failure::no("no receipt for this content: run shipit check"));
    };
    let short = git.short(&tree)?;
    match validity {
        Validity::Plain => writeln!(out, "valid receipt for {short}").ok(),
        Validity::SetupChange => writeln!(out, "valid receipt for {short} (setup change)").ok(),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::os;

    #[test]
    fn no_argument_means_head() {
        assert_eq!(parse_args(&[]), Ok(None));
    }

    #[test]
    fn an_empty_commit_is_refused() {
        assert_eq!(
            parse_args(&os(&[""])),
            Err(Failure::usage(
                "empty commit; usage: shipit verify [commit]"
            ))
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn two_or_more_arguments_are_too_many(
            words in proptest::collection::vec(".{0,10}", 2..6),
        ) {
            let args: Vec<OsString> = words.iter().map(OsString::from).collect();
            prop_assert_eq!(
                parse_args(&args),
                Err(Failure::usage("too many arguments; usage: shipit verify [commit]"))
            );
        }

        #[test]
        fn one_non_empty_argument_is_always_the_commit(word in ".{1,20}") {
            let args = [OsString::from(&word)];
            prop_assert_eq!(parse_args(&args), Ok(Some(OsStr::new(&word))));
        }
    }
}
