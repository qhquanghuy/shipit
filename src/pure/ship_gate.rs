// Which shell commands are the owner's to run. The answer is a list of reasons: empty means the
// command may go, and anything the reader cannot fully understand is a reason, never a pass.

use crate::pure::branch::parse_slice_branch;
use crate::pure::shell_words::{
    Parse, Segment, message_values, parse, program, raw_words, raw_words_unquoted,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) base: Vec<String>,
    pub(crate) heads: Vec<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) follow_tags: bool,
    pub(crate) rewires_push: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Answer {
    Pass,
    Ask(String),
    Deny(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Default,
    Plan,
    AcceptEdits,
    Auto,
    DontAsk,
    Bypass,
    Unknown,
}

impl Mode {
    pub(crate) fn parse(text: &str) -> Self {
        match text {
            "default" | "manual" => Self::Default,
            "plan" => Self::Plan,
            "acceptEdits" => Self::AcceptEdits,
            "auto" => Self::Auto,
            "dontAsk" => Self::DontAsk,
            "bypassPermissions" => Self::Bypass,
            _ => Self::Unknown,
        }
    }
}

const COMPLEX: &str = "cannot be read as plain commands by ShipIt; it may ship (write plain commands, and name branches in plain text)";
const HIDES: &str = "hides a git or gh command inside one word";
const FEEDS: &str = "feeds a shell or interpreter";
const NO_SLICE: &str =
    "pushes without naming a slice branch (write: git push -u origin slice/<goal>/<slice>)";
const TAG: &str = "pushes a tag";
const CHANGES_BRANCH: &str = "changes the branch or the config in the same command as a push";
const OPTION_BEFORE: &str = "uses a git option before push";
const FOLLOW_TAGS: &str = "pushes tags with the branch";
const MENTIONS: &str = "mentions push";
const ALIAS: &str = "sets a git alias or a shell command in git config";
const ENV: &str = "sets a GIT_ or GH_ variable";
const CONFIG: &str = "changes git config";
const REMOTE: &str = "changes the git remotes";
const REWIRED: &str = "pushes with a git config that can send it elsewhere (a remote's push or mirror setting, push.default, help.autocorrect)";
const PLUMBING: &str = "pushes with send-pack or http-push";
const CONFIRM: &str =
    "confirms a goal completion (shipit complete --confirm), which only the owner may do";
const MERGE: &str = "merges a pull request";
const RELEASE: &str = "creates or changes a release";
const REPOSITORY: &str = "changes the repository";
const API_WRITE: &str = "calls the GitHub API with a write method";
const GRAPHQL: &str = "runs a GitHub GraphQL query";

pub(crate) const INTERPRETERS: [&str; 11] = [
    "sh", "bash", "zsh", "dash", "ksh", "fish", "python", "python3", "perl", "ruby", "node",
];
const WRAPPERS: [&str; 6] = ["eval", "xargs", "exec", "su", "ssh", "watch"];
const CHANGERS: [&str; 11] = [
    "checkout",
    "switch",
    "merge",
    "rebase",
    "reset",
    "pull",
    "config",
    "branch",
    "tag",
    "update-ref",
    "symbolic-ref",
];
const FOLDER_WORD: &str = "cd";
const CONFIG_READS: [&str; 8] = [
    "--get",
    "--get-all",
    "--get-regexp",
    "--get-urlmatch",
    "--list",
    "-l",
    "get",
    "list",
];
const CONFIG_SCOPES: [&str; 8] = [
    "--global",
    "--local",
    "--system",
    "--worktree",
    "--show-origin",
    "--show-scope",
    "-z",
    "--null",
];
const CONFIG_WRITE_FLAGS: [&str; 7] = [
    "--add",
    "--unset",
    "--unset-all",
    "--replace-all",
    "--edit",
    "--rename-section",
    "--remove-section",
];
const API_FIELD_FLAGS: [&str; 5] = ["-f", "-F", "--field", "--raw-field", "--input"];

fn is_git_or_gh(word: &str) -> bool {
    word == "git" || word == "gh"
}

fn confirms_completion(text: &str) -> bool {
    let holds = |words: &[String]| {
        words.iter().any(|word| word == "complete") && words.iter().any(|word| word == "--confirm")
    };
    let raw: Vec<String> = raw_words(text).into_iter().map(str::to_owned).collect();
    holds(&raw) || holds(&raw_words_unquoted(text))
}

fn triggered_by(text: &str, parsed: &Parse) -> bool {
    let named = raw_words(text).into_iter().any(is_git_or_gh)
        || raw_words_unquoted(text)
            .iter()
            .any(|word| is_git_or_gh(word));
    let program_of_a_word = match parsed {
        Parse::Plain { segments, .. } => segments
            .iter()
            .flat_map(|segment| segment.words.iter())
            .any(|word| is_git_or_gh(program(word))),
        Parse::Complex => false,
    };
    named || program_of_a_word || confirms_completion(text)
}

// True when the text can need a judgement at all: the common command never does, and is passed
// without asking git anything.
pub(crate) fn triggered(command: &str) -> bool {
    triggered_by(command, &parse(command))
}

fn add(reasons: &mut Vec<String>, text: impl Into<String>) {
    let text = text.into();
    if !reasons.contains(&text) {
        reasons.push(text);
    }
}

enum Cause {
    Base(String),
    Tag,
    Other,
}

impl Cause {
    fn text(&self) -> String {
        match self {
            Self::Base(name) => format!("pushes to the base branch {name}"),
            Self::Tag => TAG.to_owned(),
            Self::Other => NO_SLICE.to_owned(),
        }
    }
}

// What a push target would reach, when it is not a plain slice branch.
fn target_cause(target: &str, facts: &Facts) -> Option<Cause> {
    let target = target.strip_prefix('+').unwrap_or(target);
    let (source, destination) = target.split_once(':').unwrap_or((target, target));
    let name = destination
        .strip_prefix("refs/heads/")
        .unwrap_or(destination);
    if facts.base.iter().any(|base| base == name) {
        return Some(Cause::Base(name.to_owned()));
    }
    let tag_like = source.starts_with("refs/tags/")
        || destination.starts_with("refs/tags/")
        || (facts.tags.iter().any(|tag| tag == name) && !facts.heads.iter().any(|h| h == name));
    tag_like.then_some(Cause::Tag)
}

fn cause_of(args: &[&str], facts: &Facts) -> Cause {
    args.iter()
        .filter(|arg| !arg.starts_with('-'))
        .skip(1)
        .find_map(|target| target_cause(target, facts))
        .unwrap_or(Cause::Other)
}

// `slice/<goal>/<slice>`, or `<ref>:refs/heads/slice/<goal>/<slice>`: never a base branch, a
// tag, a delete, a force or a pattern.
fn judge_target(target: &str, args: &[&str], facts: &Facts) -> Result<(), Cause> {
    let (source, name) = match target.split_once(':') {
        Some((source, destination)) => match destination.strip_prefix("refs/heads/") {
            Some(name) => (Some(source), name),
            None => return Err(cause_of(args, facts)),
        },
        None => (None, target),
    };
    if parse_slice_branch(name).is_none() {
        return Err(cause_of(args, facts));
    }
    if facts.base.iter().any(|base| base == name) {
        return Err(Cause::Base(name.to_owned()));
    }
    if let Some(source) = source {
        if source.is_empty()
            || source.contains('*')
            || source.starts_with('+')
            || source.starts_with("refs/tags/")
        {
            return Err(Cause::Other);
        }
    } else {
        if facts.tags.iter().any(|tag| tag == name) {
            return Err(Cause::Tag);
        }
        if !facts.heads.iter().any(|head| head == name) {
            return Err(Cause::Other);
        }
    }
    Ok(())
}

// Exactly `git push [-u|--set-upstream] origin <target>`: the arguments after `push`.
fn explicit_slice_push(args: &[String], facts: &Facts) -> Result<(), Cause> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let rest = match args.split_first() {
        Some((&("-u" | "--set-upstream"), tail)) => tail,
        Some(_) | None => args.as_slice(),
    };
    match rest {
        ["origin", target] => judge_target(target, &args, facts),
        _ => Err(cause_of(&args, facts)),
    }
}

