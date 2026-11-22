use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) file: String,
    pub(crate) first: u64,
    pub(crate) last: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Function {
    pub(crate) file: String,
    pub(crate) cognitive: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marked {
    pub(crate) file: String,
    pub(crate) first: u64,
    pub(crate) last: u64,
    pub(crate) why: String,
}

// One line of CSV: a quoted field may hold commas and `""`; None for a quote that never closes,
// text after a closing quote, or a quote inside an unquoted field.
pub(crate) fn csv_fields(line: &str) -> Option<Vec<String>> {
    let mut chars = line.chars().peekable();
    let mut fields = Vec::new();
    loop {
        let mut field = String::new();
        if chars.next_if_eq(&'"').is_some() {
            loop {
                match chars.next()? {
                    '"' if chars.next_if_eq(&'"').is_some() => field.push('"'),
                    '"' => break,
                    other => field.push(other),
                }
            }
            fields.push(field);
            match chars.next() {
                None => return Some(fields),
                Some(',') => {}
                Some(_junk) => return None,
            }
        } else {
            loop {
                match chars.next() {
                    None => {
                        fields.push(field);
                        return Some(fields);
                    }
                    Some(',') => {
                        fields.push(field);
                        break;
                    }
                    Some('"') => return None,
                    Some(other) => field.push(other),
                }
            }
        }
    }
}

// A clone touches the slice when any of its places is in a changed file.
pub(crate) fn touching<'a>(clones: &'a [Vec<Place>], changed: &[String]) -> Vec<&'a [Place]> {
    clones
        .iter()
        .filter(|clone| clone.iter().any(|place| changed.contains(&place.file)))
        .map(Vec::as_slice)
        .collect()
}

fn as_float(count: u64) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

// The lines of the changed files that lie inside a clone or a marked block, each counted once,
// over all the lines of the changed files, in percent. A marked block counts whether or not
// the tool found a clone in it, so hiding a block can never lower the number.
pub(crate) fn clone_ratio(
    clones: &[Vec<Place>],
    marked: &[Marked],
    lines_of: &[(String, u64)],
) -> f64 {
    let mut spans: BTreeMap<&str, Vec<(u64, u64)>> = BTreeMap::new();
    for place in clones.iter().flatten() {
        spans
            .entry(&place.file)
            .or_default()
            .push((place.first, place.last));
    }
    for block in marked {
        spans
            .entry(&block.file)
            .or_default()
            .push((block.first, block.last));
    }
    let mut total = 0_u64;
    let mut covered = 0_u64;
    for (file, lines) in lines_of {
        total = total.saturating_add(*lines);
        let mut inside: Vec<(u64, u64)> = spans
            .get(file.as_str())
            .into_iter()
            .flatten()
            .map(|(first, last)| ((*first).max(1), (*last).min(*lines)))
            .filter(|(first, last)| first <= last)
            .collect();
        inside.sort_unstable();
        let mut reached = 0_u64;
        for (first, last) in inside {
            let from = first.max(reached.saturating_add(1));
            if from <= last {
                covered = covered.saturating_add(last - from + 1);
                reached = last;
            }
        }
    }
    if total == 0 {
        return 0.0;
    }
    as_float(covered) * 100.0 / as_float(total)
}

pub(crate) fn highest(functions: &[Function], changed: &[String]) -> u64 {
    functions
        .iter()
        .filter(|function| changed.contains(&function.file))
        .map(|function| function.cognitive)
        .max()
        .unwrap_or(0)
}

// The comment words of a line that is only a comment: `//`, `///`, `//!` and `/*` open one.
// PMD honours all of them, so every one is read as a marker.
fn comment_word(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let body = trimmed
        .strip_prefix("//")
        .or_else(|| trimmed.strip_prefix("/*"))?;
    Some(body.trim_start_matches(['/', '*', '!']).trim_start())
}

fn reason(after_off: &str) -> Option<String> {
    let why = after_off.strip_prefix(':')?;
    let why = why.trim();
    let why = why.strip_suffix("*/").unwrap_or(why).trim();
    (!why.is_empty()).then(|| why.to_owned())
}

