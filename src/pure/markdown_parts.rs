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

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        assert_eq!(heading("#What"), None);
        assert_eq!(heading("####### What"), None);
    }
}
