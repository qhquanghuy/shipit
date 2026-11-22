// Which shell commands are the owner's to run. The answer is a list of reasons: empty means the
// command may go, and anything the reader cannot fully understand is a reason, never a pass.

use crate::pure::branch::parse_slice_branch;
use crate::pure::shell_words::{
    Parse, Segment, leading_program, message_values, parse, program, raw_words, raw_words_unquoted,
    unquoted,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) base: Vec<String>,
    pub(crate) heads: Vec<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) follow_tags: bool,
    pub(crate) rewires_push: bool,
    pub(crate) current: Option<String>,
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
const ENV: &str = "sets a GIT_, GH_, BASH_ENV or ENV variable";
const CONFIG: &str = "changes git config";
const REMOTE: &str = "changes the git remotes";
const TAGS: &str = "creates or deletes a tag";
const NOTES: &str = "changes git notes";
const REWIRED: &str = "pushes with a git config that can send it elsewhere (a remote's push or mirror setting, push.default, help.autocorrect)";
const GLOBAL_OPTION: &str =
    "uses a git option ShipIt does not read (only -C and --no-pager are plain)";
const UNKNOWN_GIT: &str = "runs a git subcommand ShipIt does not list (an alias, a typo, submodule, bisect, filter-branch, send-pack and the rest)";
const RUNS_PROGRAM: &str = "gives git a program to run (--exec, --upload-pack and the like)";
const MERGES_BASE: &str = "merges into the base branch";
const DOT_GIT: &str =
    "names .git, or a path under it, other than the goals folder .git/shipit/goals/";
const GOALS_ONLY: &str = "does more than read, copy or make folders under .git/shipit/goals/ (only mkdir, cp without -r -R -a -L -s -l -H, cat, ls, head, tail, wc and shipit may run in such a command, with no redirect into a file)";
const BRANCH_REWRITE: &str =
    "deletes, renames, copies or force-moves a branch (a base branch holds the setup file)";
const FETCH_REFSPEC: &str =
    "fetches into a name (a refspec with a colon moves a remote-tracking branch)";
const REFLOG: &str = "changes the reflog (expire, delete)";
const SYMBOLIC_REF: &str = "changes a symbolic ref (HEAD and the like)";
const APPLIES: &str = "applies a patch (a patch can add a link into .git; only apply --check, --stat, --numstat and --summary pass)";
const RUNS_COMMAND: &str =
    "gives a text program a command to run (rg --pre, ag --pager, man -P and the like)";
const CONFIRM: &str =
    "confirms a goal completion (shipit complete --confirm), which only the owner may do";
const MERGE: &str = "merges a pull request";
const RELEASE: &str = "creates or changes a release";
const REPOSITORY: &str = "changes the repository";
const API_WRITE: &str = "calls the GitHub API with a write method";
const GRAPHQL: &str = "runs a GitHub GraphQL query";

const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "dash", "ksh", "fish"];
const SCRIPTERS: [&str; 6] = ["python", "python3", "perl", "ruby", "node", "script"];
pub(crate) const INTERPRETERS: [&str; 12] = [
    "sh", "bash", "zsh", "dash", "ksh", "fish", "python", "python3", "perl", "ruby", "node",
    "script",
];
const WRAPPERS: [&str; 6] = ["eval", "xargs", "exec", "su", "ssh", "watch"];
// Programs whose words are text for them, whatever the words say.
const DATA_PROGRAMS: [&str; 16] = [
    "echo", "printf", "grep", "egrep", "fgrep", "rg", "ag", "which", "type", "whereis", "man",
    "ls", "cat", "head", "tail", "wc",
];
// The git subcommands that pass silently: they only read, or they edit the work and the
// branches in ways the check and the review see. `push`, `merge`, `rebase`, `tag`, `notes`,
// `config` and `remote` pass only in the forms judged below. Everything else asks.
const GIT_READS: [&str; 24] = [
    "status",
    "log",
    "diff",
    "show",
    "rev-parse",
    "rev-list",
    "ls-files",
    "ls-remote",
    "blame",
    "grep",
    "describe",
    "shortlog",
    "cat-file",
    "merge-base",
    "name-rev",
    "for-each-ref",
    "fetch",
    "ls-tree",
    "show-ref",
    "cherry",
    "range-diff",
    "check-ignore",
    "help",
    "version",
];
const GIT_EDITS: [&str; 15] = [
    "add",
    "rm",
    "mv",
    "restore",
    "commit",
    "switch",
    "checkout",
    "branch",
    "stash",
    "reset",
    "cherry-pick",
    "revert",
    "worktree",
    "clean",
    "init",
];
// A subcommand that moves the branch or the config: a push in the same command is never the
// plain one.
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
// repository (GIT_DIR) or another host, and BASH_ENV and ENV name a file a shell runs at its
// start: in a text that names git or gh, any such assignment asks.
fn sets_git_env(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.starts_with("GIT_") || name.starts_with("GH_") || matches!(name, "BASH_ENV" | "ENV")
    })
}

// The config as `git config --list` prints it: does it send a push somewhere the command does
// not say? A remote's push or mirror setting, a push.default that is not unset, `simple` or `current`
// (`upstream` sends a branch to the branch it tracks, which is the base when it was cut from it),
// or help.autocorrect that runs a mistyped command.
pub(crate) fn rewires_push(config: &str) -> bool {
    config.lines().any(|line| {
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let (key, value) = (key.to_lowercase(), value.trim().to_lowercase());
        let off = ["", "0", "never", "false", "no", "off"].contains(&value.as_str());
        (key.starts_with("remote.") && key.rsplit('.').next() == Some("push"))
            || (key.starts_with("remote.") && key.rsplit('.').next() == Some("mirror") && !off)
            || (key == "push.default" && !["simple", "current"].contains(&value.as_str()))
            || (key == "help.autocorrect" && !off)
    })
}

// Only the plain reads of the config pass: `--get`, `--list` and their kind, after any scope.
fn changes_config(args: &[String]) -> bool {
    let mut words = args
        .iter()
        .skip_while(|word| CONFIG_SCOPES.contains(&word.as_str()));
    let reads = words
        .next()
        .is_some_and(|first| CONFIG_READS.contains(&first.as_str()));
    let writes = args
        .iter()
        .any(|word| CONFIG_WRITE_FLAGS.contains(&word.as_str()));
    !reads || writes
}

// A branch that is deleted, renamed, copied or moved by force can empty what makes a folder a project.
fn rewrites_branches(args: &[String]) -> bool {
    args.iter().any(|word| {
        matches!(word.as_str(), "--delete" | "--move" | "--copy" | "--force")
            || cluster_has(word, &['d', 'D', 'm', 'M', 'c', 'C', 'f'])
    })
}

// `git fetch origin +old:refs/remotes/origin/main` moves a remote-tracking branch; a URL has a
// colon too and is no refspec.
fn fetches_into_a_name(args: &[String]) -> bool {
    args.iter()
        .any(|word| !word.starts_with('-') && word.contains(':') && !word.contains("://"))
}