// Every block that `// CPD-OFF: <why>` … `// CPD-ON` hides from the clone tool, or every
// problem: a block without a reason, or without its `CPD-ON` (which hides the rest of the file).
pub(crate) fn marked_blocks(file: &str, text: &str) -> Result<Vec<Marked>, Vec<String>> {
    let mut blocks = Vec::new();
    let mut problems: Vec<(u64, String)> = Vec::new();
    let mut open: Option<(u64, String)> = None;
    let mut number = 0_u64;
    for line in text.lines() {
        number += 1;
        let Some(word) = comment_word(line) else {
            continue;
        };
        if let Some(after) = word.strip_prefix("CPD-OFF") {
            let why = reason(after);
            if why.is_none() {
                problems.push((
                    number,
                    format!("{file}:{number}: CPD-OFF needs a reason: // CPD-OFF: <why>"),
                ));
            }
            if open.is_none() {
                open = Some((number, why.unwrap_or_default()));
            }
        } else if word.starts_with("CPD-ON")
            && let Some((first, why)) = open.take()
        {
            blocks.push(Marked {
                file: file.to_owned(),
                first,
                last: number,
                why,
            });
        }
    }
    if let Some((first, _why)) = open {
        problems.push((
            first,
            format!("{file}:{first}: CPD-OFF has no matching CPD-ON"),
        ));
    }
    if problems.is_empty() {
        return Ok(blocks);
    }
    problems.sort();
    Err(problems.into_iter().map(|(_line, text)| text).collect())
}

// Every tracked `.rs` file, from `git ls-files -v -z`.
pub(crate) fn tracked_rust_files(ls_files_v_z: &[u8]) -> Vec<String> {
    ls_files_v_z
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.get(2..))
        .filter_map(|name| String::from_utf8(name.to_vec()).ok())
        .filter(|name| Path::new(name).extension().is_some_and(|ext| ext == "rs"))
        .collect()
}

