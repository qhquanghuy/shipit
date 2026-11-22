pub(crate) fn comma_list<T: ToString>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

// Text that somebody typed, for a screen: a control character, a newline and a terminal escape
// are shown as their escapes and never act.
pub(crate) fn plain(text: &str) -> String {
    let mut shown = String::new();
    for c in text.chars() {
        if c.is_control() {
            shown.extend(c.escape_default());
        } else {
            shown.push(c);
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn no_items_make_the_empty_text() {
        assert_eq!(comma_list(Vec::<String>::new()), "");
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn plain_text_has_no_control_character_and_keeps_the_rest(text in "\\PC{0,20}", noise in "[\\x00-\\x1f\\x7f]{0,4}") {
            let typed = format!("{text}{noise}{text}");
            let shown = plain(&typed);
            prop_assert!(!shown.chars().any(char::is_control));
            prop_assert_eq!(plain(&text), text);
            prop_assert_eq!(shown.contains('\n'), false);
        }

        #[test]
        fn items_are_joined_with_a_comma_and_a_space(
            items in proptest::collection::vec("[a-z ]{1,4}", 1..6),
        ) {
            let text = comma_list(&items);
            prop_assert_eq!(text.split(", ").map(str::to_owned).collect::<Vec<_>>(), items);
        }
    }
}