// Only a listing of the remotes passes: bare, `-v`, `show` and `get-url`.
fn changes_remote(args: &[String]) -> bool {
    let mut after = args
        .iter()
        .skip_while(|word| *word == "-v" || *word == "--verbose");
    after
        .next()
        .is_some_and(|first| first != "show" && first != "get-url")
}

// A tag listing names no tag: no word that is not an option, or `-l`/`--list`.
fn lists_tags(args: &[String]) -> bool {
    args.iter().any(|word| word == "-l" || word == "--list")
        || args.iter().all(|word| word.starts_with('-'))
}

fn reads_notes(args: &[String]) -> bool {
    args.iter()
        .find(|word| !word.starts_with('-'))
        .is_none_or(|first| first == "list" || first == "show")
}

// A patch can create a symbolic link to .git/hooks, so only the forms that apply nothing pass.
fn only_checks_a_patch(args: &[String]) -> bool {
    args.iter().any(|word| {
        matches!(
            word.as_str(),
            "--check" | "--stat" | "--numstat" | "--summary"
        )
    }) && !args.iter().any(|word| word == "--apply")
}

// `git reflog expire` and `git reflog delete` change it; the rest shows it.
fn writes_reflog(args: &[String]) -> bool {
    args.iter().any(|word| word == "expire" || word == "delete")
}

// `git symbolic-ref [--short] [-q] <name>` reads; a second argument, `-d` or `-m` writes.
fn reads_symbolic_ref(args: &[String]) -> bool {
    let reads_only = args
        .iter()
        .filter(|word| word.starts_with('-'))
        .all(|word| {
            matches!(
                word.as_str(),
                "-q" | "--quiet" | "--short" | "--recurse" | "--no-recurse"
            )
        });
    reads_only && args.iter().filter(|word| !word.starts_with('-')).count() <= 1
}

// The global options of a git command, then its subcommand. Only `-C <dir>`, `--no-pager`,
// `--git-dir` and `--work-tree` (the folders are judged apart) are plain; any other option
// can run a program or change the config.
enum Global {
    Plain { sub: Option<usize>, options: bool },
    Unknown,
}

fn split_global(rest: &[String]) -> Global {
    let mut at = 0;
    let mut options = false;
    while let Some(word) = rest.get(at) {
        match word.as_str() {
            "-C" | "--git-dir" | "--work-tree" => {
                options = true;
                at += 2;
            }
            "--no-pager" | "--version" => {
                options = true;
                at += 1;
            }
            glued if glued.starts_with("--git-dir=") || glued.starts_with("--work-tree=") => {
                options = true;
                at += 1;
            }
            option if option.starts_with('-') => return Global::Unknown,
            _ => {
                return Global::Plain {
                    sub: Some(at),
                    options,
                };
            }
        }
    }
    Global::Plain { sub: None, options }
}

// A short flag cluster such as `-ix` holds the letter.
fn cluster_has(word: &str, letters: &[char]) -> bool {
    word.strip_prefix('-')
        .is_some_and(|rest| !rest.starts_with('-') && rest.chars().any(|c| letters.contains(&c)))
}

// An option that hands git a program to run.
fn runs_a_program(sub: &str, word: &str) -> bool {
    [
        "--upload-pack",
        "--receive-pack",
        "--exec",
        "--open-files-in-pager",
        "--extcmd",
    ]
    .iter()
    .any(|name| word.starts_with(name))
        || (matches!(sub, "fetch" | "ls-remote") && word == "-u")
        || (sub == "grep" && word.starts_with("-O"))
        || (sub == "init"
            && (word.starts_with("--template") || word.starts_with("--separate-git-dir")))
        || (sub == "rebase" && (word == "--interactive" || cluster_has(word, &['x', 'i'])))
        || (sub == "merge" && (word == "-s" || word.starts_with("--strategy")))
}

// The subcommand of each git command of the segment, for the ones that move the branch.
fn changes_context(segments: &[Segment]) -> bool {
    segments
        .iter()
        .filter(|segment| !is_text_only(segment))
        .any(|segment| {
            let changes_git = segment.words.iter().enumerate().any(|(at, word)| {
                program(word) == "git"
                    && matches!(
                        split_global(segment.words.get(at + 1..).unwrap_or_default()),
                        Global::Plain { sub: Some(index), .. }
                            if segment
                                .words
                                .get(at + 1 + index)
                                .is_some_and(|sub| {
                                    let args = segment.words.get(at + 2 + index..).unwrap_or_default();
                                    CHANGERS.contains(&sub.as_str())
                                        && !(sub == "symbolic-ref" && reads_symbolic_ref(args))
                                })
                    )
            });
            changes_git || segment.words.iter().any(|word| word == FOLDER_WORD)
        })
}

fn is_text_only(segment: &Segment) -> bool {
    leading_program(segment.words.iter().map(String::as_str))
        .is_some_and(|name| DATA_PROGRAMS.contains(&name))
}

struct Judge<'a> {
    facts: &'a dyn Fn(Option<&str>) -> Facts,
    changes_context: bool,
}

