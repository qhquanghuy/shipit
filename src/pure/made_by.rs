use serde_json::Value;

use crate::pure::rank::effort_name;
use crate::pure::setup::Setup;
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

pub(crate) const NO_SLICE_FOLDER: &str = "ShipIt: shipit:maker works in the slice's worktree, and this Claude Code cannot start an agent in another folder: run the slice in a session opened in its worktree (docs/claude-code.md, Sessions).";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Move {
    pub(crate) sub: String,
    pub(crate) dir: Option<String>,
    pub(crate) folder: Option<String>,
    pub(crate) trailers: Vec<String>,
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

// The command line names git and one of its moves, whatever runs it: a branch move is a flag of
// checkout, switch or branch, so the flag letter counts there.
fn names_move(text: &str) -> bool {
    let words = raw_words_unquoted(text);
    let has = |names: &[&str]| words.iter().any(|word| names.contains(&word.as_str()));
    let flag = |letter: char, long: &str| {
        words
            .iter()
            .any(|word| short_has(word, letter) || word.starts_with(long))
    };
    has(&["git"])
        && (has(&MOVES)
            || has(&["update-ref"])
            || (has(&["checkout"]) && flag('B', "-B"))
            || (has(&["switch"]) && flag('C', "--force-create"))
            || (has(&["branch"]) && flag('f', "--force")))
}

fn runs_commands(word: &str) -> bool {
    runs_as(word, &WRAPPERS) || runs_as(word, &INTERPRETERS) || runs_as(word, &RUNNERS)
}

// The value of every ShipIt-Made-By trailer the command gives; any other trailer is not one.
fn trailers_of(args: &[String], data: &[bool]) -> Vec<String> {
    let mut found = Vec::new();
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
            found.push(text.trim().to_owned());
        }
    }
    found
}

// A short flag cluster holds the letter.
fn short_has(word: &str, letter: char) -> bool {
    word.strip_prefix('-')
        .is_some_and(|rest| !rest.starts_with('-') && rest.contains(letter))
}

// A git subcommand that moves the work: a commit, a history move, or a branch pointed at another
// commit (`checkout -B`, `switch -C`, `branch -f`, `update-ref`).
fn is_move(sub: &str, args: &[String], data: &[bool]) -> bool {
    if MOVES.contains(&sub) || sub == "update-ref" {
        return true;
    }
    let flags = args
        .iter()
        .enumerate()
        .take_while(|(_, word)| *word != "--")
        .filter(|(at, _)| data.get(*at) != Some(&true))
        .map(|(_, word)| word.as_str());
    flags.into_iter().any(|word| match sub {
        "checkout" => short_has(word, 'B'),
        "switch" => {
            short_has(word, 'C') || word == "--force-create" || word.starts_with("--force-create=")
        }
        "branch" => short_has(word, 'f') || word == "--force",
        _ => false,
    })
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
    let Some(sub) = rest.get(at) else {
        return Seen::Nothing;
    };
    let args = rest.get(at + 1..).unwrap_or_default();
    let data = message_values(words);
    let args_data = data.get(lead + at + 2..).unwrap_or_default();
    if !is_move(sub, args, args_data) {
        return Seen::Nothing;
    }
    let trailers = if sub == "commit" {
        trailers_of(args, args_data)
    } else {
        Vec::new()
    };
    Seen::One(Move {
        sub: sub.clone(),
        dir: segment.dir.clone(),
        folder: folders_of(words.get(..=lead + at).unwrap_or_default()).pop(),
        trailers,
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
        trailers: &'a [&'a str],
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
                trailers,
            },
        ) => {
            if !trailers.is_empty() && trailers.iter().all(|trailer| *trailer == MAKER) {
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
                trailers,
            },
        ) if trailers.contains(&MAKER) => Verdict::Deny(MAIN_IS_NOT_MAKER.to_owned()),
        (Caller::Main, Change::Move { .. } | Change::Edit) => Verdict::Ask(OWNER_ONLY.to_owned()),
    }
}

// Off a slice branch, in a project, the maker's trailer is still the maker's alone: a commit
// carrying it, made by anyone else, could be moved onto a slice branch later.
pub(crate) fn forged_trailer(caller: &Caller<'_>, change: &Change<'_>) -> Verdict {
    match (caller, change) {
        (Caller::Agent(MAKER), _) | (_, Change::Edit) => Verdict::Pass,
        (Caller::Main, Change::Move { trailers, .. }) if trailers.contains(&MAKER) => {
            Verdict::Deny(MAIN_IS_NOT_MAKER.to_owned())
        }
        (Caller::Agent(_), Change::Move { trailers, .. }) if trailers.contains(&MAKER) => {
            Verdict::Deny(NOT_MAKER.to_owned())
        }
        (Caller::Main | Caller::Agent(_), Change::Move { .. }) => Verdict::Pass,
    }
}