// A deleted file is changed but not tracked, so it is not measured.
pub(crate) fn changed_rust_files(tracked: &[String], changed: &[Vec<u8>]) -> Vec<String> {
    let mut found: Vec<String> = changed
        .iter()
        .filter_map(|name| String::from_utf8(name.clone()).ok())
        .filter(|name| tracked.contains(name))
        .collect();
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn is_rs(name: &str) -> bool {
        name.rsplit_once('.')
            .is_some_and(|(_stem, ext)| ext == "rs")
    }

    fn quoted(field: &str) -> String {
        format!("\"{}\"", field.replace('"', "\"\""))
    }

    fn file() -> impl Strategy<Value = String> {
        proptest::sample::select(vec!["a.rs", "b.rs", "c.rs", "d.rs"]).prop_map(str::to_owned)
    }

    fn place() -> impl Strategy<Value = Place> {
        (file(), 1_u64..60, 0_u64..30).prop_map(|(file, first, length)| Place {
            file,
            first,
            last: first + length,
        })
    }

    fn clones() -> impl Strategy<Value = Vec<Vec<Place>>> {
        proptest::collection::vec(proptest::collection::vec(place(), 2..4), 0..5)
    }

    fn marks() -> impl Strategy<Value = Vec<Marked>> {
        proptest::collection::vec(
            place().prop_map(|place| Marked {
                file: place.file,
                first: place.first,
                last: place.last,
                why: "x".to_owned(),
            }),
            0..4,
        )
    }

    fn lines_of() -> impl Strategy<Value = Vec<(String, u64)>> {
        proptest::collection::vec(0_u64..80, 4).prop_map(|counts| {
            ["a.rs", "b.rs", "c.rs", "d.rs"]
                .iter()
                .zip(counts)
                .map(|(name, count)| ((*name).to_owned(), count))
                .collect()
        })
    }

    proptest! {
        #[test]
        fn csv_round_trip(
            fields in proptest::collection::vec(("\\PC{0,8}", any::<bool>()), 1..6),
        ) {
            let line = fields
                .iter()
                .map(|(field, force)| {
                    if *force || field.contains([',', '"']) {
                        quoted(field)
                    } else {
                        field.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            let expected: Vec<String> = fields.into_iter().map(|(field, _force)| field).collect();
            prop_assert_eq!(csv_fields(&line), Some(expected));
        }

        #[test]
        fn a_clone_touches_iff_a_place_is_changed(
            clone in proptest::collection::vec(place(), 1..4),
            changed in proptest::collection::vec(file(), 0..3),
        ) {
            let expected = clone.iter().any(|place| changed.contains(&place.file));
            let clones = [clone];
            prop_assert_eq!(touching(&clones, &changed).len(), usize::from(expected));
        }

        #[test]
        fn ratio_is_between_0_and_100(
            clones in clones(),
            marked in marks(),
            lines_of in lines_of(),
        ) {
            let ratio = clone_ratio(&clones, &marked, &lines_of);
            prop_assert!((0.0..=100.0).contains(&ratio), "{ratio}");
        }

        #[test]
        fn a_marked_block_never_lowers_the_ratio(
            clones in clones(),
            marked in marks(),
            extra in marks(),
            lines_of in lines_of(),
        ) {
            let before = clone_ratio(&clones, &marked, &lines_of);
            let mut all = marked;
            all.extend(extra);
            prop_assert!(clone_ratio(&clones, &all, &lines_of) >= before);
        }

        #[test]
        fn every_off_needs_a_reason_and_an_on(
            lines in proptest::collection::vec(
                proptest::sample::select(vec![
                    "let x = 1;",
                    "// CPD-OFF: because",
                    "//CPD-OFF",
                    "/* CPD-OFF: because */",
                    "/// CPD-OFF:",
                    "// CPD-ON",
                    "// plain",
                ]),
                0..12,
            ),
        ) {
            let mut open = false;
            let mut blocks = 0;
            let mut every_reason = true;
            for line in &lines {
                if line.contains("CPD-OFF") {
                    every_reason &= line.contains(": because");
                    open = true;
                } else if line.contains("CPD-ON") && open {
                    open = false;
                    blocks += 1;
                }
            }
            let result = marked_blocks("f.rs", &lines.join("\n"));
            match result {
                Ok(found) => {
                    prop_assert!(every_reason && !open);
                    prop_assert_eq!(found.len(), blocks);
                    prop_assert!(found.iter().all(|block| block.why == "because"));
                }
                Err(problems) => {
                    prop_assert!(!every_reason || open);
                    prop_assert!(!problems.is_empty());
                }
            }
        }

        #[test]
        fn highest_counts_changed_files_only(
            functions in proptest::collection::vec(
                (file(), 0_u64..50).prop_map(|(file, cognitive)| Function { file, cognitive }),
                0..8,
            ),
            changed in proptest::collection::vec(file(), 0..3),
        ) {
            let result = highest(&functions, &changed);
            let inside: Vec<Function> = functions
                .iter()
                .filter(|function| changed.contains(&function.file))
                .cloned()
                .collect();
            prop_assert_eq!(highest(&inside, &changed), result);
            prop_assert!(inside.iter().all(|function| function.cognitive <= result));
            prop_assert!(result == 0 || inside.iter().any(|function| function.cognitive == result));
        }

        #[test]
        fn rust_files_are_the_tracked_ones_that_changed(
            tracked in proptest::collection::vec("[a-c]{1,2}(\\.rs|\\.md)", 0..6),
            changed in proptest::collection::vec("[a-c]{1,2}(\\.rs|\\.md)", 0..6),
        ) {
            let listing: Vec<u8> = tracked
                .iter()
                .flat_map(|name| format!("H {name}\0").into_bytes())
                .collect();
            let names: Vec<Vec<u8>> = changed.iter().map(|name| name.clone().into_bytes()).collect();
            let all = tracked_rust_files(&listing);
            prop_assert!(all.iter().all(|name| is_rs(name) && tracked.contains(name)));
            let found = changed_rust_files(&all, &names);
            let inside = found
                .iter()
                .all(|name| is_rs(name) && tracked.contains(name) && changed.contains(name));
            prop_assert!(inside);
            let any = changed.iter().any(|name| is_rs(name) && tracked.contains(name));
            prop_assert_eq!(found.is_empty(), !any);
        }
    }

    #[test]
    fn a_marker_reads_with_its_reason_and_lines() {
        let text = "fn a() {}\n// CPD-OFF: shared fixture\nfn b() {}\n// CPD-ON\n";
        assert_eq!(
            marked_blocks("src/a.rs", text),
            Ok(vec![Marked {
                file: "src/a.rs".to_owned(),
                first: 2,
                last: 4,
                why: "shared fixture".to_owned(),
            }])
        );
    }

    #[test]
    fn the_problems_name_the_line_of_the_off() {
        let text = "// CPD-OFF\nfn a() {}\n// CPD-ON\n// CPD-OFF: why\n";
        assert_eq!(
            marked_blocks("f.rs", text),
            Err(vec![
                "f.rs:1: CPD-OFF needs a reason: // CPD-OFF: <why>".to_owned(),
                "f.rs:4: CPD-OFF has no matching CPD-ON".to_owned(),
            ])
        );
    }
}
