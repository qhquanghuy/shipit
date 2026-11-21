use std::fmt::Display;

// Every message of the formats is declared here once, so that a test can hold `docs/formats.md`
// against the code in both directions. A message that is not declared here has no way into the code.
macro_rules! messages {
    ($($name:ident = $text:expr;)*) => {
        $(pub(crate) const $name: &str = $text;)*

        #[cfg(test)]
        pub(crate) const ALL: &[&str] = &[$($name),*];
    };
}

messages! {
    NO_FILE = "no file at <path>";
    NOT_UTF8 = "<path> is not UTF-8 text";

    FENCE_UNCLOSED = "a fence opened at line <line> is never closed";
    HEADING_TWICE = "\"<heading>\" appears twice";
    BLOCK_TWICE = "more than one toml block under \"<heading>\"";
    HTML_COMMENT = "an HTML comment opens at line <line>, which hides text from the reader";
    NO_BLOCK = "<file> has no toml block under \"<heading>\"";
    FILE_PROBLEM = "<file>: <why>";
    NOT_TOML = "<file> is not valid TOML: <why>";

    GOAL_TWICE = "goal: DW<n> is numbered twice";
    GOAL_NONE = "goal: no numbered items under \"## Done-when\"";
    GOAL_PROBLEM = "goal: <why>";
    NOT_AN_ITEM = "\"<id>\" is not a done-when item of the goal";
    NO_SLICE = "no slice";

    GOAL_NAME = "must be lowercase letters, digits and \"-\", starting with a letter";
    SLICE_NAME = "must be lowercase letters, digits and \"-\", starting with a letter, and not \"roadmap\"";
    OBJECTS = "must be an array of objects";
    OBJECTS_NON_EMPTY = "must be a non-empty array of objects";
    USED_TWICE = "\"<name>\" is used twice";
    LISTED_TWICE = "\"<item>\" is listed twice";
    AFTER_ARRAY = "must be an array of slice or proof step names";
    AFTER_ITEM = "must be a slice or proof step name";
    SELF_AFTER = "\"<name>\" cannot come after itself";
    MEETS_ARRAY = "must be an array of done-when ids";
    MEETS_ITEM = "must be a done-when id of this goal, written DW4 or <goal>:DW4";
    OWNS_ARRAY = "must be an array of paths";
    OWNS_ITEM = "must be a path inside the repository, written like src/a.rs or docs/";
    OWNS_NONE = "must list at least one path";
    TRUE_OR_FALSE = "must be true or false";
    NOT_EMPTY = "must be a non-empty string";
    UNKNOWN_AFTER = "no slice or proof step named \"<name>\"";
    CYCLE = "after makes a cycle: <names>";
    OVERLAP = "\"<path>\" is also owned by <name>, which runs in parallel";

    MAP_KEY = "must be a done-when id such as \"DW4\"";
    MAP_ARRAY = "must be an array of non-empty strings";
    MAP_NONE = "must list at least one test name";
    NAME_HAS_ID = "a test name must not carry a done-when id";

    FINDING_REFUSED = "finding \"<finding>\": <why>";
    ONE_LINE = "a finding is one line, without control characters";
    BAD_START = "must start with issue, suggestion, question or nitpick, then \": \"";
    NOT_DECORATION = "\"<word>\" is not a decoration";
    NEED_BLOCKING = "issue and suggestion need (blocking) or (non-blocking)";
    NITPICK_BLOCKING = "a nitpick is never blocking";
    QUESTION_FLAG = "a question is neither blocking nor non-blocking";
    EMPTY_TEXT = "the text after \": \" is empty";

    IS_TEST_DRIVE = "<result> is a test-drive result";
    IS_REVIEW = "<result> is a review result";
    NEEDS_MUST_FIX = "<result> needs a must-fix finding";
    ALLOWS_NO_MUST_FIX = "<result> allows no must-fix finding";
    NOT_A_RESULT = "\"<word>\" is not a result: approve, approve-after-fixes, rework, pass or findings";

    USAGE = "usage: shipit format <roadmap|map|findings> …";
    ROADMAP_USAGE = "usage: shipit format roadmap <file> [--goal <goal file>]";
    MAP_USAGE = "usage: shipit format map <file>";
    FINDINGS_USAGE = "usage: shipit format findings <review|test-drive> <result> [<finding>…]";
}

// One pass, so a value that looks like `<key>` is never filled in again.
pub(crate) fn say(template: &str, fills: &[(&str, &dyn Display)]) -> String {
    let mut said = String::new();
    let mut rest = template;
    while let Some((before, after)) = rest.split_once('<') {
        said.push_str(before);
        let fill = after.split_once('>').and_then(|(key, tail)| {
            fills
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (value, tail))
        });
        if let Some((value, tail)) = fill {
            said.push_str(&value.to_string());
            rest = tail;
        } else {
            said.push('<');
            rest = after;
        }
    }
    said.push_str(rest);
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    // The doc's tables: the cell under a header named "Message", and nothing else, is a message.
    const DOC: &str = include_str!("../../tests/fixtures/formats.md");

    // Owned by pure::rules, which the setup shares.
    const SHARED: [&str; 4] = [
        "unknown key",
        "missing",
        "must be an object",
        "<file> is invalid:",
    ];

    fn cells(row: &str) -> Vec<String> {
        row.replace("\\|", "\u{1}")
            .trim()
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.replace('\u{1}', "|").trim().to_owned())
            .collect()
    }

    fn spans(cell: &str) -> Vec<String> {
        cell.split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    fn documented() -> Vec<String> {
        let mut found = Vec::new();
        let mut column: Option<usize> = None;
        for line in DOC.lines() {
            if !line.starts_with('|') {
                column = None;
                continue;
            }
            let row = cells(line);
            match column {
                None => column = row.iter().position(|cell| cell == "Message"),
                Some(at) => {
                    if let Some(cell) = row.get(at) {
                        found.extend(spans(cell));
                    }
                }
            }
        }
        found
    }

    #[test]
    fn every_message_in_the_doc_is_in_the_code_and_every_message_in_the_code_is_in_the_doc() {
        let in_doc = documented();
        let unknown: Vec<&String> = in_doc
            .iter()
            .filter(|message| {
                !ALL.contains(&message.as_str()) && !SHARED.contains(&message.as_str())
            })
            .collect();
        assert!(
            unknown.is_empty(),
            "in the doc, not in the code: {unknown:?}"
        );
        let missing: Vec<&&str> = ALL
            .iter()
            .filter(|message| !in_doc.iter().any(|documented| documented == *message))
            .collect();
        assert!(
            missing.is_empty(),
            "in the code, not in the doc: {missing:?}"
        );
    }

    #[test]
    fn a_value_that_looks_like_a_key_is_not_filled_in_again() {
        let said = say("<a> and <b> and <c>", &[("a", &"<b>"), ("b", &2)]);
        assert_eq!(said, "<b> and 2 and <c>");
    }
}
