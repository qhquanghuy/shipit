pub(crate) fn comma_list<T: ToString>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(", ")
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
        fn items_are_joined_with_a_comma_and_a_space(
            items in proptest::collection::vec("[a-z ]{1,4}", 1..6),
        ) {
            let text = comma_list(&items);
            prop_assert_eq!(text.split(", ").map(str::to_owned).collect::<Vec<_>>(), items);
        }
    }
}