// A GIT_ or GH_ variable can carry config (GIT_CONFIG_COUNT, GIT_CONFIG_PARAMETERS), another
// repository (GIT_DIR) or another host: in a text that names git or gh, any assignment asks.
fn sets_git_env(word: &str) -> bool {
    word.split_once('=')
        .is_some_and(|(name, _)| name.starts_with("GIT_") || name.starts_with("GH_"))
}

// The config as `git config --list` prints it: does it send a push somewhere the command does
// not say? A remote's push or mirror setting, a push.default that is not one branch to its own
// name, or help.autocorrect that runs a mistyped command (`git psuh` as push).
pub(crate) fn rewires_push(config: &str) -> bool {
    config.lines().any(|line| {
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let (key, value) = (key.to_lowercase(), value.trim().to_lowercase());
        let off = ["", "0", "never", "false", "no", "off"].contains(&value.as_str());
        (key.starts_with("remote.") && key.rsplit('.').next() == Some("push"))
            || (key.starts_with("remote.") && key.rsplit('.').next() == Some("mirror") && !off)
            || (key == "push.default"
                && !["simple", "current", "upstream"].contains(&value.as_str()))
            || (key == "help.autocorrect" && !off)
    })
}

// Only the plain reads of the config pass: `--get`, `--list` and their kind, after any scope.
fn changes_config(rest: &[String]) -> bool {
    let Some(at) = rest.iter().position(|word| word == "config") else {
        return false;
    };
    let after = rest.get(at + 1..).unwrap_or_default();
    let mut words = after
        .iter()
        .skip_while(|word| CONFIG_SCOPES.contains(&word.as_str()));
    let reads = words
        .next()
        .is_some_and(|first| CONFIG_READS.contains(&first.as_str()));
    let writes = after
        .iter()
        .any(|word| CONFIG_WRITE_FLAGS.contains(&word.as_str()));
    !reads || writes
}

// Only a listing of the remotes passes: bare, `-v`, `show` and `get-url`.
fn changes_remote(rest: &[String]) -> bool {
    let Some(at) = rest.iter().position(|word| word == "remote") else {
        return false;
    };
    let mut after = rest
        .get(at + 1..)
        .unwrap_or_default()
        .iter()
        .skip_while(|word| *word == "-v" || *word == "--verbose");
    after
        .next()
        .is_some_and(|first| first != "show" && first != "get-url")
}

fn names_alias(word: &str) -> bool {
    let lower = word.to_lowercase();
    let body = lower.strip_prefix("--config-env=").unwrap_or(&lower);
    let (key, value) = body.split_once('=').unwrap_or((body, ""));
    key.starts_with("alias.")
        || key == "help.autocorrect"
        || word.starts_with('!')
        || value.starts_with('!')
}

fn sets_alias(rest: &[String]) -> bool {
    let from = rest
        .iter()
        .position(|word| word == "config" || word == "-c" || word.starts_with("--config-env"));
    from.is_some_and(|at| rest.iter().skip(at).any(|word| names_alias(word)))
}

// A command that moves the branch, the config or the folder before a push can change what the
// push reaches, so a push in it is never read as the plain one.
fn changes_context(segments: &[Segment]) -> bool {
    segments.iter().any(|segment| {
        let changes_git = segment.words.iter().enumerate().any(|(at, word)| {
            program(word) == "git"
                && segment
                    .words
                    .iter()
                    .skip(at + 1)
                    .any(|later| CHANGERS.contains(&later.as_str()))
        });
        changes_git || segment.words.iter().any(|word| word == FOLDER_WORD)
    })
}

struct Judge<'a> {
    facts: &'a dyn Fn(Option<&str>) -> Facts,
    changes_context: bool,
}

impl Judge<'_> {
    fn git_word(&self, segment: &Segment, data: &[bool], at: usize, reasons: &mut Vec<String>) {
        let rest = segment.words.get(at + 1..).unwrap_or_default();
        let rest_data = data.get(at + 1..).unwrap_or_default();
        if sets_alias(rest) {
            add(reasons, ALIAS);
        }
        if changes_config(rest) {
            add(reasons, CONFIG);
        }
        if changes_remote(rest) {
            add(reasons, REMOTE);
        }
        let plumbing = rest.iter().enumerate().any(|(index, word)| {
            (word == "send-pack" || word == "http-push") && rest_data.get(index) != Some(&true)
        });
        if plumbing {
            add(reasons, PLUMBING);
        }
        for (index, word) in rest.iter().enumerate() {
            if word != "push" || rest_data.get(index) == Some(&true) {
                continue;
            }
            let prior = index
                .checked_sub(1)
                .and_then(|prior| rest.get(prior))
                .map(String::as_str);
            if prior == Some("stash") {
                continue;
            }
            if index > 0 {
                let option_first = rest.iter().take(index).any(|word| word.starts_with('-'));
                add(
                    reasons,
                    if option_first {
                        OPTION_BEFORE
                    } else {
                        MENTIONS
                    },
                );
                continue;
            }
            let facts = (self.facts)(segment.dir.as_deref());
            match explicit_slice_push(rest.get(1..).unwrap_or_default(), &facts) {
                Ok(()) => {
                    if facts.follow_tags {
                        add(reasons, FOLLOW_TAGS);
                    }
                    if facts.rewires_push {
                        add(reasons, REWIRED);
                    }
                    if self.changes_context {
                        add(reasons, CHANGES_BRANCH);
                    }
                }
                Err(cause) => add(reasons, cause.text()),
            }
        }
    }
}

