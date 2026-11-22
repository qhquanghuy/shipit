// A reader for the part of shell syntax that ShipIt can judge. Anything outside it is Complex,
// and a gate that meets Complex text asks: reading too little must never read as safe.

use std::mem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    pub(crate) dir: Option<String>,
    pub(crate) words: Vec<String>,
    pub(crate) reads_pipe: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Parse {
    Plain {
        segments: Vec<Segment>,
        heredocs: Vec<String>,
    },
    Complex,
}

const KEYWORDS: [&str; 12] = [
    "if", "then", "elif", "else", "fi", "for", "while", "until", "do", "done", "case", "esac",
];
const MESSAGE_FLAGS_GIT: [&str; 2] = ["-m", "--message"];
const MESSAGE_FLAGS_GH: [&str; 4] = ["-b", "--body", "-t", "--title"];
const FORM_C_OPEN: &str = "\"$(cat <<'";

pub(crate) fn raw_words(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .filter(|word| !word.is_empty())
        .collect()
}

// The raw words again, with every quote and backslash taken out first: `"g"it` is `git`.
pub(crate) fn raw_words_unquoted(text: &str) -> Vec<String> {
    let bare: String = text
        .chars()
        .filter(|c| !matches!(c, '\'' | '"' | '\\'))
        .collect();
    raw_words(&bare).into_iter().map(str::to_owned).collect()
}

