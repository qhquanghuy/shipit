use std::fmt;

use toml::{Table, Value};

use crate::pure::block::toml_block;
use crate::pure::logbook::{Entry, Logged, is_shipped_by};
use crate::pure::messages::{
    ENTRIES_APART, ENTRY_REMOVED, ENTRY_SHIPPED, ENTRY_SPLIT_ONLY, FILE_PROBLEM, NO_BLOCK,
    NOT_TOML, SPLIT_REFUSED, say,
};
use crate::pure::roadmap::SliceName;
use crate::pure::setup::toml_table;

const SLICES: &str = "## Slices";
const SPLIT: &str = "split_into";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Slice,
    Proof,
}

impl EntryKind {
    const fn key(self) -> &'static str {
        match self {
            Self::Slice => "slice",
            Self::Proof => "proof",
        }
    }

    const fn what(self) -> &'static str {
        match self {
            Self::Slice => "slice",
            Self::Proof => "proof step",
        }
    }
}

// Whitespace, comment wrapping and key order are not part of an entry: only its words and table.
#[derive(Debug, PartialEq)]
pub(crate) struct RoadmapEntry {
    name: SliceName,
    kind: EntryKind,
    words: Vec<String>,
    table: Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Frozen {
    Started,
    Shipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Changed,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    Frozen {
        kind: EntryKind,
        name: SliceName,
        frozen: Frozen,
        change: Change,
    },
    Split {
        name: SliceName,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let said = match self {
            Self::Frozen {
                kind,
                name,
                frozen,
                change,
            } => {
                let state = match frozen {
                    Frozen::Started => "started",
                    Frozen::Shipped => "shipped",
                };
                let template = match (change, frozen) {
                    (Change::Removed, Frozen::Started | Frozen::Shipped) => ENTRY_REMOVED,
                    (Change::Changed, Frozen::Shipped) => ENTRY_SHIPPED,
                    (Change::Changed, Frozen::Started) => ENTRY_SPLIT_ONLY,
                };
                say(
                    template,
                    &[("what", &kind.what()), ("name", name), ("state", &state)],
                )
            }
            Self::Split { name } => say(SPLIT_REFUSED, &[("name", name)]),
        };
        f.write_str(&said)
    }
}

// Whitespace removed, so `[[ slice ]]` counts; a quoted `[["slice"]]` does not, and the guard
// in `entries` is what catches it.
fn header_kind(line: &str) -> Option<EntryKind> {
    let squeezed: String = line.split_whitespace().collect();
    if squeezed.starts_with("[[slice]]") {
        Some(EntryKind::Slice)
    } else if squeezed.starts_with("[[proof]]") {
        Some(EntryKind::Proof)
    } else {
        None
    }
}

fn is_comment(line: &str) -> bool {
    line.trim().starts_with('#')
}

fn comment_words(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| is_comment(line))
        .flat_map(|line| line.trim().trim_start_matches('#').split_whitespace())
        .map(str::to_owned)
        .collect()
}

// The single table of an entry's text, which must hold exactly `kind`'s array of one table.
fn single_table(text: &str, kind: EntryKind) -> Option<Table> {
    let whole = toml_table(text.as_bytes()).ok()?;
    if whole.len() != 1 {
        return None;
    }
    match whole.get(kind.key())?.as_array()?.as_slice() {
        [Value::Table(table)] => Some(table.clone()),
        _ => None,
    }
}

// An entry starts at the comment lines right above its header, and ends where the next starts.
fn starts(lines: &[&str], headers: &[(usize, EntryKind)]) -> Vec<usize> {
    headers
        .iter()
        .map(|(header, _)| {
            let mut start = *header;
            while start > 0 && lines.get(start - 1).is_some_and(|line| is_comment(line)) {
                start -= 1;
            }
            start
        })
        .collect()
}

