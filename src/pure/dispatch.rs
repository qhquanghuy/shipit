use std::ffi::OsString;

use crate::pure::failure::Failure;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Invocation<'a> {
    Help,
    Version,
    Run { name: &'a str, args: &'a [OsString] },
}

fn is_command_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c == '-')
}

pub(crate) fn unknown_command(name: &str) -> Failure {
    Failure::usage(format!(
        "unknown command '{name}'; run 'shipit help' to list the commands"
    ))
}

pub(crate) fn dispatch<'a>(
    args: &'a [OsString],
    names: &[&str],
) -> Result<Invocation<'a>, Failure> {
    let Some((first, rest)) = args.split_first() else {
        return Ok(Invocation::Help);
    };
    let Some(name) = first.to_str() else {
        return Err(not_a_command_name(&first.to_string_lossy()));
    };
    match name {
        "help" | "-h" | "--help" => return Ok(Invocation::Help),
        "--version" | "-V" => return Ok(Invocation::Version),
        _ => {}
    }
    if !is_command_name(name) {
        return Err(not_a_command_name(name));
    }
    if names.contains(&name) {
        Ok(Invocation::Run { name, args: rest })
    } else {
        Err(unknown_command(name))
    }
}

fn not_a_command_name(name: &str) -> Failure {
    Failure::usage(format!(
        "'{name}' is not a command name; run 'shipit help' to list the commands"
    ))
}

pub(crate) fn usage(visible: &[&str]) -> String {
    let mut text = String::from("usage: shipit <command> [args]\n\ncommands:\n");
    if visible.is_empty() {
        text.push_str("  (none yet)\n");
    }
    for name in visible {
        text.push_str("  ");
        text.push_str(name);
        text.push('\n');
    }
    text.push_str("\nshipit --version prints the version.\n");
    text
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::os;

    fn not_a_name(name: &str) -> Failure {
        Failure::usage(format!(
            "'{name}' is not a command name; run 'shipit help' to list the commands"
        ))
    }

    #[test]
    fn no_argument_and_the_help_forms_give_help() {
        assert_eq!(dispatch(&[], &["check"]), Ok(Invocation::Help));
        for form in ["help", "-h", "--help"] {
            let args = os(&[form, "ignored"]);
            assert_eq!(dispatch(&args, &["check"]), Ok(Invocation::Help), "{form}");
        }
    }

    #[test]
    fn the_version_forms_give_version() {
        for form in ["--version", "-V"] {
            let args = os(&[form]);
            assert_eq!(dispatch(&args, &[]), Ok(Invocation::Version), "{form}");
        }
    }

    #[test]
    fn a_listed_name_runs_with_the_rest_of_the_arguments() {
        let args = os(&["check", "--base", "main"]);
        assert_eq!(
            dispatch(&args, &["check", "verify"]),
            Ok(Invocation::Run {
                name: "check",
                args: args.get(1..).unwrap_or_default(),
            })
        );
    }

    #[test]
    fn an_unlisted_name_is_unknown() {
        let args = os(&["nosuchcommand"]);
        assert_eq!(
            dispatch(&args, &["check"]),
            Err(Failure::usage(
                "unknown command 'nosuchcommand'; run 'shipit help' to list the commands"
            ))
        );
    }

    #[test]
    fn bad_names_are_refused_before_the_list_is_read() {
        for name in [
            "../bin/shipit",
            "a/b",
            "..",
            "../x",
            "/etc/passwd",
            "UPPER",
            "1abc",
            "-x",
            "",
        ] {
            let args = os(&[name]);
            assert_eq!(
                dispatch(&args, &[name]),
                Err(not_a_name(name)),
                "name '{name}'"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_is_refused_with_replacement_characters() {
        use std::os::unix::ffi::OsStringExt as _;

        let args = vec![OsString::from_vec(vec![b'a', 0xff])];
        assert_eq!(dispatch(&args, &[]), Err(not_a_name("a\u{fffd}")));
    }

    #[test]
    fn usage_lists_the_names() {
        assert_eq!(
            usage(&["check", "verify"]),
            "usage: shipit <command> [args]\n\ncommands:\n  check\n  verify\n\nshipit --version prints the version.\n"
        );
    }

    #[test]
    fn usage_without_names_says_none_yet() {
        assert_eq!(
            usage(&[]),
            "usage: shipit <command> [args]\n\ncommands:\n  (none yet)\n\nshipit --version prints the version.\n"
        );
    }

    fn config() -> ProptestConfig {
        ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    const SPECIAL: [&str; 5] = ["help", "-h", "--help", "-V", "--version"];

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn a_text_that_is_not_a_command_name_is_refused(
            name in prop_oneof![any::<String>(), "[a-z][a-z-]{0,6}[^a-z-][a-z-]{0,3}"],
        ) {
            prop_assume!(!SPECIAL.contains(&name.as_str()));
            prop_assume!(
                !name.starts_with(|c: char| c.is_ascii_lowercase())
                    || name.chars().any(|c| !(c.is_ascii_lowercase() || c == '-'))
            );
            let args = vec![OsString::from(&name)];
            prop_assert_eq!(dispatch(&args, &[name.as_str()]), Err(not_a_name(&name)));
        }

        #[test]
        fn a_listed_name_runs(name in "[a-z][a-z-]{0,12}") {
            prop_assume!(!SPECIAL.contains(&name.as_str()));
            let args = vec![OsString::from(&name)];
            prop_assert_eq!(
                dispatch(&args, &[name.as_str()]),
                Ok(Invocation::Run { name: &name, args: &[] })
            );
        }
    }
}
