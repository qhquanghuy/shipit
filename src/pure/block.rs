use crate::pure::markdown::{Kind, lines, section};
use crate::pure::messages::{BLOCK_TWICE, say};

// Only what a reviewer sees under the heading counts: a toml block inside another fence, or in
// a later section, is never read, and a second candidate is an error rather than a choice.
pub(crate) fn toml_block<'a>(text: &'a str, heading: &str) -> Result<Option<&'a str>, String> {
    let all = lines(text)?;
    let under = section(&all, heading)?;
    let mut candidates = under
        .iter()
        .enumerate()
        .filter(|(_, line)| matches!(line.kind, Kind::Open { info: "toml" }))
        .map(|(index, _)| index);
    let Some(open) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Err(say(BLOCK_TWICE, &[("heading", &heading)]));
    }
    let inside = under.get(open + 1..).unwrap_or_default();
    let close = inside
        .iter()
        .find(|line| matches!(line.kind, Kind::Close))
        .map(|line| line.rest.len());
    let body = inside
        .first()
        .zip(close)
        .and_then(|(first, after)| first.rest.get(..first.rest.len() - after));
    Ok(body)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const HEADING: &str = "## Slices";

    fn line_set(pattern: &'static str) -> impl Strategy<Value = Vec<String>> {
        proptest::collection::vec(pattern, 0..4)
    }

    fn joined(lines: &[String]) -> String {
        lines
            .iter()
            .flat_map(|line| [line.as_str(), "\n"])
            .collect()
    }

    fn fenced(body: &[String]) -> String {
        format!("```toml\n{}```\n", joined(body))
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn the_block_is_the_one_toml_fence_directly_under_the_heading(
            before in line_set("[a-z ]{0,10}"),
            early in line_set("[a-z =\"]{0,12}"),
            between in line_set("[a-z ]{0,10}"),
            body in line_set("[a-z =\"\\[\\]]{0,12}"),
            later in line_set("[a-z =\"]{0,12}"),
        ) {
            let text = format!(
                "{}{}{HEADING}\n{}{}## Notes\n{}",
                joined(&before),
                fenced(&early),
                joined(&between),
                fenced(&body),
                fenced(&later),
            );
            let wanted = joined(&body);
            prop_assert_eq!(toml_block(&text, HEADING), Ok(Some(wanted.as_str())));
        }

        #[test]
        fn a_block_inside_another_fence_or_in_a_later_section_is_not_read(
            outer in 3_usize..7,
            body in line_set("[a-z =\"]{0,12}"),
            real in line_set("[a-z =\"]{0,12}"),
        ) {
            let example = format!(
                "{}markdown\n{}{}\n",
                "`".repeat(outer + 1),
                fenced(&body),
                "`".repeat(outer + 1),
            );
            let hidden = format!("{HEADING}\n{example}## Notes\n{}", fenced(&body));
            prop_assert_eq!(toml_block(&hidden, HEADING), Ok(None));
            let both = format!("{HEADING}\n{example}{}", fenced(&real));
            let wanted = joined(&real);
            prop_assert_eq!(toml_block(&both, HEADING), Ok(Some(wanted.as_str())));
        }

        #[test]
        fn a_heading_inside_a_fence_is_not_the_heading(
            body in line_set("[a-z =\"]{0,12}"),
            fake in line_set("[a-z =\"]{0,12}"),
        ) {
            let text = format!(
                "````\n{HEADING}\n{}````\n{HEADING}\n{}",
                fenced(&fake),
                fenced(&body),
            );
            let wanted = joined(&body);
            prop_assert_eq!(toml_block(&text, HEADING), Ok(Some(wanted.as_str())));
        }

        #[test]
        fn two_candidates_are_an_error(body in line_set("[a-z =\"]{0,12}")) {
            let text = format!("{HEADING}\n{}{}", fenced(&body), fenced(&body));
            prop_assert_eq!(
                toml_block(&text, HEADING),
                Err("more than one toml block under \"## Slices\"".to_owned())
            );
        }
    }

    #[test]
    fn what_cannot_be_read_with_certainty_is_an_error() {
        assert_eq!(
            toml_block("## Slices\n```toml\nx = 1\n", HEADING),
            Err("a fence opened at line 2 is never closed".to_owned())
        );
        assert_eq!(
            toml_block("## Slices\n## Slices\n", HEADING),
            Err("\"## Slices\" appears twice".to_owned())
        );
        assert_eq!(
            toml_block("## Slices\n<!--\n```toml\nx = 1\n```\n-->\n", HEADING),
            Err("raw HTML opens at line 2, which hides text from the reader".to_owned())
        );
    }

    #[test]
    fn a_comment_above_the_heading_cannot_hide_the_real_section() {
        let hidden = "# Roadmap\n<!--\n## Slices\n```toml\ngoal = \"evil\"\n```\n-->\n## Slices \n```toml\ngoal = \"real\"\n```\n";
        assert_eq!(
            toml_block(hidden, HEADING),
            Err("raw HTML opens at line 2, which hides text from the reader".to_owned())
        );
        let inside_a_fence = "## Slices\n```toml\nnote = \"<!--\"\n```\n";
        assert_eq!(
            toml_block(inside_a_fence, HEADING),
            Ok(Some("note = \"<!--\"\n"))
        );
    }

    #[test]
    fn a_missing_heading_or_block_is_none_and_an_empty_block_is_empty() {
        assert_eq!(toml_block("x\n```toml\ny\n```\n", HEADING), Ok(None));
        assert_eq!(
            toml_block("## Slices\n```text\ny\n```\n", HEADING),
            Ok(None)
        );
        assert_eq!(
            toml_block("## Slices\n```toml\n```\n", HEADING),
            Ok(Some(""))
        );
    }
}
