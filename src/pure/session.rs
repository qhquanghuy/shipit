// The name of a session: the project, the goal and, for a slice, the slice, joined by `·`. A
// session answers to that name, so a slice session can write to its lead.

use std::path::Path;

const JOINER: char = '·';

// A letter or digit first, then letters, digits, `.`, `_` and `-`: no part can hold the joiner
// or a space, so a name splits back into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamePart(String);

impl NamePart {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn parse_part(text: &str) -> Result<NamePart, String> {
    let mut chars = text.chars();
    let good = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if good {
        Ok(NamePart(text.to_owned()))
    } else {
        Err(format!(
            "'{text}' is not a name part: use letters, digits, '.', '_' or '-'"
        ))
    }
}

pub(crate) fn session_name(
    project: &NamePart,
    goal: &NamePart,
    slice: Option<&NamePart>,
) -> String {
    let mut parts = vec![project.as_str(), goal.as_str()];
    parts.extend(slice.map(NamePart::as_str));
    parts.join(&JOINER.to_string())
}

// The folder of the repository, from git's common folder: `<project>/.git`, or for a bare
// repository `<project>.git`.
pub(crate) fn project_of(common_dir: &Path) -> Option<String> {
    let name = common_dir.file_name()?.to_str()?;
    if name == ".git" {
        return common_dir
            .parent()?
            .file_name()?
            .to_str()
            .map(str::to_owned);
    }
    Some(name.strip_suffix(".git").unwrap_or(name).to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use proptest::prelude::*;

    use super::*;

    fn part() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9][a-zA-Z0-9._-]{0,8}"
    }

    fn named(text: &str) -> NamePart {
        parse_part(text).expect("a name part")
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn names_join_and_split_back(project in part(), goal in part(), slice in proptest::option::of(part())) {
            let slice_part = slice.as_deref().map(named);
            let name = session_name(&named(&project), &named(&goal), slice_part.as_ref());
            let parts: Vec<&str> = name.split('·').collect();
            let mut expected = vec![project.as_str(), goal.as_str()];
            expected.extend(slice.as_deref());
            prop_assert_eq!(parts, expected);
        }

        #[test]
        fn bad_parts_refused(
            head in "[a-z0-9]{0,3}",
            bad in prop_oneof![Just('·'), Just(' '), Just('/'), Just('\u{e9}'), Just(':'), Just('\n'), Just('$')],
            tail in "[a-z0-9]{0,3}",
        ) {
            let text = format!("{head}{bad}{tail}");
            prop_assert_eq!(
                parse_part(&text),
                Err(format!("'{text}' is not a name part: use letters, digits, '.', '_' or '-'"))
            );
            for first in ['.', '_', '-'] {
                let text = format!("{first}{head}");
                prop_assert!(parse_part(&text).is_err());
            }
            prop_assert!(parse_part("").is_err());
        }

        #[test]
        fn the_project_is_the_folder_of_the_repository(
            root in "(/[a-z]{1,6}){1,3}",
            name in part(),
        ) {
            prop_assert_eq!(project_of(&PathBuf::from(format!("{root}/{name}/.git"))), Some(name.clone()));
            prop_assert_eq!(project_of(&PathBuf::from(format!("{root}/{name}.git"))), Some(name.clone()));
            prop_assert_eq!(project_of(Path::new("/")), None);
        }
    }

    #[test]
    fn a_repository_called_b_in_a_is_b() {
        assert_eq!(project_of(Path::new("/a/b/.git")), Some("b".to_owned()));
    }
}
