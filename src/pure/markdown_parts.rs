// A part starts at a heading or at a line that begins with a bold name and a colon; both are
// recognised here by shape only, and the spec decides which names count.
pub(crate) fn heading(text: &str) -> Option<(usize, &str)> {
    let hashes = text.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let name = text.get(hashes..)?.strip_prefix(' ')?;
    Some((hashes, name.trim_end_matches([' ', '#'])))
}

// The name and the text after the marker on the same line.
pub(crate) fn bold_marker(text: &str) -> Option<(&str, &str)> {
    text.strip_prefix("**")?.split_once(":**")
}

// A list item starts at the left margin with `- `, `* ` or a number and `. `.
pub(crate) fn list_item(text: &str) -> Option<&str> {
    if let Some(item) = text.strip_prefix("- ").or_else(|| text.strip_prefix("* ")) {
        return Some(item.trim());
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    text.get(digits..)?.strip_prefix(". ").map(str::trim)
}

// The text after a run of `n` backticks up to the next run of exactly `n`, as the offset after
// that closing run.
fn closing_run(text: &str, n: usize) -> Option<usize> {
    let mut at = 0;
    while let Some(found) = text.get(at..)?.find('`') {
        let start = at + found;
        let run = text.get(start..)?.chars().take_while(|c| *c == '`').count();
        if run == n {
            return Some(start + run);
        }
        at = start + run;
    }
    None
}

// The line with each code span as one space, so that a name in it is a mention and a word cannot be
// joined across it, and the text inside each span. A run of backticks with no closing run is text.
fn spans(line: &str) -> (String, Vec<&str>) {
    let mut kept = String::new();
    let mut inner = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let Some((before, from)) = rest.split_at_checked(open) else {
            break;
        };
        kept.push_str(before);
        let run = from.chars().take_while(|c| *c == '`').count();
        let inside = from.get(run..).unwrap_or_default();
        if let Some(end) = closing_run(inside, run) {
            kept.push(' ');
            inner.extend(inside.get(..end - run));
            rest = inside.get(end..).unwrap_or_default();
        } else {
            kept.push_str(from.get(..run).unwrap_or_default());
            rest = inside;
        }
    }
    kept.push_str(rest);
    (kept, inner)
}

pub(crate) fn without_code_spans(line: &str) -> String {
    spans(line).0
}

// The text inside each code span of the line.
pub(crate) fn code_spans(line: &str) -> Vec<&str> {
    spans(line).1
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_heading_has_one_to_six_hashes_a_space_and_a_name(
            level in 0_usize..9,
            name in "[A-Za-z][A-Za-z ]{0,10}[A-Za-z]",
            trailing in "[ #]{0,3}",
        ) {
            let text = format!("{} {name}{trailing}", "#".repeat(level));
            let wanted = (1..=6).contains(&level).then_some((level, name.as_str()));
            prop_assert_eq!(heading(&text), wanted);
        }

        #[test]
        fn a_bold_marker_needs_both_stars_and_the_colon(
            name in "[A-Za-z][A-Za-z ]{0,10}",
            rest in "[a-z ]{0,8}",
        ) {
            let marked = format!("**{name}:**{rest}");
            prop_assert_eq!(bold_marker(&marked), Some((name.as_str(), rest.as_str())));
            let no_colon = format!("**{name}**{rest}");
            prop_assert_eq!(bold_marker(&no_colon), None);
            let indented = format!(" **{name}:**{rest}");
            prop_assert_eq!(bold_marker(&indented), None);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn an_item_has_a_marker_at_the_margin_and_a_space(
            marker in prop::sample::select(vec!["- ", "* ", "1. ", "12. ", "007. "]),
            text in "[a-z][a-z ]{0,8}",
            indent in 0_usize..3,
        ) {
            let line = format!("{}{marker}{text}", " ".repeat(indent));
            let wanted = (indent == 0).then_some(text.trim());
            prop_assert_eq!(list_item(&line), wanted);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        // Text on either side of a span is kept; the span is one space whatever it holds.
        #[test]
        fn a_span_is_replaced_by_one_space(
            before in "[a-z ]{0,6}",
            inside in "[a-z ]{1,6}",
            after in "[a-z ]{0,6}",
            ticks in 1_usize..4,
        ) {
            let fence = "`".repeat(ticks);
            let line = format!("{before}{fence}{inside}{fence}{after}");
            prop_assert_eq!(without_code_spans(&line), format!("{before} {after}"));
            prop_assert_eq!(code_spans(&line), vec![inside.as_str()]);
        }

        #[test]
        fn text_without_a_closing_run_is_kept(
            before in "[a-z ]{0,6}",
            after in "[a-z ]{0,6}",
            open in 1_usize..4,
            shift in 1_usize..3,
        ) {
            let close = 1 + (open - 1 + shift) % 3;
            let line = format!("{before}{}{after}{}", "`".repeat(open), "`".repeat(close));
            prop_assert_eq!(without_code_spans(&line), line);
        }
    }

    #[test]
    fn a_longer_run_can_hold_a_shorter_one_and_spans_do_not_nest() {
        assert_eq!(without_code_spans("a ``b ` c`` d"), "a   d");
        assert_eq!(without_code_spans("`a` and `b`"), "  and  ");
        assert_eq!(without_code_spans("`a``b`"), " ");
        assert_eq!(code_spans("a ``b ` c`` d `e` `` "), ["b ` c", "e"]);
    }

    #[test]
    fn a_marker_without_its_space_or_a_number_without_a_dot_is_text() {
        for line in ["-x", "*x", "1.x", "1) x", "x. y", "-", ""] {
            assert_eq!(list_item(line), None, "{line}");
        }
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        assert_eq!(heading("#What"), None);
        assert_eq!(heading("####### What"), None);
    }
}