impl Judge<'_> {
    // One git command: it passes only when its global options, its subcommand and the options
    // of that subcommand are on the lists above.
    fn git_command(
        &self,
        dir: Option<&str>,
        rest: &[String],
        data: &[bool],
        reasons: &mut Vec<String>,
    ) {
        let (sub, options) = match split_global(rest) {
            Global::Unknown => return add(reasons, GLOBAL_OPTION),
            Global::Plain { sub: None, .. } => return,
            Global::Plain {
                sub: Some(sub),
                options,
            } => (sub, options),
        };
        let name = rest.get(sub).map_or("", String::as_str);
        let args = rest.get(sub + 1..).unwrap_or_default();
        let args_data = data.get(sub + 1..).unwrap_or_default();
        let before_paths = args
            .iter()
            .zip(args_data)
            .take_while(|(word, _)| *word != "--")
            .filter(|(_, is_data)| !**is_data);
        if before_paths
            .into_iter()
            .any(|(word, _)| runs_a_program(name, word))
        {
            add(reasons, RUNS_PROGRAM);
        }
        match name {
            "push" => self.push(dir, options, args, reasons),
            "config" if changes_config(args) => add(reasons, CONFIG),
            "remote" if changes_remote(args) => add(reasons, REMOTE),
            "tag" if !lists_tags(args) => add(reasons, TAGS),
            "notes" if !reads_notes(args) => add(reasons, NOTES),
            "reflog" if writes_reflog(args) => add(reasons, REFLOG),
            "branch" if rewrites_branches(args) => add(reasons, BRANCH_REWRITE),
            "fetch" if fetches_into_a_name(args) => add(reasons, FETCH_REFSPEC),
            "symbolic-ref" if !reads_symbolic_ref(args) => add(reasons, SYMBOLIC_REF),
            "apply" if !only_checks_a_patch(args) => add(reasons, APPLIES),
            "merge" => {
                let facts = (self.facts)(dir);
                if facts
                    .current
                    .as_ref()
                    .is_some_and(|on| facts.base.contains(on))
                {
                    add(reasons, MERGES_BASE);
                }
            }
            "config" | "remote" | "tag" | "notes" | "rebase" | "reflog" | "symbolic-ref"
            | "apply" | "branch" | "fetch" => {}
            known if GIT_READS.contains(&known) || GIT_EDITS.contains(&known) => {}
            _ => add(reasons, UNKNOWN_GIT),
        }
    }

    fn push(&self, dir: Option<&str>, options: bool, args: &[String], reasons: &mut Vec<String>) {
        if options {
            add(reasons, OPTION_BEFORE);
        }
        let facts = (self.facts)(dir);
        match explicit_slice_push(args, &facts) {
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

fn tokens_of(word: &str) -> Vec<String> {
    let bare: String = word
        .chars()
        .filter(|c| !matches!(c, '\'' | '"' | '\\'))
        .collect();
    bare.split(|c: char| c.is_whitespace() || ";|&()".contains(c))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

// One word that is a whole command line (`'git push origin main'`): its tokens are judged as
// commands, by the same lists, so quoting it hides nothing.
fn hidden_in_word(judge: &Judge<'_>, word: &str, reasons: &mut Vec<String>) {
    let tokens = tokens_of(word);
    let mut found = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let rest = tokens.get(at + 1..).unwrap_or_default();
        match program(token) {
            "git" => judge.git_command(None, rest, &vec![false; rest.len()], &mut found),
            "gh" => gh_word(rest, &mut found),
            _ => {}
        }
    }
    if !found.is_empty() {
        add(reasons, HIDES);
        for reason in found {
            add(reasons, reason);
        }
    }
}

// A flag that hands a shell its program as a string: `-c`, `-lc`, `-euc`, `--command`. A shell's
// `-e` is errexit.
fn is_shell_string_flag(word: &str) -> bool {
    word == "--command" || cluster_has(word, &['c'])
}

// A flag that hands the interpreter its program as a string: `-c`, `-e`, `-lc`, `-euc`.
fn is_string_flag(word: &str) -> bool {
    cluster_has(word, &['c', 'e'])
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

// A shell that is given a string or a file to run, or any other interpreter at all: a program
// the reader did not read. A file may have been written a moment ago by the same command.
fn runs_a_program_text(words: &[String]) -> bool {
    words.iter().enumerate().any(|(at, word)| {
        runs_as(word, &SCRIPTERS)
            || (runs_as(word, &SHELLS)
                && words
                    .iter()
                    .skip(at + 1)
                    .any(|later| is_shell_string_flag(later) || !later.starts_with('-')))
    })
}

// `source` and `.` run what they read: a pipe or a device such as /dev/stdin is a program the
// reader did not see.
fn sources_a_stream(segment: &Segment) -> bool {
    matches!(
        leading_program(segment.words.iter().map(String::as_str)),
        Some("source" | ".")
    ) && (segment.reads_stdin
        || segment
            .words
            .iter()
            .any(|word| word.starts_with("/dev/") || word.starts_with("/proc/")))
}

// The options of a program that only reads text, that hand it a command to run.
fn runs_a_command(segment: &Segment) -> bool {
    let flag = |word: &str, long: &str| {
        word == long
            || word
                .strip_prefix(long)
                .is_some_and(|rest| rest.starts_with('='))
    };
    let owner = leading_program(segment.words.iter().map(String::as_str));
    segment.words.iter().any(|word| match owner {
        Some("rg") => flag(word, "--pre") || flag(word, "--hostname-bin"),
        Some("ag") => flag(word, "--pager"),
        Some("man") => {
            flag(word, "--pager") || flag(word, "--html") || cluster_has(word, &['P', 'H'])
        }
        Some(_) | None => false,
    })
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
        let text_only = is_text_only(segment);
        if text_only && runs_a_command(segment) {
            add(reasons, RUNS_COMMAND);
        }
        for (at, word) in segment.words.iter().enumerate() {
            if data.get(at) == Some(&true) {
                continue;
            }
            if sets_git_env(word) {
                add(reasons, ENV);
            }
            if text_only {
                continue;
            }
            if runs_as(word, &WRAPPERS) {
                add(reasons, HIDES);
            }
            let rest = segment.words.get(at + 1..).unwrap_or_default();
            let rest_data = data.get(at + 1..).unwrap_or_default();
            if word.contains(|c: char| c.is_whitespace() || ";|&()".contains(c)) {
                hidden_in_word(&judge, word, reasons);
            } else {
                match program(word) {
                    "git" => judge.git_command(segment.dir.as_deref(), rest, rest_data, reasons),
                    "gh" => gh_word(rest, reasons),
                    _ => {}
                }
            }
        }
    }
    let reads_a_program = segments
        .iter()
        .filter(|segment| !is_text_only(segment))
        .any(|segment| {
            runs_a_program_text(&segment.words)
                || sources_a_stream(segment)
                || (segment.reads_stdin
                    && segment
                        .words
                        .iter()
                        .any(|word| runs_as(word, &INTERPRETERS)))
        });
    if !heredocs.is_empty() || reads_a_program {
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

const GOALS_FOLDER: &str = "/shipit/goals/";
// A link made in goals/ can point at the hooks or the receipts, and an archive can hold one, so
// the programs that may run in a text naming the goals are a list, not a ban.
const GOALS_PROGRAMS: [&str; 8] = ["mkdir", "cp", "cat", "ls", "head", "tail", "wc", "shipit"];
// The letters of the `cp` options that only copy a file: `-p -f -n -i -v`, and `-t` and `-T` that name
// the target. Any other option can copy a link as a link (`-P`, `-d`, `--preserve=links`) or build
// paths from the source (`--parents`).
const COPY_LETTERS: &str = "pfnivtT";

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

// A `.git` followed by a slash is a path under .git/, and passes only as `.git/shipit/goals/` (the
// goals of the store, which the skills write) with no `..` part after it and no `receipts/`
// anywhere. A text cannot show where a link inside goals/ points: only ShipIt and the skills
// write there.
fn dot_git_beyond_goals(text: &str) -> bool {
    text.match_indices(".git").any(|(at, found)| {
        let after = text.get(at + found.len()..).unwrap_or_default();
        match after.strip_prefix(GOALS_FOLDER) {
            Some(tail) => {
                text.contains("receipts/")
                    || tail
                        .split(|c: char| !is_name_char(c))
                        .any(|part| part == "..")
            }
            None => after.starts_with('/'),
        }
    })
}

// A `>` that is not a descriptor copy (`2>&1`) or a discard (`/dev/null`) writes a file.
fn redirects_to_a_file(command: &str) -> bool {
    command.match_indices('>').any(|(at, _)| {
        let after = command
            .get(at + 1..)
            .unwrap_or_default()
            .trim_start_matches(['>', ' ', '\t']);
        let descriptor = after.strip_prefix('&').is_some_and(|rest| {
            rest.chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '-')
        });
        !(descriptor || after.starts_with("/dev/null"))
    })
}

// Every option before `--` is on the list; a word that is no option is a path.
fn copies_plainly(words: &[String]) -> bool {
    words
        .iter()
        .take_while(|word| *word != "--")
        .filter(|word| word.starts_with('-'))
        .all(|word| match word.strip_prefix("--") {
            Some(long) => long == "target-directory" || long.starts_with("target-directory="),
            None => word
                .strip_prefix('-')
                .is_some_and(|letters| letters.chars().all(|c| COPY_LETTERS.contains(c))),
        })
}

fn only_copies_and_reads(command: &str, parsed: &Parse) -> bool {
    let Parse::Plain { segments, .. } = parsed else {
        return false;
    };
    !redirects_to_a_file(command)
        && segments.iter().all(|segment| {
            let name = leading_program(segment.words.iter().map(String::as_str));
            name.is_some_and(|name| GOALS_PROGRAMS.contains(&name))
                && (name != Some("cp") || copies_plainly(&segment.words))
        })
}

fn names_goals(text: &str) -> bool {
    text.match_indices(".git").any(|(at, found)| {
        text.get(at + found.len()..)
            .is_some_and(|after| after.starts_with(GOALS_FOLDER))
    })
}

// `.git` as the whole last part of a path in a word (`cd .git`, `--git-dir=x/.git`): not `.github`,
// `x.git`, and not `.git/…`, which the other rule judges.
fn is_bare_dot_git(word: &str) -> bool {
    word.match_indices(".git").any(|(at, found)| {
        let before = word.get(..at).and_then(|head| head.chars().next_back());
        let after = word
            .get(at + found.len()..)
            .and_then(|tail| tail.chars().next());
        !before.is_some_and(is_name_char) && !after.is_some_and(|c| is_name_char(c) || c == '/')
    })
}

fn names_dot_git(command: &str, parsed: &Parse) -> bool {
    // Writes under .git/ (the config, the hooks) are redirects that the words do not show.
    let bare = match parsed {
        Parse::Plain { segments, .. } => segments
            .iter()
            .filter(|segment| !is_text_only(segment))
            .any(|segment| {
                let data = message_values(&segment.words);
                segment
                    .words
                    .iter()
                    .zip(data)
                    .any(|(word, is_data)| !is_data && is_bare_dot_git(word))
            }),
        Parse::Complex => false,
    };
    bare || dot_git_beyond_goals(command) || dot_git_beyond_goals(&unquoted(command))
}

// Every folder the command passes through is judged, the working folder too: a ship command may
// sit in a word that another program runs (`cd /x && bash -c 'cd /project && git push'`), and a
// `.git` path is the same repository from any folder.
pub(crate) fn judged_dirs(command: &str) -> Vec<Option<String>> {
    let mut dirs = vec![None];
    if let Parse::Plain { segments, .. } = parse(command) {
        dirs.extend(segments.into_iter().map(|segment| segment.dir));
    }
    dirs.sort();
    dirs.dedup();
    dirs
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
    if names_dot_git(command, &parsed) {
        add(&mut reasons, DOT_GIT);
    }
    if (names_goals(command) || names_goals(&unquoted(command)))
        && !only_copies_and_reads(command, &parsed)
    {
        add(&mut reasons, GOALS_ONLY);
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
            current: Some("slice/v1/x".to_owned()),
        }
    }

    fn reasons(command: &str) -> Vec<String> {
        ship_reasons(command, &|_dir| facts())
    }

    fn asks(command: &str) -> bool {
        !reasons(command).is_empty()
    }

    const PLAIN_WORDS: [&str; 9] = [
        "ls",
        "x",
        "-la",
        "foo",
        "commit",
        "status",
        "--no-verify",
        "log",
        "fetch",
    ];
    const BRANCHES: [&str; 7] = [
        "slice/v1/x",
        "main",
        "master",
        "v1.0",
        "slice/x",
        "slice/a/b/c",
        "slice/v1/y",
    ];
    const ORACLE_SHELLS: [&str; 3] = ["bash", "sh", "python3"];

    const GOOD_C: &str = "\"$(cat <<'EOF'\nfix git push\nEOF\n)\"";
    // Bash ends this body at `EOF)`, and runs what follows as commands.
    const BAD_C: &str = "\"$(cat <<'EOF'\nEOF)\" ; git push origin main ; echo \"\nEOF\n)\"";
    const QUOTED: [&str; 4] = [
        "'git push origin main'",
        "\"git push origin main\"",
        GOOD_C,
        BAD_C,
    ];
    // The git subcommands the oracle knows pass, and the options that hand git a program.
    const OK_SUBS: [&str; 8] = [
        "status", "log", "fetch", "add", "commit", "checkout", "switch", "rebase",
    ];
    const ARGS_OF_GIT: [&str; 5] = ["--exec", "-x", "foreach", "core.fsmonitor=x", "psuh"];

    // Tokens of the property that a command never passes outside the allowed forms. A lone
    // quote and a redirection are not tokens: they change which words there are, which the
    // oracle below cannot follow without being a second parser. A quoted text is one token.
    fn token() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            8 => proptest::sample::select(&PLAIN_WORDS[..]),
            4 => Just("git"),
            4 => Just("push"),
            1 => Just("\"g\"it"),
            1 => Just("g\"\"it"),
            1 => Just("'git'"),
            1 => Just("gh"),
            1 => Just("pr"),
            1 => Just("merge"),
            2 => proptest::sample::select(&["origin", "other"][..]),
            3 => proptest::sample::select(&BRANCHES[..]),
            3 => proptest::sample::select(&["-u", "--set-upstream"][..]),
            2 => proptest::sample::select(&QUOTED[..]),
            2 => proptest::sample::select(&ARGS_OF_GIT[..]),
            2 => proptest::sample::select(&[
                "-m", "-c", "-lc", "#", "GIT_CONFIG_COUNT=1", "GH_TOKEN=x", "A=1",
                "symbolic-ref", "remote", "submodule", "tag", "notes",
            ][..]),
            3 => proptest::sample::select(&[
                "HEAD", "@", ":x", "+x", "--force", "--tags", "--all", "--delete", "-C", "d",
                "--no-pager", "GIT_DIR=x", "switch", "config", "cd", "pushd", "popd",
                "eval", "xargs", "bash", "sh", "python3",
            ][..]),
            2 => proptest::sample::select(&[
                "$x", "$(echo", "`x`", "(", ")", "{", "}", "!", "~", "if", "then", "fi", "for",
                "done", "\u{5c}", "<<'EOF'", "<<EOF", "{a,b}",
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
        "\u{5c}",
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
        "python3",
    ];
    const KEYWORDS: [&str; 5] = ["if", "then", "fi", "for", "done"];

    fn canonical(token: &'static str) -> &'static str {
        match token {
            "\"g\"it" | "g\"\"it" | "'git'" => "git",
            other => other,
        }
    }

    // A segment as the oracle reads it: its canonical words, and which of them are text: the
    // value of `-m` in a segment whose program is git, or every word of a segment whose program
    // only reads text (`ls`), whatever the words say.
    fn view(segment: &[&'static str]) -> (Vec<&'static str>, Vec<bool>) {
        let words: Vec<&'static str> = segment.iter().map(|t| canonical(t)).collect();
        let owner = words.iter().find(|t| !t.contains('=')).copied();
        let data = (0..words.len())
            .map(|at| {
                owner == Some("ls")
                    || (owner == Some("git") && at > 0 && words.get(at - 1) == Some(&"-m"))
            })
            .collect();
        (words, data)
    }

    // Quoted text is data only as such text; a shell given a string runs it.
    fn reads_ok(segment: &[&'static str]) -> bool {
        let (words, data) = view(segment);
        words.iter().enumerate().all(|(at, word)| {
            let string_flag = |t: &&str| *t == "-c" || *t == "-lc";
            let hides = QUOTED.contains(word) && data.get(at) != Some(&true);
            let runs_string = ORACLE_SHELLS.contains(word)
                && data.get(at) != Some(&true)
                && words.iter().skip(at + 1).any(string_flag);
            !hides && !runs_string
        })
    }

    fn merges_a_pull_request(segment: &[&'static str]) -> bool {
        let (words, data) = view(segment);
        words.iter().enumerate().any(|(at, word)| {
            let later: Vec<&&str> = words
                .iter()
                .skip(at + 1)
                .filter(|t| !t.starts_with('-'))
                .take(2)
                .collect();
            *word == "gh" && data.get(at) != Some(&true) && later == [&"pr", &"merge"]
        })
    }

    // What a git command of the generated text is, read from its tokens by the rules in the
    // task: the global options, then the subcommand, then what the subcommand allows.
    #[derive(PartialEq)]
    enum Git {
        Ok,
        Changes,
        Push {
            options: bool,
            args: Vec<&'static str>,
        },
        Asks,
    }

    fn read_git(rest: &[&'static str]) -> Git {
        let mut at = 0;
        let mut options = false;
        loop {
            match rest.get(at) {
                None => return Git::Ok,
                Some(&"-C") => {
                    options = true;
                    at += 2;
                }
                Some(&"--no-pager") => {
                    options = true;
                    at += 1;
                }
                Some(word) if word.starts_with('-') => return Git::Asks,
                Some(_) => break,
            }
        }
        let sub = rest.get(at).copied().unwrap_or("");
        let args: Vec<&'static str> = rest.iter().skip(at + 1).copied().collect();
        let runs_program = args.iter().any(|w| w.starts_with("--exec"))
            || (sub == "rebase" && args.contains(&"-x"));
        if runs_program {
            return Git::Asks;
        }
        match sub {
            "push" => Git::Push { options, args },
            "tag" if args.iter().all(|w| w.starts_with('-')) => Git::Changes,
            "remote" if args.is_empty() => Git::Ok,
            "notes" if args.iter().all(|w| w.starts_with('-')) => Git::Ok,
            "merge" => Git::Changes,
            "symbolic-ref" if args.len() <= 1 && args.iter().all(|w| !w.starts_with('-')) => {
                Git::Ok
            }
            "symbolic-ref" | "update-ref" | "reset" | "branch" | "pull" => Git::Asks,
            known if OK_SUBS.contains(&known) => {
                if CHANGERS.contains(&known) {
                    Git::Changes
                } else {
                    Git::Ok
                }
            }
            _ => Git::Asks,
        }
    }

    // The independent reading of a generated command, token by token: it is safe to pass only
    // when no token needs a reading the gate lacks, no pipe feeds a shell, no `gh pr merge`
    // stands in it, and every git command is on the list of the ones that pass, a push being
    // exactly the explicit slice push with nothing that could move its branch, config or folder.
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
                && views.get(at + 1).is_some_and(|(next, data)| {
                    next.iter()
                        .zip(data)
                        .any(|(t, is_data)| !is_data && ORACLE_SHELLS.contains(t))
                })
        });
        if keyword_first
            || piped_to_shell
            || tokens.iter().any(|token| DANGER.contains(token))
            || !segments.iter().all(|s| reads_ok(s))
            || segments.iter().any(|s| merges_a_pull_request(s))
        {
            return false;
        }
        let git_commands: Vec<Git> = views
            .iter()
            .flat_map(|(words, data)| {
                let git_at: Vec<usize> = (0..words.len())
                    .filter(|at| words.get(*at) == Some(&"git") && data.get(*at) != Some(&true))
                    .collect();
                git_at.into_iter().map(move |at| {
                    let rest: Vec<&'static str> = words
                        .iter()
                        .zip(data)
                        .skip(at + 1)
                        .filter(|(_, is_data)| !**is_data)
                        .map(|(t, _)| *t)
                        .collect();
                    read_git(&rest)
                })
            })
            .collect();
        let context_change = tokens.contains(&FOLDER_WORD) || git_commands.contains(&Git::Changes);
        git_commands.iter().all(|git| match git {
            Git::Ok | Git::Changes => true,
            Git::Asks => false,
            Git::Push { options, args } => {
                let target = match args.as_slice() {
                    ["-u" | "--set-upstream", "origin", target] | ["origin", target] => *target,
                    _ => return false,
                };
                !options && target == "slice/v1/x" && !context_change
            }
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
            sub in proptest::sample::select(&[
                "status", "log", "diff", "show", "rev-parse", "rev-list", "ls-files", "ls-remote",
                "blame", "grep", "describe", "shortlog", "cat-file", "merge-base", "name-rev",
                "for-each-ref", "fetch", "add", "rm", "mv", "restore", "commit", "switch",
                "checkout", "branch", "stash", "reset", "cherry-pick", "revert", "worktree",
                "ls-tree", "show-ref", "cherry", "range-diff", "check-ignore", "help", "version",
                "clean", "init",
            ][..]),
            words in proptest::collection::vec(
                proptest::sample::select(&["x", "foo", "-la", "config", "remote", "push", "HEAD~1"][..]),
                0..4,
            ),
            body in proptest::collection::vec("[ -~]{1,20}", 1..4),
            text in "[a-z ]{0,10}git push origin main[a-z ]{0,10}",
            quote in prop_oneof![Just("'"), Just("\"")],
        ) {
            prop_assume!(body.iter().all(|line| line != "EOF"));
            let heredoc = format!("\"$(cat <<'EOF'\n{}\nEOF\n)\"", body.join("\n"));
            let commands = [
                format!("git {sub} {}", words.join(" ")),
                format!("cd d && git {sub} {} 2>&1 | wc -l > out", words.join(" ")),
                format!("git --no-pager -C d {sub} {}", words.join(" ")),
                "git add -A && git commit -m 'x git push origin main' && git push -u origin slice/v1/x".to_owned(),
                "git add config && git log -- config && git checkout remote".to_owned(),
                "git log --grep push && git stash push -m wip".to_owned(),
                "git tag && git tag -l v1* && git remote && git notes && git notes show".to_owned(),
                "git rebase main && git merge main && git cherry-pick -x abc".to_owned(),
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
        fn a_git_gh_or_shell_startup_variable_asks_wherever_it_is_set(
            name in prop_oneof![
                Just("GIT_DIR"), Just("GIT_WORK_TREE"), Just("GIT_CONFIG_COUNT"),
                Just("GIT_CONFIG_KEY_0"), Just("GIT_CONFIG_VALUE_0"), Just("GIT_CONFIG_PARAMETERS"),
                Just("GIT_EXEC_PATH"), Just("GIT_EDITOR"), Just("GH_TOKEN"), Just("GH_HOST"),
                Just("BASH_ENV"), Just("ENV"),
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
        fn a_string_given_to_a_shell_and_any_other_interpreter_is_a_program_nobody_read(
            shell in proptest::sample::select(&SHELLS[..]),
            scripter in proptest::sample::select(&SCRIPTERS[..]),
            path in prop_oneof![Just(""), Just("/bin/"), Just("./")],
            flag in prop_oneof![Just("-c"), Just("-lc"), Just("-euc"), Just("-ec"), Just("--command")],
            other in prop_oneof![
                Just("-e"), Just("-p"), Just("-E"), Just("--eval"), Just("--print"), Just("-i"),
                Just("-qc"), Just("script.sh"),
            ],
            text in "[a-z]{1,8}[0-9]",
        ) {
            let with_flag = format!("git status; {path}{shell} {flag} '{text}'");
            prop_assert_eq!(reasons(&with_flag), vec![FEEDS.to_owned()], "{}", with_flag);
            let wrapped = format!("git status; env X=1 {path}{shell} {flag} \"{text}\"");
            prop_assert!(asks(&wrapped), "{}", wrapped);
            let any_flags = format!("git status; {path}{scripter} {other} '{text}'");
            prop_assert_eq!(reasons(&any_flags), vec![FEEDS.to_owned()], "{}", any_flags);
            let bare = format!("git status; {path}{scripter} script.sh");
            prop_assert_eq!(reasons(&bare), vec![FEEDS.to_owned()], "{}", bare);
            // A shell's -e is errexit, and a shell given no string and no file has nothing to run.
            for command in [
                format!("git status; {path}{shell} -e"),
                format!("git status; {path}{shell} --version"),
                format!("git status; echo {flag} {scripter}"),
                format!("git status; echo {shell} {text}.sh"),
            ] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
        }

        // A file may have been written by the same command: `echo 'git push' > f.sh && sh f.sh`.
        #[test]
        fn a_shell_given_a_file_is_a_program_nobody_read(
            shell in proptest::sample::select(&SHELLS[..]),
            path in prop_oneof![Just(""), Just("/bin/"), Just("./")],
            option in prop_oneof![Just(""), Just("-e "), Just("-x "), Just("--norc ")],
            file in "[a-z]{1,8}[.]sh",
        ) {
            for command in [
                format!("git status; {path}{shell} {option}{file}"),
                format!("echo x > {file} && git status; {path}{shell} {option}./{file}"),
                format!("git status; env X=1 {path}{shell} {option}{file}"),
                format!("git status; {path}{shell} < {file}"),
                format!("git status; {path}{shell} {option}<{file}"),
            ] {
                prop_assert_eq!(reasons(&command), vec![FEEDS.to_owned()], "{}", command);
            }
        }

        #[test]
        fn source_reading_a_pipe_or_a_device_is_a_program_nobody_read(
            source in prop_oneof![Just("source"), Just(".")],
            device in prop_oneof![Just("/dev/stdin"), Just("/dev/fd/0"), Just("/proc/self/fd/0")],
            file in "[a-z]{1,8}[.]sh",
        ) {
            for command in [
                format!("git status; {source} {device}"),
                format!("git status; echo x | {source} {file}"),
                format!("git status; echo x | {source} {device}"),
            ] {
                prop_assert_eq!(reasons(&command), vec![FEEDS.to_owned()], "{}", command);
            }
            for command in [
                format!("git status; {source} ./{file}"),
                format!("git status; {source} {file} && git log"),
                format!("git status; echo {source} {device}"),
            ] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
        }

        // The command slots of the programs that otherwise only read text.
        #[test]
        fn a_command_slot_of_a_text_program_asks(
            command in "[a-z]{1,8}",
            pattern in "[a-z]{1,8}",
        ) {
            for text in [
                format!("rg --pre {command} {pattern} src"),
                format!("rg --pre={command} {pattern}"),
                format!("rg --hostname-bin {command} {pattern}"),
                format!("ag --pager {command} {pattern}"),
                format!("ag --pager={command} {pattern}"),
                format!("man -P {command} {pattern}"),
                format!("man -P{command} {pattern}"),
                format!("man -aP {command} {pattern}"),
                format!("man --pager={command} {pattern}"),
                format!("man -H{command} {pattern}"),
                format!("A=1 rg --pre 'git push origin main' {pattern}"),
            ] {
                prop_assert_eq!(reasons(&format!("git status; {text}")), vec![RUNS_COMMAND.to_owned()], "{}", text);
            }
            for text in [
                format!("rg -n {pattern} src"),
                format!("rg -P {pattern}"),
                format!("rg --pre-glob {pattern} {command}"),
                format!("ag -i {pattern}"),
                format!("ag --nopager {pattern}"),
                format!("man {pattern}"),
                format!("man -k {pattern}"),
                format!("grep --pre {command} {pattern}"),
                format!("echo man -P {command}"),
            ] {
                prop_assert_eq!(reasons(&format!("git status; {text}")), Vec::<String>::new(), "{}", text);
            }
        }
        // The goals of the store are the skills' own place; nothing else of .git passes.
        #[test]
        fn only_the_goals_folder_of_the_store_may_be_named_under_dot_git(
            root in "/?[a-z]{1,6}(/[a-z]{1,6}){0,2}",
            goal in name(),
            other in name(),
            file in "[a-z]{1,6}[.]md",
        ) {
            let goals = format!("{root}/.git/shipit/goals");
            for command in [
                format!("mkdir -p {goals}/{goal} && cp a {goals}/{goal}/roadmap.v1.md"),
                format!("cat {goals}/{goal}/{file}"),
                format!("ls {goals}/"),
                format!("ls -la {goals}/{goal}/slices/{other} | wc -l"),
                format!("shipit validate build-plan {goals}/{goal}/slices/{other}/build-plan.md"),
                format!("cp ../{file} {goals}/{goal}/{file}"),
                format!("cp {goals}/{goal}/{file} {goals}/{other}/{file}"),
                format!("cp a '{goals}/{goal}/{file}'"),
            ] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
            for command in [
                format!("cp x {goals}/../../hooks/{other}"),
                format!("cp x {goals}/{goal}/../../../hooks/{other}"),
                format!("cat {goals}/{goal}/.."),
                format!("cp x {goals}/{goal}/{file} ../{other}"),
                format!("cp x {root}/.git/shipit/receipts/check/{file}"),
                format!("cp x {goals}/{goal}/receipts/{file}"),
                format!("cat {goals}/{goal}/{file} {root}/.git/config"),
                format!("cat {root}/.git/config {goals}/{goal}/{file}"),
                format!("cat {goals}x/{file}"),
                format!("ls {goals}"),
                format!("cp x {root}/.git/hooks/{other}"),
                format!("cp x {root}/.g'it'/hooks/{other}"),
                format!("cp x '{root}/.git'/shipit/goals/../hooks/{other}"),
                format!("echo x > {root}/.git/shipit/goals/../hooks/{other}"),
                // `.git` as a path part of its own.
                format!("cd {root}/.git && cp x hooks/{other}"),
                "cd .git && ls".to_owned(),
                format!("git --git-dir={root}/.git status"),
                format!("cp x {root}/.git"),
            ] {
                prop_assert!(reasons(&command).contains(&DOT_GIT.to_owned()), "{}", command);
            }
            // A link made in goals/ reaches the hooks, an archive can hold one: only copies and reads run.
            for command in [
                format!("ln -s ../../../hooks {goals}/{goal}/h"),
                format!("ln -s ../../hooks {goals}/{other}"),
                format!("tar -xf x.tar -C {goals}/{goal}"),
                format!("rsync -a x {goals}/{goal}/"),
                format!("mv x {goals}/{goal}/{file}"),
                format!("install x {goals}/{goal}/{file}"),
                format!("tee {goals}/{goal}/{file}"),
                format!("cat x > {goals}/{goal}/{file}"),
                format!("cat x >> {goals}/{goal}/{file}"),
                format!("cat x &> {goals}/{goal}/{file}"),
                format!("cp -rL x {goals}/{goal}/"),
                format!("cp -r x {goals}/{goal}/"),
                format!("cp -R x {goals}/{goal}/"),
                format!("cp -a x {goals}/{goal}/"),
                format!("cp -s ../../hooks {goals}/{goal}/h"),
                format!("cp -ls x {goals}/{goal}/h"),
                format!("cp --dereference x {goals}/{goal}/"),
                format!("cp --symbolic-link x {goals}/{goal}/"),
                format!("cp -P x {goals}/{goal}/h"),
                format!("cp -d x {goals}/{goal}/h"),
                format!("cp -pP x {goals}/{goal}/h"),
                format!("cp --preserve=links x {goals}/{goal}/h"),
                format!("cp --no-dereference x {goals}/{goal}/h"),
                format!("cp --parents a/b/../../x {goals}/{goal}/"),
                format!("cp -u x {goals}/{goal}/h"),
                format!("env cp x {goals}/{goal}/{file}"),
                format!("git status && cp x {goals}/{goal}/{file}"),
                format!("cp x {goals}/{goal}/{file} && ln -s a b"),
            ] {
                prop_assert!(reasons(&command).contains(&GOALS_ONLY.to_owned()), "{}", command);
            }
            for command in [
                format!("cat {goals}/{goal}/{file} 2>&1"),
                format!("ls {goals}/ 2>/dev/null"),
                format!("cp x {goals}/{goal}/{file} && ls -R {goals}/ | head -n 3"),
                format!("cp -p -t {goals}/{goal}/ x"),
                format!("cp -pfnivT x {goals}/{goal}/{file}"),
                format!("cp --target-directory={goals}/{goal}/ x"),
                format!("cp -- x {goals}/{goal}/{file}"),
            ] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
            // A name that only holds the letters is no path part, and a message is text.
            for command in [
                format!("git add .gitignore .github/{file}"),
                format!("git fetch https://example.com/{other}.git"),
                format!("git log -- ..git {other}.gitx"),
                format!("git commit -m 'the .git folder, and {other}.git'"),
                format!("ls .git {other}"),
            ] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
        }

        #[test]
        fn harmless_git_reads_and_edits_pass_and_the_forms_that_write_ask(
            reference in "[a-z]{1,6}[0-9]",
            file in "[a-z]{1,6}[.]patch",
        ) {
            for command in [
                "git reflog".to_owned(),
                format!("git reflog show {reference}"),
                format!("git reflog -n 5 {reference}@{{1}}"),
                "git reflog exists HEAD".to_owned(),
                format!("git ls-tree -r {reference}"),
                "git show-ref --heads".to_owned(),
                format!("git cherry -v {reference}"),
                format!("git range-diff {reference}...{reference} {reference}"),
                format!("git check-ignore -v {reference}"),
                "git symbolic-ref HEAD".to_owned(),
                "git symbolic-ref --short -q HEAD".to_owned(),
                "git help log".to_owned(),
                "git version".to_owned(),
                "git --version".to_owned(),
                format!("git apply --check {file}"),
                format!("git apply --index --check {file}"),
                format!("git apply --stat --numstat --summary {file}"),
                format!("git clean -fdn {reference}"),
                format!("git init -b {reference} d"),
                // A read of a symbolic ref moves nothing, so a push may follow it.
                "git symbolic-ref --short HEAD && git push -u origin slice/v1/x".to_owned(),
                "git branch".to_owned(),
                format!("git branch -avv --contains {reference}"),
                "git branch --show-current".to_owned(),
                format!("git branch {reference} HEAD"),
                "git fetch".to_owned(),
                format!("git fetch --prune origin {reference}"),
                format!("git fetch https://example.com/{reference}.git"),
            ] {
                prop_assert_eq!(reasons(&command), Vec::<String>::new(), "{}", command);
            }
            for command in [
                format!("git reflog expire --all {reference}"),
                format!("git reflog delete {reference}@{{1}}"),
                format!("git symbolic-ref HEAD refs/heads/{reference}"),
                "git symbolic-ref -d HEAD".to_owned(),
                format!("git symbolic-ref -m {reference} HEAD"),
                "git symbolic-ref --delete HEAD".to_owned(),
                format!("git apply {file}"),
                format!("git apply --index {file}"),
                format!("git apply --stat --apply {file}"),
                format!("git apply --unsafe-paths --check --apply {file}"),
                format!("git init --separate-git-dir={reference} d"),
                format!("git init --separate-git-dir {reference}"),
                format!("git init --template={reference} d"),
                // What can empty the rule that a repository is a project.
                format!("git branch -d {reference}"),
                format!("git branch -D {reference}"),
                format!("git branch -rd origin/{reference}"),
                format!("git branch -m main {reference}"),
                format!("git branch -M {reference}"),
                format!("git branch -c main {reference}"),
                format!("git branch -C main {reference}"),
                format!("git branch --delete {reference}"),
                format!("git branch --move main {reference}"),
                format!("git branch --copy main {reference}"),
                format!("git branch -f main {reference}"),
                format!("git fetch . +{reference}:refs/remotes/origin/main"),
                format!("git fetch origin {reference}:{reference}"),
                format!("git init --template {reference}"),
                format!("git symbolic-ref HEAD {reference} && git push -u origin slice/v1/x"),
            ] {
                prop_assert!(asks(&command), "{}", command);
                prop_assert!(denies_in_bypass(&command), "{}", command);
            }
        }

        // A `cd` to another folder never takes the working folder out of the judged ones.
        #[test]
        fn the_working_folder_and_every_folder_passed_are_judged(
            a in "[a-z]{1,5}[0-9]",
            b in "[a-z]{1,5}[0-9]",
        ) {
            prop_assume!(a != b);
            let dirs = |command: String| judged_dirs(&command);
            for command in [
                format!("cd /{a} && git push origin x"),
                format!("cd /{a} && bash -c 'cd /{b} && git push origin main'"),
                format!("cd /{a} && gh pr merge 1 -R o/r"),
                format!("cd /{a} && shipit complete d --confirm"),
                format!("cd /{a} && cp x /{b}/.git/hooks/h"),
            ] {
                prop_assert_eq!(dirs(command.clone()), vec![None, Some(format!("/{a}"))], "{}", command);
            }
            prop_assert_eq!(dirs(format!("cd ~/{a} && git log")), vec![None, Some(format!("~/{a}"))]);
            prop_assert_eq!(
                dirs(format!("git status && cd /{a} && git log && cd {b} && git log")),
                vec![None, Some(format!("/{a}")), Some(format!("/{a}/{b}"))]
            );
            // Unreadable, or no folder passed: the working folder.
            prop_assert_eq!(dirs(format!("cd /{a} && git push $x")), vec![None]);
            prop_assert_eq!(dirs(format!("git push origin {a}")), vec![None]);
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
                "push.default=upstream".to_owned(),
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
            // git runs programs of its own; only the subcommands and options on the list pass.
            "git -c core.fsmonitor='g\"\"it push origin main #' status",
            "git -c diff.external='g\"\"it push origin main #' diff",
            "git --config-env=core.pager=X log",
            "git --exec-path=/x status",
            "git rebase --exec 'g\"\"it push origin main' HEAD~1",
            "git rebase -x 'sh' HEAD~1",
            "git rebase -i HEAD~2",
            "git submodule foreach 'g\"\"it push origin main'",
            "git bisect run sh",
            "git filter-branch --tree-filter x",
            "git difftool -x sh",
            "git fetch --upload-pack='sh' /elsewhere",
            "git ls-remote -u sh /elsewhere",
            "git grep -Osh x",
            "git merge -s x main",
            "git psuh origin main",
            "git p origin main",
            "git clone /x y",
            "git pull",
            "git tag v1",
            "git tag -d v1",
            "git notes add -m x",
            "git config --get x --unset y",
            // A word that is a whole command line is read by the same lists.
            "nix-shell --run 'git submodule foreach x'",
            "docker run img 'git -c core.pager=x log'",
            // A write under .git/ is a redirect the words do not show.
            "printf '[help]\n\tautocorrect = -1\n' >> .git/config",
            "echo x >> .git/hooks/pre-push",
            "git status; cat y > .git/info/exclude",
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
            "nix-shell --run 'gh repo delete o/r --yes'",
            "nix-shell --run 'gh repo edit --default-branch x'",
            "nix-shell --run 'cd d; gh pr -R o/r merge 1'",
            "docker run img 'gh api -XPUT repos/o/r/pulls/1/merge'",
            "docker run img 'gh release new v1'",
            "docker run img 'git send-pack u slice/v1/x:refs/heads/main'",
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

    // Each unreadable form is asked about with its own cause; the texts are written out here so a
    // reworded cause fails.
    #[test]
    fn an_unreadable_command_is_asked_about_with_its_cause() {
        let braces = "a brace expansion (`{a,b}` or `{1..3}`)";
        let tilde = "a `~` other than in `cd ~/<folder>`";
        let group = "a subshell, a group or a shell keyword (`(`, `{`, `!`, `if`, `for`, `while`, `case`, `function` and the like)";
        let expansion = "a `$` or a backquote other than `$?`";
        let comment = "a comment (a word that starts with `#`)";
        let backslash = "a backslash outside quotes";
        let unclosed = "a quote or a heredoc that is not closed";
        let heredoc = "a heredoc ShipIt does not read (`<<<`, no delimiter, or an unquoted delimiter whose text has `$`, a backquote or a backslash)";
        let redirect = "a redirect ShipIt does not read (`<(`, `>|`, `<>` or no target)";
        let folder = "a folder ShipIt does not follow (`cd` alone, with `-`, an option, a glob, `$?` or two folders, a `cd` that may not run, `pushd`, `popd`, or a git `-C`, `--git-dir` or `--work-tree` that holds `$?`)";
        for (command, cause) in [
            ("git log {a,b}", braces),
            ("git log {1..3}", braces),
            ("git add ~/x", tilde),
            ("git { ls; }", group),
            ("git status; (ls)", group),
            ("git status; if ls; then ls; fi", group),
            ("git status; echo $(date)", expansion),
            ("git status; echo `date`", expansion),
            ("git status # x", comment),
            ("git status \\x", backslash),
            ("git log 'x", unclosed),
            ("git status; cat <<< x", heredoc),
            ("git status; cat <<EOF\n$x\nEOF\n", heredoc),
            ("git status; ls >| x", redirect),
            ("git status; ls <(x)", redirect),
            ("git status; cd -", folder),
            ("git status; pushd x", folder),
        ] {
            assert_eq!(
                reasons(command),
                [format!(
                    "cannot be read as plain commands by ShipIt: it holds {cause}; it may ship (write plain commands, and name branches in plain text)"
                )],
                "{command}"
            );
        }
    }

    // A write to the config is one reason, whatever the key says.
    #[test]
    fn a_config_write_says_it_changes_the_config() {
        for command in [
            "git config alias.p push",
            "git config help.autocorrect immediate",
            "git config remote.origin.push x",
            "git config --global core.pager x",
        ] {
            assert_eq!(reasons(command), ["changes git config"], "{command}");
        }
        assert_eq!(reasons("git -c alias.p=push p"), [GLOBAL_OPTION]);
    }

    // A local merge into a base branch is the shipping itself, so it asks; into a slice, it passes.
    #[test]
    fn a_merge_asks_when_the_branch_checked_out_is_a_base() {
        let on = |branch: &str| {
            let mut here = facts();
            here.current = Some(branch.to_owned());
            move |command: &str| ship_reasons(command, &|_dir| here.clone())
        };
        for base in ["main", "master"] {
            assert_eq!(on(base)("git merge slice/v1/x"), [MERGES_BASE]);
        }
        assert_eq!(on("slice/v1/x")("git merge main"), Vec::<String>::new());
        let mut detached = facts();
        detached.current = None;
        assert_eq!(
            ship_reasons("git merge main", &|_dir| detached.clone()),
            Vec::<String>::new()
        );
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
