use std::ffi::{OsStr, OsString};

use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::rank::{Worker, parse_worker};

// The word after a flag is taken whatever it starts with, but it must be text and not empty.
pub(crate) fn take_value<'a>(
    flag: &str,
    next: Option<&'a OsString>,
    usage: &str,
) -> Result<&'a str, Failure> {
    next.and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Failure::usage(format!("{flag} needs a value; {usage}")))
}

pub(crate) fn once(
    slot: &mut Option<String>,
    flag: &str,
    next: Option<&OsString>,
    usage: &str,
) -> Result<(), Failure> {
    let value = take_value(flag, next, usage)?;
    if slot.replace(value.to_owned()).is_some() {
        return Err(Failure::usage(format!("{flag} given twice; {usage}")));
    }
    Ok(())
}

pub(crate) fn goal_flag(
    slot: &mut Option<GoalName>,
    next: Option<&OsString>,
    usage: &str,
) -> Result<(), Failure> {
    let name = take_value("--goal", next, usage)?;
    let goal = GoalName::parse(name)
        .ok_or_else(|| Failure::usage(format!("--goal '{name}' is not a goal name; {usage}")))?;
    if slot.replace(goal).is_some() {
        return Err(Failure::usage(format!("--goal given twice; {usage}")));
    }
    Ok(())
}

pub(crate) fn worker_flag(flag: &str, text: Option<&str>, usage: &str) -> Result<Worker, Failure> {
    let text = text.ok_or_else(|| Failure::usage(format!("{flag} is required; {usage}")))?;
    parse_worker(text).ok_or_else(|| {
        Failure::usage(format!(
            "{flag} '{text}' is not <model>:<effort> or <model>:<effort>@<tier>; {usage}"
        ))
    })
}

pub(crate) fn unknown_argument(arg: &OsStr, usage: &str) -> Failure {
    Failure::usage(format!(
        "unknown argument '{}'; {usage}",
        arg.to_string_lossy()
    ))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const USAGE: &str = "usage: shipit x";

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_flag_takes_the_next_word_and_only_once(word in ".{1,10}", other in ".{1,10}") {
            let mut slot = None;
            let first = OsString::from(&word);
            prop_assert_eq!(once(&mut slot, "--by", Some(&first), USAGE), Ok(()));
            prop_assert_eq!(slot.as_deref(), Some(word.as_str()));
            let second = OsString::from(&other);
            prop_assert_eq!(
                once(&mut slot, "--by", Some(&second), USAGE),
                Err(Failure::usage(format!("--by given twice; {USAGE}")))
            );
            prop_assert_eq!(
                take_value("--by", None, USAGE),
                Err(Failure::usage(format!("--by needs a value; {USAGE}")))
            );
            prop_assert!(take_value("--by", Some(&OsString::new()), USAGE).is_err());
        }

        #[test]
        fn a_goal_flag_takes_exactly_a_goal_name(word in "[a-zA-Z0-9 -]{1,8}") {
            let mut slot = None;
            let next = OsString::from(&word);
            let result = goal_flag(&mut slot, Some(&next), USAGE);
            prop_assert_eq!(result.is_ok(), GoalName::parse(&word).is_some());
            prop_assert_eq!(slot.is_some(), result.is_ok());
        }
    }
}
