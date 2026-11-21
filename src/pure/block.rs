fn take_line(text: &str) -> Option<(&str, &str)> {
    if text.is_empty() {
        return None;
    }
    let (line, rest) = text.split_once('\n').unwrap_or((text, ""));
    Some((line.strip_suffix('\r').unwrap_or(line), rest))
}

fn skip_until<'a>(mut rest: &'a str, wanted: &str) -> Option<&'a str> {
    loop {
        let (line, after) = take_line(rest)?;
        rest = after;
        if line == wanted {
            return Some(rest);
        }
    }
}

// The lines between the fences, each with its line end; an unclosed fence is no block.
pub(crate) fn toml_block<'a>(text: &'a str, heading: &str) -> Option<&'a str> {
    let body = skip_until(skip_until(text, heading)?, "```toml")?;
    let mut rest = body;
    loop {
        let (line, after) = take_line(rest)?;
        if line == "```" {
            return body.get(..body.len() - rest.len());
        }
        rest = after;
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const HEADING: &str = "## Slices";

    fn lines(pattern: &'static str) -> impl Strategy<Value = Vec<String>> {
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
        fn the_block_is_the_first_toml_fence_after_the_heading(
            before in lines("[a-z ]{0,10}"),
            early in lines("[a-z =\"]{0,12}"),
            between in lines("[a-z ]{0,10}"),
            body in lines("[a-z =\"\\[\\]]{0,12}"),
            later in lines("[a-z =\"]{0,12}"),
        ) {
            let text = format!(
                "{}{}{HEADING}\n{}{}{}",
                joined(&before),
                fenced(&early),
                joined(&between),
                fenced(&body),
                fenced(&later),
            );
            let wanted = joined(&body);
            prop_assert_eq!(toml_block(&text, HEADING), Some(wanted.as_str()));
        }

        #[test]
        fn there_is_no_block_without_the_heading_or_a_closed_toml_fence(
            body in lines("[a-z =\"]{0,12}"),
            prose in lines("[a-z ]{0,10}"),
        ) {
            let fence = fenced(&body);
            let unclosed = format!("```toml\n{}", joined(&body));
            let prose = joined(&prose);
            let no_heading = format!("{prose}{fence}");
            let no_fence = format!("{HEADING}\n{prose}");
            let no_close = format!("{HEADING}\n{unclosed}");
            prop_assert!(toml_block(&no_heading, HEADING).is_none());
            prop_assert!(toml_block(&no_fence, HEADING).is_none());
            prop_assert!(toml_block(&no_close, HEADING).is_none());
        }
    }
}
