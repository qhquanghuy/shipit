pub(crate) fn comma_list<T: ToString>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_joined_with_a_comma_and_a_space() {
        assert_eq!(comma_list(Vec::<String>::new()), "");
        assert_eq!(comma_list(["a"]), "a");
        assert_eq!(comma_list(["a", "b", "c"]), "a, b, c");
    }
}
