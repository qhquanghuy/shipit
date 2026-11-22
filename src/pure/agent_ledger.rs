// How many agents a session has running, kept as one entry per agent or per start in flight.
// The files are the edge's; what an entry means, and what each hook event does to the entries,
// is decided here.

use chrono::DateTime;

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

pub(crate) fn seconds(utc: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(utc)
        .ok()
        .map(|time| time.timestamp())
}

pub(crate) fn utc_text(at: i64) -> String {
    DateTime::from_timestamp(at, 0)
        .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

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
        fn seconds_round_trips(at in 0_i64..4_102_444_800) {
            let text = utc_text(at);
            prop_assert_eq!(seconds(&text), Some(at));
            prop_assert_eq!(seconds(&format!("{text}\n")), None);
            prop_assert_eq!(parse_entry("a-x", &format!("{text}\n")), Some(Entry::Running { agent_id: "x".to_owned(), at }));
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
}
