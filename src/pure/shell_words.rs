// A reader for the part of shell syntax that ShipIt can judge. Anything outside it is Complex,
// and a gate that meets Complex text asks: reading too little must never read as safe.

use std::mem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    pub(crate) dir: Option<String>,
    pub(crate) words: Vec<String>,
    // The input is a pipe or a file (`<`): a program read from it was written somewhere else.
    pub(crate) reads_stdin: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Parse {
    Plain {
        segments: Vec<Segment>,
        heredocs: Vec<String>,
    },
    Complex(Unreadable),
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

// The raw words again, with every quote and backslash taken out first, and the line joins:
// `"g"it` and a `gi`, backslash, newline, `t` are `git`.
pub(crate) fn unquoted(text: &str) -> String {
    text.replace("\\\n", "")
        .chars()
        .filter(|c| !matches!(c, '\'' | '"' | '\\'))
        .collect()
}

pub(crate) fn raw_words_unquoted(text: &str) -> Vec<String> {
    raw_words(&unquoted(text))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub(crate) fn program(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        let mut chars = name.chars();
        chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

// The program of a segment: its first word that is not a `NAME=value` prefix.
pub(crate) fn leading_program<'a>(mut words: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    words.find(|word| !is_assignment(word)).map(program)
}

// The value after a message flag is text, not a command, whatever it says: but only when the
// segment's own program is the one that takes the message. `sh -c '$2' git -m x` is a shell.
pub(crate) fn message_values(words: &[String]) -> Vec<bool> {
    let owner = leading_program(words.iter().map(String::as_str));
    let git = owner == Some("git");
    let gh = owner == Some("gh");
    words
        .iter()
        .enumerate()
        .map(|(at, word)| {
            let after_flag = at
                .checked_sub(1)
                .and_then(|prior| words.get(prior))
                .is_some_and(|flag| {
                    (git && MESSAGE_FLAGS_GIT.contains(&flag.as_str()))
                        || (gh && MESSAGE_FLAGS_GH.contains(&flag.as_str()))
                });
            let glued = (git && word.starts_with("--message="))
                || (gh && (word.starts_with("--body=") || word.starts_with("--title=")));
            after_flag || glued
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unreadable {
    Expansion,
    Group,
    Tilde,
    Comment,
    Braces,
    Backslash,
    Unclosed,
    Heredoc,
    Redirect,
    Folder,
}

impl Unreadable {
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::Expansion => "a `$` or a backquote other than `$?`",
            Self::Group => {
                "a subshell, a group or a shell keyword (`(`, `{`, `!`, `if`, `for`, `while`, `case`, `function` and the like)"
            }
            Self::Tilde => "a `~` other than in `cd ~/<folder>`",
            Self::Comment => "a comment (a word that starts with `#`)",
            Self::Braces => "a brace expansion (`{a,b}` or `{1..3}`)",
            Self::Backslash => "a backslash outside quotes",
            Self::Unclosed => "a quote or a heredoc that is not closed",
            Self::Heredoc => {
                "a heredoc ShipIt does not read (`<<<`, no delimiter, or an unquoted delimiter whose text has `$`, a backquote or a backslash)"
            }
            Self::Redirect => "a redirect ShipIt does not read (`<(`, `>|`, `<>` or no target)",
            Self::Folder => {
                "a folder ShipIt does not follow (`cd` alone, with `-`, an option, a glob, `$?` or two folders, a `cd` that may not run, `pushd`, `popd`, or a git `-C`, `--git-dir` or `--work-tree` that holds `$?`)"
            }
        }
    }
}

type Read<T> = Result<T, Unreadable>;

struct Word {
    text: String,
    unquoted: String,
    quoted: bool,
    starts_with_tilde: bool,
    starts_with_hash: bool,
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
    reads_stdin: bool,
    from_file: bool,
}

fn has_brace_expansion(unquoted: &str) -> bool {
    unquoted.match_indices('{').any(|(open, _)| {
        unquoted
            .get(open + 1..)
            .and_then(|tail| tail.split_once('}'))
            .is_some_and(|(inside, _)| inside.contains(',') || inside.contains(".."))
    })
}

// Only git takes a folder whose runtime digits could name a repository a gate must judge: `$?`
// expands to digits only, so elsewhere it can never spell `git`, `.git` or a branch. The program is
// `git` itself (after `NAME=value`, by any path), or any word after `env` or `command`, which
// run the next program (reading too much here only asks).
fn git_folder_holds_exit_status(words: &[String]) -> bool {
    let mut rest = words.iter().skip_while(|word| is_assignment(word));
    let args: Vec<&String> = match rest.next().map(|word| program(word)) {
        Some("git") => rest.collect(),
        Some("env" | "command") => rest
            .skip_while(|word| program(word) != "git")
            .skip(1)
            .collect(),
        Some(_) | None => return false,
    };
    args.iter().enumerate().any(|(at, word)| {
        let after_flag = at
            .checked_sub(1)
            .and_then(|prior| args.get(prior))
            .is_some_and(|flag| matches!(flag.as_str(), "-C" | "--git-dir" | "--work-tree"));
        let glued = word.starts_with("--git-dir=") || word.starts_with("--work-tree=");
        (after_flag || glued) && word.contains("$?")
    })
}

fn join_dir(base: Option<&str>, dir: &str) -> String {
    match base {
        Some(base) if !dir.starts_with('/') && !dir.starts_with("~/") => format!("{base}/{dir}"),
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
            reads_stdin: false,
            from_file: false,
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
                        return Err(Unreadable::Group);
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
            return Err(Unreadable::Unclosed);
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
        if operator == Some('<') && !matches!(self.peek(), Some('<' | '&' | '(' | '>')) {
            self.from_file = true;
        }
        match (operator, self.peek()) {
            (Some('>'), Some('>' | '&')) | (Some('<'), Some('&')) => self.at += 1,
            (Some('>'), Some('|')) | (Some('<'), Some('(' | '>')) => {
                return Err(Unreadable::Redirect);
            }
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
            return Err(Unreadable::Redirect);
        }
        Ok(())
    }

    fn heredoc(&mut self) -> Read<()> {
        self.at += 1;
        if self.peek() == Some('<') {
            return Err(Unreadable::Heredoc);
        }
        let strip_tabs = self.peek() == Some('-');
        if strip_tabs {
            self.at += 1;
        }
        self.skip_blanks();
        let delimiter = self.read_word()?;
        if delimiter.text.is_empty() {
            return Err(Unreadable::Heredoc);
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
            return Err(Unreadable::Unclosed);
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
                return Err(Unreadable::Heredoc);
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
        if whole && matches!(text, "!" | "{" | "}") {
            return Err(Unreadable::Group);
        }
        if word.starts_with_tilde && !self.is_home_folder(&word) {
            return Err(Unreadable::Tilde);
        }
        if word.starts_with_hash {
            return Err(Unreadable::Comment);
        }
        if matches!(text, "pushd" | "popd") {
            return Err(Unreadable::Folder);
        }
        if has_brace_expansion(&word.unquoted) {
            return Err(Unreadable::Braces);
        }
        if (self.words.is_empty() && whole && KEYWORDS.contains(&text))
            || (whole && text == "function")
        {
            return Err(Unreadable::Group);
        }
        self.words.push(word);
        Ok(())
    }

    // `cd ~/x`: the one place a tilde is read, as the folder `$HOME/x`; any other tilde (`~`,
    // `~user`, `ls ~/x`) is expanded by the shell in a way ShipIt does not follow.
    fn is_home_folder(&self, word: &Word) -> bool {
        word.text.starts_with("~/")
            && matches!(self.words.as_slice(), [first] if first.text == "cd" && !first.quoted)
    }

    fn read_word(&mut self) -> Read<Word> {
        let mut word = Word {
            text: String::new(),
            unquoted: String::new(),
            quoted: false,
            starts_with_tilde: false,
            starts_with_hash: false,
        };
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' => break,
                '\'' => {
                    self.at += 1;
                    word.quoted = true;
                    loop {
                        match self.peek() {
                            None => return Err(Unreadable::Unclosed),
                            Some('\'') => {
                                self.at += 1;
                                break;
                            }
                            Some('\n') if !self.pending.is_empty() => {
                                return Err(Unreadable::Heredoc);
                            }
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
                        return Err(Unreadable::Backslash);
                    }
                }
                '$' if self.peek_ahead(1) == Some('?') => {
                    self.at += 2;
                    word.text.push_str("$?");
                    word.unquoted.push_str("$?");
                }
                '$' | '`' => return Err(Unreadable::Expansion),
                '(' | ')' => return Err(Unreadable::Group),
                plain => {
                    self.at += 1;
                    if word.text.is_empty() && plain == '~' {
                        word.starts_with_tilde = true;
                    }
                    if word.text.is_empty() && plain == '#' {
                        word.starts_with_hash = true;
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
                return Err(Unreadable::Unclosed);
            };
            self.at += 1;
            match c {
                '"' => return Ok(()),
                '$' if self.peek() == Some('?') => {
                    self.at += 1;
                    text.push_str("$?");
                }
                '$' | '`' => return Err(Unreadable::Expansion),
                '\n' if !self.pending.is_empty() => return Err(Unreadable::Heredoc),
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

    // A message flag of the program that takes one: the segment's own program, not a word that
    // happens to be called git.
    fn message_context(&self) -> bool {
        let Some((flag, earlier)) = self.words.split_last() else {
            return false;
        };
        let owner = leading_program(earlier.iter().map(|word| word.text.as_str()));
        (owner == Some("git") && MESSAGE_FLAGS_GIT.contains(&flag.text.as_str()))
            || (owner == Some("gh") && MESSAGE_FLAGS_GH.contains(&flag.text.as_str()))
    }

    // `"$(cat <<'D'` newline, the body, `D` on a line of its own, newline, `)"`: the form that
    // writes a long message. The body is one literal word. Bash ends the body also at a line
    // that only starts with `D` (`D)`), so any such line before the real end is refused.
    fn form_c(&self) -> Option<(String, usize)> {
        let chars = self.chars.get(self.at..)?;
        let open: Vec<char> = FORM_C_OPEN.chars().collect();
        let after_open = chars.strip_prefix(open.as_slice())?;
        let quote = after_open.iter().position(|c| *c == '\'')?;
        let (delimiter, from_quote) = after_open.split_at(quote);
        let mut remaining = from_quote.get(1..)?.strip_prefix(&['\n'])?;
        if delimiter.is_empty()
            || !delimiter
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            return None;
        }
        let mut lines: Vec<String> = Vec::new();
        loop {
            let newline = remaining.iter().position(|c| *c == '\n')?;
            let (line, from_newline) = remaining.split_at(newline);
            let tail = from_newline.get(1..)?;
            if line == delimiter {
                let closed = tail.strip_prefix(&[')', '"'])?;
                if !matches!(
                    closed.first(),
                    None | Some(' ' | '\t' | '\n' | ';' | '&' | '|')
                ) {
                    return None;
                }
                return Some((lines.join("\n"), self.at + chars.len() - closed.len()));
            }
            if line.starts_with(delimiter) {
                return None;
            }
            lines.push(line.iter().collect());
            remaining = tail;
        }
    }

    fn end_segment(&mut self, after: Sep) -> Read<()> {
        let words = mem::take(&mut self.words);
        let before = mem::replace(&mut self.before, after);
        let piped = mem::replace(&mut self.reads_stdin, after == Sep::Pipe);
        let from_file = mem::take(&mut self.from_file);
        let reads_stdin = piped || from_file;
        if words.is_empty() {
            return Ok(());
        }
        let texts: Vec<String> = words.iter().map(|word| word.text.clone()).collect();
        if git_folder_holds_exit_status(&texts) {
            return Err(Unreadable::Folder);
        }
        let mut next_dir = None;
        if texts.first().map(String::as_str) == Some("cd") {
            let [_, dir] = texts.as_slice() else {
                return Err(Unreadable::Folder);
            };
            let glob = words
                .get(1)
                .is_some_and(|word| word.unquoted.contains(['*', '?', '[']));
            let plain_before = matches!(before, Sep::Start | Sep::Semi | Sep::And | Sep::Newline);
            let plain_after = matches!(after, Sep::Semi | Sep::And | Sep::Newline | Sep::End);
            if dir.is_empty()
                || dir.starts_with('-')
                || dir.contains("$?")
                || glob
                || !plain_before
                || !plain_after
            {
                return Err(Unreadable::Folder);
            }
            // A tilde inside quotes is a folder called `~`, not the home folder.
            let literal_tilde =
                dir.starts_with("~/") && !words.get(1).is_some_and(|word| word.starts_with_tilde);
            let dir = if literal_tilde {
                format!("./{dir}")
            } else {
                dir.clone()
            };
            next_dir = Some(join_dir(self.dir.as_deref(), &dir));
        }
        self.segments.push(Segment {
            dir: self.dir.clone(),
            words: texts,
            reads_stdin,
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
        Err(cause) => Parse::Complex(cause),
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
            Parse::Complex(_) => panic!("{command:?} is complex"),
        }
    }

    fn is_complex(command: &str) -> bool {
        matches!(parse(command), Parse::Complex(_))
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
        fn dollar_question_is_text_and_any_other_dollar_is_unreadable(
            before in "[a-z=]{0,6}",
            after in "[a-z=]{0,6}",
            wrap in prop_oneof![Just(""), Just("\"")],
            other in "[a-zA-Z0-9_{(@#!*$'-]",
            lead in prop_oneof![
                Just(""), Just("X=1 "), Just("/usr/bin/"), Just("X=1 /usr/bin/"),
                Just("env "), Just("env -i X=1 "), Just("/usr/bin/env "), Just("command "),
                Just("command -p "), Just("command -- "),
            ],
        ) {
            prop_assert_eq!(
                plain(&format!("echo {wrap}{before}$?{after}{wrap}")),
                vec![vec!["echo".to_owned(), format!("{before}$?{after}")]]
            );
            prop_assert_eq!(
                parse(&format!("echo {wrap}{before}${other}{after}{wrap}")),
                Parse::Complex(Unreadable::Expansion)
            );
            let folder = Parse::Complex(Unreadable::Folder);
            prop_assert_eq!(parse(&format!("cd {wrap}{before}$?{wrap} && ls")), folder);
            for flag in ["-C ", "--git-dir ", "--work-tree ", "--git-dir=", "--work-tree="] {
                let command = format!("{lead}git {flag}{wrap}{before}$?{wrap} status");
                prop_assert_eq!(parse(&command), Parse::Complex(Unreadable::Folder), "{}", command);
            }
            prop_assert_eq!(
                plain(&format!("ls -C {before}$?")),
                vec![vec!["ls".to_owned(), "-C".to_owned(), format!("{before}$?")]]
            );
            prop_assert_eq!(
                plain(&format!("{lead}git -c x=y -C {before}a status")).len(),
                1
            );
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
        fn cd_to_a_folder_under_home_is_read_and_any_other_tilde_is_complex(
            a in "[a-z]{1,5}",
            b in "[a-z]{1,5}",
            other in "[a-z]{1,5}",
        ) {
            let dirs = |command: String| match parse(&command) {
                Parse::Plain { segments, .. } => segments.into_iter().map(|s| s.dir).collect(),
                Parse::Complex(_) => vec![Some("complex".to_owned())],
            };
            prop_assert_eq!(dirs(format!("cd ~/{a}\nls")), vec![None, Some(format!("~/{a}"))]);
            // A later folder is under the home folder, or replaces it, or is replaced by it.
            prop_assert_eq!(
                dirs(format!("cd ~/{a} && cd {b}\nls")),
                vec![None, Some(format!("~/{a}")), Some(format!("~/{a}/{b}"))]
            );
            prop_assert_eq!(
                dirs(format!("cd {a} && cd ~/{b}\nls")),
                vec![None, Some(a.clone()), Some(format!("~/{b}"))]
            );
            // Quoted, it is a folder called `~`.
            prop_assert_eq!(dirs(format!("cd \"~/{a}\"\nls")), vec![None, Some(format!("./~/{a}"))]);
            for bad in [
                "cd ~".to_owned(), format!("cd ~{other}/{a}"), format!("cd -P ~/{a}"),
                format!("ls ~/{a}"), format!("A=1 cd ~/{a}"), format!("cd ~/{a} ~/{b}"),
                format!("cd ~/{a}* && ls"), format!("cd ~/{a} || ls"), format!("cd ~/{{{a},{b}}}"),
            ] {
                prop_assert!(is_complex(&bad), "{}", bad);
            }
        }

        #[test]
        fn a_cd_that_may_not_run_or_has_no_plain_folder_is_complex(
            dir in "[a-z]{1,5}",
            tail in prop_oneof![Just(" || ls"), Just(" | ls"), Just(" & ls")],
        ) {
            { let text = format!("cd {dir}{tail}"); prop_assert!(is_complex(&text)); }
            { let text = format!("ls | cd {dir}"); prop_assert!(is_complex(&text)); }
            { let text = format!("ls || cd {dir}"); prop_assert!(is_complex(&text)); }
            for bad in ["cd", "cd -", "cd -P x", "cd a b", "cd *", "pushd x", "popd"] {
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
        fn that_form_is_complex_when_the_program_is_not_the_first_word(
            prefix in prop_oneof![
                Just("sh -c '$2' git"), Just("env git"), Just("nice git"), Just("echo git"),
            ],
            flag in prop_oneof![Just("-m"), Just("--message")],
        ) {
            let text = format!("{prefix} {flag} \"$(cat <<'EOF'\nx\nEOF\n)\"");
            prop_assert!(is_complex(&text));
            let owned = format!("A=1 B=2 git {flag} \"$(cat <<'EOF'\nx\nEOF\n)\"");
            prop_assert!(!is_complex(&owned));
        }

        #[test]
        fn a_message_is_data_only_for_the_program_that_takes_it(
            before in prop_oneof![Just(""), Just("A=1 "), Just("A=1 B=2 "), Just("/usr/bin/")],
            text in "[a-z ]{1,10}",
        ) {
            let to_words = |command: String| match parse(&command) {
                Parse::Plain { mut segments, .. } => segments.remove(0).words,
                Parse::Complex(_) => Vec::new(),
            };
            let git = to_words(format!("{before}git commit -m '{text}' --message='{text}'"));
            prop_assert_eq!(message_values(&git).iter().filter(|data| **data).count(), 2);
            let gh = to_words(format!("{before}gh pr create --title '{text}' --body='{text}'"));
            prop_assert_eq!(message_values(&gh).iter().filter(|data| **data).count(), 2);
            for wrapped in [
                format!("sh -c '$2' git -m '{text}'"),
                format!("env git commit -m '{text}'"),
                format!("echo gh --title '{text}'"),
                format!("git --title '{text}'"),
                format!("gh -m '{text}'"),
            ] {
                let words = to_words(wrapped);
                prop_assert!(message_values(&words).iter().all(|data| !data));
            }
        }

        #[test]
        fn a_body_line_that_only_starts_with_the_delimiter_ends_the_form_in_bash_so_it_is_refused(
            tail in prop_oneof![Just(")"), Just(" )"), Just("\t)"), Just("X"), Just("")],
            lines in proptest::collection::vec("[a-z ]{0,10}", 0..3),
        ) {
            let line = format!("EOF{tail}");
            let body = format!("{}\n{line}\nrest", lines.join("\n"));
            let text = format!("git commit -m \"$(cat <<'EOF'\n{body}\nEOF\n)\" ; git push origin main");
            prop_assert!(is_complex(&text));
        }

        #[test]
        fn a_word_that_starts_with_a_hash_is_complex_in_any_place(
            before in "[a-z ]{0,6}",
            after in "[a-z ']{0,6}",
        ) {
            let text = format!("echo {before} #{after}");
            prop_assert!(is_complex(&text));
            let quoted = format!("echo \"#{after}\"").replace('\'', "");
            prop_assert!(!is_complex(&quoted));
        }

        #[test]
        fn a_line_join_is_not_a_word_break_for_the_trigger_words(
            cut in 1_usize..3,
        ) {
            let word = "git";
            let (head, tail) = word.split_at(cut);
            let text = format!("{head}\\\n{tail} push");
            prop_assert_eq!(raw_words_unquoted(&text), ["git", "push"]);
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
        fn a_segment_reads_stdin_from_a_pipe_or_a_file_and_from_nothing_else(
            a in "[a-z]{1,5}[0-9]",
            file in "[a-z/]{1,6}",
        ) {
            let reads = |command: String| match parse(&command) {
                Parse::Plain { segments, .. } => segments.into_iter().map(|s| s.reads_stdin).collect(),
                Parse::Complex(_) => vec![],
            };
            prop_assert_eq!(reads(format!("{a} < {file}")), vec![true]);
            prop_assert_eq!(reads(format!("{a} <{file} > out")), vec![true]);
            prop_assert_eq!(reads(format!("{a} x | wc < {file}\nls")), vec![false, true, false]);
            prop_assert_eq!(reads(format!("{a} | ls")), vec![false, true]);
            prop_assert_eq!(reads(format!("{a} > {file} 2>&1 <&0")), vec![false]);
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
                Just("echo ok # note"), Just("# it's"), Just("ls <<< x"), Just("ls >| x"), Just("ls >"), Just("case x in esac"),
            ],
        ) {
            prop_assert!(is_complex(form));
        }

        #[test]
        fn these_forms_are_plain(
            form in prop_oneof![
                Just("git log @{u}.."), Just("git stash show stash@{0}"), Just("git diff HEAD~1"),
                Just("echo a\\\nb"), Just("ls 2>&1 | wc -l"), Just("echo a#b"),
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