fn split_entries(block: &str) -> Option<Vec<RoadmapEntry>> {
    let whole = toml_table(block.as_bytes()).ok()?;
    let lines: Vec<&str> = block.lines().collect();
    let headers: Vec<(usize, EntryKind)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| header_kind(line).map(|kind| (index, kind)))
        .collect();
    let begins = starts(&lines, &headers);
    let mut found = Vec::new();
    for (position, (_, kind)) in headers.iter().enumerate() {
        let from = *begins.get(position)?;
        let to = begins.get(position + 1).copied().unwrap_or(lines.len());
        let own = lines.get(from..to)?;
        let table = single_table(&own.join("\n"), *kind)?;
        let name = SliceName::parse(table.get("name")?.as_str()?)?;
        found.push(RoadmapEntry {
            name,
            kind: *kind,
            words: comment_words(own),
            table,
        });
    }
    // A header the reader cannot see as one (a quoted key, a multi-line string) would split wrongly.
    for kind in [EntryKind::Slice, EntryKind::Proof] {
        let mine: Vec<Value> = found
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| Value::Table(entry.table.clone()))
            .collect();
        let held = whole.get(kind.key()).and_then(Value::as_array);
        if held.map_or(!mine.is_empty(), |held| *held != mine) {
            return None;
        }
    }
    Some(found)
}

pub(crate) fn entries(file: &str, text: &str) -> Result<Vec<RoadmapEntry>, String> {
    let block = match toml_block(text, SLICES) {
        Ok(Some(block)) => block,
        Ok(None) => return Err(say(NO_BLOCK, &[("file", &file), ("heading", &SLICES)])),
        Err(why) => return Err(say(FILE_PROBLEM, &[("file", &file), ("why", &why)])),
    };
    toml_table(block.as_bytes()).map_err(|why| say(NOT_TOML, &[("file", &file), ("why", &why)]))?;
    split_entries(block).ok_or_else(|| say(ENTRIES_APART, &[("file", &file)]))
}

fn frozen(logbook: &[Logged], name: &SliceName) -> Option<Frozen> {
    if is_shipped_by(logbook, name) {
        return Some(Frozen::Shipped);
    }
    logbook
        .iter()
        .any(|logged| logged.slice == *name && matches!(logged.entry, Entry::Started { .. }))
        .then_some(Frozen::Started)
}

fn only_gains_split(stored: &RoadmapEntry, draft: &RoadmapEntry) -> bool {
    let mut rest = draft.table.clone();
    stored.kind == draft.kind
        && stored.words == draft.words
        && !stored.table.contains_key(SPLIT)
        && rest.remove(SPLIT).is_some()
        && rest == stored.table
}

fn split_list(entry: &RoadmapEntry) -> Option<&Value> {
    entry.table.get(SPLIT)
}

fn names_a_stored(list: &Value, stored: &[RoadmapEntry]) -> bool {
    list.as_array().is_some_and(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .any(|part| stored.iter().any(|entry| entry.name.to_string() == part))
    })
}