// Two answers to one command: the stronger kind, and both texts.
pub(crate) fn both(first: Answer, second: Answer) -> Answer {
    match (first, second) {
        (Answer::Pass, other) | (other, Answer::Pass) => other,
        (Answer::Deny(a), Answer::Deny(b) | Answer::Ask(b)) | (Answer::Ask(a), Answer::Deny(b)) => {
            Answer::Deny(format!("{a} {b}"))
        }
        (Answer::Ask(a), Answer::Ask(b)) => Answer::Ask(format!("{a} {b}")),
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

// The setup's maker as a start is judged: its model, its effort when the setup gives one, and the
// stand-in of its tier. The effort is the receipt's word when the setup gives none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MakerRank {
    pub(crate) model: String,
    pub(crate) effort: Option<String>,
    pub(crate) stand_in: Option<(String, String)>,
}

pub(crate) fn maker_rank(setup: &Setup) -> Option<MakerRank> {
    let role = &setup.agents.roles.maker;
    let tier = setup
        .agents
        .tiers
        .iter()
        .find(|tier| u64::from(&tier.tier) == u64::from(&role.tier))?;
    Some(MakerRank {
        model: tier.model.as_str().to_owned(),
        effort: role.effort.map(|effort| effort_name(effort).to_owned()),
        stand_in: tier.stand_in.as_ref().map(|stand_in| {
            (
                stand_in.model.as_str().to_owned(),
                effort_name(stand_in.effort).to_owned(),
            )
        }),
    })
}

fn not_at_rank(maker: &MakerRank) -> String {
    let effort = maker
        .effort
        .as_ref()
        .map(|effort| format!(" and effort \"{effort}\""))
        .unwrap_or_default();
    format!(
        "ShipIt: start shipit:maker with model \"{}\"{effort}, the setup's maker (agents.roles.maker), or its stand-in: the receipts name that rank as the author.",
        maker.model
    )
}

// None for an agent that is not the maker. For a maker: the folder the call names (`cwd`), when it
// starts at the setup's maker rank or its stand-in, else the denial. The rank is read only for a
// maker, and a setup that cannot be read is that denial.
pub(crate) fn maker_start(
    tool_input: &Value,
    maker: impl FnOnce() -> Result<MakerRank, String>,
) -> Option<Result<Option<String>, String>> {
    let text = |key: &str| tool_input.get(key).and_then(Value::as_str);
    if text("subagent_type") != Some(MAKER) {
        return None;
    }
    Some(maker().and_then(|rank| {
        let (model, effort) = (text("model"), text("effort"));
        let own = model == Some(rank.model.as_str())
            && rank
                .effort
                .as_deref()
                .is_none_or(|want| effort == Some(want));
        let stand_in = rank
            .stand_in
            .as_ref()
            .is_some_and(|(m, e)| model == Some(m.as_str()) && effort == Some(e.as_str()));
        if own || stand_in {
            Ok(text("cwd").map(str::to_owned))
        } else {
            Err(not_at_rank(&rank))
        }
    }))
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
    const READS: [&str; 9] = [
        "log",
        "status",
        "diff",
        "show",
        "checkout",
        "checkout -b",
        "switch -c",
        "branch",
        "branch -d",
    ];
    const BRANCH_MOVES: [&str; 5] = [
        "checkout -B",
        "switch -C",
        "switch --force-create",
        "branch -f",
        "update-ref",
    ];

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
                proptest::sample::select(BRANCH_MOVES.to_vec()),
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

    fn is_move_shape(shape: &Shape) -> bool {
        MOVES.contains(&shape.sub.as_str()) || BRANCH_MOVES.contains(&shape.sub.as_str())
    }

    fn expected(shape: &Shape) -> Vec<Move> {
        if !is_move_shape(shape) {
            return Vec::new();
        }
        let trailers = shape
            .trailer
            .iter()
            .filter(|_| shape.sub == "commit")
            .map(|(_, value)| value.trim().to_owned())
            .collect();
        vec![Move {
            sub: shape
                .sub
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned(),
            dir: shape.dir.clone(),
            folder: shape.folder.clone(),
            trailers,
        }]
    }

    fn table(caller: &Caller<'_>, change: &Change<'_>) -> Verdict {
        match (caller, change) {
            (
                Caller::Agent(MAKER),
                Change::Move {
                    sub: "commit",
                    trailers,
                },
            ) => {
                if !trailers.is_empty() && trailers.iter().all(|trailer| *trailer == MAKER) {
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
                    trailers,
                },
            ) if trailers.contains(&MAKER) => Verdict::Deny(MAIN_IS_NOT_MAKER.to_owned()),
            (Caller::Main, _) => Verdict::Ask(OWNER_ONLY.to_owned()),
        }
    }

    const MODELS: [&str; 3] = ["sonnet", "opus", "fable"];
    const EFFORTS: [&str; 3] = ["low", "medium", "high"];

    fn rank() -> impl Strategy<Value = MakerRank> {
        (
            proptest::sample::select(MODELS.to_vec()),
            proptest::option::of(proptest::sample::select(EFFORTS.to_vec())),
            proptest::option::of((
                proptest::sample::select(MODELS.to_vec()),
                proptest::sample::select(EFFORTS.to_vec()),
            )),
        )
            .prop_map(|(model, effort, stand_in)| MakerRank {
                model: model.to_owned(),
                effort: effort.map(str::to_owned),
                stand_in: stand_in.map(|(model, effort)| (model.to_owned(), effort.to_owned())),
            })
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
            if is_move_shape(&first) {
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
        fn a_maker_starts_at_the_setups_maker_rank_or_its_stand_in(
            rank in rank(),
            kind in proptest::sample::select(vec![MAKER, "shipit:reader"]),
            model in proptest::option::of(proptest::sample::select(MODELS.to_vec())),
            effort in proptest::option::of(proptest::sample::select(EFFORTS.to_vec())),
            folder in proptest::option::of("[a-z/]{1,8}"),
        ) {
            let mut call = serde_json::json!({ "subagent_type": kind });
            for (key, value) in [("model", model), ("effort", effort)] {
                if let Some(value) = value {
                    call[key] = serde_json::json!(value);
                }
            }
            if let Some(folder) = &folder {
                call["cwd"] = serde_json::json!(folder);
            }
            let own = model == Some(rank.model.as_str())
                && rank.effort.as_deref().is_none_or(|want| effort == Some(want));
            let stand_in = rank
                .stand_in
                .as_ref()
                .is_some_and(|(m, e)| model == Some(m.as_str()) && effort == Some(e.as_str()));
            let wanted = if kind != MAKER {
                None
            } else if own || stand_in {
                Some(Ok(folder.clone()))
            } else {
                Some(Err(not_at_rank(&rank)))
            };
            let loaded = rank.clone();
            prop_assert_eq!(maker_start(&call, move || Ok(loaded)), wanted);
            let unread = maker_start(&call, || Err("no setup".to_owned()));
            prop_assert_eq!(unread, (kind == MAKER).then(|| Err("no setup".to_owned())));
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
            trailers in proptest::collection::vec(prop_oneof![Just(MAKER), Just(OWNER), Just("x")], 0..3),
            edit in any::<bool>(),
            mode in proptest::sample::select(MODES.to_vec()),
        ) {
            let change = if edit { Change::Edit } else { Change::Move { sub, trailers: &trailers } };
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

        #[test]
        fn a_non_maker_never_writes_the_makers_trailer_on_any_branch(
            caller in prop_oneof![Just(Caller::Main), Just(Caller::Agent(MAKER)), Just(Caller::Agent("shipit:slice"))],
            trailers in proptest::collection::vec(prop_oneof![Just(MAKER), Just(OWNER)], 0..3),
            edit in any::<bool>(),
        ) {
            let change = if edit { Change::Edit } else { Change::Move { sub: "commit", trailers: &trailers } };
            let maker_trailer = !edit && trailers.contains(&MAKER);
            let wanted = match (caller, maker_trailer) {
                (Caller::Agent(MAKER), _) | (_, false) => Verdict::Pass,
                (Caller::Main, true) => Verdict::Deny(MAIN_IS_NOT_MAKER.to_owned()),
                (Caller::Agent(_), true) => Verdict::Deny(NOT_MAKER.to_owned()),
            };
            prop_assert_eq!(forged_trailer(&caller, &change), wanted);
        }

        #[test]
        fn two_answers_to_one_command_keep_the_stronger_kind_and_both_texts(
            first in proptest::option::of((any::<bool>(), "[a-z]{1,6}")),
            second in proptest::option::of((any::<bool>(), "[A-Z]{1,6}")),
        ) {
            let make = |part: &Option<(bool, String)>| match part {
                None => Answer::Pass,
                Some((true, text)) => Answer::Deny(text.clone()),
                Some((false, text)) => Answer::Ask(text.clone()),
            };
            let joined = both(make(&first), make(&second));
            let texts: Vec<&String> = first.iter().chain(second.iter()).map(|(_, text)| text).collect();
            let denied = first.iter().chain(second.iter()).any(|(deny, _)| *deny);
            match joined {
                Answer::Pass => prop_assert!(texts.is_empty()),
                Answer::Deny(text) => {
                    prop_assert!(denied);
                    prop_assert!(texts.iter().all(|part| text.contains(part.as_str())));
                }
                Answer::Ask(text) => {
                    prop_assert!(!denied);
                    prop_assert!(texts.iter().all(|part| text.contains(part.as_str())));
                }
            }
        }
    }
}
