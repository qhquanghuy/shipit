// Which agents a session runs, and which maker worktree belongs to which slice. How many agents a
// session has running is kept as one entry per agent or per start in flight. The files are the
// edge's; what an entry means, what each hook event does to the entries, and what a commit in a
// maker's worktree may do, is decided here.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pure::claude_hook::BRANCH_LINE;

// A start that never became an agent (another hook may refuse the call) stops counting after
// this long.
const RESERVATION_SECONDS: i64 = 120;
const STALE_SECONDS: i64 = 7 * 24 * 60 * 60;
const MAX_ID: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Entry {
    Reserved { tool_use_id: String, at: i64 },
    Running { agent_id: String, at: i64 },
}

impl Entry {
    const fn at(&self) -> i64 {
        match self {
            Self::Reserved { at, .. } | Self::Running { at, .. } => *at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    Write(Entry),
    Remove(Entry),
}

// An id becomes a file name: nothing else may be in it.
pub(crate) fn safe_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_ID
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// Times are kept as `YYYY-MM-DDTHH:MM:SSZ`, in UTC, the one form ShipIt writes. The calendar is
// the proleptic Gregorian one, in Howard Hinnant's day arithmetic: eras of 400 years, years that
// start on March 1st so the leap day comes last.
const DAY: i64 = 24 * 60 * 60;
const ERA_DAYS: i64 = 146_097;
// Days from 0000-03-01 to 1970-01-01.
const EPOCH_SHIFT: i64 = 719_468;

fn days_since_1970(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    year.div_euclid(400) * ERA_DAYS + day_of_era - EPOCH_SHIFT
}

fn date_of(days: i64) -> (i64, i64, i64) {
    let days = days + EPOCH_SHIFT;
    let day_of_era = days.rem_euclid(ERA_DAYS);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = (month_from_march + 2) % 12 + 1;
    let year = days.div_euclid(ERA_DAYS) * 400 + year_of_era + i64::from(month <= 2);
    (year, month, day)
}

fn digits(text: &str, width: usize) -> Option<i64> {
    if text.len() == width && text.bytes().all(|byte| byte.is_ascii_digit()) {
        text.parse().ok()
    } else {
        None
    }
}

// Only the form ShipIt writes is a time. A date that does not exist (a February 30th, a month 13,
// a day 0) is none: only a real date comes back from its own day count.
pub(crate) fn seconds(utc: &str) -> Option<i64> {
    let (date, clock) = utc.strip_suffix('Z')?.split_once('T')?;
    let (year, rest) = date.split_once('-')?;
    let (month, day) = rest.split_once('-')?;
    let (hour, rest) = clock.split_once(':')?;
    let (minute, second) = rest.split_once(':')?;
    let (year, month, day) = (digits(year, 4)?, digits(month, 2)?, digits(day, 2)?);
    let (hour, minute, second) = (digits(hour, 2)?, digits(minute, 2)?, digits(second, 2)?);
    let days = days_since_1970(year, month, day);
    let real = date_of(days) == (year, month, day) && hour < 24 && minute < 60 && second < 60;
    real.then_some(days * DAY + hour * 3600 + minute * 60 + second)
}

// A time outside the years 0000 to 9999 has no text in that form.
pub(crate) fn utc_text(at: i64) -> String {
    let (year, month, day) = date_of(at.div_euclid(DAY));
    let second = at.rem_euclid(DAY);
    if (0..=9999).contains(&year) {
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            second / 3600,
            second % 3600 / 60,
            second % 60
        )
    } else {
        String::new()
    }
}

pub(crate) fn file_name(entry: &Entry) -> String {
    match entry {
        Entry::Reserved { tool_use_id, .. } => format!("r-{tool_use_id}"),
        Entry::Running { agent_id, .. } => format!("a-{agent_id}"),
    }
}

// The file name says which entry it is and its text when it was written; a file that is neither
// is not an entry.
pub(crate) fn parse_entry(name: &str, text: &str) -> Option<Entry> {
    let at = seconds(text.trim())?;
    if let Some(id) = name.strip_prefix("r-").filter(|id| safe_id(id)) {
        return Some(Entry::Reserved {
            tool_use_id: id.to_owned(),
            at,
        });
    }
    let id = name.strip_prefix("a-").filter(|id| safe_id(id))?;
    Some(Entry::Running {
        agent_id: id.to_owned(),
        at,
    })
}

pub(crate) fn live(entries: &[Entry], now: i64) -> usize {
    entries
        .iter()
        .filter(|entry| match entry {
            Entry::Running { .. } => true,
            Entry::Reserved { at, .. } => now.saturating_sub(*at) < RESERVATION_SECONDS,
        })
        .count()
}

pub(crate) fn admit(entries: &[Entry], now: i64, cap: u64) -> Result<(), String> {
    let running = live(entries, now);
    if u64::try_from(running).is_ok_and(|running| running < cap) {
        return Ok(());
    }
    Err(format!(
        "ShipIt: {running} agents are already running in this session; the setup allows {cap} ([agents.parallel] agents). Wait for one to finish, then start it again."
    ))
}

fn oldest<'a>(entries: impl Iterator<Item = &'a Entry>) -> Option<&'a Entry> {
    entries.min_by_key(|entry| (entry.at(), file_name(entry)))
}

