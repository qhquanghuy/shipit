// Text that came from git or from a person is quoted before it is printed, so that it cannot be
// taken for the tool's own words.
pub(crate) fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_why| String::from("\"\""))
}

// In the job summary Markdown must not open a comment, a tag, a code span, a link or an image,
// and a bare address must not turn into a link by itself (`://`, `www.`, an email's `@`).
pub(crate) fn summary_safe(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('`', "&#96;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
        .replace('@', "&#64;")
        .replace("://", "&#58;//")
        .replace("www.", "www&#46;")
}

// A `::warning` line ends at a line break, and `%` starts the escapes of the runner.
pub(crate) fn warning_safe(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
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

        #[test]
        fn a_safe_summary_has_no_raw_markup_and_reads_back(text in "[a-zw<>&`;#%\r\n:!/.@()\\[\\]-]{0,30}") {
            let safe = summary_safe(&text);
            prop_assert!(!safe.contains(['<', '>', '`', '[', ']', '@']));
            prop_assert!(!safe.contains("://") && !safe.contains("www."));
            let back = safe
                .replace("www&#46;", "www.")
                .replace("&#58;//", "://")
                .replace("&#64;", "@")
                .replace("&#93;", "]")
                .replace("&#91;", "[")
                .replace("&#96;", "`")
                .replace("&gt;", ">")
                .replace("&lt;", "<")
                .replace("&amp;", "&");
            prop_assert_eq!(back, text);
        }

        #[test]
        fn a_safe_warning_is_one_line_and_every_percent_is_an_escape(
            text in "[a-z<>&`%\r\n:!-]{0,30}",
        ) {
            let safe = warning_safe(&text);
            prop_assert!(!safe.contains(['\r', '\n']));
            for (at, _) in safe.match_indices('%') {
                let rest = safe.get(at + 1..).unwrap_or_default();
                prop_assert!(["25", "0D", "0A"].iter().any(|code| rest.starts_with(code)));
            }
        }
    }
}
