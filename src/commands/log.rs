use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::commands::setup::load_setup_table;
use crate::edge::clock::now_utc;
use crate::edge::git::Git;
use crate::edge::records::{
    append_line, goal_names, load_goal, logbook_path, read_logbook, receipt_path, store_dir,
};
use crate::edge::slice_git::{current_key, load_policy, merge_base, parents, patch_id};
use crate::edge::store::read_json;
use crate::pure::base::BaseRef;
use crate::pure::failure::Failure;
use crate::pure::flags::{goal_flag, once, unknown_argument, worker_flag};
use crate::pure::goal::GoalName;
use crate::pure::goal_pick::{element_named, which_goal};
use crate::pure::logbook::{Entry, Logged, closed, is_shipped_by, line};
use crate::pure::receipt::Oid;
use crate::pure::review::{ReceiptKind, stored_findings, tool};
use crate::pure::roadmap::{Element, SliceName};
use crate::pure::setup::{HEAD, Setup};

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit log <slice> <note|known-issue|answer|shipped> [--text <t>] [--kind <k>] [--finding <Fn>] [--reason <t>] [--by <w>] [--merge <commit>] [--note <evidence>] [--unmatched] [--goal <name>]";
pub(crate) const SUMMARY: &str = "Appends one event to the goal's logbook.";
pub(crate) const EXITS: [&str; 4] = [
    "logged",
    "refused: no receipt of that kind for this version, no such finding, already closed, or a shipped entry that does not match the merge",
    "usage, setup, goal or logbook error",
    "",
];

#[derive(Debug, Default)]
struct Args {
    positional: Vec<String>,
    given: Vec<&'static str>,
    text: Option<String>,
    kind: Option<String>,
    finding: Option<String>,
    reason: Option<String>,
    by: Option<String>,
    merge: Option<String>,
    note: Option<String>,
    unmatched: bool,
    goal: Option<GoalName>,
}

fn parse_args(args: &[OsString]) -> Result<Args, Failure> {
    let mut parsed = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (name, slot): (&'static str, &mut Option<String>) = match arg.to_str() {
            Some("--text") => ("--text", &mut parsed.text),
            Some("--kind") => ("--kind", &mut parsed.kind),
            Some("--finding") => ("--finding", &mut parsed.finding),
            Some("--reason") => ("--reason", &mut parsed.reason),
            Some("--by") => ("--by", &mut parsed.by),
            Some("--merge") => ("--merge", &mut parsed.merge),
            Some("--note") => ("--note", &mut parsed.note),
            Some("--unmatched") => {
                parsed.unmatched = true;
                parsed.given.push("--unmatched");
                continue;
            }
            Some("--goal") => {
                goal_flag(&mut parsed.goal, rest.next(), USAGE)?;
                continue;
            }
            Some(flag) if flag.starts_with("--") => return Err(unknown_argument(arg, USAGE)),
            Some(word) => {
                parsed.positional.push(word.to_owned());
                continue;
            }
            None => return Err(unknown_argument(arg, USAGE)),
        };
        once(slot, name, rest.next(), USAGE)?;
        parsed.given.push(name);
    }
    Ok(parsed)
}

#[derive(Debug)]
enum Event {
    Note {
        text: String,
    },
    KnownIssue {
        kind: ReceiptKind,
        finding: String,
        reason: String,
        by: String,
    },
    Answer {
        kind: ReceiptKind,
        finding: String,
        text: String,
        by: String,
    },
    ShippedMerge {
        merge: String,
        unmatched: bool,
        note: Option<String>,
    },
    ShippedProof {
        note: String,
    },
}

fn required(value: Option<String>, flag: &str, event: &str) -> Result<String, Failure> {
    value.ok_or_else(|| Failure::usage(format!("{event} needs {flag}; {USAGE}")))
}

fn receipt_kind(kind: Option<String>, event: &str) -> Result<ReceiptKind, Failure> {
    let kind = required(kind, "--kind", event)?;
    ReceiptKind::parse(&kind)
        .ok_or_else(|| Failure::usage(format!("unknown kind \"{kind}\"; {USAGE}")))
}

// The flags each event takes; any other is a mistake worth naming.
fn only(given: &[&str], allowed: &[&str], event: &str) -> Result<(), Failure> {
    match given.iter().find(|flag| !allowed.contains(flag)) {
        Some(flag) => Err(Failure::usage(format!(
            "{event} does not take {flag}; {USAGE}"
        ))),
        None => Ok(()),
    }
}