fn api_writes(rest: &[String]) -> bool {
    let mut methods: Vec<&str> = Vec::new();
    let mut fields = false;
    let mut words = rest.iter().map(String::as_str);
    while let Some(word) = words.next() {
        if word == "-X" || word == "--method" {
            methods.push(words.next().unwrap_or(""));
        } else if let Some(method) = word.strip_prefix("--method=") {
            methods.push(method);
        } else if let Some(method) = word.strip_prefix("-X") {
            methods.push(method);
        } else if API_FIELD_FLAGS.iter().any(|flag| word.starts_with(flag)) {
            fields = true;
        }
    }
    fields || methods.iter().any(|method| *method != "GET")
}

// The words of `gh` that are not options, after `-R <repo>` and `--repo <repo>` in any place.
fn gh_words(rest: &[String]) -> Vec<&str> {
    let mut found = Vec::new();
    let mut skip = false;
    for word in rest {
        if skip {
            skip = false;
        } else if word == "-R" || word == "--repo" {
            skip = true;
        } else if !word.starts_with('-') {
            found.push(word.as_str());
        }
    }
    found
}

fn gh_word(rest: &[String], reasons: &mut Vec<String>) {
    let words = gh_words(rest);
    match (words.first().copied(), words.get(1).copied()) {
        (Some("pr"), Some("merge")) => add(reasons, MERGE),
        (
            Some("release"),
            Some("create" | "new" | "edit" | "upload" | "delete" | "delete-asset"),
        ) => add(reasons, RELEASE),
        (Some("repo"), Some("sync" | "edit" | "rename" | "delete" | "archive")) => {
            add(reasons, REPOSITORY);
        }
        (Some("api"), _) => {
            if rest.iter().any(|word| word == "graphql") {
                add(reasons, GRAPHQL);
            } else if api_writes(rest) {
                add(reasons, API_WRITE);
            }
        }
        (Some(_) | None, _) => {}
    }
}

// A word runs a program by its name or by a path to it; a branch such as `slice/v1/watch` is neither.
pub(crate) fn runs_as(word: &str, names: &[&str]) -> bool {
    let base = if word.starts_with('/') || word.starts_with("./") || word.starts_with("../") {
        program(word)
    } else {
        word
    };
    names.contains(&base)
}

// A word that holds a git push or a gh write: by its raw words, and by the rules of gh itself.
fn hides_a_command(word: &str) -> bool {
    let words = raw_words(word);
    let has = |name: &str| words.contains(&name);
    let git = has("git") && (has("push") || has("send-pack") || has("http-push"));
    let gh_loose = has("gh")
        && (has("merge")
            || has("release")
            || has("sync")
            || has("api")
            || (has("repo")
                && ["edit", "rename", "delete", "archive"]
                    .iter()
                    .any(|name| has(name))));
    let gh_rules = words
        .iter()
        .position(|name| *name == "gh")
        .is_some_and(|at| {
            let rest: Vec<String> = words
                .iter()
                .skip(at + 1)
                .map(|name| (*name).to_owned())
                .collect();
            let mut found = Vec::new();
            gh_word(&rest, &mut found);
            !found.is_empty()
        });
    git || gh_loose || gh_rules
}

// A flag that hands the interpreter its program as a string: `-c`, `-e`, `-lc`, `-euc`.
fn is_string_flag(word: &str) -> bool {
    word.strip_prefix('-')
        .is_some_and(|rest| !rest.starts_with('-') && rest.chars().any(|c| c == 'c' || c == 'e'))
}

// For each interpreter in a segment that is given a string to run, the string (None when the
// flag is the last word).
pub(crate) fn interpreter_strings(words: &[String]) -> Vec<Option<&str>> {
    let mut found = Vec::new();
    for (at, word) in words.iter().enumerate() {
        if !runs_as(word, &INTERPRETERS) {
            continue;
        }
        let later = words.get(at + 1..).unwrap_or_default();
        for (offset, flag) in later.iter().enumerate() {
            if is_string_flag(flag) {
                found.push(later.get(offset + 1).map(String::as_str));
            }
        }
    }
    found
}

fn plain_reasons(
    segments: &[Segment],
    heredocs: &[String],
    facts: &dyn Fn(Option<&str>) -> Facts,
    reasons: &mut Vec<String>,
) {
    let judge = Judge {
        facts,
        changes_context: changes_context(segments),
    };
    for segment in segments {
        let data = message_values(&segment.words);
        for (at, word) in segment.words.iter().enumerate() {
            if data.get(at) == Some(&true) {
                continue;
            }
            if hides_a_command(word) || runs_as(word, &WRAPPERS) {
                add(reasons, HIDES);
            }
            if sets_git_env(word) {
                add(reasons, ENV);
            }
            match program(word) {
                "git" => judge.git_word(segment, &data, at, reasons),
                "gh" => gh_word(segment.words.get(at + 1..).unwrap_or_default(), reasons),
                _ => {}
            }
        }
    }
    let piped_into_interpreter = segments.iter().any(|segment| {
        segment.reads_pipe
            && segment
                .words
                .iter()
                .any(|word| runs_as(word, &INTERPRETERS))
    });
    let runs_a_string = segments
        .iter()
        .any(|segment| !interpreter_strings(&segment.words).is_empty());
    if !heredocs.is_empty() || piped_into_interpreter || runs_a_string {
        add(reasons, FEEDS);
    }
}

// The folders a git command names for itself, as the segment's folder and the text: `-C <dir>`,
// `--git-dir`, `--work-tree`, and the GIT_DIR and GIT_WORK_TREE variables.
pub(crate) fn named_folders(command: &str) -> Vec<(Option<String>, String)> {
    let Parse::Plain { segments, .. } = parse(command) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for segment in &segments {
        let words = &segment.words;
        for (at, word) in words.iter().enumerate() {
            let glued = ["GIT_DIR=", "GIT_WORK_TREE=", "--git-dir=", "--work-tree="]
                .iter()
                .find_map(|prefix| word.strip_prefix(prefix));
            let next = ["-C", "--git-dir", "--work-tree"]
                .contains(&word.as_str())
                .then(|| words.get(at + 1).map(String::as_str))
                .flatten();
            if let Some(folder) = glued.or(next) {
                let folder = folder.strip_suffix("/.git").unwrap_or(folder);
                found.push((segment.dir.clone(), folder.to_owned()));
            }
        }
    }
    found
}

pub(crate) fn ship_reasons(command: &str, facts: &dyn Fn(Option<&str>) -> Facts) -> Vec<String> {
    let parsed = parse(command);
    let mut reasons = Vec::new();
    if !triggered_by(command, &parsed) {
        return reasons;
    }
    if confirms_completion(command) {
        add(&mut reasons, CONFIRM);
    }
    match &parsed {
        Parse::Complex => add(&mut reasons, COMPLEX),
        Parse::Plain { segments, heredocs } => {
            plain_reasons(segments, heredocs, facts, &mut reasons);
        }
    }
    reasons
}