// A start that was admitted turns its reservation into an agent. One with no reservation (an
// agent that was resumed, or a teammate's message) is counted too, if it is not counted yet.
pub(crate) fn on_start(entries: &[Entry], agent_id: &str, now: i64) -> Vec<Change> {
    let counted = entries
        .iter()
        .any(|entry| matches!(entry, Entry::Running { agent_id: id, .. } if id == agent_id));
    if counted {
        return Vec::new();
    }
    let running = Entry::Running {
        agent_id: agent_id.to_owned(),
        at: now,
    };
    let reservation = oldest(entries.iter().filter(|entry| {
        matches!(entry, Entry::Reserved { at, .. } if now.saturating_sub(*at) < RESERVATION_SECONDS)
    }));
    let mut changes: Vec<Change> = reservation
        .map(|entry| Change::Remove(entry.clone()))
        .into_iter()
        .collect();
    changes.push(Change::Write(running));
    changes
}

pub(crate) fn on_stop(entries: &[Entry], agent_id: &str) -> Vec<Change> {
    entries
        .iter()
        .filter(|entry| matches!(entry, Entry::Running { agent_id: id, .. } if id == agent_id))
        .map(|entry| Change::Remove(entry.clone()))
        .collect()
}

pub(crate) fn on_failure(entries: &[Entry], tool_use_id: &str) -> Vec<Change> {
    entries
        .iter()
        .filter(
            |entry| matches!(entry, Entry::Reserved { tool_use_id: id, .. } if id == tool_use_id),
        )
        .map(|entry| Change::Remove(entry.clone()))
        .collect()
}

// When the turn ends no start is in flight, and no more agents run than the session lists as
// background tasks: the oldest entries beyond that are leaks.
pub(crate) fn reconcile(entries: &[Entry], background_subagents: usize) -> Vec<Change> {
    let mut running: Vec<&Entry> = entries
        .iter()
        .filter(|entry| matches!(entry, Entry::Running { .. }))
        .collect();
    running.sort_by_key(|entry| (entry.at(), file_name(entry)));
    let extra = running.len().saturating_sub(background_subagents);
    let reservations = entries
        .iter()
        .filter(|entry| matches!(entry, Entry::Reserved { .. }));
    reservations
        .chain(running.into_iter().take(extra))
        .map(|entry| Change::Remove(entry.clone()))
        .collect()
}

// A ledger started more than a week ago is taken to belong to a session that is gone: the start
// time is written once, so a session that runs longer loses its count when another session
// starts (a known limit, in the docs). One with no readable start time is not judged.
pub(crate) fn is_stale(started: Option<&str>, now: i64) -> bool {
    started
        .and_then(|text| seconds(text.trim()))
        .is_some_and(|at| now.saturating_sub(at) > STALE_SECONDS)
}

// Outside the store folder: the store's layout is docs/store.md's, and this file is the agent hooks'.
pub(crate) const MAKERS_LOG: &str = "shipit-makers.jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub(crate) enum MakerRecord {
    Start {
        session: String,
        call: String,
        branch: String,
    },
    Launched {
        session: String,
        call: String,
        agent: String,
    },
    Maker {
        session: String,
        agent: String,
    },
    // The call returned an error: nothing was launched, so its start claims no worktree.
    Failed {
        session: String,
        call: String,
    },
}

pub(crate) fn record_line(record: &MakerRecord) -> String {
    // Strings and nothing else are serialized, which cannot fail.
    let mut line = serde_json::to_string(record).unwrap_or_default();
    line.push('\n');
    line
}

fn parse_record(line: &str) -> Option<MakerRecord> {
    serde_json::from_str(line).ok()
}

fn records(log: &str) -> Vec<MakerRecord> {
    log.lines().filter_map(parse_record).collect()
}