fn event(args: Args, event: &str) -> Result<Event, Failure> {
    match event {
        "note" => {
            only(&args.given, &["--text"], event)?;
            Ok(Event::Note {
                text: required(args.text, "--text", event)?,
            })
        }
        "known-issue" => {
            only(
                &args.given,
                &["--kind", "--finding", "--reason", "--by"],
                event,
            )?;
            let by = worker_flag("--by", args.by.as_deref(), USAGE)?.to_string();
            Ok(Event::KnownIssue {
                kind: receipt_kind(args.kind, event)?,
                finding: required(args.finding, "--finding", event)?,
                reason: required(args.reason, "--reason", event)?,
                by,
            })
        }
        "answer" => {
            only(
                &args.given,
                &["--kind", "--finding", "--text", "--by"],
                event,
            )?;
            let by = worker_flag("--by", args.by.as_deref(), USAGE)?.to_string();
            Ok(Event::Answer {
                kind: receipt_kind(args.kind, event)?,
                finding: required(args.finding, "--finding", event)?,
                text: required(args.text, "--text", event)?,
                by,
            })
        }
        "shipped" => {
            only(&args.given, &["--merge", "--note", "--unmatched"], event)?;
            match (args.merge, args.note, args.unmatched) {
                (Some(merge), note, unmatched) => {
                    if unmatched && note.is_none() {
                        return Err(Failure::usage(format!(
                            "--unmatched needs --note <why the merge does not match>; {USAGE}"
                        )));
                    }
                    if !unmatched && note.is_some() {
                        return Err(Failure::usage(format!(
                            "--note is for a proof step; a slice ships with --merge \
                             (or --merge --unmatched --note <why>); {USAGE}"
                        )));
                    }
                    Ok(Event::ShippedMerge {
                        merge,
                        unmatched,
                        note,
                    })
                }
                (None, Some(note), false) => Ok(Event::ShippedProof { note }),
                (None, _, true) | (None, None, false) => Err(Failure::usage(format!(
                    "shipped needs --merge <commit> for a slice or --note <evidence> for a proof \
                     step; {USAGE}"
                ))),
            }
        }
        "receipt" => Err(Failure::usage(
            "\"receipt\" entries are written by shipit receipt".to_owned(),
        )),
        "started" => Err(Failure::usage(
            "\"started\" entries are written by shipit start".to_owned(),
        )),
        other => Err(Failure::usage(format!(
            "unknown event \"{other}\"; {USAGE}"
        ))),
    }
}

// The receipt of this kind for the slice's current version, and the findings it holds.
fn receipt_findings(
    git: &Git,
    common: &Path,
    (goal, slice): (&GoalName, &SliceName),
    kind: ReceiptKind,
) -> Result<(Oid, Vec<crate::pure::review::Stored>), Failure> {
    let refused = || {
        Failure::no(format!(
            "{slice} has no {} receipt for its current version",
            kind.name()
        ))
    };
    let key = current_key(git, common, (goal, slice), kind)?.ok_or_else(refused)?;
    let docs = read_json(&receipt_path(&store_dir(common), kind, &key));
    let findings = match docs.as_deref() {
        Some([doc]) => stored_findings(doc),
        Some(_) | None => None,
    };
    Ok((key, findings.ok_or_else(refused)?))
}

// Every check of `shipped --merge` but the patch id of the merge.
fn merge_is_of_the_base(
    git: &Git,
    base_ref: &BaseRef,
    commit: &Oid,
    typed: &str,
) -> Result<(), Failure> {
    let on_base = git
        .commit_of(OsStr::new(&base_ref.full()))
        .is_some_and(|tip| git.is_ancestor(commit, &tip));
    if on_base {
        Ok(())
    } else {
        Err(Failure::no(format!(
            "{typed} is not a commit on {base_ref}"
        )))
    }
}

// (a) a merge commit whose second parent holds the reviewed change, (b) a squash commit that is
// the reviewed change on its parent, (c) the reviewed commit itself (a fast-forward).
fn is_the_reviewed_change(
    git: &Git,
    commit: &Oid,
    (key, reviewed): (&str, &str),
) -> Result<bool, Failure> {
    if commit.as_str() == reviewed {
        return Ok(true);
    }
    let same = |id: Option<Oid>| id.is_some_and(|id| id.as_str() == key);
    match parents(git, commit)?.as_slice() {
        [] => Ok(false),
        [only] => Ok(same(patch_id(git, only, commit)?)),
        [first, second, ..] => match merge_base(git, first, second) {
            Some(fork) => Ok(same(patch_id(git, &fork, second)?)),
            None => Ok(false),
        },
    }
}

