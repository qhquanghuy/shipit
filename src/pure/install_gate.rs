// Commands that install something on the machine itself, which a project's tools must never do:
// the machine notes list them, and the agent gets the notes' recipe instead.

use crate::pure::machine_notes::Notes;
use crate::pure::shell_words::{Parse, parse, program, raw_words_unquoted};
use crate::pure::ship_gate::{INTERPRETERS, runs_as};

// `-S` is also in `-Syu`, but never in `--S…`.
fn matches_word(pattern: &str, word: &str) -> bool {
    if pattern == word {
        return true;
    }
    let mut letters = pattern.strip_prefix('-').unwrap_or_default().chars();
    match (letters.next(), letters.next(), pattern.starts_with('-')) {
        (Some(letter), None, true) if letter.is_ascii_alphanumeric() => word
            .strip_prefix('-')
            .is_some_and(|rest| !rest.starts_with('-') && rest.contains(letter)),
        _ => false,
    }
}

// Some word is the pattern's program, and every other word of the pattern comes after it.
fn in_segment(pattern: &[String], words: &[String]) -> bool {
    let Some((first, others)) = pattern.split_first() else {
        return false;
    };
    words.iter().enumerate().any(|(at, word)| {
        program(word) == first
            && others.iter().all(|other| {
                words
                    .iter()
                    .skip(at + 1)
                    .any(|later| matches_word(other, later))
            })
    })
}

// What cannot be read as commands is judged by its words alone, in order: it may hide the
// pattern, and a gate that cannot tell asks as if it were there.
fn in_raw_words(pattern: &[String], words: &[String]) -> bool {
    let mut remaining = pattern.iter();
    let mut wanted = remaining.next();
    for word in words {
        if wanted.is_some_and(|want| matches_word(want, word)) {
            wanted = remaining.next();
        }
    }
    !pattern.is_empty() && wanted.is_none()
}

fn matched<'a>(command: &str, notes: &'a Notes) -> Option<&'a Vec<String>> {
    let found = |holds: &dyn Fn(&[String]) -> bool| notes.never.0.iter().find(|p| holds(p));
    match parse(command) {
        Parse::Complex => {
            let words = raw_words_unquoted(command);
            found(&|pattern| in_raw_words(pattern, &words))
        }
        Parse::Plain { segments, heredocs } => {
            let direct = found(&|pattern| {
                segments
                    .iter()
                    .any(|segment| in_segment(pattern, &segment.words))
            });
            // A heredoc is text for a file unless a shell or interpreter reads it.
            let read_as_commands = segments.iter().any(|segment| {
                segment
                    .words
                    .iter()
                    .any(|word| runs_as(word, &INTERPRETERS))
            });
            direct.or_else(|| {
                if read_as_commands {
                    heredocs.iter().find_map(|body| matched(body, notes))
                } else {
                    None
                }
            })
        }
    }
}