// The branch a prompt names with `shipit-branch:` lines, read as the review gate reads them: the
// first word after the colon, a trailing full stop dropped. Two different names, or an empty one,
// are refused in the review gate's own words.
pub(crate) fn branch_line(prompt: &str) -> Result<Option<&str>, &'static str> {
    let mut named: Vec<&str> = Vec::new();
    for line in prompt.lines() {
        let Some(rest) = line.trim().strip_prefix(BRANCH_LINE) else {
            continue;
        };
        let word = rest
            .split_whitespace()
            .next()
            .map(|word| word.trim_end_matches('.'))
            .filter(|word| !word.is_empty())
            .ok_or("the shipit-branch line names no branch")?;
        if !named.contains(&word) {
            named.push(word);
        }
    }
    match named.as_slice() {
        [] => Ok(None),
        [branch] => Ok(Some(branch)),
        [_, _, ..] => Err("the prompt names two branches"),
    }
}

// The worktree Claude Code made for agent <id>: `<anything>/.claude/worktrees/agent-<id>`.
pub(crate) fn agent_of_worktree(top: &Path) -> Option<String> {
    let id = top.file_name()?.to_str()?.strip_prefix("agent-")?;
    let worktrees = top.parent()?;
    let claude = worktrees.parent()?;
    (worktrees.file_name()? == "worktrees" && claude.file_name()? == ".claude" && safe_id(id))
        .then(|| id.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MakerSlice {
    NoRecord,
    Known(String),
    Unknown,
}

// The call that launched the agent, and then the start of that same call: the pairing is exact,
// in any order of the lines.
pub(crate) fn maker_slice(log: &str, agent: &str) -> MakerSlice {
    let records = records(log);
    let launched = records.iter().find_map(|record| match record {
        MakerRecord::Launched {
            session,
            call,
            agent: launched,
        } if launched == agent => Some((session, call)),
        MakerRecord::Launched { .. }
        | MakerRecord::Start { .. }
        | MakerRecord::Maker { .. }
        | MakerRecord::Failed { .. } => None,
    });
    if let Some((launched_session, launched_call)) = launched {
        let start = records.iter().find_map(|record| match record {
            MakerRecord::Start {
                session,
                call,
                branch,
            } if session == launched_session && call == launched_call => Some(branch),
            MakerRecord::Start { .. }
            | MakerRecord::Launched { .. }
            | MakerRecord::Maker { .. }
            | MakerRecord::Failed { .. } => None,
        });
        return start.map_or(MakerSlice::Unknown, |branch| {
            MakerSlice::Known(branch.clone())
        });
    }
    let started = records
        .iter()
        .any(|record| matches!(record, MakerRecord::Maker { agent: made, .. } if made == agent));
    if started {
        MakerSlice::Unknown
    } else {
        MakerSlice::NoRecord
    }
}

// A SubagentStart of an agent the log knows as a launched isolated maker, outside its own
// worktree: a resume. A maker with no launch line may be one that never had a worktree of its own.
pub(crate) fn resumed_away(log: &str, agent: &str, cwd: &Path) -> bool {
    records(log).iter().any(
        |record| matches!(record, MakerRecord::Launched { agent: launched, .. } if launched == agent),
    ) && agent_of_worktree(cwd).as_deref() != Some(agent)
}

// A start that no launch or failure has claimed: a foreground maker, whose launch is told only
// when it ends, or a start whose launch line was lost.
fn unpaired_start(log: &str, branch: &str) -> bool {
    let records = records(log);
    records.iter().any(|record| match record {
        MakerRecord::Start {
            session,
            call,
            branch: named,
        } if named == branch => !records.iter().any(|other| {
            matches!(other, MakerRecord::Launched { session: s, call: c, .. } | MakerRecord::Failed { session: s, call: c } if s == session && c == call)
        }),
        MakerRecord::Start { .. }
        | MakerRecord::Launched { .. }
        | MakerRecord::Maker { .. }
        | MakerRecord::Failed { .. } => false,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommitVerdict {
    Pass,
    Warn(String),
    Refuse(String),
}

const UNKNOWN_SLICE: &str = "not checked: this maker's slice branch is not known; start it again with run_in_background (docs/claude-code.md, Parallel makers)";

fn refusal(branch: &str) -> String {
    format!(
        "this worktree is shipit:maker's for {branch}; commit there: git switch {branch} (docs/claude-code.md, Parallel makers)"
    )
}

fn detached(branch: &str) -> String {
    format!(
        "not checked: HEAD is detached in this maker's worktree, so its commit cannot be matched to its slice branch {branch} (docs/claude-code.md, Parallel makers)"
    )
}

const NO_COMMIT_LINE: &str = "not checked: the pre-commit hook of this worktree has no shipit pre-commit line, so a commit here may not be checked (docs/claude-code.md, Parallel makers)";

fn unpaired(branch: &str) -> String {
    format!(
        "not checked: no maker record names this worktree, but a maker was started for {branch} (docs/claude-code.md, Parallel makers)"
    )
}

// The only refusal is a known maker's worktree on a branch that is not its slice's; every doubt
// passes, silent or with a warning. A detached HEAD is a rebase stop or an amend, no switch away.
pub(crate) fn commit_verdict(top: &Path, branch: Option<&str>, log: Option<&str>) -> CommitVerdict {
    let (Some(agent), Some(log)) = (agent_of_worktree(top), log) else {
        return CommitVerdict::Pass;
    };
    match maker_slice(log, &agent) {
        MakerSlice::Known(slice) => match branch {
            Some(named) if named == slice => CommitVerdict::Pass,
            Some(_other) => CommitVerdict::Refuse(refusal(&slice)),
            None => CommitVerdict::Warn(detached(&slice)),
        },
        MakerSlice::Unknown => CommitVerdict::Warn(UNKNOWN_SLICE.to_owned()),
        MakerSlice::NoRecord => match branch.filter(|named| unpaired_start(log, named)) {
            Some(named) => CommitVerdict::Warn(unpaired(named)),
            None => CommitVerdict::Pass,
        },
    }
}

// The self-check in an agent worktree finds a hook file that never runs the check: a hooks folder
// that is generated or not committed is missing there.
pub(crate) fn missing_commit_line(top: &Path, hook: Option<&str>) -> Option<String> {
    agent_of_worktree(top)?;
    hook.is_none_or(|text| !text.contains("shipit pre-commit"))
        .then(|| NO_COMMIT_LINE.to_owned())
}

pub(crate) fn subagent_context(text: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "SubagentStart",
            "additionalContext": text,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use std::path::PathBuf;

    use super::*;
    use crate::pure::hooks::COMMIT_LINE;

    // 0000-01-01T00:00:00Z and 9999-12-31T23:59:59Z.
    const FIRST: i64 = -62_167_219_200;
    const LAST: i64 = 253_402_300_799;

    fn id() -> impl Strategy<Value = String> {
        "[a-z0-9_-]{1,8}"
    }

    fn entry() -> impl Strategy<Value = Entry> {
        prop_oneof![
            (id(), 0_i64..1000).prop_map(|(tool_use_id, at)| Entry::Reserved { tool_use_id, at }),
            (id(), 0_i64..1000).prop_map(|(agent_id, at)| Entry::Running { agent_id, at }),
        ]
    }

    // A ledger holds one entry per file name.
    fn unique(entries: Vec<Entry>) -> Vec<Entry> {
        let mut kept: Vec<Entry> = Vec::new();
        for entry in entries {
            if kept.iter().all(|have| file_name(have) != file_name(&entry)) {
                kept.push(entry);
            }
        }
        kept
    }

    fn applied(mut entries: Vec<Entry>, changes: &[Change]) -> Vec<Entry> {
        for change in changes {
            match change {
                Change::Write(entry) => {
                    entries.retain(|have| file_name(have) != file_name(entry));
                    entries.push(entry.clone());
                }
                Change::Remove(entry) => entries.retain(|have| file_name(have) != file_name(entry)),
            }
        }
        entries
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn never_admits_at_cap(
            entries in proptest::collection::vec(entry(), 0..8),
            now in 0_i64..1200,
            cap in 1_u64..8,
        ) {
            let counted = live(&entries, now);
            prop_assert_eq!(admit(&entries, now, cap).is_ok(), u64::try_from(counted).is_ok_and(|n| n < cap));
        }

        #[test]
        fn start_then_stop_returns_to_zero(
            order in (1_usize..6).prop_flat_map(|n| {
                let turns: Vec<usize> = (0..n).flat_map(|i| [i, i, i]).collect();
                Just(turns).prop_shuffle()
            }),
            at in 0_i64..1000,
        ) {
            let mut seen = [0_u8; 6];
            let mut entries: Vec<Entry> = Vec::new();
            for who in order {
                let phase = seen.get_mut(who).expect("an id below six");
                let name = format!("x{who}");
                let changes = match *phase {
                    0 => vec![Change::Write(Entry::Reserved { tool_use_id: name, at })],
                    1 => on_start(&entries, &name, at),
                    _ => on_stop(&entries, &name),
                };
                *phase += 1;
                entries = applied(entries, &changes);
            }
            prop_assert_eq!(live(&entries, at), 0);
            prop_assert_eq!(entries, Vec::<Entry>::new());
        }

        #[test]
        fn a_started_agent_is_counted_once(
            entries in proptest::collection::vec(entry(), 0..6).prop_map(unique),
            agent in id(),
            now in 0_i64..1000,
        ) {
            let once = applied(entries, &on_start(&[], &agent, now));
            prop_assert!(on_start(&once, &agent, now + 1).is_empty());
            let running = once.iter().filter(|e| matches!(e, Entry::Running { agent_id, .. } if *agent_id == agent)).count();
            prop_assert_eq!(running, 1);
        }

        #[test]
        fn leaked_reservations_expire(tool_use_id in id(), at in 0_i64..1000, age in 0_i64..400) {
            let entries = [Entry::Reserved { tool_use_id, at }];
            prop_assert_eq!(live(&entries, at + age), usize::from(age < 120));
        }

        #[test]
        fn a_failed_call_frees_its_reservation(
            entries in proptest::collection::vec(entry(), 0..6).prop_map(unique),
            tool_use_id in id(),
            now in 0_i64..1000,
        ) {
            let reserved = Entry::Reserved { tool_use_id: tool_use_id.clone(), at: now };
            let before = applied(entries, &[Change::Write(reserved.clone())]);
            let after = applied(before.clone(), &on_failure(&before, &tool_use_id));
            prop_assert!(!after.contains(&reserved));
            prop_assert_eq!(after.len(), before.len() - 1);
        }

        #[test]
        fn stop_reconciles_down(
            entries in proptest::collection::vec(entry(), 0..8).prop_map(unique),
            background in 0_usize..8,
            now in 0_i64..1200,
        ) {
            let after = applied(entries.clone(), &reconcile(&entries, background));
            prop_assert!(live(&after, now) <= live(&entries, now));
            let running = after.iter().filter(|e| matches!(e, Entry::Running { .. })).count();
            let before = entries.iter().filter(|e| matches!(e, Entry::Running { .. })).count();
            prop_assert_eq!(running, before.min(background));
            let only_running = after.iter().all(|e| matches!(e, Entry::Running { .. }));
            prop_assert!(only_running);
        }

        #[test]
        fn seconds_round_trips(at in FIRST..=LAST) {
            let text = utc_text(at);
            prop_assert_eq!(seconds(&text), Some(at));
            prop_assert_eq!(seconds(&format!("{text}\n")), None);
            prop_assert_eq!(parse_entry("a-x", &format!("{text}\n")), Some(Entry::Running { agent_id: "x".to_owned(), at }));
        }

        #[test]
        fn a_time_reads_iff_its_date_exists(
            year in 0_i64..10_000,
            month in 0_i64..14,
            day in 0_i64..33,
            hour in 0_i64..26,
            minute in 0_i64..62,
            second in 0_i64..62,
        ) {
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            let month_days = match month {
                2 if leap => 29,
                2 => 28,
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            let exists = (1..=12).contains(&month)
                && (1..=month_days).contains(&day)
                && hour < 24
                && minute < 60
                && second < 60;
            let text = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z");
            prop_assert_eq!(seconds(&text).is_some(), exists);
            prop_assert_eq!(seconds(&format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:60Z")), None);
        }

        #[test]
        fn a_text_reads_only_if_it_is_the_text_of_its_second(at in FIRST..=LAST) {
            let written = utc_text(at);
            for cut in 0..written.len() {
                let (head, tail) = written.split_at(cut);
                let tail = tail.get(1..).unwrap_or("");
                for put in ["", "+", "-", " ", "t", "z", ".", "0", ":", "T", "Z"] {
                    let text = format!("{head}{put}{tail}");
                    if let Some(read) = seconds(&text) {
                        prop_assert_eq!(utc_text(read), text);
                    }
                }
            }
        }

        #[test]
        fn the_next_second_has_the_next_text(at in FIRST..LAST) {
            prop_assert!(utc_text(at) < utc_text(at + 1));
        }

        #[test]
        fn a_time_outside_the_years_0000_to_9999_has_no_text(
            at in prop_oneof![
                i64::MIN..FIRST,
                (FIRST - 1_000_000_000_000)..FIRST,
                (LAST + 1)..(LAST + 1_000_000_000_000),
                (LAST + 1)..=i64::MAX,
                Just(LAST + 1),
                Just(FIRST - 1),
            ],
        ) {
            prop_assert_eq!(utc_text(at), "");
        }

        #[test]
        fn entries_round_trip_through_their_files(entry in entry()) {
            let text = utc_text(entry.at());
            prop_assert_eq!(parse_entry(&file_name(&entry), &text), Some(entry));
        }

        #[test]
        fn a_file_that_is_not_an_entry_is_not_read(name in "[a-z.-]{0,8}", text in "[a-z0-9 :-]{0,20}") {
            prop_assume!(seconds(&text).is_none());
            prop_assert_eq!(parse_entry(&name, &text), None);
        }

        #[test]
        fn safe_id_accepts_only_the_set(text in "[a-zA-Z0-9_./ :-]{0,120}") {
            let in_set = !text.is_empty()
                && text.len() <= 100
                && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            prop_assert_eq!(safe_id(&text), in_set);
        }

        #[test]
        fn a_ledger_is_stale_after_a_week_and_only_with_a_start_time(
            at in 0_i64..2_000_000_000,
            age in 0_i64..1_000_000,
        ) {
            let text = utc_text(at);
            prop_assert_eq!(is_stale(Some(&text), at + age), age > 7 * 24 * 60 * 60);
            prop_assert!(!is_stale(None, at + age));
            prop_assert!(!is_stale(Some("not a time"), at + age));
        }
    }

    fn word() -> impl Strategy<Value = String> {
        "[a-z0-9]{1,8}"
    }

    fn slice_branch() -> impl Strategy<Value = String> {
        "slice/[a-z]{1,4}/[a-z]{1,6}"
    }

    fn maker_record() -> impl Strategy<Value = MakerRecord> {
        prop_oneof![
            (word(), word(), slice_branch()).prop_map(|(session, call, branch)| {
                MakerRecord::Start {
                    session,
                    call,
                    branch,
                }
            }),
            (word(), word(), word()).prop_map(|(session, call, agent)| MakerRecord::Launched {
                session,
                call,
                agent
            }),
            (word(), word()).prop_map(|(session, agent)| MakerRecord::Maker { session, agent }),
            (word(), word()).prop_map(|(session, call)| MakerRecord::Failed { session, call }),
        ]
    }

    fn log_of(records: &[MakerRecord]) -> String {
        records.iter().map(record_line).collect()
    }

    // What the hooks wrote for one call of a lead. A decoy is the start of the same call in
    // another session.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Line {
        Start,
        Decoy,
        Launched,
        Maker,
    }

    #[derive(Debug, Clone)]
    struct Call {
        session: &'static str,
        branch: String,
        lines: Vec<Line>,
    }

    impl Call {
        fn has(&self, line: Line) -> bool {
            self.lines.contains(&line)
        }
    }

    fn call() -> impl Strategy<Value = Call> {
        (
            proptest::sample::select(vec!["s1", "s2"]),
            slice_branch(),
            proptest::sample::subsequence(
                vec![Line::Start, Line::Decoy, Line::Launched, Line::Maker],
                0..=4,
            ),
        )
            .prop_map(|(session, branch, lines)| Call {
                session,
                branch,
                lines,
            })
    }

    fn lines_of_calls(calls: &[Call]) -> Vec<MakerRecord> {
        let mut records = Vec::new();
        for (at, call) in calls.iter().enumerate() {
            let (id, agent) = (format!("t{at}"), format!("ag{at}"));
            let session = call.session.to_owned();
            let other = if call.session == "s1" { "s2" } else { "s1" };
            for line in &call.lines {
                records.push(match line {
                    Line::Start => MakerRecord::Start {
                        session: session.clone(),
                        call: id.clone(),
                        branch: call.branch.clone(),
                    },
                    Line::Decoy => MakerRecord::Start {
                        session: other.to_owned(),
                        call: id.clone(),
                        branch: "slice/zz/decoy".to_owned(),
                    },
                    Line::Launched => MakerRecord::Launched {
                        session: session.clone(),
                        call: id.clone(),
                        agent: agent.clone(),
                    },
                    Line::Maker => MakerRecord::Maker {
                        session: session.clone(),
                        agent: agent.clone(),
                    },
                });
            }
        }
        records
    }

    fn small_record() -> impl Strategy<Value = MakerRecord> {
        let session = || proptest::sample::select(vec!["s1", "s2"]).prop_map(str::to_owned);
        let call = || proptest::sample::select(vec!["c1", "c2", "c3"]).prop_map(str::to_owned);
        let agent = || proptest::sample::select(vec!["a1", "a2"]).prop_map(str::to_owned);
        let branch =
            || proptest::sample::select(vec!["slice/g/x", "slice/g/y"]).prop_map(str::to_owned);
        prop_oneof![
            (session(), call(), branch()).prop_map(|(session, call, branch)| MakerRecord::Start {
                session,
                call,
                branch
            }),
            (session(), call(), agent()).prop_map(|(session, call, agent)| {
                MakerRecord::Launched {
                    session,
                    call,
                    agent,
                }
            }),
            (session(), agent()).prop_map(|(session, agent)| MakerRecord::Maker { session, agent }),
            (session(), call()).prop_map(|(session, call)| MakerRecord::Failed { session, call }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn maker_records_round_trip(
            record in maker_record(),
            junk in "[a-z ]{0,10}",
            agent in word(),
        ) {
            let line = record_line(&record);
            prop_assert!(line.ends_with('\n'));
            prop_assert_eq!(line.lines().count(), 1);
            prop_assert_eq!(parse_record(line.trim_end()), Some(record));
            // a line that is not a record is not read, and the lines around it still are
            prop_assert_eq!(parse_record(&junk), None);
            prop_assert_eq!(
                maker_slice(&format!("{junk}\n{line}"), &agent),
                maker_slice(&line, &agent)
            );
        }

        #[test]
        fn branch_line_reads_one_name(
            free in proptest::collection::vec("[a-z ]{0,10}", 0..3),
            first in "[a-z][a-z/0-9-]{0,10}",
            second in "[a-z][a-z/0-9-]{0,10}",
            times in 1_usize..4,
            stops in proptest::collection::vec(any::<bool>(), 3),
        ) {
            let free = free.join("\n");
            prop_assert_eq!(branch_line(&free), Ok(None));
            let named: Vec<String> = stops
                .iter()
                .take(times)
                .map(|stop| format!("  shipit-branch: {first}{}", if *stop { "." } else { "" }))
                .collect();
            let prompt = format!("{free}\n{}\n{free}", named.join("\n"));
            prop_assert_eq!(branch_line(&prompt), Ok(Some(first.as_str())));
            if first != second {
                let two = format!("shipit-branch: {first}\n{free}\nshipit-branch: {second}");
                prop_assert_eq!(branch_line(&two), Err("the prompt names two branches"));
            }
            for empty in ["shipit-branch:", "shipit-branch:   ", "shipit-branch: .."] {
                let prompt = format!("shipit-branch: {first}\n{empty}");
                prop_assert_eq!(
                    branch_line(&prompt),
                    Err("the shipit-branch line names no branch")
                );
            }
        }

        #[test]
        fn a_maker_worktree_is_paired_with_its_start_by_call(
            (calls, shuffled) in proptest::collection::vec(call(), 1..6).prop_flat_map(|calls| {
                let records = lines_of_calls(&calls);
                (Just(calls), Just(records).prop_shuffle())
            }),
        ) {
            let log = log_of(&shuffled);
            for (at, call) in calls.iter().enumerate() {
                let expected = if call.has(Line::Launched) && call.has(Line::Start) {
                    MakerSlice::Known(call.branch.clone())
                } else if call.has(Line::Launched) || call.has(Line::Maker) {
                    MakerSlice::Unknown
                } else {
                    MakerSlice::NoRecord
                };
                prop_assert_eq!(maker_slice(&log, &format!("ag{at}")), expected);
            }
            prop_assert_eq!(maker_slice(&log, "nobody"), MakerSlice::NoRecord);
        }

        #[test]
        fn an_agent_worktree_is_read_from_its_path(
            parent in "(/[a-z]{1,6}){1,3}",
            id in "[a-zA-Z0-9_-]{1,20}",
        ) {
            let top = |rest: &str| PathBuf::from(format!("{parent}/{rest}"));
            let own = top(&format!(".claude/worktrees/agent-{id}"));
            prop_assert_eq!(agent_of_worktree(&own), Some(id.clone()));
            for other in [
                format!(".claude/trees/agent-{id}"),
                format!("claude/worktrees/agent-{id}"),
                format!(".claude/worktrees/work-{id}"),
                format!(".claude/worktrees/agent-{id}."),
                ".claude/worktrees/agent-".to_owned(),
            ] {
                prop_assert_eq!(agent_of_worktree(&top(&other)), None, "{}", other);
            }
            // a resume is a launched maker whose folder is not its own worktree; a maker that only
            // started is not one: it may be a maker that never had a worktree of its own
            let launched = [
                MakerRecord::Start { session: "s".to_owned(), call: "c".to_owned(), branch: "slice/g/x".to_owned() },
                MakerRecord::Launched { session: "s".to_owned(), call: "c".to_owned(), agent: id.clone() },
            ];
            let made = [MakerRecord::Maker { session: "s".to_owned(), agent: id.clone() }];
            let log = log_of(&launched);
            prop_assert!(!resumed_away(&log, &id, &own));
            prop_assert!(resumed_away(&log, &id, Path::new(&parent)));
            prop_assert!(resumed_away(&log, &id, &top(".claude/worktrees/agent-other.")));
            prop_assert!(!resumed_away(&log_of(&made), &id, Path::new(&parent)));
            prop_assert!(!resumed_away("", &id, Path::new(&parent)));
            prop_assert!(!resumed_away(&log_of(&launched), "nobody", Path::new(&parent)));
        }

        #[test]
        fn the_self_check_finds_a_hook_that_has_no_commit_line(
            parent in "(/[a-z]{1,6}){1,3}",
            id in "[a-z0-9]{1,8}",
            text in "[a-z #!/]{0,30}",
        ) {
            let own = PathBuf::from(format!("{parent}/.claude/worktrees/agent-{id}"));
            let with_line = format!("{text}\n{COMMIT_LINE}\n");
            prop_assert_eq!(missing_commit_line(&own, Some(&with_line)), None);
            prop_assert!(missing_commit_line(&own, Some(&text)).is_some());
            prop_assert!(missing_commit_line(&own, None).is_some());
            prop_assert_eq!(missing_commit_line(Path::new(&parent), None), None);
            prop_assert_eq!(missing_commit_line(Path::new(&parent), Some(&text)), None);
        }

        #[test]
        fn the_commit_verdict_refuses_only_a_known_maker_off_its_branch(
            top in prop_oneof![
                proptest::sample::select(vec!["a1", "a2", "a3"])
                    .prop_map(|id| PathBuf::from(format!("/r/.claude/worktrees/agent-{id}"))),
                Just(PathBuf::from("/r")),
                Just(PathBuf::from("/r/.claude/worktrees/other")),
            ],
            branch in proptest::option::of(proptest::sample::select(vec!["slice/g/x", "slice/g/y", "main"])),
            records in proptest::option::of(proptest::collection::vec(small_record(), 0..8)),
        ) {
            let log = records.as_deref().map(log_of);
            let verdict = commit_verdict(&top, branch, log.as_deref());
            let launched_for = |session: &String, call: &String| {
                records.iter().flatten().any(|other| {
                    matches!(other, MakerRecord::Launched { session: s, call: c, .. } if s == session && c == call)
                })
            };
            let failed_for = |session: &String, call: &String| {
                records.iter().flatten().any(|other| {
                    matches!(other, MakerRecord::Failed { session: s, call: c } if s == session && c == call)
                })
            };
            let names_unpaired = |wanted: &str| {
                records.iter().flatten().any(|record| {
                    matches!(record, MakerRecord::Start { session, call, branch }
                        if branch == wanted && !launched_for(session, call) && !failed_for(session, call))
                })
            };
            let slice = agent_of_worktree(&top)
                .zip(log.as_deref())
                .map(|(agent, log)| maker_slice(log, &agent));
            let refuse = matches!(&slice, Some(MakerSlice::Known(b)) if branch.is_some_and(|named| named != b.as_str()));
            let detached = matches!(&slice, Some(MakerSlice::Known(_))) && branch.is_none();
            let warn = match &slice {
                Some(MakerSlice::Unknown) => true,
                Some(MakerSlice::NoRecord) => branch.is_some_and(names_unpaired),
                Some(MakerSlice::Known(_)) => detached,
                None => false,
            };
            match (&verdict, &slice) {
                (CommitVerdict::Refuse(text), Some(MakerSlice::Known(b))) => {
                    prop_assert!(refuse);
                    prop_assert_eq!(text.matches(b.as_str()).count(), 2);
                }
                (CommitVerdict::Warn(text), Some(MakerSlice::Known(b))) => {
                    prop_assert!(detached);
                    prop_assert!(text.contains(b.as_str()));
                }
                (CommitVerdict::Warn(_), _) => prop_assert!(warn),
                (CommitVerdict::Pass, _) => prop_assert!(!refuse && !warn),
                (CommitVerdict::Refuse(_), _) => prop_assert!(false, "a refusal without a known maker"),
            }
            prop_assert_eq!(matches!(verdict, CommitVerdict::Warn(_)), warn);
            prop_assert_eq!(matches!(verdict, CommitVerdict::Refuse(_)), refuse);
        }
    }

    #[test]
    fn the_cap_message_is_exact() {
        let entries = [
            Entry::Running {
                agent_id: "a".to_owned(),
                at: 0,
            },
            Entry::Running {
                agent_id: "b".to_owned(),
                at: 0,
            },
        ];
        assert_eq!(
            admit(&entries, 5, 2),
            Err("ShipIt: 2 agents are already running in this session; the setup allows 2 ([agents.parallel] agents). Wait for one to finish, then start it again.".to_owned())
        );
        assert_eq!(admit(&entries, 5, 3), Ok(()));
    }

    // Ledger files written while the clock went through chrono still read, to the same second.
    #[test]
    fn files_written_with_chrono_still_read() {
        for (text, at) in [
            ("0000-01-01T00:00:00Z", -62_167_219_200),
            ("1969-12-31T23:59:59Z", -1),
            ("1970-01-01T00:00:00Z", 0),
            ("2000-02-29T00:00:00Z", 951_782_400),
            ("2026-11-22T16:13:17Z", 1_795_363_997),
            ("2100-03-01T00:00:00Z", 4_107_542_400),
            ("9999-12-31T23:59:59Z", 253_402_300_799),
        ] {
            assert_eq!(seconds(text), Some(at), "{text}");
            assert_eq!(utc_text(at), text);
        }
    }
}
