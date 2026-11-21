use std::ffi::OsString;
use std::fmt::Write as _;

use crate::pure::failure::Failure;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Invocation<'a> {
    Help { command: Option<&'a str> },
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
        return Ok(Invocation::Help { command: None });
    };
    let Some(name) = first.to_str() else {
        return Err(not_a_command_name(&first.to_string_lossy()));
    };
    match name {
        "help" | "-h" | "--help" => return help_of(rest),
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

fn help_of(rest: &[OsString]) -> Result<Invocation<'_>, Failure> {
    match rest {
        [] => Ok(Invocation::Help { command: None }),
        [command] => command
            .to_str()
            .map(|command| Invocation::Help {
                command: Some(command),
            })
            .ok_or_else(|| not_a_command_name(&command.to_string_lossy())),
        [_, _, ..] => Err(Failure::usage(
            "too many arguments; usage: shipit help [command]",
        )),
    }
}

fn not_a_command_name(name: &str) -> Failure {
    Failure::usage(format!(
        "'{name}' is not a command name; run 'shipit help' to list the commands"
    ))
}

// What a command says about itself: the compiler requires every command file to declare it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Described<'a> {
    pub(crate) usage: &'a str,
    pub(crate) summary: &'a str,
    pub(crate) exits: [&'a str; 4],
    pub(crate) needs_setup: bool,
}

const USAGE_PREFIX: &str = "usage: shipit ";

fn synopsis<'a>(command: &Described<'a>) -> &'a str {
    command
        .usage
        .strip_prefix(USAGE_PREFIX)
        .unwrap_or(command.usage)
}

pub(crate) fn usage(visible: &[Described<'_>]) -> String {
    let mut text = String::from("usage: shipit <command> [args]\n\ncommands:\n");
    if visible.is_empty() {
        text.push_str("  (none yet)\n");
    }
    let width = visible
        .iter()
        .map(|command| synopsis(command).len())
        .max()
        .unwrap_or(0);
    for command in visible {
        writeln!(text, "  {:<width$}  {}", synopsis(command), command.summary).ok();
    }
    text.push_str("\nshipit help <command> shows what a command needs and its exit codes.\n");
    text.push_str("shipit --version prints the version.\n");
    text
}

pub(crate) fn command_help(command: &Described<'_>) -> String {
    let mut text = format!("{}\n{}\n\n", command.usage, command.summary);
    text.push_str(if command.needs_setup {
        "needs a setup file: yes\n"
    } else {
        "needs a setup file: no\n"
    });
    text.push_str("exit codes:\n");
    for (code, meaning) in command.exits.iter().enumerate() {
        if !meaning.is_empty() {
            writeln!(text, "  {code}  {meaning}").ok();
        }
    }
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
        let none = Invocation::Help { command: None };
        assert_eq!(dispatch(&[], &["check"]), Ok(none));
        for form in ["help", "-h", "--help"] {
            let args = os(&[form]);
            let none = Invocation::Help { command: None };
            assert_eq!(dispatch(&args, &["check"]), Ok(none), "{form}");
            let args = os(&[form, "check"]);
            let one = Invocation::Help {
                command: Some("check"),
            };
            assert_eq!(dispatch(&args, &["check"]), Ok(one), "{form} check");
            let args = os(&[form, "a", "b"]);
            assert_eq!(
                dispatch(&args, &["check"]),
                Err(Failure::usage(
                    "too many arguments; usage: shipit help [command]"
                )),
                "{form} a b"
            );
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
    fn an_unlisted_name_is_unknown() {
        let args = os(&["nosuchcommand"]);
        assert_eq!(
            dispatch(&args, &["check"]),
            Err(Failure::usage(
                "unknown command 'nosuchcommand'; run 'shipit help' to list the commands"
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_is_refused_with_replacement_characters() {
        use std::os::unix::ffi::OsStringExt as _;

        let args = vec![OsString::from_vec(vec![b'a', 0xff])];
        assert_eq!(dispatch(&args, &[]), Err(not_a_name("a\u{fffd}")));
    }

    fn described<'a>(usage: &'a str, summary: &'a str) -> Described<'a> {
        Described {
            usage,
            summary,
            exits: ["done", "", "bad", ""],
            needs_setup: true,
        }
    }

    #[test]
    fn usage_lists_each_command_with_its_summary() {
        let list = [
            described("usage: shipit check [--base <name>]", "Checks."),
            described("usage: shipit verify", "Verifies."),
        ];
        assert_eq!(
            usage(&list),
            "usage: shipit <command> [args]\n\ncommands:\n  check [--base <name>]  Checks.\n  verify                 Verifies.\n\nshipit help <command> shows what a command needs and its exit codes.\nshipit --version prints the version.\n"
        );
    }

    #[test]
    fn usage_without_names_says_none_yet() {
        assert_eq!(
            usage(&[]),
            "usage: shipit <command> [args]\n\ncommands:\n  (none yet)\n\nshipit help <command> shows what a command needs and its exit codes.\nshipit --version prints the version.\n"
        );
    }

    #[test]
    fn a_commands_help_says_its_setup_need_and_skips_unused_exit_codes() {
        let mut command = described("usage: shipit check", "Checks.");
        assert_eq!(
            command_help(&command),
            "usage: shipit check\nChecks.\n\nneeds a setup file: yes\nexit codes:\n  0  done\n  2  bad\n"
        );
        command.needs_setup = false;
        assert!(command_help(&command).contains("needs a setup file: no\n"));
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
            name in prop_oneof![
                any::<String>(),
                "[a-z][a-z-]{0,6}[^a-z-][a-z-]{0,3}",
                "[A-Z0-9/.-][a-z/.-]{0,6}",
            ],
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
        fn a_listed_name_runs_with_the_rest_of_the_arguments(
            name in "[a-z][a-z-]{0,12}",
            rest in proptest::collection::vec(".{0,6}", 0..4),
        ) {
            prop_assume!(!SPECIAL.contains(&name.as_str()));
            let mut args = vec![OsString::from(&name)];
            args.extend(rest.iter().map(OsString::from));
            prop_assert_eq!(
                dispatch(&args, &[name.as_str()]),
                Ok(Invocation::Run { name: &name, args: args.get(1..).unwrap_or_default() })
            );
        }
    }
}