fn newest_review<'a>(entries: &'a [Logged], slice: &SliceName) -> Option<(&'a str, &'a str)> {
    entries
        .iter()
        .rev()
        .filter(|logged| logged.slice == *slice)
        .find_map(|logged| match &logged.entry {
            Entry::Receipt {
                kind,
                key,
                commit: Some(commit),
                ..
            } if kind == "informed-review" => Some((key.as_str(), commit.as_str())),
            Entry::Receipt { .. }
            | Entry::Note { .. }
            | Entry::KnownIssue { .. }
            | Entry::Answer { .. }
            | Entry::ShippedMerge { .. }
            | Entry::ShippedProof { .. }
            | Entry::Started { .. } => None,
        })
}

fn already_named_by<'a>(entries: &'a [Logged], commit: &Oid) -> Option<&'a SliceName> {
    entries.iter().find_map(|logged| match &logged.entry {
        Entry::ShippedMerge {
            commit: shipped, ..
        } if shipped == commit.as_str() => Some(&logged.slice),
        Entry::ShippedMerge { .. }
        | Entry::Receipt { .. }
        | Entry::Note { .. }
        | Entry::KnownIssue { .. }
        | Entry::Answer { .. }
        | Entry::ShippedProof { .. }
        | Entry::Started { .. } => None,
    })
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (head, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let args = parse_args(args)?;
    let [slice, name] = args.positional.as_slice() else {
        return Err(Failure::usage(USAGE));
    };
    let (slice_text, name) = (slice.clone(), name.clone());
    let flag_goal = args.goal.clone();
    let event = event(args, &name)?;
    let common = git.common_dir()?;
    let store = store_dir(&common);
    let goal = which_goal(
        flag_goal.as_ref(),
        git.current_branch().as_deref(),
        &goal_names(&store),
    )?;
    let roadmap = load_goal(&common, &goal)?;
    let slice = SliceName::parse(&slice_text).ok_or_else(|| {
        Failure::usage(format!(
            "no slice named \"{slice_text}\" in the roadmap of goal \"{goal}\""
        ))
    })?;
    let element = element_named(&roadmap, &slice).ok_or_else(|| {
        Failure::usage(format!(
            "no slice named \"{slice_text}\" in the roadmap of goal \"{goal}\""
        ))
    })?;
    let entries = read_logbook(&store, &goal)?;

    let entry = entry_for(
        (&git, &common, head, err),
        (&goal, &slice, element, &entries),
        event,
    )?;
    let said = match &entry {
        Entry::Note { .. } => "note",
        Entry::KnownIssue { .. } => "known-issue",
        Entry::Answer { .. } => "answer",
        Entry::ShippedMerge { .. } | Entry::ShippedProof { .. } => "shipped",
        Entry::Receipt { .. } => "receipt",
        Entry::Started { .. } => "started",
    };
    let text = line(&entry, &slice, &now_utc(), &tool()).map_err(Failure::usage)?;
    append_line(&logbook_path(&store, &goal), &text)?;
    writeln!(out, "logged {said} for {slice}").ok();
    Ok(())
}

// The key of the receipt whose finding closes, after the refusals.
fn close_checks(
    git: &Git,
    common: &Path,
    (goal, slice, entries): (&GoalName, &SliceName, &[Logged]),
    kind: ReceiptKind,
    finding: &str,
    answering: bool,
) -> Result<String, Failure> {
    let (key, findings) = receipt_findings(git, common, (goal, slice), kind)?;
    let found = findings
        .iter()
        .find(|stored| stored.id == finding)
        .ok_or_else(|| Failure::no(format!("{} has no finding {finding}", kind.name())))?;
    if closed(entries, slice, kind.name(), key.as_str())
        .iter()
        .any(|id| id == finding)
    {
        return Err(Failure::no(format!(
            "{} {finding} is already closed",
            kind.name()
        )));
    }
    match (answering, found.is_question) {
        (true, false) => Err(Failure::no(format!(
            "{finding} is not a question: fix it, or log it as a known issue"
        ))),
        (false, true) => Err(Failure::no(format!(
            "{finding} is a question: answer it with shipit log {slice} answer"
        ))),
        (true, true) | (false, false) => Ok(key.to_string()),
    }
}

fn refuse_second_shipping(entries: &[Logged], slice: &SliceName) -> Result<(), Failure> {
    if is_shipped_by(entries, slice) {
        return Err(Failure::no(format!("{slice} is already shipped")));
    }
    Ok(())
}