pub(crate) fn program(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

// The value after a message flag is text, not a command, whatever it says.
pub(crate) fn message_values(words: &[String]) -> Vec<bool> {
    let has = |name: &str, before: usize| words.iter().take(before).any(|w| program(w) == name);
    words
        .iter()
        .enumerate()
        .map(|(at, word)| {
            let after_flag = at
                .checked_sub(1)
                .and_then(|prior| words.get(prior).map(|flag| (prior, flag.as_str())))
                .is_some_and(|(prior, flag)| {
                    (MESSAGE_FLAGS_GIT.contains(&flag) && has("git", prior))
                        || (MESSAGE_FLAGS_GH.contains(&flag) && has("gh", prior))
                });
            let glued = (word.starts_with("--message=") && has("git", at))
                || ((word.starts_with("--body=") || word.starts_with("--title=")) && has("gh", at));
            after_flag || glued
        })
        .collect()
}

#[derive(Debug)]
struct Complex;

type Read<T> = Result<T, Complex>;

struct Word {
    text: String,
    unquoted: String,
    quoted: bool,
    starts_with_tilde: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sep {
    Start,
    Semi,
    And,
    Or,
    Pipe,
    Background,
    Newline,
    End,
}

struct Pending {
    delimiter: String,
    strip_tabs: bool,
    quoted: bool,
}

struct Reader {
    chars: Vec<char>,
    at: usize,
    words: Vec<Word>,
    segments: Vec<Segment>,
    heredocs: Vec<String>,
    pending: Vec<Pending>,
    dir: Option<String>,
    before: Sep,
    reads_pipe: bool,
}

fn has_brace_expansion(unquoted: &str) -> bool {
    unquoted.match_indices('{').any(|(open, _)| {
        unquoted
            .get(open + 1..)
            .and_then(|tail| tail.split_once('}'))
            .is_some_and(|(inside, _)| inside.contains(',') || inside.contains(".."))
    })
}

fn join_dir(base: Option<&str>, dir: &str) -> String {
    match base {
        Some(base) if !dir.starts_with('/') => format!("{base}/{dir}"),
        Some(_) | None => dir.to_owned(),
    }
}

impl Reader {
    fn new(command: &str) -> Self {
        Self {
            chars: command.chars().collect(),
            at: 0,
            words: Vec::new(),
            segments: Vec::new(),
            heredocs: Vec::new(),
            pending: Vec::new(),
            dir: None,
            before: Sep::Start,
            reads_pipe: false,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn peek_ahead(&self, n: usize) -> Option<char> {
        self.chars.get(self.at + n).copied()
    }

    fn skip_blanks(&mut self) {
        loop {
            match (self.peek(), self.peek_ahead(1)) {
                (Some(' ' | '\t'), _) => self.at += 1,
                (Some('\\'), Some('\n')) => self.at += 2,
                _ => return,
            }
        }
    }

    fn run(mut self) -> Read<(Vec<Segment>, Vec<String>)> {
        loop {
            self.skip_blanks();
            let Some(c) = self.peek() else { break };
            match c {
                '\n' => {
                    self.at += 1;
                    self.end_segment(Sep::Newline)?;
                    self.read_bodies()?;
                }
                ';' => {
                    self.at += 1;
                    if self.peek() == Some(';') {
                        return Err(Complex);
                    }
                    self.end_segment(Sep::Semi)?;
                }
                '&' => self.ampersand()?,
                '|' => {
                    self.at += 1;
                    if self.peek() == Some('|') {
                        self.at += 1;
                        self.end_segment(Sep::Or)?;
                    } else {
                        if self.peek() == Some('&') {
                            self.at += 1;
                        }
                        self.end_segment(Sep::Pipe)?;
                    }
                }
                '>' | '<' => self.redirect()?,
                _ => self.word()?,
            }
        }
        if !self.pending.is_empty() {
            return Err(Complex);
        }
        self.end_segment(Sep::End)?;
        Ok((self.segments, self.heredocs))
    }

    fn ampersand(&mut self) -> Read<()> {
        self.at += 1;
        match self.peek() {
            Some('&') => {
                self.at += 1;
                self.end_segment(Sep::And)
            }
            Some('>') => {
                self.at += 1;
                if self.peek() == Some('>') {
                    self.at += 1;
                }
                self.drop_target()
            }
            Some(_) | None => self.end_segment(Sep::Background),
        }
    }

    fn redirect(&mut self) -> Read<()> {
        let operator = self.peek();
        self.at += 1;
        match (operator, self.peek()) {
            (Some('>'), Some('>' | '&')) | (Some('<'), Some('&')) => self.at += 1,
            (Some('>'), Some('|')) | (Some('<'), Some('(' | '>')) => return Err(Complex),
            (Some('<'), Some('<')) => return self.heredoc(),
            (Some(_) | None, Some(_) | None) => {}
        }
        self.drop_target()
    }

    // The target of a redirection is not a word of the command.
    fn drop_target(&mut self) -> Read<()> {
        self.skip_blanks();
        let target = self.read_word()?;
        if target.text.is_empty() && !target.quoted {
            return Err(Complex);
        }
        Ok(())
    }

    fn heredoc(&mut self) -> Read<()> {
        self.at += 1;
        if self.peek() == Some('<') {
            return Err(Complex);
        }
        let strip_tabs = self.peek() == Some('-');
        if strip_tabs {
            self.at += 1;
        }
        self.skip_blanks();
        let delimiter = self.read_word()?;
        if delimiter.text.is_empty() {
            return Err(Complex);
        }
        self.pending.push(Pending {
            delimiter: delimiter.text,
            strip_tabs,
            quoted: delimiter.quoted,
        });
        Ok(())
    }

    fn read_line(&mut self) -> Read<String> {
        if self.at >= self.chars.len() {
            return Err(Complex);
        }
        let mut line = String::new();
        while let Some(c) = self.peek() {
            self.at += 1;
            if c == '\n' {
                break;
            }
            line.push(c);
        }
        Ok(line)
    }

    // The bodies of the heredocs opened on the line that has just ended, in order.
    fn read_bodies(&mut self) -> Read<()> {
        for pending in mem::take(&mut self.pending) {
            let mut lines: Vec<String> = Vec::new();
            loop {
                let line = self.read_line()?;
                let compared = if pending.strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if compared == pending.delimiter {
                    break;
                }
                lines.push(line);
            }
            let body = lines.join("\n");
            if !pending.quoted && body.contains(['$', '`', '\\']) {
                return Err(Complex);
            }
            self.heredocs.push(body);
        }
        Ok(())
    }

    fn word(&mut self) -> Read<()> {
        let word = self.read_word()?;
        let is_fd = !word.quoted
            && !word.text.is_empty()
            && word.text.chars().all(|c| c.is_ascii_digit())
            && matches!(self.peek(), Some('>' | '<'));
        if is_fd || (word.text.is_empty() && !word.quoted) {
            return Ok(());
        }
        self.finish_word(word)
    }

    fn finish_word(&mut self, word: Word) -> Read<()> {
        let text = word.text.as_str();
        let whole = !word.quoted;
        if (whole && matches!(text, "!" | "{" | "}"))
            || word.starts_with_tilde
            || matches!(text, "pushd" | "popd")
            || has_brace_expansion(&word.unquoted)
            || (self.words.is_empty() && whole && KEYWORDS.contains(&text))
            || (whole && text == "function")
        {
            return Err(Complex);
        }
        self.words.push(word);
        Ok(())
    }

    fn read_word(&mut self) -> Read<Word> {
        let mut word = Word {
            text: String::new(),
            unquoted: String::new(),
            quoted: false,
            starts_with_tilde: false,
        };
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' => break,
                '\'' => {
                    self.at += 1;
                    word.quoted = true;
                    loop {
                        match self.peek() {
                            None => return Err(Complex),
                            Some('\'') => {
                                self.at += 1;
                                break;
                            }
                            Some('\n') if !self.pending.is_empty() => return Err(Complex),
                            Some(inside) => {
                                self.at += 1;
                                word.text.push(inside);
                            }
                        }
                    }
                }
                '"' => {
                    if word.text.is_empty()
                        && !word.quoted
                        && self.message_context()
                        && let Some((body, to)) = self.form_c()
                    {
                        word.text = body;
                        word.quoted = true;
                        self.at = to;
                        continue;
                    }
                    word.quoted = true;
                    self.double_quoted(&mut word.text)?;
                }
                '\\' => {
                    if self.peek_ahead(1) == Some('\n') {
                        self.at += 2;
                    } else {
                        return Err(Complex);
                    }
                }
                '$' | '`' | '(' | ')' => return Err(Complex),
                plain => {
                    self.at += 1;
                    if word.text.is_empty() && plain == '~' {
                        word.starts_with_tilde = true;
                    }
                    word.text.push(plain);
                    word.unquoted.push(plain);
                }
            }
        }
        Ok(word)
    }

    fn double_quoted(&mut self, text: &mut String) -> Read<()> {
        self.at += 1;
        loop {
            let Some(c) = self.peek() else {
                return Err(Complex);
            };
            self.at += 1;
            match c {
                '"' => return Ok(()),
                '$' | '`' => return Err(Complex),
                '\n' if !self.pending.is_empty() => return Err(Complex),
                '\\' => match self.peek() {
                    Some(escaped @ ('"' | '\\' | '$' | '`')) => {
                        self.at += 1;
                        text.push(escaped);
                    }
                    Some('\n') => self.at += 1,
                    Some(_) | None => text.push('\\'),
                },
                other => text.push(other),
            }
        }
    }

    // A message flag of a program that takes one, after that program in the same segment.
    fn message_context(&self) -> bool {
        let Some((flag, earlier)) = self.words.split_last() else {
            return false;
        };
        let has = |name: &str| earlier.iter().any(|word| program(&word.text) == name);
        (MESSAGE_FLAGS_GIT.contains(&flag.text.as_str()) && has("git"))
            || (MESSAGE_FLAGS_GH.contains(&flag.text.as_str()) && has("gh"))
    }

    // `"$(cat <<'D'` newline, the body, `D` on a line of its own, newline, `)"`: the form that
    // writes a long message. The body is one literal word.
    fn form_c(&self) -> Option<(String, usize)> {
        let rest: String = self.chars.get(self.at..)?.iter().collect();
        let after = rest.strip_prefix(FORM_C_OPEN)?;
        let (delimiter, mut remaining) = after.split_once("'\n")?;
        if delimiter.is_empty()
            || !delimiter
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return None;
        }
        let mut lines: Vec<&str> = Vec::new();
        loop {
            let (line, tail) = remaining.split_once('\n')?;
            if line == delimiter {
                let closed = tail.strip_prefix(")\"")?;
                if !matches!(
                    closed.chars().next(),
                    None | Some(' ' | '\t' | '\n' | ';' | '&' | '|')
                ) {
                    return None;
                }
                let used = rest.chars().count() - closed.chars().count();
                return Some((lines.join("\n"), self.at + used));
            }
            lines.push(line);
            remaining = tail;
        }
    }

    fn end_segment(&mut self, after: Sep) -> Read<()> {
        let words = mem::take(&mut self.words);
        let before = mem::replace(&mut self.before, after);
        let reads_pipe = mem::replace(&mut self.reads_pipe, after == Sep::Pipe);
        if words.is_empty() {
            return Ok(());
        }
        let texts: Vec<String> = words.iter().map(|word| word.text.clone()).collect();
        let mut next_dir = None;
        if texts.first().map(String::as_str) == Some("cd") {
            let [_, dir] = texts.as_slice() else {
                return Err(Complex);
            };
            let glob = words
                .get(1)
                .is_some_and(|word| word.unquoted.contains(['*', '?', '[']));
            let plain_before = matches!(before, Sep::Start | Sep::Semi | Sep::And | Sep::Newline);
            let plain_after = matches!(after, Sep::Semi | Sep::And | Sep::Newline | Sep::End);
            if dir.is_empty() || dir.starts_with('-') || glob || !plain_before || !plain_after {
                return Err(Complex);
            }
            next_dir = Some(join_dir(self.dir.as_deref(), dir));
        }
        self.segments.push(Segment {
            dir: self.dir.clone(),
            words: texts,
            reads_pipe,
        });
        if next_dir.is_some() {
            self.dir = next_dir;
        }
        Ok(())
    }
}

pub(crate) fn parse(command: &str) -> Parse {
    match Reader::new(command).run() {
        Ok((segments, heredocs)) => Parse::Plain { segments, heredocs },
        Err(Complex) => Parse::Complex,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn plain(command: &str) -> Vec<Vec<String>> {
        match parse(command) {
            Parse::Plain { segments, .. } => {
                segments.into_iter().map(|segment| segment.words).collect()
            }
            Parse::Complex => panic!("{command:?} is complex"),
        }
    }

    fn is_complex(command: &str) -> bool {
        parse(command) == Parse::Complex
    }

    fn word() -> impl Strategy<Value = String> {
        "[a-z0-9][a-z0-9_./=:16562]{0,8}".prop_filter(
            "a word that is no keyword or folder command",
            |word| {
                !KEYWORDS.contains(&word.as_str())
                    && !["cd", "pushd", "popd", "function"].contains(&word.as_str())
            },
        )
    }

    fn quoted(word: &str, how: u8) -> String {
        match how % 3 {
            0 => word.to_owned(),
            1 => format!("'{word}'"),
            _ => format!("\"{word}\""),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn quoting_keeps_words(
            words in proptest::collection::vec((word(), any::<u8>()), 1..6),
        ) {
            let text: Vec<String> = words.iter().map(|(w, how)| quoted(w, *how)).collect();
            let expected: Vec<String> = words.iter().map(|(w, _)| w.clone()).collect();
            prop_assert_eq!(plain(&text.join(" ")), vec![expected]);
        }

        #[test]
        fn separators_split_segments(
            a in word(),
            b in word(),
            sep in prop_oneof![Just(";"), Just("&&"), Just("||"), Just("|"), Just("&"), Just("\n")],
        ) {
            prop_assume!(!KEYWORDS.contains(&a.as_str()) && !KEYWORDS.contains(&b.as_str()));
            prop_assume!(a != "cd" && b != "cd");
            prop_assert_eq!(
                plain(&format!("{a} x {sep} {b} y")),
                vec![vec![a, "x".to_owned()], vec![b, "y".to_owned()]]
            );
        }

        #[test]
        fn single_quotes_hold_anything_but_a_quote(text in "[^'\n]{0,20}") {
            prop_assert_eq!(plain(&format!("echo '{text}'")), vec![vec!["echo".to_owned(), text]]);
        }

        #[test]
        fn a_dollar_or_backtick_outside_single_quotes_is_complex(
            before in "[a-z ]{0,6}",
            mark in prop_oneof![Just("$"), Just("`")],
            wrap in prop_oneof![Just(""), Just("\"")],
        ) {
            let text = format!("echo {before}{wrap}{mark}x{wrap}");
            prop_assert!(is_complex(&text));
        }

        #[test]
        fn cd_sets_the_folder_of_the_later_segments(
            a in "[a-z]{1,5}",
            b in "[a-z]{1,5}",
        ) {
            let Parse::Plain { segments, .. } = parse(&format!("cd {a} && cd {b}\nls")) else {
                return Err(TestCaseError::fail("complex"));
            };
            let dirs: Vec<Option<String>> = segments.into_iter().map(|s| s.dir).collect();
            prop_assert_eq!(dirs, vec![None, Some(a.clone()), Some(format!("{a}/{b}"))]);
        }

        #[test]
        fn a_cd_that_may_not_run_or_has_no_plain_folder_is_complex(
            dir in "[a-z]{1,5}",
            tail in prop_oneof![Just(" || ls"), Just(" | ls"), Just(" & ls")],
        ) {
            { let text = format!("cd {dir}{tail}"); prop_assert!(is_complex(&text)); }
            { let text = format!("ls | cd {dir}"); prop_assert!(is_complex(&text)); }
            { let text = format!("ls || cd {dir}"); prop_assert!(is_complex(&text)); }
            for bad in ["cd", "cd -", "cd ~/x", "cd -P x", "cd a b", "cd *", "pushd x", "popd"] {
                prop_assert!(is_complex(bad), "{}", bad);
            }
        }

        #[test]
        fn the_form_that_writes_a_long_message_is_one_word(
            body in proptest::collection::vec("[ -~]{1,20}", 1..4),
            flag in prop_oneof![Just("-m"), Just("--message")],
        ) {
            prop_assume!(body.iter().all(|line| line != "EOF"));
            let text = format!(
                "git commit {flag} \"$(cat <<'EOF'\n{}\nEOF\n)\" && ls",
                body.join("\n")
            );
            let expected = vec![
                vec!["git".to_owned(), "commit".to_owned(), flag.to_owned(), body.join("\n")],
                vec!["ls".to_owned()],
            ];
            prop_assert_eq!(plain(&text), expected);
        }

        #[test]
        fn that_form_is_complex_for_a_program_that_takes_no_message(
            program in "[a-z]{1,5}",
        ) {
            prop_assume!(program != "git" && program != "gh");
            { let text = format!("{program} -m \"$(cat <<'EOF'\nx\nEOF\n)\""); prop_assert!(is_complex(&text)); }
        }

        #[test]
        fn heredoc_bodies_are_collected_not_read_as_commands(
            body in proptest::collection::vec("[ -~&&[^$`\\\\]]{0,20}", 1..4),
        ) {
            prop_assume!(body.iter().all(|line| line != "EOF"));
            let text = format!("cat <<'EOF' > out\n{}\nEOF\nls", body.join("\n"));
            let Parse::Plain { segments, heredocs } = parse(&text) else {
                return Err(TestCaseError::fail("complex"));
            };
            prop_assert_eq!(heredocs, vec![body.join("\n")]);
            prop_assert_eq!(segments.len(), 2);
        }

        #[test]
        fn an_unquoted_heredoc_that_expands_is_complex(mark in prop_oneof![Just("$x"), Just("`x`")]) {
            { let text = format!("cat <<EOF\n{mark}\nEOF\n"); prop_assert!(is_complex(&text)); }
            prop_assert!(!is_complex("cat <<EOF\nplain\nEOF\n"));
            { let text = format!("cat <<'EOF'\n{mark}\nEOF\n"); prop_assert!(!is_complex(&text)); }
        }

        #[test]
        fn redirections_are_dropped_with_their_target(
            target in "[a-z/]{1,6}",
            op in prop_oneof![Just(">"), Just(">>"), Just("2>"), Just("&>"), Just("<"), Just("2>&")],
        ) {
            prop_assert_eq!(plain(&format!("ls x {op}{target}")), vec![vec!["ls".to_owned(), "x".to_owned()]]);
            prop_assert_eq!(plain(&format!("ls {op} {target} x")), vec![vec!["ls".to_owned(), "x".to_owned()]]);
        }

        #[test]
        fn these_forms_are_complex(
            form in prop_oneof![
                Just("( ls )"), Just("{ ls; }"), Just("! ls"), Just("ls <(x)"), Just("ls ~/x"),
                Just("echo {a,b}"), Just("echo {1..3}"), Just("if ls; then ls; fi"),
                Just("for x in a; do ls; done"), Just("ls \\x"), Just("ls 'x"), Just("ls \"x"),
                Just("ls <<< x"), Just("ls >| x"), Just("ls >"), Just("case x in esac"),
            ],
        ) {
            prop_assert!(is_complex(form));
        }

        #[test]
        fn these_forms_are_plain(
            form in prop_oneof![
                Just("git log @{u}.."), Just("git stash show stash@{0}"), Just("git diff HEAD~1"),
                Just("echo a\\\nb"), Just("ls 2>&1 | wc -l"), Just("echo ok # note"),
                Just("A=1 B=2 make x"),
            ],
        ) {
            prop_assert!(!is_complex(form));
        }

        #[test]
        fn programs_are_base_names(dir in "[a-z/]{0,8}", name in "[a-z]{1,5}") {
            let path = format!("{dir}/{name}");
            prop_assert_eq!(program(&path), name.as_str());
        }

        #[test]
        fn raw_words_are_the_runs_of_word_characters(text in "[a-z _/'\"-]{0,20}") {
            for word in raw_words(&text) {
                prop_assert!(word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
            }
            prop_assert_eq!(raw_words_unquoted("\"g\"it 'p'ush"), ["git", "push"]);
        }
    }
}
