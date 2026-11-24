use std::collections::BTreeMap;

use crate::pure::goal::{DoneWhen, goal_item_texts};
use crate::pure::logbook::{Entry, Event, Logged, StoredVersion};
use crate::pure::receipt::Oid;
use crate::pure::roadmap::SliceName;
use crate::pure::store_path::OwnerFile;

fn stored_of(history: &[Event], kind: OwnerFile) -> impl Iterator<Item = &StoredVersion> {
    history.iter().filter_map(move |event| match event {
        Event::Stored(stored) if stored.kind == kind => Some(stored),
        Event::Stored(_) | Event::Logged(_) => None,
    })
}

// The slice's work began at its first start entry; a restart does not move it.
pub(crate) fn started_at(history: &[Event], slice: &SliceName) -> Option<usize> {
    history.iter().position(|event| match event {
        Event::Logged(Logged {
            slice: logged,
            entry: Entry::Started { .. },
        }) => logged == slice,
        Event::Logged(_) | Event::Stored(_) => false,
    })
}

// From the version recorded before the start (else the highest in the store below the first
// recorded after it) to the newest in the store.
pub(crate) fn span(
    history: &[Event],
    start: usize,
    kind: OwnerFile,
    in_store: &[u64],
) -> Option<(Option<u64>, u64)> {
    let before = history.get(..start).unwrap_or_default();
    let after = history.get(start..).unwrap_or_default();
    let to = in_store.iter().copied().max()?;
    let earlier = stored_of(before, kind).map(|stored| stored.version).max();
    let first_after = stored_of(after, kind).map(|stored| stored.version).min();
    let from = earlier.or_else(|| {
        let lowest = first_after?;
        in_store.iter().copied().filter(|n| *n < lowest).max()
    });
    match from {
        Some(from) => (to > from).then_some((Some(from), to)),
        None => first_after.map(|_| (None, to)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flag {
    NotRecorded,
    ChangedByHand,
}

// A goal with no record of a kind shows only its newest version as not recorded: older ones
// cannot be told from versions put before recording began.
pub(crate) fn flags(
    history: &[Event],
    kind: OwnerFile,
    in_store: &[(u64, Option<Oid>)],
) -> Vec<(u64, Flag)> {
    let mut last: BTreeMap<u64, &Oid> = BTreeMap::new();
    for stored in stored_of(history, kind) {
        last.insert(stored.version, &stored.hash);
    }
    let mut found: Vec<&(u64, Option<Oid>)> = in_store.iter().collect();
    found.sort_by_key(|(version, _)| *version);
    let Some(lowest) = last.keys().next().copied() else {
        return found
            .last()
            .map(|(version, _)| vec![(*version, Flag::NotRecorded)])
            .unwrap_or_default();
    };
    found
        .into_iter()
        .filter_map(|(version, hash)| {
            let flag = match last.get(version) {
                None => (*version > lowest).then_some(Flag::NotRecorded),
                Some(recorded) => (hash.as_ref() != Some(*recorded)).then_some(Flag::ChangedByHand),
            };
            flag.map(|flag| (*version, flag))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemChange {
    Removed,
    Reworded,
}

// By id order; an added id is not listed.
pub(crate) fn item_changes(old: &str, new: &str) -> Result<Vec<(DoneWhen, ItemChange)>, String> {
    let old = goal_item_texts(old)?;
    let new = goal_item_texts(new)?;
    let mut changes: Vec<(DoneWhen, ItemChange)> = old
        .iter()
        .filter_map(
            |(id, text)| match new.iter().find(|(found, _)| found == id) {
                None => Some((*id, ItemChange::Removed)),
                Some((_, now)) if now != text => Some((*id, ItemChange::Reworded)),
                Some(_) => None,
            },
        )
        .collect();
    changes.sort_by_key(|(id, _)| *id);
    Ok(changes)
}

// The items are read only for a goal that has a version to compare with.
pub(crate) fn span_line(
    kind: OwnerFile,
    (from, to): (Option<u64>, u64),
    items: Option<Result<Vec<(DoneWhen, ItemChange)>, String>>,
) -> String {
    let stem = kind.stem();
    let head = match from {
        Some(from) => format!("{stem} v{from} -> v{to} since start"),
        None => format!("{stem} v{to} since start"),
    };
    match items {
        None => head,
        Some(Err(_why)) => format!("{head}: done-when items not readable"),
        Some(Ok(changes)) if changes.is_empty() => head,
        Some(Ok(changes)) => {
            let named: Vec<String> = changes
                .iter()
                .map(|(id, change)| match change {
                    ItemChange::Removed => format!("{id} removed"),
                    ItemChange::Reworded => format!("{id} reworded"),
                })
                .collect();
            format!("{head}: {}", named.join(", "))
        }
    }
}

pub(crate) fn flag_line(kind: OwnerFile, version: u64, flag: Flag) -> String {
    let stem = kind.stem();
    match flag {
        Flag::NotRecorded => format!("{stem} v{version}: stored by hand, not recorded"),
        Flag::ChangedByHand => format!("{stem} v{version}: changed by hand after it was stored"),
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use super::*;

    fn slice(name: &str) -> SliceName {
        SliceName::parse(name).expect("a slice name")
    }

    fn oid(n: u8) -> Oid {
        Oid::parse(&format!("{n:040x}")).expect("40 hex digits")
    }

    fn stored(kind: OwnerFile, version: u64, hash: u8) -> Event {
        Event::Stored(StoredVersion {
            kind,
            version,
            hash: oid(hash),
        })
    }

    fn started(name: &str) -> Event {
        Event::Logged(Logged {
            slice: slice(name),
            entry: Entry::Started {
                branch: format!("slice/demo/{name}"),
            },
        })
    }

    fn kind() -> impl Strategy<Value = OwnerFile> {
        prop_oneof![Just(OwnerFile::Goal), Just(OwnerFile::Roadmap)]
    }

    fn goal_text(items: &BTreeMap<u64, String>) -> String {
        let mut text = "## Done-when\n\n".to_owned();
        for (id, words) in items {
            writeln!(text, "{id}. {words}").ok();
        }
        text
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_span_runs_from_the_version_before_the_start_to_the_newest(
            before in proptest::collection::vec((kind(), 1_u64..12), 0..6),
            after in proptest::collection::vec((kind(), 1_u64..12), 0..6),
            other_start in any::<bool>(),
            restart in any::<bool>(),
            store in proptest::collection::btree_set(1_u64..14, 0..8),
            wanted in kind(),
        ) {
            let mut history: Vec<Event> = Vec::new();
            if other_start {
                history.push(started("gamma"));
            }
            history.extend(before.iter().map(|(k, v)| stored(*k, *v, 1)));
            let start = history.len();
            history.push(started("beta"));
            history.extend(after.iter().map(|(k, v)| stored(*k, *v, 2)));
            if restart {
                history.push(started("beta"));
            }
            prop_assert_eq!(started_at(&history, &slice("beta")), Some(start));
            prop_assert_eq!(started_at(&history, &slice("alpha")), None);
            prop_assert_eq!(span(&history, start, wanted, &[]), None);

            let in_store: Vec<u64> = store.iter().copied().collect();
            let earlier = before.iter().filter(|(k, _)| *k == wanted).map(|(_, v)| *v).max();
            let later = after.iter().filter(|(k, _)| *k == wanted).map(|(_, v)| *v).min();
            let from = earlier.or_else(|| {
                let lowest = later?;
                store.iter().copied().filter(|n| *n < lowest).max()
            });
            let expected = store.iter().copied().max().and_then(|to| match from {
                Some(from) => (to > from).then_some((Some(from), to)),
                None => later.map(|_| (None, to)),
            });
            prop_assert_eq!(span(&history, start, wanted, &in_store), expected);
            if let Some((Some(from), to)) = expected {
                prop_assert!(from < to);
            }
        }

        #[test]
        fn a_version_is_flagged_when_unrecorded_or_changed_by_hand(
            records in proptest::collection::vec((kind(), 1_u64..10, 0_u8..3), 0..8),
            files in proptest::collection::btree_map(1_u64..12, proptest::option::of(0_u8..3), 0..8),
            wanted in kind(),
        ) {
            let history: Vec<Event> = records.iter().map(|(k, v, h)| stored(*k, *v, *h)).collect();
            let in_store: Vec<(u64, Option<Oid>)> =
                files.iter().map(|(v, h)| (*v, h.map(oid))).collect();
            let mut recorded: BTreeMap<u64, u8> = BTreeMap::new();
            for (k, v, h) in &records {
                if *k == wanted {
                    recorded.insert(*v, *h);
                }
            }
            let mut expected: Vec<(u64, Flag)> = Vec::new();
            match recorded.keys().next() {
                None => {
                    if let Some(newest) = files.keys().next_back() {
                        expected.push((*newest, Flag::NotRecorded));
                    }
                }
                Some(lowest) => {
                    for (version, hash) in &files {
                        match recorded.get(version) {
                            None if version > lowest => expected.push((*version, Flag::NotRecorded)),
                            Some(kept) if *hash != Some(*kept) => {
                                expected.push((*version, Flag::ChangedByHand));
                            }
                            None | Some(_) => {}
                        }
                    }
                }
            }
            prop_assert_eq!(flags(&history, wanted, &in_store), expected);
        }

        #[test]
        fn item_changes_name_removed_and_reworded_ids(
            old in proptest::collection::btree_map(1_u64..8, "[a-z]{1,4}( [a-z]{1,4}){0,2}", 1..6),
            new in proptest::collection::btree_map(1_u64..8, "[a-z]{1,4}( [a-z]{1,4}){0,2}", 1..6),
        ) {
            let expected: Vec<(String, ItemChange)> = old
                .iter()
                .filter_map(|(id, words)| match new.get(id) {
                    None => Some((format!("DW{id}"), ItemChange::Removed)),
                    Some(now) if now != words => Some((format!("DW{id}"), ItemChange::Reworded)),
                    Some(_) => None,
                })
                .collect();
            let found = item_changes(&goal_text(&old), &goal_text(&new)).expect("readable goals");
            let named: Vec<(String, ItemChange)> = found
                .iter()
                .map(|(id, change)| (id.to_string(), *change))
                .collect();
            prop_assert_eq!(named, expected);
            prop_assert!(item_changes("no items", &goal_text(&new)).is_err());
        }
    }
}