// Frozen entries first, in the stored order, then the split rule in the draft's order.
pub(crate) fn refusals(
    stored: &[RoadmapEntry],
    draft: &[RoadmapEntry],
    logbook: &[Logged],
) -> Vec<Refusal> {
    let mut found = Vec::new();
    let mut refused: Vec<&SliceName> = Vec::new();
    for old in stored {
        let Some(state) = frozen(logbook, &old.name) else {
            continue;
        };
        let change = match draft.iter().find(|entry| entry.name == old.name) {
            None => Some(Change::Removed),
            Some(new) if new == old || (state == Frozen::Started && only_gains_split(old, new)) => {
                None
            }
            Some(_) => Some(Change::Changed),
        };
        if let Some(change) = change {
            refused.push(&old.name);
            found.push(Refusal::Frozen {
                kind: old.kind,
                name: old.name.clone(),
                frozen: state,
                change,
            });
        }
    }
    for new in draft {
        let Some(list) = split_list(new) else {
            continue;
        };
        if refused.contains(&&new.name) {
            continue;
        }
        let kept = stored
            .iter()
            .find(|old| old.name == new.name)
            .is_some_and(|old| split_list(old) == Some(list));
        let allowed =
            frozen(logbook, &new.name) == Some(Frozen::Started) && !names_a_stored(list, stored);
        if !kept && !allowed {
            found.push(Refusal::Split {
                name: new.name.clone(),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const NAMES: [&str; 5] = ["alpha", "beta", "gamma", "delta", "omega"];

    #[derive(Debug, Clone)]
    struct Raw {
        name: String,
        proof: bool,
        words: Vec<String>,
        items: Vec<String>,
        split: Option<Vec<String>>,
    }

    #[derive(Debug, Clone)]
    struct Style {
        per_line: usize,
        indent: usize,
        blanks: usize,
        key_first: bool,
    }

    const PLAIN: Style = Style {
        per_line: 3,
        indent: 0,
        blanks: 1,
        key_first: false,
    };

    fn wrap(block: &str) -> String {
        format!("## Slices\n\n```toml\n{block}```\n")
    }

    fn quoted(items: &[String]) -> String {
        let items: Vec<String> = items.iter().map(|item| format!("\"{item}\"")).collect();
        format!("[{}]", items.join(", "))
    }

    fn render(raw: &Raw, style: &Style) -> String {
        let mut text = String::new();
        for chunk in raw.words.chunks(style.per_line) {
            text.push_str(&" ".repeat(style.indent));
            text.push_str("# ");
            text.push_str(&chunk.join("  "));
            text.push('\n');
        }
        let first = raw.items.first().cloned().unwrap_or_default();
        let (header, key, value) = if raw.proof {
            ("[[proof]]", "how", format!("\"{first}\""))
        } else {
            ("[[slice]]", "owns", quoted(&raw.items))
        };
        text.push_str(header);
        text.push('\n');
        let name = format!("name = \"{}\"\n", raw.name);
        let own = format!("{key} = {value}\n");
        let fixed = if raw.proof {
            "after = []\nmeets = []\n"
        } else {
            "after = []\nmeets = []\ntest_drive = false\n"
        };
        if style.key_first {
            text.push_str(&own);
            text.push_str(&name);
        } else {
            text.push_str(&name);
            text.push_str(&own);
        }
        text.push_str(fixed);
        if let Some(parts) = &raw.split {
            text.push_str(SPLIT);
            text.push_str(" = ");
            text.push_str(&quoted(parts));
            text.push('\n');
        }
        text.push_str(&"\n".repeat(style.blanks));
        text
    }

    fn block(raws: &[Raw], styles: &[Style], order: &[usize]) -> String {
        let mut text = String::from("goal = \"g\"\n\n");
        for index in order {
            if let (Some(raw), Some(style)) = (raws.get(*index), styles.get(*index)) {
                text.push_str(&render(raw, style));
            }
        }
        text
    }

    fn read(raws: &[Raw]) -> Vec<RoadmapEntry> {
        let styles = vec![PLAIN; raws.len()];
        let order: Vec<usize> = (0..raws.len()).collect();
        entries("f", &wrap(&block(raws, &styles, &order))).expect("entries")
    }

    fn specs() -> impl Strategy<Value = Vec<Raw>> {
        proptest::sample::subsequence(NAMES.to_vec(), 1..=5).prop_flat_map(|names| {
            let count = names.len();
            (
                Just(names),
                proptest::collection::vec(
                    (
                        any::<bool>(),
                        proptest::collection::vec("[a-z]{1,6}", 0..6),
                        proptest::collection::vec("[a-z]{1,5}\\.rs", 1..3),
                        any::<bool>(),
                    ),
                    count,
                ),
            )
                .prop_map(|(names, rest)| {
                    names
                        .into_iter()
                        .zip(rest)
                        .map(|(name, (proof, words, items, split))| Raw {
                            name: name.to_owned(),
                            proof,
                            words,
                            items,
                            split: split.then(|| vec!["part".to_owned()]),
                        })
                        .collect()
                })
        })
    }

    fn styles(count: usize) -> impl Strategy<Value = Vec<Style>> {
        proptest::collection::vec(
            (1..4_usize, 0..3_usize, 1..3_usize, any::<bool>()).prop_map(
                |(per_line, indent, blanks, key_first)| Style {
                    per_line,
                    indent,
                    blanks,
                    key_first,
                },
            ),
            count,
        )
    }

    fn layout(count: usize) -> impl Strategy<Value = (Vec<Style>, Vec<usize>)> {
        (
            styles(count),
            Just((0..count).collect::<Vec<_>>()).prop_shuffle(),
        )
    }

    fn sorted(mut found: Vec<RoadmapEntry>) -> Vec<RoadmapEntry> {
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    fn state_of(states: &[u8], raws: &[Raw], name: &str) -> u8 {
        raws.iter()
            .position(|raw| raw.name == name)
            .and_then(|index| states.get(index).copied())
            .unwrap_or(0)
    }

    fn slice_name(name: &str) -> SliceName {
        SliceName::parse(name).expect("a slice name")
    }

    fn logged(name: &str, entry: Entry) -> Logged {
        Logged {
            slice: slice_name(name),
            entry,
        }
    }

    fn logbook_of(raws: &[Raw], states: &[u8]) -> Vec<Logged> {
        let started = |name: &str| {
            logged(
                name,
                Entry::Started {
                    branch: format!("slice/g/{name}"),
                },
            )
        };
        let mut book = vec![started("zeta")];
        for (index, raw) in raws.iter().enumerate() {
            let state = states.get(index).copied().unwrap_or(0);
            book.push(logged(
                &raw.name,
                Entry::Note {
                    text: "a note".to_owned(),
                },
            ));
            if state == 1 || state == 3 {
                book.push(started(&raw.name));
            }
            if state >= 2 {
                let shipped = if raw.proof {
                    Entry::ShippedProof {
                        note: "done".to_owned(),
                    }
                } else {
                    Entry::ShippedMerge {
                        commit: "0".repeat(40),
                        matched: true,
                        note: None,
                    }
                };
                book.push(logged(&raw.name, shipped));
            }
        }
        book
    }

    fn edited(raw: &Raw, edit: u8, first: &str) -> Option<Raw> {
        let mut next = raw.clone();
        let own = format!("{}-a", raw.name);
        match edit {
            0 => {}
            1 => next.words.push("changed".to_owned()),
            2 => {
                if raw.proof {
                    if let Some(how) = next.items.first_mut() {
                        how.push('x');
                    }
                } else {
                    next.items.push("x.rs".to_owned());
                }
            }
            3 => {
                next.proof = !raw.proof;
                next.items = vec![if raw.proof { "h.rs" } else { "h" }.to_owned()];
                next.split = None;
            }
            4 => return None,
            5 => next.name = format!("{}-two", raw.name),
            6 => next.split = Some(vec![own]),
            7 => {
                next.split = Some(vec![own]);
                next.words.push("changed".to_owned());
            }
            _ => next.split = Some(vec![first.to_owned()]),
        }
        Some(next)
    }

    // The rule written out again, entry by entry, from the cases the owner's answer lists.
    fn expected(
        raws: &[Raw],
        states: &[u8],
        edits: &[u8],
        draft: &[Raw],
        present: bool,
    ) -> Vec<Refusal> {
        let mut found = Vec::new();
        let mut refused: Vec<String> = Vec::new();
        if present {
            for (index, raw) in raws.iter().enumerate() {
                let state = states.get(index).copied().unwrap_or(0);
                let edit = edits.get(index).copied().unwrap_or(0);
                if state == 0 || edit == 0 {
                    continue;
                }
                let shipped = state >= 2;
                let change = if edit == 4 || edit == 5 {
                    Change::Removed
                } else if !shipped && (edit == 6 || edit == 8) && raw.split.is_none() {
                    continue;
                } else {
                    Change::Changed
                };
                refused.push(raw.name.clone());
                found.push(Refusal::Frozen {
                    kind: if raw.proof {
                        EntryKind::Proof
                    } else {
                        EntryKind::Slice
                    },
                    name: slice_name(&raw.name),
                    frozen: if shipped {
                        Frozen::Shipped
                    } else {
                        Frozen::Started
                    },
                    change,
                });
            }
        }
        for new in draft {
            let Some(list) = &new.split else { continue };
            if refused.contains(&new.name) {
                continue;
            }
            let kept = present
                && raws
                    .iter()
                    .find(|old| old.name == new.name)
                    .is_some_and(|old| old.split.as_ref() == Some(list));
            let started = state_of(states, raws, &new.name) == 1;
            let names_stored = present
                && list
                    .iter()
                    .any(|part| raws.iter().any(|raw| raw.name == *part));
            let allowed = kept || (started && !names_stored);
            if !allowed {
                found.push(Refusal::Split {
                    name: slice_name(&new.name),
                });
            }
        }
        found
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn an_entry_reads_the_same_whatever_its_layout_and_place(
            (raws, first, second) in specs().prop_flat_map(|raws| {
                let count = raws.len();
                (Just(raws), layout(count), layout(count))
            }),
        ) {
            let read_with = |(styles, order): &(Vec<Style>, Vec<usize>)| {
                entries("f", &wrap(&block(&raws, styles, order))).expect("entries")
            };
            let (one, two) = (read_with(&first), read_with(&second));
            let names = |found: &[RoadmapEntry], order: &[usize]| {
                (
                    found.iter().map(|entry| entry.name.to_string()).collect::<Vec<_>>(),
                    order.iter().filter_map(|i| raws.get(*i)).map(|raw| raw.name.clone()).collect::<Vec<_>>(),
                )
            };
            let (got, wanted) = names(&one, &first.1);
            prop_assert_eq!(got, wanted);
            let (got, wanted) = names(&two, &second.1);
            prop_assert_eq!(got, wanted);
            prop_assert_eq!(sorted(one), sorted(two));
        }

        #[test]
        fn only_a_frozen_entry_that_changes_or_goes_is_refused(
            (raws, states, edits) in specs().prop_flat_map(|raws| {
                let count = raws.len();
                (
                    Just(raws),
                    proptest::collection::vec(0..4_u8, count),
                    proptest::collection::vec(0..9_u8, count),
                )
            }),
            present in any::<bool>(),
        ) {
            let first = raws.first().map(|raw| raw.name.clone()).unwrap_or_default();
            let draft: Vec<Raw> = raws
                .iter()
                .zip(&edits)
                .filter_map(|(raw, edit)| edited(raw, *edit, &first))
                .collect();
            let stored = if present { read(&raws) } else { Vec::new() };
            let got = refusals(&stored, &read(&draft), &logbook_of(&raws, &states));
            prop_assert_eq!(got, expected(&raws, &states, &edits, &draft, present));
        }
    }

    fn raw(name: &str, split: Option<&str>) -> Raw {
        Raw {
            name: name.to_owned(),
            proof: false,
            words: Vec::new(),
            items: vec!["a.rs".to_owned()],
            split: split.map(|part| vec![part.to_owned()]),
        }
    }

    fn change(kind: EntryKind, name: &str, frozen: Frozen, change: Change) -> Refusal {
        Refusal::Frozen {
            kind,
            name: slice_name(name),
            frozen,
            change,
        }
    }

    #[test]
    fn a_frozen_entry_and_a_split_are_refused_as_the_owner_said() {
        let names = ["alpha", "beta", "gamma", "delta", "omega"];
        let stored: Vec<Raw> = names.iter().map(|name| raw(name, None)).collect();
        let draft = vec![
            raw("alpha", Some("alpha-a")),
            raw("beta-two", None),
            raw("gamma", Some("gamma-a")),
            raw("delta", Some("delta-a")),
            raw("omega", Some("alpha")),
        ];
        let book = logbook_of(&stored, &[2, 1, 1, 0, 1]);
        let got = refusals(&read(&stored), &read(&draft), &book);
        assert_eq!(
            got,
            [
                change(EntryKind::Slice, "alpha", Frozen::Shipped, Change::Changed),
                change(EntryKind::Slice, "beta", Frozen::Started, Change::Removed),
                Refusal::Split {
                    name: slice_name("delta")
                },
                Refusal::Split {
                    name: slice_name("omega")
                },
            ]
        );
        let lone = vec![raw("beta", None), raw("gamma", None)];
        let split = vec![raw("beta", Some("beta-a")), raw("gamma", Some("gamma-a"))];
        let book = logbook_of(&lone, &[1, 0]);
        assert_eq!(
            refusals(&[], &read(&split), &book),
            [Refusal::Split {
                name: slice_name("gamma")
            }]
        );
        assert_eq!(
            got.iter().map(ToString::to_string).collect::<Vec<_>>()[..2],
            [
                "slice alpha has shipped: its entry cannot change",
                "slice beta has started: its entry cannot be removed",
            ]
        );
    }

    #[test]
    fn the_comment_right_above_a_header_belongs_to_its_entry() {
        let block = "goal = \"g\"\n# head note\n\n# alpha words\n[[slice]]\nname = \"alpha\"\nafter = []\nmeets = []\nowns = [\"a/\"]\ntest_drive = false\n# a stray note\n\n# about beta\n[[slice]]\nname = \"beta\"\nafter = []\nmeets = []\nowns = [\"b/\"]\ntest_drive = false\n\n# loose\n\n[[slice]]\nname = \"gamma\"\nafter = []\nmeets = []\nowns = [\"c/\"]\ntest_drive = false\n";
        let found = entries("road.md", &wrap(block)).expect("entries");
        let words: Vec<(String, Vec<&str>)> = found
            .iter()
            .map(|entry| {
                (
                    entry.name.to_string(),
                    entry.words.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            words,
            [
                (
                    "alpha".to_owned(),
                    vec!["alpha", "words", "a", "stray", "note"]
                ),
                ("beta".to_owned(), vec!["about", "beta", "loose"]),
                ("gamma".to_owned(), Vec::<&str>::new()),
            ]
        );
    }

    #[test]
    fn a_split_that_cannot_be_trusted_is_refused() {
        let apart = Err(say(ENTRIES_APART, &[("file", &"road.md")]));
        let quoted_header = "goal = \"g\"\n\n[[slice]]\nname = \"a\"\nafter = []\nmeets = []\nowns = [\"a/\"]\ntest_drive = false\n\n[[\"slice\"]]\nname = \"b\"\nafter = []\nmeets = []\nowns = [\"b/\"]\ntest_drive = false\n";
        assert_eq!(entries("road.md", &wrap(quoted_header)), apart);
        let in_string = "goal = \"g\"\n\n[[proof]]\nname = \"p\"\nafter = []\nmeets = []\nhow = \"\"\"\nrun it\n[[proof]]\n\"\"\"\n";
        assert_eq!(entries("road.md", &wrap(in_string)), apart);
        let sub_table = "goal = \"g\"\n\n[[slice]]\nname = \"a\"\n[slice.extra]\nk = 1\n";
        let found = entries("road.md", &wrap(sub_table)).expect("one entry");
        assert_eq!(found.len(), 1);
        assert!(found.iter().all(|entry| {
            entry.name.to_string() == "a"
                && entry
                    .table
                    .get("extra")
                    .and_then(Value::as_table)
                    .is_some_and(|extra| extra.get("k") == Some(&Value::Integer(1)))
        }));
    }
}
