// Text that came from git or from a person is quoted before it is printed, so that it cannot be
// taken for the tool's own words.
pub(crate) fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_why| String::from("\"\""))
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
        fn a_quoted_text_is_one_line_that_reads_back(text in ".{0,40}") {
            let quoted = json_string(&text);
            prop_assert!(!quoted.contains('\n') && !quoted.contains('\r'));
            prop_assert_eq!(serde_json::from_str::<String>(&quoted).ok(), Some(text));
        }
    }
}