pub(crate) fn ship_answer(mode: Mode, reasons: &[String]) -> Answer {
    if reasons.is_empty() {
        return Answer::Pass;
    }
    let reasons = reasons.join("; ");
    match mode {
        Mode::Bypass | Mode::Unknown => Answer::Deny(format!(
            "ShipIt: only the owner ships, and this permission mode cannot ask the owner. This command {reasons}. Ask the owner to run it."
        )),
        Mode::Default | Mode::Plan | Mode::AcceptEdits | Mode::Auto | Mode::DontAsk => {
            Answer::Ask(format!(
                "ShipIt: only the owner ships. This command {reasons}. Answer yes to let the agent run it."
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn facts() -> Facts {
        Facts {
            base: vec!["main".to_owned(), "master".to_owned()],
            heads: ["main", "master", "slice/v1/x", "slice/x"]
                .map(str::to_owned)
                .to_vec(),
            tags: vec!["v1.0".to_owned()],
            follow_tags: false,
            rewires_push: false,
        }
    }

    fn reasons(command: &str) -> Vec<String> {
        ship_reasons(command, &|_dir| facts())
    }

    fn asks(command: &str) -> bool {
        !reasons(command).is_empty()
    }

    const PLAIN_WORDS: [&str; 7] = ["ls", "x", "-la", "foo", "commit", "status", "--no-verify"];
    const BRANCHES: [&str; 7] = [
        "slice/v1/x",
        "main",
        "master",
        "v1.0",
        "slice/x",
        "slice/a/b/c",
        "slice/v1/y",
    ];
    const SHELLS: [&str; 3] = ["bash", "sh", "python3"];

    const GOOD_C: &str = "\"$(cat <<'EOF'\nfix git push\nEOF\n)\"";
    // Bash ends this body at `EOF)`, and runs what follows as commands.
    const BAD_C: &str = "\"$(cat <<'EOF'\nEOF)\" ; git push origin main ; echo \"\nEOF\n)\"";
    const QUOTED: [&str; 4] = [
        "'git push origin main'",
        "\"git push origin main\"",
        GOOD_C,
        BAD_C,
    ];

    // Tokens of the property that a command never passes outside the allowed forms. A lone
    // quote and a redirection are not tokens: they change which words there are, which the
    // oracle below cannot follow without being a second parser. A quoted text is one token.
    fn token() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            8 => proptest::sample::select(&PLAIN_WORDS[..]),
            4 => Just("git"),
            4 => Just("push"),
            1 => Just("\"g\"it"),
            1 => Just("'git'"),
            1 => Just("gh"),
            1 => Just("pr"),
            1 => Just("merge"),
            2 => proptest::sample::select(&["origin", "other"][..]),
            3 => proptest::sample::select(&BRANCHES[..]),
            3 => proptest::sample::select(&["-u", "--set-upstream"][..]),
            2 => proptest::sample::select(&QUOTED[..]),
            2 => proptest::sample::select(&[
                "-m", "-c", "-lc", "#", "GIT_CONFIG_COUNT=1", "GH_TOKEN=x", "A=1",
                "symbolic-ref", "remote",
            ][..]),
            3 => proptest::sample::select(&[
                "HEAD", "@", ":x", "+x", "--force", "--tags", "--all", "--delete", "-C", "d",
                "--no-pager", "GIT_DIR=x", "checkout", "switch", "config", "cd", "pushd", "popd",
                "eval", "xargs", "bash", "sh", "python3",
            ][..]),
            2 => proptest::sample::select(&[
                "$x", "$(echo", "`x`", "(", ")", "{", "}", "!", "~", "if", "then", "fi", "for",
                "done", "\\", "<<'EOF'", "<<EOF", "{a,b}",
            ][..]),
        ]
    }

    fn separator() -> impl Strategy<Value = &'static str> {
        proptest::sample::select(&[";", "&&", "||", "|", "&", "\n"][..])
    }

    const DANGER: [&str; 22] = [
        "$x",
        "$(echo",
        "`x`",
        "(",
        ")",
        "{",
        "}",
        "!",
        "~",
        "\\",
        "<<'EOF'",
        "<<EOF",
        "{a,b}",
        "eval",
        "pushd",
        "xargs",
        "popd",
        "#",
        "GIT_DIR=x",
        "GIT_CONFIG_COUNT=1",
        "GH_TOKEN=x",
        BAD_C,
    ];
    const KEYWORDS: [&str; 5] = ["if", "then", "fi", "for", "done"];

    fn canonical(token: &'static str) -> &'static str {
        match token {
            "\"g\"it" | "'git'" => "git",
            other => other,
        }
    }

    // A segment as the oracle reads it: its canonical words, and which of them are the value of
    // `-m` in a segment whose program is git, which is text whatever it says.
    fn view(segment: &[&'static str]) -> (Vec<&'static str>, Vec<bool>) {
        let words: Vec<&'static str> = segment.iter().map(|t| canonical(t)).collect();
        let owner = words.iter().find(|t| !t.contains('=')).copied();
        let data = (0..words.len())
            .map(|at| owner == Some("git") && at > 0 && words.get(at - 1) == Some(&"-m"))
            .collect();
        (words, data)
    }

    // Quoted text is data only as such a message; an interpreter given a string runs it.
    fn reads_ok(segment: &[&'static str]) -> bool {
        let (words, data) = view(segment);
        words.iter().enumerate().all(|(at, word)| {
            let string_flag = |t: &&str| *t == "-c" || *t == "-lc";
            let hides = QUOTED.contains(word) && data.get(at) != Some(&true);
            let runs_string = SHELLS.contains(word) && words.iter().skip(at + 1).any(string_flag);
            !hides && !runs_string
        })
    }

    fn merges_a_pull_request(segment: &[&'static str]) -> bool {
        let (words, _data) = view(segment);
        words.iter().enumerate().any(|(at, word)| {
            let later: Vec<&&str> = words
                .iter()
                .skip(at + 1)
                .filter(|t| !t.starts_with('-'))
                .take(2)
                .collect();
            *word == "gh" && later == [&"pr", &"merge"]
        })
    }

    // The independent reading of a generated command, token by token: it is safe to pass only
    // when no token needs a reading the gate lacks, no pipe feeds a shell, no `gh pr merge`
    // stands in it, and every git push is exactly the explicit slice push with nothing that
    // could move its branch, config or folder.
    fn oracle(segments: &[Vec<&'static str>], separators: &[&str]) -> bool {
        let views: Vec<(Vec<&str>, Vec<bool>)> = segments.iter().map(|s| view(s)).collect();
        let tokens: Vec<&str> = views
            .iter()
            .flat_map(|(words, data)| {
                words
                    .iter()
                    .zip(data)
                    .filter(|(_, is_data)| !**is_data)
                    .map(|(t, _)| *t)
            })
            .collect();
        let keyword_first = views
            .iter()
            .any(|(words, _)| words.first().is_some_and(|t| KEYWORDS.contains(t)));
        let piped_to_shell = separators.iter().enumerate().any(|(at, sep)| {
            *sep == "|"
                && segments
                    .get(at + 1)
                    .is_some_and(|next| next.iter().any(|t| SHELLS.contains(t)))
        });
        if keyword_first
            || piped_to_shell
            || tokens.iter().any(|token| DANGER.contains(token))
            || !segments.iter().all(|s| reads_ok(s))
            || segments.iter().any(|s| merges_a_pull_request(s))
        {
            return false;
        }
        let folder_change = tokens.contains(&FOLDER_WORD);
        let context_change = views.iter().any(|(words, _)| {
            words.iter().enumerate().any(|(at, t)| {
                *t == "git"
                    && words
                        .iter()
                        .skip(at + 1)
                        .any(|later| CHANGERS.contains(later))
            })
        });
        views.iter().all(|(words, data)| {
            words.iter().enumerate().all(|(at, token)| {
                let rest: Vec<&str> = words
                    .iter()
                    .zip(data)
                    .skip(at + 1)
                    .filter(|(_, is_data)| !**is_data)
                    .map(|(t, _)| *t)
                    .collect();
                if *token != "git" || !rest.contains(&"push") {
                    return true;
                }
                let target = match rest.as_slice() {
                    ["push", "-u" | "--set-upstream", "origin", target]
                    | ["push", "origin", target] => *target,
                    _ => return false,
                };
                target == "slice/v1/x" && !folder_change && !context_change
            })
        })
    }

    fn join(segments: &[Vec<&str>], separators: &[&str]) -> String {
        let mut text = String::new();
        for (at, segment) in segments.iter().enumerate() {
            if let Some(sep) = at.checked_sub(1).and_then(|prior| separators.get(prior)) {
                text.push(' ');
                text.push_str(sep);
                text.push(' ');
            }
            text.push_str(&segment.join(" "));
        }
        text
    }

    fn holds_git_and_push(text: &str) -> bool {
        let squeezed: String = text.chars().filter(|c| !matches!(c, '\'' | '"')).collect();
        let words: Vec<&str> = squeezed
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .collect();
        words.contains(&"git") && words.contains(&"push")
    }

    fn mode() -> impl Strategy<Value = Mode> {
        prop_oneof![
            Just(Mode::Default),
            Just(Mode::Plan),
            Just(Mode::AcceptEdits),
            Just(Mode::Auto),
            Just(Mode::DontAsk),
            Just(Mode::Bypass),
            Just(Mode::Unknown),
        ]
    }

    fn denies_in_bypass(command: &str) -> bool {
        matches!(
            ship_answer(Mode::Bypass, &reasons(command)),
            Answer::Deny(_)
        )
    }

    fn name() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9-]{0,8}".prop_filter(
            "not roadmap, and no word the gate reads as a program",
            |name| name != "roadmap" && !WRAPPERS.contains(&name.as_str()),
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            max_global_rejects: 100_000,
            ..ProptestConfig::default()
        })]

        #[test]
        fn git_push_never_passes_outside_allowed_forms(
            parts in proptest::collection::vec(
                proptest::collection::vec(token(), 1..7),
                1..4,
            ),
            seps in proptest::collection::vec(separator(), 3),
        ) {
            let text = join(&parts, &seps);
            prop_assume!(holds_git_and_push(&text));
            if reasons(&text).is_empty() {
                prop_assert!(oracle(&parts, &seps), "{:?}", text);
            }
        }

        #[test]
        fn text_without_git_or_gh_passes(text in "[a-z0-9 _/.;|&$()'\"~{}<>`\\\\-]{0,40}") {
            // The words as the text has them, and with every quote taken out first.
            let squeezed: String = text.chars().filter(|c| !matches!(c, '\'' | '"' | '\\')).collect();
            let words_of = |text: &str| -> Vec<String> {
                text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                    .map(str::to_owned)
                    .collect()
            };
            for words in [words_of(&text), words_of(&squeezed)] {
                let has = |word: &str| words.iter().any(|have| have == word);
                prop_assume!(!has("git") && !has("gh"));
                prop_assume!(!(has("complete") && has("--confirm")));
            }
            prop_assert!(reasons(&text).is_empty());
        }

        #[test]
        fn allowed_forms_pass(
            words in proptest::collection::vec(proptest::sample::select(&PLAIN_WORDS[..]), 1..5),
            body in proptest::collection::vec("[ -~]{1,20}", 1..4),
            text in "[a-z ]{0,10}git push origin main[a-z ]{0,10}",
            quote in prop_oneof![Just("'"), Just("\"")],
        ) {
            prop_assume!(body.iter().all(|line| line != "EOF"));
            let heredoc = format!("\"$(cat <<'EOF'\n{}\nEOF\n)\"", body.join("\n"));
            let commands = [
                format!("git {}", words.join(" ")),
                format!("cd d && git {} 2>&1 | wc -l > out", words.join(" ")),
                format!("git commit -m {quote}{text}{quote}"),
                format!("git commit -m {heredoc}"),
                format!("git commit --message {heredoc} && ls"),
                format!("gh pr create --title t --body {heredoc}"),
                format!("gh pr create --title {quote}{text}{quote} --body {quote}x{quote}"),
                "git stash push".to_owned(),
                "git push -u origin slice/v1/x".to_owned(),
                "git push --set-upstream origin slice/v1/x".to_owned(),
                "git push origin HEAD:refs/heads/slice/v1/x".to_owned(),
                "git status && git push -u origin slice/v1/x".to_owned(),
                "nix develop -c git push -u origin slice/v1/x".to_owned(),
            ];
            for command in commands {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
        }

        #[test]
        fn read_only_git_forms_pass(n in 1_u32..50, sha in "[0-9a-f]{7,12}") {
            let commands = [
                format!("git diff HEAD~{n}"),
                format!("git log HEAD~{n}.."),
                "git log @{u}..".to_owned(),
                "git rev-parse --abbrev-ref @{u}".to_owned(),
                format!("git stash show stash@{{{n}}}"),
                format!("git log --oneline -{n}"),
                "git status".to_owned(),
                format!("git show {sha}"),
                format!("gh pr view {n}"),
                "gh release list".to_owned(),
                "gh pr create --title t".to_owned(),
                "gh pr checks".to_owned(),
                "gh api repos/o/r/pulls".to_owned(),
                "gh api -X GET repos/o/r".to_owned(),
                "gh api -XGET repos/o/r".to_owned(),
                "gh api --method GET repos/o/r".to_owned(),
                "gh api --method=GET repos/o/r/releases/latest".to_owned(),
            ];
            for command in commands {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
            for command in ["git log {a,b}", "git log {1..3}", "git add ~/x", "git { ls; }"] {
                prop_assert_eq!(reasons(command), vec![COMPLEX.to_owned()], "{}", command);
            }
        }

        #[test]
        fn slice_push_forms_pass(goal in name(), slice in name(), up in prop_oneof![
            Just(""), Just("-u "), Just("--set-upstream "),
        ]) {
            let branch = format!("slice/{goal}/{slice}");
            let mut known = facts();
            known.heads.push(branch.clone());
            let named_like_a_program = format!("slice/{goal}/watch");
            known.heads.push(named_like_a_program.clone());
            let judged = |command: &str| ship_reasons(command, &|_dir| known.clone());
            for command in [
                format!("git push {up}origin {branch}"),
                format!("git push {up}origin HEAD:refs/heads/{branch}"),
                format!("git push {up}origin some-ref:refs/heads/{branch}"),
                format!("git push {up}origin {named_like_a_program}"),
            ] {
                prop_assert_eq!(judged(&command), Vec::<String>::new(), "{}", command);
            }
        }

        #[test]
        fn a_slice_branch_that_is_a_tag_or_a_base_asks(goal in name(), slice in name()) {
            let branch = format!("slice/{goal}/{slice}");
            let mut as_tag = facts();
            as_tag.heads.push(branch.clone());
            as_tag.tags.push(branch.clone());
            let mut as_base = facts();
            as_base.heads.push(branch.clone());
            as_base.base.push(branch.clone());
            let command = format!("git push -u origin {branch}");
            prop_assert_eq!(ship_reasons(&command, &|_dir| as_tag.clone()), vec![TAG.to_owned()]);
            prop_assert_eq!(
                ship_reasons(&command, &|_dir| as_base.clone()),
                vec![format!("pushes to the base branch {branch}")]
            );
        }

        #[test]
        fn push_without_named_slice_asks(remote in "[a-z]{1,6}", n in 1_u32..5) {
            let cases = [
                "git push".to_owned(),
                "git push origin HEAD".to_owned(),
                "git push origin @".to_owned(),
                "git push --delete origin slice/v1/x".to_owned(),
                "git push -d origin slice/v1/x".to_owned(),
                "git push origin :slice/v1/x".to_owned(),
                "git push origin +slice/v1/x".to_owned(),
                "git push --force origin slice/v1/x".to_owned(),
                "git push --all origin".to_owned(),
                "git push --mirror origin".to_owned(),
                "git push --tags origin".to_owned(),
                "git push -o ci.skip origin slice/v1/x".to_owned(),
                format!("git push {remote} slice/v1/x"),
                format!("git push origin feature/{remote}"),
                "git push origin slice/x".to_owned(),
                "git push origin slice/a/b/c".to_owned(),
                "git push origin slice/v1/y".to_owned(),
                "git push origin slice/v1/x slice/v1/x".to_owned(),
                "git push origin v1.0:refs/heads/slice/v1/x:y".to_owned(),
                "git push origin x:refs/tags/slice/v1/z".to_owned(),
                "git push origin refs/tags/v1.0:refs/heads/slice/v1/x*".to_owned(),
                "git push origin *:refs/heads/slice/v1/z".to_owned(),
                "git push origin refs/tags/v1.0:refs/heads/slice/v1/z".to_owned(),
                format!("git push origin {n}:refs/heads/main"),
            ];
            for command in cases {
                prop_assert!(asks(&command), "{}", command);
                prop_assert!(denies_in_bypass(&command), "{}", command);
            }
        }

        #[test]
        fn a_form_b_push_asks_when_anything_could_move_its_branch(
            changer in proptest::sample::select(&[
                "checkout main", "switch main", "merge x", "rebase x", "reset --hard", "pull",
                "config x y", "branch -f slice/v1/x main", "tag t", "update-ref x y",
                "symbolic-ref HEAD refs/heads/main",
            ][..]),
        ) {
            let push = "git push -u origin slice/v1/x";
            for command in [
                format!("git {changer} && {push}"),
                format!("{push} && git {changer}"),
                format!("cd d && {push}"),
                format!("pushd d && {push}"),
                format!("{push}; popd"),
                format!("GIT_DIR=x {push}"),
                "git -C d push -u origin slice/v1/x".to_owned(),
                "git --no-pager push -u origin slice/v1/x".to_owned(),
            ] {
                prop_assert!(asks(&command), "{}", command);
            }
            let mut following = facts();
            following.follow_tags = true;
            prop_assert_eq!(
                ship_reasons(push, &|_dir| following.clone()),
                vec![FOLLOW_TAGS.to_owned()]
            );
            let mut rewired = facts();
            rewired.rewires_push = true;
            prop_assert_eq!(
                ship_reasons(push, &|_dir| rewired.clone()),
                vec![REWIRED.to_owned()]
            );
        }

        #[test]
        fn complete_confirm_asks(
            program in prop_oneof![
                Just("shipit"), Just("bin/shipit"), Just("/x/shipit"), Just("\"shipit\""),
                Just("nix run . --"), Just("cargo run --"),
            ],
            goal in name(),
            position in 0_usize..3,
            quote in prop_oneof![Just(""), Just("\""), Just("'")],
        ) {
            let flag = format!("{quote}--confirm{quote}");
            let tail = match position {
                0 => format!("{flag} {goal}"),
                1 => format!("{goal} {flag}"),
                _ => format!("{goal} --other {flag}"),
            };
            let command = format!("{program} complete {tail}");
            prop_assert_eq!(reasons(&command), vec![CONFIRM.to_owned()], "{}", command);
            prop_assert!(denies_in_bypass(&command));
        }

        #[test]
        fn complete_without_confirm_passes(goal in name()) {
            for command in [format!("shipit complete {goal}"), format!("cd d && shipit complete {goal}")] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new());
            }
        }

        #[test]
        fn gh_writes_ask(
            path in "repos/[a-z]{1,5}/[a-z]{1,5}/pulls/[0-9]{1,3}",
            repo in prop_oneof![Just(""), Just("-R o/r "), Just("--repo o/r "), Just("--repo=o/r ")],
            write in proptest::sample::select(&[
                "-X PUT", "-XPUT", "--method PUT", "--method=PUT", "-X POST", "-X DELETE",
                "-X PATCH", "-f a=b", "-F a=b", "-fa=b", "--field a=b", "--raw-field a=b",
                "--input f", "-X get", "-X GET -f a=b", "--method GET --input f",
            ][..]),
        ) {
            let command = format!("gh api {repo}{write} {path}");
            prop_assert!(asks(&command), "{}", command);
            let graphql = format!("gh api {repo}graphql -F query=@q.graphql");
            prop_assert!(asks(&graphql), "{}", graphql);
            prop_assert!(asks("gh api -X GET graphql"));
            { let text = format!("gh api {repo}{path}"); prop_assert!(!asks(&text)); }
            { let text = format!("gh api {repo}-X GET {path}"); prop_assert!(!asks(&text)); }
        }

        #[test]
        fn only_plain_reads_of_the_config_and_the_remotes_pass(
            key in "[a-z]{1,6}\\.[a-z]{1,6}",
            value in prop_oneof![Just("list".to_owned()), Just("get".to_owned()), "[a-z/:.-]{1,10}"],
            scope in prop_oneof![Just(""), Just("--global "), Just("--local --show-origin ")],
            name in "[a-z]{1,6}[0-9]",
        ) {
            for command in [
                format!("git config {scope}--get {key}"),
                format!("git config {scope}--get-regexp {key}"),
                format!("git config {scope}--list"),
                format!("git config {scope}-l"),
                format!("git config {scope}get {key}"),
                "git remote".to_owned(),
                "git remote -v".to_owned(),
                format!("git remote show {name}"),
                format!("git remote get-url {name}"),
            ] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
            for command in [
                format!("git config {scope}{key} {value}"),
                format!("git config {scope}--add {key} {value}"),
                format!("git config {scope}--unset {key}"),
                format!("git config {scope}--get {key} --unset {key}"),
                format!("git config {scope}--edit"),
                format!("git config {scope}set {key} {value}"),
                format!("git config {scope}{key}"),
                format!("git remote add {name} {value}"),
                format!("git remote set-url {name} {value}"),
                format!("git remote remove {name}"),
                format!("git remote rename {name} {value}"),
            ] {
                prop_assert!(asks(&command), "{}", command);
                prop_assert!(denies_in_bypass(&command), "{}", command);
            }
        }

        #[test]
        fn a_git_or_gh_variable_asks_wherever_it_is_set(
            name in prop_oneof![
                Just("GIT_DIR"), Just("GIT_WORK_TREE"), Just("GIT_CONFIG_COUNT"),
                Just("GIT_CONFIG_KEY_0"), Just("GIT_CONFIG_VALUE_0"), Just("GIT_CONFIG_PARAMETERS"),
                Just("GIT_EXEC_PATH"), Just("GIT_EDITOR"), Just("GH_TOKEN"), Just("GH_HOST"),
            ],
            program in prop_oneof![Just("git status"), Just("gh pr view 1")],
        ) {
            for command in [
                format!("{name}=x {program}"),
                format!("A=1 {name}=x {program}"),
                format!("export {name}=x; {program}"),
                format!("env {name}=x {program}"),
                format!("{program}; {name}=x true"),
            ] {
                prop_assert_eq!(reasons(&command), vec![ENV.to_owned()], "{}", command);
            }
            for command in [format!("A=1 {program}"), format!("GITHUB={name} {program}"), format!("{program} {name}")] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
        }

        #[test]
        fn a_string_given_to_an_interpreter_is_a_program_nobody_read(
            interpreter in proptest::sample::select(&INTERPRETERS[..]),
            path in prop_oneof![Just(""), Just("/bin/"), Just("./")],
            flag in prop_oneof![Just("-c"), Just("-e"), Just("-lc"), Just("-euc"), Just("-ec")],
            text in "[a-z]{1,8}[0-9]",
        ) {
            let with_flag = format!("git status; {path}{interpreter} {flag} '{text}'");
            prop_assert_eq!(reasons(&with_flag), vec![FEEDS.to_owned()], "{}", with_flag);
            let wrapped = format!("git status; env X=1 {path}{interpreter} {flag} \"{text}\"");
            prop_assert!(asks(&wrapped), "{}", wrapped);
            for command in [
                format!("git status; {path}{interpreter} script.sh"),
                format!("git status; {path}{interpreter} --version"),
                format!("git status; echo {flag}"),
            ] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
        }

        #[test]
        fn a_folder_the_command_names_for_git_is_named_with_its_segment_folder(
            dir in "[a-z]{1,6}",
            folder in "[a-z]{1,6}(/[a-z]{1,6}){0,2}",
        ) {
            let from = |command: String| named_folders(&command);
            prop_assert_eq!(from(format!("git -C {folder} status")), vec![(None, folder.clone())]);
            prop_assert_eq!(from(format!("git --git-dir={folder}/.git status")), vec![(None, folder.clone())]);
            prop_assert_eq!(from(format!("git --work-tree {folder} status")), vec![(None, folder.clone())]);
            prop_assert_eq!(from(format!("GIT_DIR={folder}/.git git status")), vec![(None, folder.clone())]);
            prop_assert_eq!(from(format!("GIT_WORK_TREE={folder} git status")), vec![(None, folder.clone())]);
            prop_assert_eq!(
                from(format!("cd {dir} && git -C {folder} status")),
                vec![(Some(dir.clone()), folder.clone())]
            );
            prop_assert_eq!(from(format!("git status {folder}")), Vec::<(Option<String>, String)>::new());
            prop_assert_eq!(from(format!("git -C $x status {folder}")), Vec::<(Option<String>, String)>::new());
        }

        #[test]
        fn a_push_setting_in_the_config_rewires_a_push(
            remote in "[a-z]{1,6}",
            value in "[a-z/:*+]{1,12}",
            noise in proptest::collection::vec("[a-z]{1,5}\\.[a-z]{1,5}=[a-z0-9]{1,5}", 0..4),
        ) {
            prop_assume!(noise.iter().all(|line| !line.starts_with("remote.") && !line.starts_with("push.") && !line.starts_with("help.")));
            let lines = noise.join("\n");
            prop_assert!(!rewires_push(&lines));
            for setting in [
                format!("remote.{remote}.push={value}"),
                format!("remote.{remote}.mirror=true"),
                format!("remote.{remote}.mirror=yes"),
                "push.default=matching".to_owned(),
                "push.default=nothing".to_owned(),
                "help.autocorrect=immediate".to_owned(),
                "help.autocorrect=prompt".to_owned(),
                "help.autocorrect=5".to_owned(),
                "Remote.Origin.Push=x".to_owned(),
            ] {
                prop_assert!(rewires_push(&format!("{lines}\n{setting}\n{lines}")), "{}", setting);
            }
            for setting in [
                format!("remote.{remote}.url={value}"),
                format!("remote.{remote}.mirror=false"),
                "push.default=simple".to_owned(),
                "push.default=current".to_owned(),
                "push.default=upstream".to_owned(),
                "help.autocorrect=0".to_owned(),
                "help.autocorrect=never".to_owned(),
                format!("branch.{remote}.remote=origin"),
            ] {
                prop_assert!(!rewires_push(&format!("{lines}\n{setting}\n{lines}")), "{}", setting);
            }
        }

        #[test]
        fn ship_answer_by_mode(
            mode in mode(),
            reasons in proptest::collection::vec("[a-z ]{1,10}", 1..3),
        ) {
            let answer = ship_answer(mode, &reasons);
            let joined = reasons.join("; ");
            match mode {
                Mode::Bypass | Mode::Unknown => prop_assert_eq!(
                    answer,
                    Answer::Deny(format!("ShipIt: only the owner ships, and this permission mode cannot ask the owner. This command {joined}. Ask the owner to run it."))
                ),
                Mode::Default | Mode::Plan | Mode::AcceptEdits | Mode::Auto | Mode::DontAsk => prop_assert_eq!(
                    answer,
                    Answer::Ask(format!("ShipIt: only the owner ships. This command {joined}. Answer yes to let the agent run it."))
                ),
            }
            prop_assert_eq!(ship_answer(mode, &[]), Answer::Pass);
        }

        #[test]
        fn modes_parse_the_documented_strings_and_nothing_else(text in "[a-zA-Z]{0,12}") {
            let known = [
                ("default", Mode::Default),
                ("plan", Mode::Plan),
                ("acceptEdits", Mode::AcceptEdits),
                ("auto", Mode::Auto),
                ("dontAsk", Mode::DontAsk),
                ("bypassPermissions", Mode::Bypass),
                ("manual", Mode::Default),
            ];
            for (word, expected) in known {
                prop_assert_eq!(Mode::parse(word), expected);
            }
            if known.iter().all(|(word, _)| *word != text) {
                prop_assert_eq!(Mode::parse(&text), Mode::Unknown);
            }
        }
    }

    // Each written with quotes, and each must give a reason and be refused in bypass mode.
    #[test]
    fn hidden_commands_ask() {
        let commands = [
            "bash -c \"git push origin main\"",
            "bash -lc 'git push origin main'",
            "sh -euc 'git push origin main'",
            "/bin/sh -c 'git push origin main'",
            "eval \"git push origin main\"",
            "out=$(git push origin main 2>&1)",
            "out=`git push origin main`",
            "p=push; git $p origin main",
            "git --no-pager push origin main",
            "git -c x=y push origin main",
            "env GIT_DIR=x git push origin main",
            "G=gh; $G pr merge 1",
            "ssh h 'git push origin main'",
            "xargs git push",
            "f() { git push origin main; }; f",
            "git push -u origin \"$(git branch --show-current)\"",
            "git checkout main && git merge slice/x && git push",
            "git switch main && git pull && git push origin HEAD",
            "git config remote.origin.push x && git push origin slice/x",
            "bash <<'EOF'\ngit push origin main\nEOF",
            "cat <<'EOF' | sh\ngit push origin main\nEOF",
            "echo 'git push origin main' | python3 -",
            "pushd d && git push origin slice/x",
            "\"g\"it push origin main",
            "gh api -XPUT repos/o/r/pulls/1/merge",
            "gh api --method=PUT repos/o/r/pulls/1/merge",
            "gh api graphql -F query=@q.graphql",
            "gh pr -R o/r merge 1",
            "gh --repo=o/r pr merge 1",
            "sh -t \"$(cat <<'D'\ngit push origin main\nD\n)\"",
            "git config alias.x '!git push origin main' && git x",
            "git -c alias.x='!sh' x",
            "git config alias.p push",
            "git log --grep push",
            // A message is text only for the program that takes it.
            "sh -c '$2' git -m 'git push origin main'",
            "bash -c 'eval \"$2\"' git -m \"$(cat <<'D'\ngit push origin main\nD\n)\"",
            // Bash ends the body of the long form at `D)`.
            "git commit --allow-empty -m \"$(cat <<'D'\nD)\" ; git push origin main ; echo \"\nD\n)\"",
            "git commit -m \"$(cat <<'D'\nD )\" ; git push origin main ; echo \"\nD\n)\"",
            // Config through the environment can define an alias or move a directory.
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=alias.p GIT_CONFIG_VALUE_0=push git p origin main",
            "GIT_CONFIG_PARAMETERS=\"'alias.p=push'\" git p origin main",
            "export GIT_DIR=/x; git status",
            "env GH_TOKEN=x gh pr view 1",
            // Config and remotes that change where a push goes, and the plumbing that pushes.
            "git config remote.origin.push refs/heads/slice/v1/x:refs/heads/main",
            "git config help.autocorrect immediate",
            "git -c help.autocorrect=immediate psuh origin main",
            "git remote set-url origin /elsewhere",
            "git remote add x /elsewhere",
            "git send-pack /elsewhere slice/v1/x:refs/heads/main",
            "git http-push http://x main",
            // A comment is not skipped.
            "echo x # it's\ngi\\\nt push origin main #'",
            // A string given to an interpreter is a program the reader did not read.
            "bash -c 'g\\it push origin main'",
            "sh -c 'gi\"\"t push origin main'",
            "git status; python3 -c 'print(1)'",
            "git status; node -e 'x'",
            // The rules of gh apply inside one word, too.
            "bash -c 'gh repo delete o/r --yes'",
            "bash -c 'gh repo edit --default-branch x'",
            "git symbolic-ref HEAD refs/heads/main && git push -u origin slice/v1/x",
        ];
        for command in commands {
            assert!(asks(command), "{command}");
            assert!(denies_in_bypass(command), "{command}");
        }
    }

    #[test]
    fn a_base_or_tag_push_names_what_it_reaches() {
        assert_eq!(
            reasons("git push origin main"),
            ["pushes to the base branch main"]
        );
        assert_eq!(
            reasons("git push origin HEAD:main"),
            ["pushes to the base branch main"]
        );
        assert_eq!(reasons("git push origin v1.0"), [TAG]);
        assert_eq!(reasons("git push origin refs/tags/v1.0"), [TAG]);
        assert_eq!(reasons("git push"), [NO_SLICE]);
    }

    #[test]
    fn the_owner_action_names_its_reason_exactly() {
        assert_eq!(
            reasons("shipit complete demo --confirm"),
            ["confirms a goal completion (shipit complete --confirm), which only the owner may do"]
        );
    }

    #[test]
    fn merge_and_release_commands_ask() {
        assert_eq!(reasons("gh pr merge 12 --squash"), [MERGE]);
        assert_eq!(reasons("gh release create v1"), [RELEASE]);
        assert_eq!(reasons("gh repo sync"), [REPOSITORY]);
        for command in [
            "gh release upload v1 f",
            "gh release new v1",
            "gh release delete-asset v1 f",
            "gh release edit v1",
        ] {
            assert_eq!(reasons(command), [RELEASE], "{command}");
        }
        assert_eq!(reasons("gh repo delete o/r --yes"), [REPOSITORY]);
        assert_eq!(
            reasons("gh api -X PUT repos/o/r/pulls/1/merge"),
            [API_WRITE]
        );
        assert_eq!(reasons("gh api graphql -f query=x"), [GRAPHQL]);
    }

    #[test]
    fn the_answer_texts_are_exact() {
        let because = vec![MERGE.to_owned()];
        assert_eq!(
            ship_answer(Mode::Auto, &because),
            Answer::Ask("ShipIt: only the owner ships. This command merges a pull request. Answer yes to let the agent run it.".to_owned())
        );
        assert_eq!(
            ship_answer(Mode::Bypass, &because),
            Answer::Deny("ShipIt: only the owner ships, and this permission mode cannot ask the owner. This command merges a pull request. Ask the owner to run it.".to_owned())
        );
    }

    #[test]
    fn the_common_command_is_not_triggered() {
        for command in [
            "ls -la",
            "cargo test",
            "nix develop -c cargo clippy",
            "echo hi | wc",
        ] {
            assert!(!triggered(command), "{command}");
        }
        for command in [
            "git status",
            "\"g\"it x",
            "gh pr view",
            "shipit complete d --confirm",
        ] {
            assert!(triggered(command), "{command}");
        }
    }
}