// With no patch id match the owner's word stands, but the commit must still come after the point
// where the reviewed change left the base: the root commit is nobody's merge of it.
fn refuse_a_commit_before_the_change(
    git: &Git,
    base_ref: &BaseRef,
    commit: &Oid,
    (typed, reviewed): (&str, &str),
) -> Result<(), Failure> {
    let fork = Oid::parse(reviewed)
        .zip(git.commit_of(OsStr::new(&base_ref.full())))
        .and_then(|(reviewed, tip)| merge_base(git, &reviewed, &tip));
    match fork {
        Some(fork) if git.is_ancestor(commit, &fork) => Err(Failure::no(format!(
            "{typed} is not after the reviewed change left {base_ref}"
        ))),
        Some(_) | None => Ok(()),
    }
}

fn shipped_merge(
    git: &Git,
    base_ref: &BaseRef,
    (slice, entries): (&SliceName, &[Logged]),
    (typed, unmatched, note): (&str, bool, Option<String>),
) -> Result<Entry, Failure> {
    refuse_second_shipping(entries, slice)?;
    let (key, reviewed) = newest_review(entries, slice).ok_or_else(|| {
        Failure::no(format!(
            "{slice} has no review receipt: nothing was reviewed to ship"
        ))
    })?;
    let not_on_base = || Failure::no(format!("{typed} is not a commit on {base_ref}"));
    let commit = git.commit_of(OsStr::new(typed)).ok_or_else(not_on_base)?;
    merge_is_of_the_base(git, base_ref, &commit, typed)?;
    if let Some(other) = already_named_by(entries, &commit) {
        return Err(Failure::no(format!(
            "{typed} is already named by {other}'s shipped entry"
        )));
    }
    if unmatched {
        refuse_a_commit_before_the_change(git, base_ref, &commit, (typed, reviewed))?;
    } else if !is_the_reviewed_change(git, &commit, (key, reviewed))? {
        return Err(Failure::no(format!(
            "{typed} is not the merge of {slice}'s reviewed change into {base_ref}"
        )));
    }
    Ok(Entry::ShippedMerge {
        commit: commit.to_string(),
        matched: !unmatched,
        note,
    })
}

fn entry_for(
    (git, common, head, err): (&Git, &Path, Setup, &mut dyn Write),
    (goal, slice, element, entries): (&GoalName, &SliceName, &Element, &[Logged]),
    event: Event,
) -> Result<Entry, Failure> {
    let closing = |kind, finding: &str, answering| {
        close_checks(
            git,
            common,
            (goal, slice, entries),
            kind,
            finding,
            answering,
        )
    };
    let entry = match (event, element) {
        (Event::Note { text }, _) => Entry::Note { text },
        (
            Event::KnownIssue {
                kind,
                finding,
                reason,
                by,
            },
            Element::Slice(_),
        ) => Entry::KnownIssue {
            kind: kind.name().to_owned(),
            key: closing(kind, &finding, false)?,
            finding,
            reason,
            by,
        },
        (
            Event::Answer {
                kind,
                finding,
                text,
                by,
            },
            Element::Slice(_),
        ) => Entry::Answer {
            kind: kind.name().to_owned(),
            key: closing(kind, &finding, true)?,
            finding,
            text,
            by,
        },
        (Event::KnownIssue { .. } | Event::Answer { .. }, Element::Proof(_)) => {
            return Err(Failure::usage(format!(
                "\"{slice}\" is a proof step: it has no receipts"
            )));
        }
        (Event::ShippedProof { note }, Element::Proof(_)) => {
            refuse_second_shipping(entries, slice)?;
            Entry::ShippedProof { note }
        }
        (Event::ShippedProof { .. }, Element::Slice(_)) => {
            return Err(Failure::usage(format!(
                "\"{slice}\" is a slice: it ships with --merge <commit>"
            )));
        }
        (Event::ShippedMerge { .. }, Element::Proof(_)) => {
            return Err(Failure::usage(format!(
                "\"{slice}\" is a proof step: it ships with --note <evidence>"
            )));
        }
        (
            Event::ShippedMerge {
                merge,
                unmatched,
                note,
            },
            Element::Slice(_),
        ) => {
            let (_policy, base_ref) = load_policy(git, head, err)?;
            shipped_merge(git, &base_ref, (slice, entries), (&merge, unmatched, note))?
        }
    };
    Ok(entry)
}
