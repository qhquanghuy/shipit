use crate::pure::messages::{FENCE_UNCLOSED, HEADING_TWICE, say};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind<'a> {
    Prose,
    Open { info: &'a str },
    Inside,
    Close,
}

#[derive(Debug)]
pub(crate) struct Line<'a> {
    pub(crate) text: &'a str,
    // The text from the start of this line to the end of the file.
    pub(crate) rest: &'a str,
    pub(crate) kind: Kind<'a>,
}

struct Fence {
    mark: char,
    len: usize,
}

// More than three spaces of indent make a code block, not a fence.
fn unindented(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches(' ');
    (line.len() - rest.len() <= 3).then_some(rest)
}

fn opening(line: &str) -> Option<(Fence, &str)> {
    let rest = unindented(line)?;
    let mark = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = rest.chars().take_while(|c| *c == mark).count();
    let info = rest.get(len..)?.trim();
    // An info string with a backtick would end a backtick fence on its own line.
    let valid = len >= 3 && !(mark == '`' && info.contains('`'));
    valid.then_some((Fence { mark, len }, info))
}

fn closes(line: &str, fence: &Fence) -> bool {
    unindented(line).is_some_and(|rest| {
        let run = rest.chars().take_while(|c| *c == fence.mark).count();
        run >= fence.len && rest.get(run..).is_some_and(|tail| tail.trim().is_empty())
    })
}

// A fence that never closes would swallow the rest of the file: that is an error, not a guess.
pub(crate) fn lines(text: &str) -> Result<Vec<Line<'_>>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut found = Vec::new();
    let mut open: Option<(Fence, usize)> = None;
    let mut offset = 0;
    for (index, raw) in text.split_inclusive('\n').enumerate() {
        let body = raw.strip_suffix('\n').unwrap_or(raw);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let kind = if let Some((fence, _)) = &open {
            if closes(body, fence) {
                open = None;
                Kind::Close
            } else {
                Kind::Inside
            }
        } else if let Some((fence, info)) = opening(body) {
            open = Some((fence, index + 1));
            Kind::Open { info }
        } else {
            Kind::Prose
        };
        found.push(Line {
            text: body,
            rest: text.get(offset..).unwrap_or_default(),
            kind,
        });
        offset += raw.len();
    }
    match open {
        Some((_, line)) => Err(say(FENCE_UNCLOSED, &[("line", &line)])),
        None => Ok(found),
    }
}

fn is_heading(line: &str) -> bool {
    line.starts_with("# ") || line.starts_with("## ")
}

// The lines under the heading, up to the next heading; a heading inside a fence is not one.
// A missing heading gives no lines.
pub(crate) fn section<'l, 'a>(
    lines: &'l [Line<'a>],
    heading: &str,
) -> Result<&'l [Line<'a>], String> {
    let mut found = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| matches!(line.kind, Kind::Prose) && line.text == heading)
        .map(|(index, _)| index);
    let Some(first) = found.next() else {
        return Ok(&[]);
    };
    if found.next().is_some() {
        return Err(say(HEADING_TWICE, &[("heading", &heading)]));
    }
    let after = lines.get(first + 1..).unwrap_or_default();
    let end = after
        .iter()
        .position(|line| matches!(line.kind, Kind::Prose) && is_heading(line.text))
        .unwrap_or(after.len());
    Ok(after.get(..end).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn kinds(text: &str) -> Vec<&'static str> {
        lines(text)
            .expect("closed fences")
            .iter()
            .map(|line| match line.kind {
                Kind::Prose => "prose",
                Kind::Open { .. } => "open",
                Kind::Inside => "inside",
                Kind::Close => "close",
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_fence_closes_only_on_a_line_as_long_as_its_opening(
            len in 3_usize..8,
            shorter in 3_usize..8,
            longer in 0_usize..3,
            tilde in any::<bool>(),
            indent in 0_usize..4,
        ) {
            prop_assume!(shorter < len);
            let (mark, other) = if tilde { ('~', '`') } else { ('`', '~') };
            let pad = " ".repeat(indent);
            let open = format!("{pad}{}toml", mark.to_string().repeat(len));
            let short = mark.to_string().repeat(shorter);
            let wrong_mark = other.to_string().repeat(len);
            let close = mark.to_string().repeat(len + longer);
            let text = format!("{open}\n{short}\n{wrong_mark}\n{short} x\n{pad}{close}  \nafter\n");
            prop_assert_eq!(
                kinds(&text),
                vec!["open", "inside", "inside", "inside", "close", "prose"]
            );
        }

        #[test]
        fn a_line_that_is_not_a_fence_opens_nothing(
            len in 0_usize..3,
            mark in prop::sample::select(vec!['`', '~']),
            indent in 4_usize..8,
            junk in "[a-z]{1,4}",
        ) {
            let short = format!("{}{junk}", mark.to_string().repeat(len));
            let deep = format!("{}{}", " ".repeat(indent), mark.to_string().repeat(3));
            let mixed = format!("```toml {junk}`");
            prop_assert_eq!(kinds(&format!("{short}\n{deep}\n{mixed}\n")), vec!["prose"; 3]);
        }
    }

    #[test]
    fn a_fence_that_never_closes_is_an_error_naming_its_line() {
        assert_eq!(
            lines("a\n\n```toml\nx\n").map(|found| found.len()),
            Err("a fence opened at line 3 is never closed".to_owned())
        );
        assert_eq!(
            lines("````\n```\nx\n").map(|found| found.len()),
            Err("a fence opened at line 1 is never closed".to_owned())
        );
    }

    #[test]
    fn a_section_runs_to_the_next_heading_outside_a_fence() {
        let text = "# Title\n## A\none\n```\n## B\n```\ntwo\n## C\nthree\n";
        let found = lines(text).expect("closed");
        let section = section(&found, "## A").expect("one heading");
        let shown: Vec<&str> = section.iter().map(|line| line.text).collect();
        assert_eq!(shown, ["one", "```", "## B", "```", "two"]);
        assert!(super::section(&found, "## Z").expect("none").is_empty());
    }

    #[test]
    fn a_heading_twice_is_an_error_but_not_inside_a_fence() {
        let twice = lines("## A\n## A\n").expect("closed");
        assert_eq!(
            section(&twice, "## A").map(<[Line<'_>]>::len),
            Err("\"## A\" appears twice".to_owned())
        );
        let once = lines("```\n## A\n```\n## A\nx\n").expect("closed");
        assert_eq!(section(&once, "## A").map(<[Line<'_>]>::len), Ok(1));
    }

    #[test]
    fn a_byte_order_mark_and_carriage_returns_are_not_part_of_a_line() {
        let found = lines("\u{feff}## A\r\nx\r\n").expect("closed");
        assert_eq!(section(&found, "## A").map(<[Line<'_>]>::len), Ok(1));
    }
}