pub(crate) fn install_refusal(command: &str, notes: &Notes) -> Option<String> {
    let pattern = matched(command, notes)?;
    Some(format!(
        "ShipIt: never install globally on this machine (\"{}\"). {}",
        pattern.join(" "),
        notes.recipe
    ))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::machine_notes::{BUILT_IN_RECIPE, Never, parse_notes};

    fn notes_with(patterns: &[Vec<String>]) -> Notes {
        Notes {
            never: Never(patterns.to_vec()),
            recipe: "Use the dev shell.".to_owned(),
        }
    }

    // The `x` keeps a generated name from being a keyword, `cd` or a wrapper.
    fn name() -> impl Strategy<Value = String> {
        "x[a-z]{1,5}"
    }

    fn pattern() -> impl Strategy<Value = Vec<String>> {
        (name(), proptest::collection::vec(name(), 0..3)).prop_map(|(program, rest)| {
            let mut words = vec![program];
            words.extend(rest);
            words
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn pattern_anywhere_is_refused(
            pattern in pattern(),
            extra in proptest::collection::vec(name(), 0..3),
            wrapper in proptest::sample::select(&["", "sudo -H ", "env X=1 ", "nix develop -c ", "python -m "][..]),
            before in proptest::sample::select(&["", "ls; ", "cd d && "][..]),
        ) {
            let notes = notes_with(std::slice::from_ref(&pattern));
            let mut words = pattern.clone();
            words.extend(extra);
            let command = format!("{before}{wrapper}{}", words.join(" "));
            let refusal = install_refusal(&command, &notes).expect("refused");
            prop_assert_eq!(
                refusal,
                format!("ShipIt: never install globally on this machine (\"{}\"). Use the dev shell.", pattern.join(" "))
            );
        }

        #[test]
        fn text_that_cannot_be_read_is_judged_by_its_words(
            pattern in pattern(),
            between in name(),
            mark in proptest::sample::select(&["$x", "$(id)", "`id`", "{a,b}", "~/x"][..]),
        ) {
            let notes = notes_with(std::slice::from_ref(&pattern));
            let spread = pattern.join(&format!(" {between} "));
            let command = format!("echo {mark} && {spread}");
            prop_assert!(install_refusal(&command, &notes).is_some());
            let reversed: Vec<String> = pattern.iter().rev().cloned().collect();
            let backwards = format!("echo {mark} && {}", reversed.join(" "));
            let wanted = in_raw_words(&pattern, &raw_words_unquoted(&backwards));
            prop_assert_eq!(install_refusal(&backwards, &notes).is_some(), wanted);
        }

        #[test]
        fn flag_clusters_match(rest in "[a-z]{0,3}", target in name()) {
            let notes = notes_with(&[vec!["pacman".to_owned(), "-S".to_owned()]]);
            let cluster = format!("pacman -S{rest} {target}");
            prop_assert!(install_refusal(&cluster, &notes).is_some());
            let before = format!("pacman -x{rest}S {target}");
            prop_assert!(install_refusal(&before, &notes).is_some());
            let long = format!("pacman --S{rest} {target}");
            prop_assert!(install_refusal(&long, &notes).is_none());
            let query = format!("pacman -Q{rest} {target}");
            prop_assert!(install_refusal(&query, &notes).is_none());
        }

        #[test]
        fn heredoc_bodies_are_data(
            pattern in pattern(),
            lines in proptest::collection::vec("[a-z ]{0,10}", 0..3),
            program in proptest::sample::select(&["bash", "sh", "/bin/bash", "python3"][..]),
        ) {
            let notes = notes_with(std::slice::from_ref(&pattern));
            let body = format!("{}\n{}", lines.join("\n"), pattern.join(" "));
            prop_assume!(lines.iter().all(|line| line != "EOF"));
            let as_file = format!("cat > notes.md <<'EOF'\n{body}\nEOF");
            prop_assert_eq!(install_refusal(&as_file, &notes), None);
            let read_by_shell = format!("{program} <<'EOF'\n{body}\nEOF");
            prop_assert!(install_refusal(&read_by_shell, &notes).is_some());
            let piped = format!("cat <<'EOF' | sh\n{body}\nEOF");
            prop_assert!(install_refusal(&piped, &notes).is_some());
        }

        #[test]
        fn quoted_text_never_installs(
            pattern in pattern().prop_filter("a pattern of a program and an argument", |p| p.len() > 1),
            quote in proptest::sample::select(&["'", "\""][..])) {
            let notes = notes_with(std::slice::from_ref(&pattern));
            let text = pattern.join(" ");
            for command in [
                format!("git commit -m {quote}{text}{quote}"),
                format!("echo {quote}{text}{quote}"),
                format!("git commit -m \"$(cat <<'EOF'\nuse {text} here\nEOF\n)\""),
            ] {
                prop_assert_eq!(install_refusal(&command, &notes), None, "{}", command);
            }
        }

        #[test]
        fn other_programs_pass(words in proptest::collection::vec(name(), 0..4)) {
            let notes = parse_notes(None);
            for command in [
                "cargo build".to_owned(),
                "cargo test --all-targets".to_owned(),
                "nix develop -c cargo test".to_owned(),
                "npm install".to_owned(),
                "npm install left-pad".to_owned(),
                "pip --version".to_owned(),
                format!("echo {}", words.join(" ")),
            ] {
                prop_assert_eq!(install_refusal(&command, &notes), None, "{}", command);
            }
            for command in ["pip install x", "npm install -g x", "npm i -gD x", "cargo install ripgrep", "brew install jq", "nix profile install nixpkgs#jq"] {
                prop_assert!(install_refusal(command, &notes).is_some(), "{}", command);
            }
        }
    }

    #[test]
    fn the_message_is_exact() {
        assert_eq!(
            install_refusal("sudo pip install requests", &parse_notes(None)),
            Some(format!(
                "ShipIt: never install globally on this machine (\"pip install\"). {BUILT_IN_RECIPE}"
            ))
        );
    }
}
