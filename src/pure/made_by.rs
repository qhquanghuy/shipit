use crate::pure::shell_words::{
    Parse, Segment, leading_program, message_values, parse, program, raw_words_unquoted,
};
use crate::pure::ship_gate::{
    Answer, Global, INTERPRETERS, Mode, WRAPPERS, folders_of, runs_as, split_global,
};

pub(crate) const MAKER: &str = "shipit:maker";
pub(crate) const OWNER: &str = "owner";
pub(crate) const TRAILER_KEY: &str = "ShipIt-Made-By";

const SHORT: usize = 12;

// A git command that makes or moves commits on the branch it runs on.
pub(crate) const MOVES: [&str; 8] = [
    "commit",
    "cherry-pick",
    "revert",
    "merge",
    "rebase",
    "am",
    "pull",
    "reset",
];

// Programs that run another command line, besides the shells, interpreters and wrappers the ship
// gate already lists.
const RUNNERS: [&str; 8] = [
    "env", "nix", "command", "timeout", "nice", "sudo", "nohup", "time",
];

const NOT_MAKER: &str = "ShipIt: on a slice branch only shipit:maker commits or moves the work, so that a receipt names who made it. Start shipit:maker for this change.";
const NO_TRAILER: &str = "ShipIt: a maker commits with one plain command that carries its trailer: git commit -m \"<message>\" --trailer \"ShipIt-Made-By: shipit:maker\".";
const NOT_MAKER_EDIT: &str = "ShipIt: on a slice branch only shipit:maker edits the work (a file git would commit), so that a receipt names who made it. Start shipit:maker for this change.";
const OWNER_ONLY: &str = "ShipIt: only shipit:maker changes a slice's work. Answer yes only if you, the owner, make this change yourself; commit it with --trailer \"ShipIt-Made-By: owner\".";
const MAIN_IS_NOT_MAKER: &str =
    "ShipIt: only shipit:maker may write its own trailer; the main session is not the maker.";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Move {
    pub(crate) sub: String,
    pub(crate) dir: Option<String>,
    pub(crate) folder: Option<String>,
    pub(crate) trailer: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Moves {
    Plain(Vec<Move>),
    Unreadable,
}

enum Seen {
    Nothing,
    One(Move),
    Unreadable,
}

// The command line names git and one of its moves, whatever runs it.
fn names_move(text: &str) -> bool {
    let words = raw_words_unquoted(text);
    words.iter().any(|word| word == "git")
        && words.iter().any(|word| MOVES.contains(&word.as_str()))
}

fn runs_commands(word: &str) -> bool {
    runs_as(word, &WRAPPERS) || runs_as(word, &INTERPRETERS) || runs_as(word, &RUNNERS)
}

// The value of the last ShipIt-Made-By trailer the command gives; any other trailer is not one.
fn trailer_of(args: &[String], data: &[bool]) -> Option<String> {
    let mut found = None;
    for (at, word) in args.iter().enumerate() {
        if data.get(at) == Some(&true) {
            continue;
        }
        let value = match word.strip_prefix("--trailer=") {
            Some(glued) => Some(glued),
            None if word == "--trailer" => args.get(at + 1).map(String::as_str),
            None => None,
        };
        let Some((key, text)) = value.and_then(|value| value.split_once([':', '='])) else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case(TRAILER_KEY) {
            found = Some(text.trim().to_owned());
        }
    }
    found
}

fn seen(segment: &Segment, command: &str, heredocs: bool) -> Seen {
    let words = &segment.words;
    let Some(lead) = words
        .iter()
        .position(|word| leading_program(std::iter::once(word.as_str())).is_some())
    else {
        return Seen::Nothing;
    };
    let Some(first) = words.get(lead) else {
        return Seen::Nothing;
    };
    if program(first) != "git" {
        let hidden = names_move(&words.join(" "))
            || ((segment.reads_stdin || heredocs) && names_move(command));
        return if runs_commands(first) && hidden {
            Seen::Unreadable
        } else {
            Seen::Nothing
        };
    }
    let rest = words.get(lead + 1..).unwrap_or_default();
    let at = match split_global(rest) {
        Global::Unknown => {
            return if names_move(&words.join(" ")) {
                Seen::Unreadable
            } else {
                Seen::Nothing
            };
        }
        Global::Plain { sub: None, .. } => return Seen::Nothing,
        Global::Plain { sub: Some(at), .. } => at,
    };
    let Some(sub) = rest.get(at).filter(|sub| MOVES.contains(&sub.as_str())) else {
        return Seen::Nothing;
    };
    let before = words.get(..=lead + at).unwrap_or_default();
    let trailer = (sub == "commit")
        .then(|| {
            let args = rest.get(at + 1..).unwrap_or_default();
            let data = message_values(words);
            trailer_of(args, data.get(lead + at + 2..).unwrap_or_default())
        })
        .flatten();
    Seen::One(Move {
        sub: sub.clone(),
        dir: segment.dir.clone(),
        folder: folders_of(before).pop(),
        trailer,
    })
}

// The moves a command makes, each with the folder it runs in and the trailer a commit gives. A
// command that names git and a move but is not all plain git commands (a wrapper, a shell
// string, a `git -c`, an expansion) is Unreadable: reading too little never reads as no move.
pub(crate) fn moves(command: &str) -> Moves {
    match parse(command) {
        Parse::Complex(_) => {
            if names_move(command) {
                Moves::Unreadable
            } else {
                Moves::Plain(Vec::new())
            }
        }
        Parse::Plain { segments, heredocs } => {
            let heredocs = !heredocs.is_empty();
            let mut found = Vec::new();
            for segment in &segments {
                match seen(segment, command, heredocs) {
                    Seen::Nothing => {}
                    Seen::One(one) => found.push(one),
                    Seen::Unreadable => return Moves::Unreadable,
                }
            }
            Moves::Plain(found)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Caller<'a> {
    Main,
    Agent(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change<'a> {
    Move {
        sub: &'a str,
        trailer: Option<&'a str>,
    },
    Edit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Pass,
    Ask(String),
    Deny(String),
}

// Who may change a slice's work: the maker (a commit with its own trailer), and the owner, who
// is asked. The main session is no maker, so it may not write the maker's trailer.
pub(crate) fn author_verdict(caller: &Caller<'_>, change: &Change<'_>) -> Verdict {
    match (caller, change) {
        (
            Caller::Agent(MAKER),
            Change::Move {
                sub: "commit",
                trailer,
            },
        ) => {
            if *trailer == Some(MAKER) {
                Verdict::Pass
            } else {
                Verdict::Deny(NO_TRAILER.to_owned())
            }
        }
        (Caller::Agent(MAKER), Change::Move { .. } | Change::Edit) => Verdict::Pass,
        (Caller::Agent(_), Change::Move { .. }) => Verdict::Deny(NOT_MAKER.to_owned()),
        (Caller::Agent(_), Change::Edit) => Verdict::Deny(NOT_MAKER_EDIT.to_owned()),
        (
            Caller::Main,
            Change::Move {
                sub: "commit",
                trailer: Some(MAKER),
            },
        ) => Verdict::Deny(MAIN_IS_NOT_MAKER.to_owned()),
        (Caller::Main, Change::Move { .. } | Change::Edit) => Verdict::Ask(OWNER_ONLY.to_owned()),
    }
}

// A question the permission mode cannot put to the owner is a denial, as the ship gate's is.
pub(crate) fn answer(mode: Mode, verdict: Verdict) -> Answer {
    match (verdict, mode) {
        (Verdict::Pass, _) => Answer::Pass,
        (Verdict::Deny(text), _) | (Verdict::Ask(text), Mode::Bypass | Mode::Unknown) => {
            Answer::Deny(text)
        }
        (
            Verdict::Ask(text),
            Mode::Default | Mode::Plan | Mode::AcceptEdits | Mode::Auto | Mode::DontAsk,
        ) => Answer::Ask(text),
    }
}

// The first commit of `git log` lines `<oid>\t<values>` whose values are empty or hold anything
// but MAKER or OWNER, shortened to 12 characters.
pub(crate) fn unmade(log: &str) -> Option<String> {
    log.lines().find_map(|line| {
        let (oid, values) = line.split_once('\t')?;
        let made = !values.is_empty()
            && values
                .split(',')
                .all(|value| matches!(value.trim(), MAKER | OWNER));
        (!made).then(|| oid.chars().take(SHORT).collect())
    })
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use super::*;

    const MODES: [Mode; 7] = [
        Mode::Default,
        Mode::Plan,
        Mode::AcceptEdits,
        Mode::Auto,
        Mode::DontAsk,
        Mode::Bypass,
        Mode::Unknown,
    ];
    const READS: [&str; 4] = ["log", "status", "diff", "show"];

    fn values() -> impl Strategy<Value = Vec<&'static str>> {
        proptest::collection::vec(
            prop_oneof![
                Just(MAKER),
                Just(OWNER),
                Just("shipit:lead"),
                Just("x"),
                Just("")
            ],
            0..3,
        )
    }

    // `[cd <dir> && ]git [-C <folder>] [--no-pager] <sub> [-q] [<trailer>]`, and what moves it holds.
    #[derive(Debug, Clone)]
    struct Shape {
        sub: String,
        dir: Option<String>,
        folder: Option<String>,
        pager: bool,
        trailer: Option<(bool, String)>,
    }

    fn shape() -> impl Strategy<Value = Shape> {
        (
            prop_oneof![
                proptest::sample::select(MOVES.to_vec()),
                proptest::sample::select(READS.to_vec())
            ],
            proptest::option::of("[a-z]{1,4}"),
            proptest::option::of("[a-z]{1,4}"),
            any::<bool>(),
            proptest::option::of((any::<bool>(), "[a-z:]{1,6}")),
        )
            .prop_map(|(sub, dir, folder, pager, trailer)| Shape {
                sub: sub.to_owned(),
                dir,
                folder,
                pager,
                trailer,
            })
    }

    fn text_of(shape: &Shape) -> String {
        let mut text = String::new();
        if let Some(dir) = &shape.dir {
            write!(text, "cd {dir} && ").ok();
        }
        text.push_str("git");
        if let Some(folder) = &shape.folder {
            write!(text, " -C {folder}").ok();
        }
        if shape.pager {
            text.push_str(" --no-pager");
        }
        write!(text, " {} -q", shape.sub).ok();
        match &shape.trailer {
            Some((true, value)) => write!(text, " --trailer \"{TRAILER_KEY}: {value}\"").ok(),
            Some((false, value)) => write!(text, " \"--trailer={TRAILER_KEY}: {value}\"").ok(),
            None => None,
        };
        text
    }

    fn expected(shape: &Shape) -> Vec<Move> {
        if !MOVES.contains(&shape.sub.as_str()) {
            return Vec::new();
        }
        let trailer = shape
            .trailer
            .as_ref()
            .filter(|_| shape.sub == "commit")
            .map(|(_, value)| value.trim().to_owned());
        vec![Move {
            sub: shape.sub.clone(),
            dir: shape.dir.clone(),
            folder: shape.folder.clone(),
            trailer,
        }]
    }

    fn table(caller: &Caller<'_>, change: &Change<'_>) -> Verdict {
        match (caller, change) {
            (
                Caller::Agent(MAKER),
                Change::Move {
                    sub: "commit",
                    trailer,
                },
            ) => {
                if *trailer == Some(MAKER) {
                    Verdict::Pass
                } else {
                    Verdict::Deny(NO_TRAILER.to_owned())
                }
            }
            (Caller::Agent(MAKER), _) => Verdict::Pass,
            (Caller::Agent(_), Change::Move { .. }) => Verdict::Deny(NOT_MAKER.to_owned()),
            (Caller::Agent(_), Change::Edit) => Verdict::Deny(NOT_MAKER_EDIT.to_owned()),
            (
                Caller::Main,
                Change::Move {
                    sub: "commit",
                    trailer: Some(MAKER),
                },
            ) => Verdict::Deny(MAIN_IS_NOT_MAKER.to_owned()),
            (Caller::Main, _) => Verdict::Ask(OWNER_ONLY.to_owned()),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn every_commit_of_the_work_must_carry_a_makers_or_owners_trailer(
            commits in proptest::collection::vec(("[0-9a-f]{40}", values()), 0..6),
        ) {
            let mut log = String::new();
            for (oid, values) in &commits {
                writeln!(log, "{oid}\t{}", values.join(",")).ok();
            }
            let expected = commits
                .iter()
                .find(|(_oid, values)| {
                    values.is_empty() || values.iter().any(|value| *value != MAKER && *value != OWNER)
                })
                .map(|(oid, _values)| oid.chars().take(12).collect::<String>());
            prop_assert_eq!(unmade(&log), expected);
        }

        #[test]
        fn a_move_is_found_with_its_folder_and_trailer_and_a_wrapped_one_is_unreadable(
            first in shape(),
            second in shape(),
            wrapper in 0..4_usize,
        ) {
            let text = format!("ls && {} ; ./test.sh && git log --grep commit && echo git commit", text_of(&first));
            prop_assert_eq!(moves(&text), Moves::Plain(expected(&first)));
            let both = format!("{}\n{}", text_of(&Shape { dir: None, ..first.clone() }), text_of(&Shape { dir: None, ..second.clone() }));
            let mut all = expected(&Shape { dir: None, ..first.clone() });
            all.extend(expected(&Shape { dir: None, ..second.clone() }));
            prop_assert_eq!(moves(&both), Moves::Plain(all));
            let plain = text_of(&Shape { dir: None, ..first.clone() });
            if MOVES.contains(&first.sub.as_str()) {
                let wrapped = match wrapper {
                    0 => format!("env {plain}"),
                    1 => format!("nix develop -c {plain}"),
                    2 => format!("sh -c '{plain}'"),
                    _ => plain.replacen("git", "git -c k=v", 1),
                };
                prop_assert_eq!(moves(&wrapped), Moves::Unreadable);
            }
        }

        #[test]
        fn the_author_verdict_follows_the_table(
            caller in prop_oneof![
                Just(Caller::Main),
                Just(Caller::Agent(MAKER)),
                Just(Caller::Agent("shipit:slice")),
                Just(Caller::Agent("")),
            ],
            sub in proptest::sample::select(MOVES.to_vec()),
            trailer in proptest::option::of(prop_oneof![Just(MAKER), Just(OWNER), Just("x")]),
            edit in any::<bool>(),
            mode in proptest::sample::select(MODES.to_vec()),
        ) {
            let change = if edit { Change::Edit } else { Change::Move { sub, trailer } };
            let verdict = author_verdict(&caller, &change);
            prop_assert_eq!(&verdict, &table(&caller, &change));
            let denied = matches!(mode, Mode::Bypass | Mode::Unknown);
            let expected = match &verdict {
                Verdict::Pass => Answer::Pass,
                Verdict::Deny(text) => Answer::Deny(text.clone()),
                Verdict::Ask(text) if denied => Answer::Deny(text.clone()),
                Verdict::Ask(text) => Answer::Ask(text.clone()),
            };
            prop_assert_eq!(answer(mode, verdict), expected);
        }
    }
}
