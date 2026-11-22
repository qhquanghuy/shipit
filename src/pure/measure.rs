use std::collections::BTreeMap;

use crate::pure::receipt::Oid;

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

// A place that lies wholly inside a marked block is approved; a clone touches the slice when any
// of its places is in a changed file and not approved. The clone tool never sees the markers, so
// this is the only place that applies them.
fn approved(place: &Place, marked: &[Marked]) -> bool {
    marked.iter().any(|block| {
        block.file == place.file && block.first <= place.first && place.last <= block.last
    })
}

pub(crate) fn touching<'a>(
    clones: &'a [Vec<Place>],
    changed: &[String],
    marked: &[Marked],
) -> Vec<&'a [Place]> {
    clones
        .iter()
        .filter(|clone| {
            clone
                .iter()
                .any(|place| changed.contains(&place.file) && !approved(place, marked))
        })
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

// The marker words are spelled in two pieces so that this file, which the check reads too, does
// not hold them.
pub(crate) const OFF: &str = concat!("CPD", "-OFF");
pub(crate) const ON: &str = concat!("CPD", "-ON");

#[derive(Debug, PartialEq, Eq)]
enum Line {
    Code,
    Off(String),
    On,
    NeedsReason,
    Stray,
}

// PMD switches off on any comment that holds the off word, and on at any that holds the on
// word, so the only accepted forms are the two alone on their line; any other line that holds
// either word is stray, never read as a marker.
fn classify(line: &str) -> Line {
    let text = line.trim();
    if !text.contains(OFF) && !text.contains(ON) {
        return Line::Code;
    }
    if text == format!("// {ON}") {
        return Line::On;
    }
    let Some(rest) = text.strip_prefix(&format!("// {OFF}")) else {
        return Line::Stray;
    };
    match rest.trim() {
        "" | ":" => Line::NeedsReason,
        _ => match rest.strip_prefix(':').map(str::trim) {
            Some(why) if !why.contains("CPD-") => Line::Off(why.to_owned()),
            Some(_) | None => Line::Stray,
        },
    }
}

// Every block that the off marker with its reason and the on marker hide from the clone tool,
// or every problem: a stray word, a block without a reason, or without its on marker (which
// hides the rest of the file).
pub(crate) fn marked_blocks(file: &str, text: &str) -> Result<Vec<Marked>, Vec<String>> {
    let mut blocks = Vec::new();
    let mut problems: Vec<(u64, String)> = Vec::new();
    let mut open: Option<(u64, String)> = None;
    let mut number = 0_u64;
    for line in text.lines() {
        number += 1;
        match classify(line) {
            Line::Code => {}
            Line::Off(why) => {
                open.get_or_insert((number, why));
            }
            Line::NeedsReason => {
                problems.push((
                    number,
                    format!("{file}:{number}: {OFF} needs a reason: // {OFF}: <why>"),
                ));
                open.get_or_insert((number, String::new()));
            }
            Line::Stray => problems.push((
                number,
                format!(
                    "{file}:{number}: {OFF} and {ON} must stand alone on their line: \
                     // {OFF}: <why> or // {ON}"
                ),
            )),
            Line::On => {
                if let Some((first, why)) = open.take() {
                    blocks.push(Marked {
                        file: file.to_owned(),
                        first,
                        last: number,
                        why,
                    });
                }
            }
        }
    }
    if let Some((first, _why)) = open {
        problems.push((first, format!("{file}:{first}: {OFF} has no matching {ON}")));
    }
    if problems.is_empty() {
        return Ok(blocks);
    }
    problems.sort();
    Err(problems.into_iter().map(|(_line, text)| text).collect())
}

// The text as the clone tool reads it: each marker word becomes a word of the same length that
// it ignores, so no line moves and no line changes length, and the only reader of the markers
// is `marked_blocks`.
pub(crate) fn neutralised(text: &[u8]) -> Vec<u8> {
    let words = [
        (OFF.as_bytes(), OFF.replace('-', "_").into_bytes()),
        (ON.as_bytes(), ON.replace('-', "_").into_bytes()),
    ];
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(rest) = text.get(at..).filter(|rest| !rest.is_empty()) {
        if let Some((word, tame)) = words.iter().find(|(word, _tame)| rest.starts_with(word)) {
            out.extend_from_slice(tame);
            at += word.len();
        } else {
            out.extend(rest.first());
            at += 1;
        }
    }
    out
}

// Inside `shipit check` the merge base is the one the check was given. The message names what
// is wrong when the check variable is set and the merge base is not a commit id; with no check
// variable a stray merge base means nothing.
pub(crate) fn given_merge_base(
    check: Option<&str>,
    merge_base: Option<&str>,
) -> Result<Option<Oid>, String> {
    if check.is_none() {
        return Ok(None);
    }
    merge_base.and_then(Oid::parse).map(Some).ok_or_else(|| {
        "SHIPIT_CHECK is set but SHIPIT_MERGE_BASE is missing or not a commit id".to_owned()
    })
}

// A name that ends in `.rs` and has something before it: `.rs` alone is not a Rust file.
fn is_rust_name(name: &[u8]) -> bool {
    name.rsplit(|byte| *byte == b'/')
        .next()
        .is_some_and(|base| base.ends_with(b".rs") && base.len() > 3)
}

// Every tracked `.rs` file, from `git ls-files -v -z`; the error holds the names that cannot be
// measured: PMD reads its file list by lines and commas, and a name that is not UTF-8 cannot be
// handed on, so none of them is dropped silently.
pub(crate) fn tracked_rust_files(ls_files_v_z: &[u8]) -> Result<Vec<String>, Vec<String>> {
    let mut found = Vec::new();
    let mut unusable = Vec::new();
    for name in ls_files_v_z
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.get(2..))
        .filter(|name| is_rust_name(name))
    {
        match String::from_utf8(name.to_vec()) {
            Ok(text) if !text.contains([',', '\n', '\r']) => found.push(text),
            Ok(_) | Err(_) => unusable.push(String::from_utf8_lossy(name).into_owned()),
        }
    }
    if unusable.is_empty() {
        Ok(found)
    } else {
        Err(unusable)
    }
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
            .is_some_and(|(stem, ext)| ext == "rs" && !stem.is_empty())
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Code,
        GoodOff,
        GoodOn,
        Bad,
    }

    // The forms PMD honours that are not the two accepted ones: after code, inside a longer
    // comment, in a block comment over several lines, with no space, as a doc comment, with no
    // reason.
    fn alphabet() -> Vec<(String, Kind)> {
        let line = |text: String, kind| (text, kind);
        vec![
            line("let x = 1;".to_owned(), Kind::Code),
            line("// plain".to_owned(), Kind::Code),
            line("/*".to_owned(), Kind::Code),
            line("*/".to_owned(), Kind::Code),
            line(format!("// {OFF}: because"), Kind::GoodOff),
            line(format!("    // {OFF}: because"), Kind::GoodOff),
            line(format!("// {ON}"), Kind::GoodOn),
            line(format!("  // {ON}  "), Kind::GoodOn),
            line(format!("let z = 1; // {OFF}"), Kind::Bad),
            line(format!("let z = 1; // {OFF}: because"), Kind::Bad),
            line(format!("// note: {OFF} here"), Kind::Bad),
            line(format!("   {OFF}"), Kind::Bad),
            line(format!("/* {OFF}: because */"), Kind::Bad),
            line(format!("//{OFF}: because"), Kind::Bad),
            line(format!("/// {OFF}: because"), Kind::Bad),
            line(format!("// {OFF}"), Kind::Bad),
            line(format!("// {OFF}:"), Kind::Bad),
            line(format!("// {OFF}: because {ON}"), Kind::Bad),
            line(format!("let z = 1; // {ON}"), Kind::Bad),
            line(format!("// not {ON}"), Kind::Bad),
        ]
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
        fn a_clone_touches_iff_a_place_is_changed_and_not_approved(
            clone in proptest::collection::vec(place(), 1..4),
            changed in proptest::collection::vec(file(), 0..3),
        ) {
            let in_changed: Vec<&Place> =
                clone.iter().filter(|place| changed.contains(&place.file)).collect();
            let approving: Vec<Marked> = in_changed
                .iter()
                .map(|place| Marked {
                    file: place.file.clone(),
                    first: place.first,
                    last: place.last,
                    why: "x".to_owned(),
                })
                .collect();
            let clones = [clone.clone()];
            prop_assert_eq!(touching(&clones, &changed, &[]).len(), usize::from(!in_changed.is_empty()));
            prop_assert!(touching(&clones, &changed, &approving).is_empty());
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
        fn every_marker_word_is_a_whole_line_with_a_reason_and_an_on(
            lines in proptest::collection::vec(proptest::sample::select(alphabet()), 0..12),
        ) {
            let bad = lines.iter().filter(|(_text, kind)| *kind == Kind::Bad).count();
            let mut open = false;
            let mut blocks = 0;
            for (_text, kind) in &lines {
                match kind {
                    Kind::GoodOff => open = true,
                    Kind::GoodOn if open => {
                        open = false;
                        blocks += 1;
                    }
                    Kind::GoodOn | Kind::Code | Kind::Bad => {}
                }
            }
            let text: Vec<&str> = lines.iter().map(|(text, _kind)| text.as_str()).collect();
            match marked_blocks("f.rs", &text.join("\n")) {
                Ok(found) => {
                    prop_assert!(bad == 0 && !open);
                    prop_assert_eq!(found.len(), blocks);
                    prop_assert!(found.iter().all(|block| block.why == "because"));
                }
                Err(problems) => {
                    prop_assert!(bad > 0 || open);
                    prop_assert!(problems.len() >= bad);
                }
            }
        }

        #[test]
        fn a_marked_block_counts_as_cloned_lines(
            marked in marks(),
            lines_of in lines_of(),
        ) {
            let as_places: Vec<Vec<Place>> = marked
                .iter()
                .map(|block| vec![Place {
                    file: block.file.clone(),
                    first: block.first,
                    last: block.last,
                }])
                .collect();
            prop_assert_eq!(
                clone_ratio(&[], &marked, &lines_of).to_bits(),
                clone_ratio(&as_places, &[], &lines_of).to_bits()
            );
        }

        #[test]
        fn neutralising_keeps_every_line_and_its_length(
            pieces in proptest::collection::vec(
                proptest::sample::select(vec![
                    OFF.to_owned(),
                    ON.to_owned(),
                    "a".to_owned(),
                    "\n".to_owned(),
                    "CPD-".to_owned(),
                    "O".to_owned(),
                    "\u{e9}".to_owned(),
                ]),
                0..20,
            ),
        ) {
            let text = pieces.concat();
            let tame = neutralised(text.as_bytes());
            let lengths = |bytes: &[u8]| -> Vec<usize> { bytes.split(|b| *b == b'\n').map(<[u8]>::len).collect() };
            prop_assert_eq!(lengths(&tame), lengths(text.as_bytes()));
            let tame_text = String::from_utf8(tame).expect("replaces ASCII with ASCII");
            prop_assert!(!tame_text.contains(OFF) && !tame_text.contains(ON));
            prop_assert_eq!(neutralised(tame_text.as_bytes()), tame_text.into_bytes());
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
        fn a_tracked_rust_file_is_measured_or_named(
            tracked in proptest::collection::vec("[a-c,\\n]{0,2}(\\.rs|\\.md)", 0..6),
            changed in proptest::collection::vec("[a-c]{1,2}(\\.rs|\\.md)", 0..6),
        ) {
            let listing: Vec<u8> = tracked
                .iter()
                .flat_map(|name| format!("H {name}\0").into_bytes())
                .collect();
            let rust = tracked.iter().filter(|name| is_rs(name)).count();
            let unusable = tracked.iter().filter(|name| is_rs(name) && name.contains([',', '\n'])).count();
            let names: Vec<Vec<u8>> = changed.iter().map(|name| name.clone().into_bytes()).collect();
            match tracked_rust_files(&listing) {
                Ok(all) => {
                    prop_assert_eq!(unusable, 0);
                    prop_assert_eq!(all.len(), rust);
                    prop_assert!(all.iter().all(|name| is_rs(name) && tracked.contains(name)));
                    let found = changed_rust_files(&all, &names);
                    let inside = found
                        .iter()
                        .all(|name| is_rs(name) && tracked.contains(name) && changed.contains(name));
                    prop_assert!(inside);
                    let any = changed.iter().any(|name| is_rs(name) && tracked.contains(name));
                    prop_assert_eq!(found.is_empty(), !any);
                }
                Err(named) => prop_assert_eq!(named.len(), unusable),
            }
        }
    }

    #[test]
    fn a_name_that_is_not_utf8_is_named_not_dropped() {
        assert_eq!(
            tracked_rust_files(b"H src/ok.rs\0H src/o\xffne.rs\0"),
            Err(vec!["src/o\u{fffd}ne.rs".to_owned()])
        );
    }

    #[test]
    fn a_marker_reads_with_its_reason_and_lines() {
        let text = format!("fn a() {{}}\n// {OFF}: shared fixture\nfn b() {{}}\n// {ON}\n");
        assert_eq!(
            marked_blocks("src/a.rs", &text),
            Ok(vec![Marked {
                file: "src/a.rs".to_owned(),
                first: 2,
                last: 4,
                why: "shared fixture".to_owned(),
            }])
        );
    }

    #[test]
    fn the_problems_name_the_line_and_the_accepted_form() {
        let text = format!("// {OFF}\nfn a() {{}}\n// {ON}\nlet z = 1; // {ON}\n// {OFF}: why\n");
        assert_eq!(
            marked_blocks("f.rs", &text),
            Err(vec![
                format!("f.rs:1: {OFF} needs a reason: // {OFF}: <why>"),
                format!(
                    "f.rs:4: {OFF} and {ON} must stand alone on their line: \
                     // {OFF}: <why> or // {ON}"
                ),
                format!("f.rs:5: {OFF} has no matching {ON}"),
            ])
        );
    }

    #[test]
    fn a_marker_word_never_reaches_the_clone_tool() {
        let text = format!("a {OFF}\n// {ON} b\n").into_bytes();
        let tame = neutralised(&text);
        assert_eq!(tame, b"a CPD_OFF\n// CPD_ON b\n".to_vec());
    }

    #[test]
    fn a_merge_base_is_needed_exactly_when_the_check_variable_is_set() {
        let oid = "a".repeat(40);
        assert_eq!(given_merge_base(None, None), Ok(None));
        assert_eq!(given_merge_base(None, Some("nonsense")), Ok(None));
        assert_eq!(
            given_merge_base(Some("x"), Some(&oid)).map(|found| found.map(|oid| oid.to_string())),
            Ok(Some(oid))
        );
        for bad in [None, Some(""), Some("nonsense")] {
            assert_eq!(
                given_merge_base(Some("x"), bad),
                Err(
                    "SHIPIT_CHECK is set but SHIPIT_MERGE_BASE is missing or not a commit id"
                        .to_owned()
                )
            );
        }
    }
}
