use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::{Serialize, Serializer};

use crate::pure::repo_path::RepoPath;
use crate::pure::setup::{SETUP_PATH, Setup};
use crate::pure::text::comma_list;

// The owner's list: the lock files of the package managers ShipIt meets most.
pub(crate) const LOCK_FILES: [&str; 9] = [
    "Cargo.lock",
    "flake.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "poetry.lock",
    "uv.lock",
    "go.sum",
    "Gemfile.lock",
];

// A path written like a `RepoPath`, where a `*` inside one part matches any run of characters
// but `/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct RiskPath(String);

fn part_matches(pattern: &[u8], name: &[u8]) -> bool {
    let mut pieces = pattern.split(|byte| *byte == b'*');
    let Some(first) = pieces.next() else {
        return pattern == name;
    };
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let middle: Vec<&[u8]> = pieces.collect();
    let Some((last, between)) = middle.split_last() else {
        return rest.is_empty();
    };
    for piece in between.iter().filter(|piece| !piece.is_empty()) {
        let Some(at) = rest
            .windows(piece.len())
            .position(|window| window == *piece)
        else {
            return false;
        };
        rest = rest.get(at + piece.len()..).unwrap_or_default();
    }
    rest.ends_with(last)
}

impl RiskPath {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        RepoPath::parse(&text.replace('*', "a"))?;
        Some(Self(text.strip_suffix('/').unwrap_or(text).to_owned()))
    }

    // Part by part, like `RepoPath::touches`: the path itself, below it, or a folder above it.
    pub(crate) fn touches(&self, name: &[u8]) -> bool {
        if name.is_empty() {
            return false;
        }
        let pattern: Vec<&[u8]> = self.0.as_bytes().split(|byte| *byte == b'/').collect();
        let parts: Vec<&[u8]> = name.split(|byte| *byte == b'/').collect();
        pattern
            .iter()
            .zip(&parts)
            .all(|(want, part)| part_matches(want, part))
    }
}

// One file name, no folder: a lock file anywhere in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct LockName(String);

impl LockName {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        (!text.is_empty() && !text.contains('/') && text != "." && text != "..")
            .then(|| Self(text.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Level {
    Light,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Basis {
    Owns,
    Diff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reason {
    RiskyPaths(Vec<String>),
    LockFiles(Vec<String>),
    Files { count: u64, light: u64 },
    MappedTests(Vec<String>),
    UnreadableMaps(Vec<String>),
    BaseGiven(String),
    OtherBase { checked: String, policy: String },
}

// At most three names, then how many more.
fn listed(names: &[String], one: &str, many: &str) -> String {
    let word = if names.len() == 1 { one } else { many };
    let mut text = format!("{word} {}", comma_list(names.iter().take(3)));
    if let Some(more) = names.len().checked_sub(3).filter(|more| *more > 0) {
        write!(text, " and {more} more").ok();
    }
    text
}

impl Reason {
    pub(crate) fn text(&self) -> String {
        match self {
            Self::RiskyPaths(names) => listed(names, "risky path", "risky paths"),
            Self::LockFiles(names) => listed(names, "lock file", "lock files"),
            Self::Files { count, light } => format!("{count} files, light allows {light}"),
            Self::MappedTests(names) => {
                let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
                let mut text = listed(&quoted, "mapped test", "mapped tests");
                text.push_str(" edited or removed");
                text
            }
            Self::UnreadableMaps(paths) => {
                let mut text = listed(paths, "coverage map", "coverage maps");
                text.push_str(" could not be read");
                text
            }
            Self::BaseGiven(name) => format!("check base given by hand: {name}"),
            Self::OtherBase { checked, policy } => {
                format!("checked against {checked}, not {policy}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Risk {
    basis: Basis,
    reasons: Vec<Reason>,
}

impl Risk {
    pub(crate) const fn basis(&self) -> Basis {
        self.basis
    }

    pub(crate) const fn level(&self) -> Level {
        if self.reasons.is_empty() {
            Level::Light
        } else {
            Level::Full
        }
    }

    pub(crate) fn reasons_text(&self) -> String {
        self.reasons
            .iter()
            .map(Reason::text)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Serialize)]
struct Shown {
    level: Level,
    from: Basis,
    reasons: Vec<String>,
}

impl Serialize for Risk {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Shown {
            level: self.level(),
            from: self.basis,
            reasons: self.reasons.iter().map(Reason::text).collect(),
        }
        .serialize(serializer)
    }
}

// The setup file and `gate.setup_files` are always risky; `review.full_paths` adds to them.
fn risky(setup: &Setup, name: &str) -> bool {
    let bytes = name.as_bytes();
    RepoPath::parse(SETUP_PATH).is_some_and(|path| path.touches(bytes))
        || setup
            .gate
            .setup_files
            .iter()
            .any(|path| path.touches(bytes))
        || setup
            .review
            .full_paths
            .iter()
            .any(|path| path.touches(bytes))
}

fn is_lock(setup: &Setup, name: &str) -> bool {
    let last = name.rsplit('/').next().unwrap_or(name);
    LOCK_FILES.contains(&last)
        || setup
            .review
            .lock_files
            .iter()
            .any(|lock| lock.as_str() == last)
}

fn path_reasons(setup: &Setup, names: &[String], count: u64) -> Vec<Reason> {
    let risky_paths: Vec<String> = names
        .iter()
        .filter(|name| risky(setup, name))
        .cloned()
        .collect();
    let locks: Vec<String> = names
        .iter()
        .filter(|name| is_lock(setup, name))
        .cloned()
        .collect();
    let light = setup.review.light_max_files.get();
    let mut reasons = Vec::new();
    if !risky_paths.is_empty() {
        reasons.push(Reason::RiskyPaths(risky_paths));
    }
    if !locks.is_empty() {
        reasons.push(Reason::LockFiles(locks));
    }
    if count > light {
        reasons.push(Reason::Files { count, light });
    }
    reasons
}

// Before the work: owns name files and folders, not tests, so no mapped test can count here.
pub(crate) fn estimate(setup: &Setup, owns: &[RepoPath]) -> Risk {
    let names: Vec<String> = owns.iter().map(ToString::to_string).collect();
    let count = u64::try_from(owns.len()).unwrap_or(u64::MAX);
    Risk {
        basis: Basis::Owns,
        reasons: path_reasons(setup, &names, count),
    }
}

// What status reads for a checked version; `other_base` is the receipt's base and the policy's,
// set only when they differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffFacts {
    pub(crate) names: Vec<String>,
    pub(crate) files: u64,
    pub(crate) mapped: Vec<String>,
    pub(crate) unreadable: Vec<String>,
    pub(crate) given_base: Option<String>,
    pub(crate) other_base: Option<(String, String)>,
}

// After `shipit check`: the changed names, the receipt's file count, and what status could not
// judge, each make the slice full.
pub(crate) fn judged(setup: &Setup, diff: &DiffFacts) -> Risk {
    let mut reasons = path_reasons(setup, &diff.names, diff.files);
    if !diff.mapped.is_empty() {
        reasons.push(Reason::MappedTests(diff.mapped.clone()));
    }
    if !diff.unreadable.is_empty() {
        reasons.push(Reason::UnreadableMaps(diff.unreadable.clone()));
    }
    if let Some(name) = &diff.given_base {
        reasons.push(Reason::BaseGiven(name.clone()));
    }
    if let Some((checked, policy)) = &diff.other_base {
        reasons.push(Reason::OtherBase {
            checked: checked.clone(),
            policy: policy.clone(),
        });
    }
    Risk {
        basis: Basis::Diff,
        reasons,
    }
}

// The old side of one `git diff -U0` hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hunk {
    pub(crate) start: u64,
    pub(crate) count: u64,
}

fn number(text: &str) -> Option<u64> {
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn range(text: &str) -> Option<(u64, u64)> {
    match text.split_once(',') {
        Some((start, count)) => Some((number(start)?, number(count)?)),
        None => Some((number(text)?, 1)),
    }
}

pub(crate) fn hunk(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _tail) = rest.split_once(" @@")?;
    let (start, count) = range(old)?;
    range(new)?;
    Some(Hunk { start, count })
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

// The name occurs with no word character next to it, at an end where the name has one.
fn names_test(line: &str, name: &str) -> bool {
    let check_before = name.starts_with(is_word);
    let check_after = name.ends_with(is_word);
    line.match_indices(name).any(|(at, _)| {
        let before = line.get(..at).and_then(|head| head.chars().next_back());
        let after = line
            .get(at + name.len()..)
            .and_then(|tail| tail.chars().next());
        let joined_before = check_before && before.is_some_and(is_word);
        let joined_after = check_after && after.is_some_and(is_word);
        !joined_before && !joined_after
    })
}

struct Line<'a> {
    indent: usize,
    text: &'a str,
}

impl Line<'_> {
    fn blank(&self) -> bool {
        self.text.is_empty()
    }
}

fn lines_of(base: &str) -> Vec<Line<'_>> {
    base.lines()
        .map(|line| {
            let text = line.trim_start_matches([' ', '\t']);
            Line {
                indent: line.len() - text.len(),
                text: text.trim_end(),
            }
        })
        .collect()
}

// Indexes from 0; the span is (first, last), both included.
fn span(lines: &[Line<'_>], at: usize) -> (usize, usize) {
    let Some(named) = lines.get(at) else {
        return (at, at);
    };
    let indent = named.indent;
    let at_indent = |index: &usize, prefix: &[&str]| {
        lines.get(*index).is_some_and(|line| {
            line.indent == indent && prefix.iter().any(|start| line.text.starts_with(start))
        })
    };
    let mut first = at;
    while first > 0 && at_indent(&(first - 1), &["#[", "@"]) {
        first -= 1;
    }
    let last_text = lines.iter().rposition(|line| !line.blank()).unwrap_or(at);
    let signature = if named.text.ends_with('(') {
        (at + 1..lines.len())
            .find(|index| at_indent(index, &[")"]))
            .unwrap_or(at)
    } else {
        at
    };
    let braced = lines
        .get(signature)
        .is_some_and(|line| line.text.ends_with('{'));
    let last = if braced {
        (signature + 1..lines.len())
            .find(|index| at_indent(index, &["}"]))
            .unwrap_or(last_text)
    } else {
        (signature + 1..lines.len())
            .find(|index| {
                lines.get(*index).is_some_and(|line| {
                    !line.blank()
                        && line.indent <= indent
                        && ![")", "]", "}"]
                            .iter()
                            .any(|start| line.text.starts_with(start))
                })
            })
            .and_then(|cut| {
                (signature..cut)
                    .rev()
                    .find(|index| lines.get(*index).is_some_and(|line| !line.blank()))
            })
            .unwrap_or(last_text)
    };
    (first, last.max(signature))
}

// Line numbers from 1, as git writes them.
fn touches_span(hunk: Hunk, (first, last): (usize, usize)) -> bool {
    let (Ok(first), Ok(last)) = (u64::try_from(first), u64::try_from(last)) else {
        return false;
    };
    let (first, last) = (first + 1, last + 1);
    let inside = |line: u64| (first..=last).contains(&line);
    if hunk.count == 0 {
        inside(hunk.start) && inside(hunk.start.saturating_add(1))
    } else {
        hunk.start <= last && hunk.start.saturating_add(hunk.count) > first
    }
}

pub(crate) fn touched_tests(base: &str, hunks: &[Hunk], mapped: &BTreeSet<String>) -> Vec<String> {
    let lines = lines_of(base);
    let mut found = BTreeSet::new();
    for name in mapped {
        let spans: Vec<(usize, usize)> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| names_test(line.text, name))
            .map(|(at, _)| span(&lines, at))
            .collect();
        let touched = !spans.is_empty()
            && (hunks.is_empty()
                || spans
                    .iter()
                    .any(|span| hunks.iter().any(|hunk| touches_span(*hunk, *span))));
        if touched {
            found.insert(name.clone());
        }
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use toml::Value;

    use super::*;
    use crate::pure::testing::setup_with;

    fn config() -> ProptestConfig {
        ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    fn joined(parts: &[&str], slash: bool) -> String {
        let mut text = parts.join("/");
        if slash {
            text.push('/');
        }
        text
    }

    fn small_path() -> impl Strategy<Value = String> {
        (
            proptest::collection::vec(proptest::sample::select(vec!["a", "b", "ab", "a.b"]), 1..4),
            any::<bool>(),
        )
            .prop_map(|(parts, slash)| joined(&parts, slash))
    }

    // The path, whether it is risky by the test setup, and whether it is a lock file.
    const UNIVERSE: [(&str, bool, bool); 11] = [
        (".config/shipit.toml", true, false),
        (".config/", true, false),
        ("deny.toml", true, false),
        ("docs/", true, false),
        ("docs/a.md", true, false),
        ("src/a.rs", false, false),
        ("src/b1.rs", true, false),
        ("Cargo.lock", false, true),
        ("web/yarn.lock", false, true),
        ("x/deps.lock", false, true),
        ("x/", false, false),
    ];

    fn test_setup(light: u64) -> Setup {
        let review: toml::Table = toml::from_str(&format!(
            "full_paths = [\"docs/\", \"src/b*.rs\"]\nlight_max_files = {light}\nlock_files = [\"deps.lock\"]"
        ))
        .unwrap();
        setup_with(&[
            (
                "gate.setup_files",
                Value::Array(vec![Value::String("deny.toml".to_owned())]),
            ),
            ("review", Value::Table(review)),
        ])
    }

    fn shown(path: &str) -> String {
        RepoPath::parse(path).unwrap().to_string()
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn a_plain_risk_path_touches_as_a_repo_path_does(path in small_path(), name in small_path()) {
            let risk = RiskPath::parse(&path).unwrap();
            let repo = RepoPath::parse(&path).unwrap();
            prop_assert_eq!(risk.touches(name.as_bytes()), repo.touches(name.as_bytes()));
            let docs = RiskPath::parse("docs/").unwrap();
            prop_assert!(docs.touches(b"docs/goal.md") && docs.touches(b"docs"));
            prop_assert!(!docs.touches(b"docs2/x"));
        }

        #[test]
        fn a_star_matches_within_one_part(x in "[a-z._]{0,4}", y in "[a-z._]{0,4}", m in "[a-z._]{0,4}") {
            let pattern = RiskPath::parse(&format!("src/{x}*{y}")).unwrap();
            let (inside, below) = (format!("src/{x}{m}{y}"), format!("src/{x}{m}{y}/z"));
            let (split, elsewhere) = (format!("src/{x}{m}/{y}"), format!("lib/{x}{m}{y}"));
            prop_assert!(pattern.touches(inside.as_bytes()));
            prop_assert!(pattern.touches(below.as_bytes()));
            prop_assert!(pattern.touches(b"src"));
            // A `/` in the run is no match, unless the part before it matches on its own.
            let head = format!("{x}{m}");
            let head_matches = head.len() >= x.len() + y.len() && head.ends_with(y.as_str());
            prop_assert_eq!(pattern.touches(split.as_bytes()), head_matches);
            prop_assert!(!pattern.touches(elsewhere.as_bytes()));
            let claude = RiskPath::parse("src/commands/claude_*.rs").unwrap();
            prop_assert!(claude.touches(b"src/commands/claude_agent_gate.rs"));
            prop_assert!(!claude.touches(b"src/pure/claude_hook.rs"));
        }

        #[test]
        fn a_risk_path_parses_as_a_repo_path_does_with_a_star(text in "[a*/. ?\\[\\\\]{0,8}") {
            prop_assert_eq!(
                RiskPath::parse(&text).is_some(),
                RepoPath::parse(&text.replace('*', "a")).is_some()
            );
            for good in ["src/", "claude_*.rs"] {
                prop_assert!(RiskPath::parse(good).is_some(), "{}", good);
            }
            for bad in ["", "/a", "a//b", "a/..", "a?", "[a]"] {
                prop_assert!(RiskPath::parse(bad).is_none(), "{}", bad);
            }
        }

        #[test]
        fn the_estimate_names_each_reason_from_owns(
            owns in proptest::sample::subsequence(UNIVERSE.to_vec(), 0..=UNIVERSE.len()),
            light in 0_u64..6,
        ) {
            let paths: Vec<RepoPath> = owns.iter().map(|(path, _, _)| RepoPath::parse(path).unwrap()).collect();
            let risk = estimate(&test_setup(light), &paths);
            let mut expected = Vec::new();
            let risky: Vec<String> = owns.iter().filter(|(_, risky, _)| *risky).map(|(path, _, _)| shown(path)).collect();
            if !risky.is_empty() {
                expected.push(Reason::RiskyPaths(risky));
            }
            let locks: Vec<String> = owns.iter().filter(|(_, _, lock)| *lock).map(|(path, _, _)| shown(path)).collect();
            if !locks.is_empty() {
                expected.push(Reason::LockFiles(locks));
            }
            let count = u64::try_from(owns.len()).unwrap();
            if count > light {
                expected.push(Reason::Files { count, light });
            }
            let level = if expected.is_empty() { Level::Light } else { Level::Full };
            prop_assert_eq!(risk.level(), level);
            prop_assert_eq!(risk, Risk { basis: Basis::Owns, reasons: expected });
            let light = estimate(&test_setup(10), &[RepoPath::parse("src/a.rs").unwrap()]);
            prop_assert_eq!(light.level(), Level::Light);
            prop_assert_eq!(light.reasons_text(), "");
        }
    }

    #[test]
    fn a_lock_name_is_one_file_name() {
        assert!(LockName::parse("deps.lock").is_some());
        for bad in ["", "a/b", ".", ".."] {
            assert!(LockName::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_reason_texts() {
        let names = |list: &[&str]| {
            list.iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(Reason::RiskyPaths(names(&["a"])).text(), "risky path a");
        assert_eq!(
            Reason::RiskyPaths(names(&["a", "b", "c"])).text(),
            "risky paths a, b, c"
        );
        assert_eq!(
            Reason::RiskyPaths(names(&["a", "b", "c", "d", "e"])).text(),
            "risky paths a, b, c and 2 more"
        );
        assert_eq!(
            Reason::LockFiles(names(&["Cargo.lock"])).text(),
            "lock file Cargo.lock"
        );
        assert_eq!(
            Reason::LockFiles(names(&["Cargo.lock", "flake.lock"])).text(),
            "lock files Cargo.lock, flake.lock"
        );
        assert_eq!(
            Reason::Files {
                count: 14,
                light: 10
            }
            .text(),
            "14 files, light allows 10"
        );
        let risk = Risk {
            basis: Basis::Owns,
            reasons: vec![
                Reason::RiskyPaths(names(&["a"])),
                Reason::Files {
                    count: 14,
                    light: 10,
                },
            ],
        };
        assert_eq!(
            risk.reasons_text(),
            "risky path a; 14 files, light allows 10"
        );
        assert_eq!(
            Reason::MappedTests(names(&["one"])).text(),
            "mapped test \"one\" edited or removed"
        );
        assert_eq!(
            Reason::MappedTests(names(&["a", "b", "c", "d"])).text(),
            "mapped tests \"a\", \"b\", \"c\" and 1 more edited or removed"
        );
        assert_eq!(
            Reason::UnreadableMaps(names(&["m.toml"])).text(),
            "coverage map m.toml could not be read"
        );
        assert_eq!(
            Reason::BaseGiven("main".to_owned()).text(),
            "check base given by hand: main"
        );
        assert_eq!(
            Reason::OtherBase {
                checked: "side".to_owned(),
                policy: "main".to_owned()
            }
            .text(),
            "checked against side, not main"
        );
    }

    // One test block of a file: its kind (0 Rust-like, 1 bats-like, 2 indent-only), its name,
    // its body lines (digits, and whether the line is at column 0), a multi-line signature
    // (Rust-like only), and the blank lines after it.
    #[derive(Debug, Clone)]
    struct Block {
        kind: u8,
        name: String,
        body: Vec<(String, bool)>,
        multi: bool,
        blanks: usize,
    }

    impl Block {
        fn lines(&self) -> Vec<String> {
            let body = |indent: &str| {
                self.body
                    .iter()
                    .map(|(digits, column_0)| {
                        if *column_0 && self.kind != 2 {
                            digits.clone()
                        } else {
                            format!("{indent}{digits}")
                        }
                    })
                    .collect::<Vec<_>>()
            };
            let name = &self.name;
            let mut lines = Vec::new();
            match self.kind {
                0 => {
                    lines.push("    #[test]".to_owned());
                    if self.multi {
                        lines.push(format!("    fn {name}("));
                        lines.push("        x: u8,".to_owned());
                        lines.push("    ) {".to_owned());
                    } else {
                        lines.push(format!("    fn {name}() {{"));
                    }
                    lines.extend(body("        "));
                    lines.push("    }".to_owned());
                }
                1 => {
                    lines.push(format!("@test \"{name} x\" {{"));
                    lines.extend(body("  "));
                    lines.push("}".to_owned());
                }
                _ => {
                    lines.push(format!("def {name}():"));
                    lines.extend(body("    "));
                }
            }
            lines
        }
    }

    // Blocks with unique names. An indent-only block comes last: its span ends only at a line
    // at its own indent or less, and a Rust-like block starts at indent 4.
    fn blocks() -> impl Strategy<Value = Vec<Block>> {
        proptest::collection::btree_set("t_[a-z]{3,6}", 1..=5).prop_flat_map(|names| {
            let count = names.len();
            let parts = proptest::collection::vec(
                (
                    0_u8..3,
                    proptest::collection::vec(("[0-9]{1,6}", any::<bool>()), 1..=3),
                    any::<bool>(),
                    1_usize..=2,
                ),
                count..=count,
            );
            (Just(names.into_iter().collect::<Vec<_>>()), parts).prop_map(|(names, parts)| {
                let mut blocks: Vec<Block> = names
                    .into_iter()
                    .zip(parts)
                    .map(|(name, (kind, mut body, multi, blanks))| {
                        // At most one column-0 line per block.
                        let mut seen = false;
                        for (_, column_0) in &mut body {
                            *column_0 = *column_0 && !seen;
                            seen = seen || *column_0;
                        }
                        Block {
                            kind,
                            name,
                            body,
                            multi,
                            blanks,
                        }
                    })
                    .collect();
                blocks.sort_by_key(|block| block.kind);
                blocks
            })
        })
    }

    // The file, each block's first and last line (from 1), and the blank lines.
    fn file_of(blocks: &[Block]) -> (String, Vec<(u64, u64)>, Vec<u64>) {
        let mut text = String::new();
        let mut spans = Vec::new();
        let mut blank = Vec::new();
        let mut at = 0_u64;
        for block in blocks {
            let lines = block.lines();
            let first = at + 1;
            for line in &lines {
                text.push_str(line);
                text.push('\n');
                at += 1;
            }
            spans.push((first, at));
            for _ in 0..block.blanks {
                text.push('\n');
                at += 1;
                blank.push(at);
            }
        }
        (text, spans, blank)
    }

    fn universe_names(picked: &[(&str, bool, bool)]) -> Vec<String> {
        picked.iter().map(|(path, _, _)| shown(path)).collect()
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn a_hunk_header_reads_back(
            (a, b, c, d) in (0_u64..10_000, 0_u64..10_000, 0_u64..10_000, 0_u64..10_000),
            tail in "[ -~]{0,8}",
            junk in "[a-z+-]{1,3}",
        ) {
            prop_assert_eq!(hunk(&format!("@@ -{a},{b} +{c},{d} @@{tail}")), Some(Hunk { start: a, count: b }));
            prop_assert_eq!(hunk(&format!("@@ -{a} +{c},{d} @@{tail}")), Some(Hunk { start: a, count: 1 }));
            prop_assert_eq!(hunk(&format!("@@ -{junk},{b} +{c},{d} @@")), None);
            prop_assert_eq!(hunk(&format!("@@ -{a},{junk} +{c} @@")), None);
            prop_assert_eq!(hunk(&format!("@ -{a},{b} +{c},{d} @@")), None);
            prop_assert_eq!(hunk(&format!("x{tail}@@ -{a},{b} +{c},{d} @@")), None);
            prop_assert_eq!(hunk("@@ -3,0 +4,2 @@"), Some(Hunk { start: 3, count: 0 }));
            prop_assert_eq!(hunk("@@ -7 +7 @@"), Some(Hunk { start: 7, count: 1 }));
        }

        #[test]
        fn a_change_inside_a_mapped_test_names_it(
            blocks in blocks(),
            picks in proptest::collection::vec(any::<bool>(), 5),
        ) {
            let (text, spans, blank) = file_of(&blocks);
            let mapped: BTreeSet<String> = blocks
                .iter()
                .zip(&picks)
                .filter(|(_, pick)| **pick)
                .map(|(block, _)| block.name.clone())
                .collect();
            let only = |block: &Block| -> Vec<String> {
                if mapped.contains(&block.name) { vec![block.name.clone()] } else { Vec::new() }
            };
            for (block, (first, last)) in blocks.iter().zip(&spans) {
                for line in *first..=*last {
                    prop_assert_eq!(touched_tests(&text, &[Hunk { start: line, count: 1 }], &mapped), only(block), "{}", line);
                    prop_assert_eq!(touched_tests(&text, &[Hunk { start: line, count: last - line + 1 }], &mapped), only(block));
                }
                for after in *first..*last {
                    prop_assert_eq!(touched_tests(&text, &[Hunk { start: after, count: 0 }], &mapped), only(block));
                }
                prop_assert_eq!(touched_tests(&text, &[Hunk { start: *last, count: 0 }], &mapped), Vec::<String>::new());
            }
            for line in blank {
                prop_assert_eq!(touched_tests(&text, &[Hunk { start: line, count: 0 }], &mapped), Vec::<String>::new());
            }
            let every: Vec<String> = mapped.iter().cloned().collect();
            let total = u64::try_from(text.lines().count()).unwrap();
            prop_assert_eq!(touched_tests(&text, &[Hunk { start: 1, count: total }], &mapped), every.clone());
            prop_assert_eq!(touched_tests(&text, &[], &mapped), every);
        }

        #[test]
        fn the_diff_decides_the_level(
            names in proptest::sample::subsequence(UNIVERSE.to_vec(), 0..=UNIVERSE.len()),
            files in 0_u64..15,
            light in 0_u64..6,
            mapped in proptest::collection::vec("[a-z]{1,4}", 0..=4),
            unreadable in proptest::collection::vec("[a-z]{1,4}", 0..=4),
            given in any::<bool>(),
            other in any::<bool>(),
        ) {
            let diff = DiffFacts {
                names: universe_names(&names),
                files,
                mapped: mapped.clone(),
                unreadable: unreadable.clone(),
                given_base: given.then(|| "main".to_owned()),
                other_base: other.then(|| ("side".to_owned(), "main".to_owned())),
            };
            let risk = judged(&test_setup(light), &diff);
            let mut expected = Vec::new();
            let risky: Vec<String> = names.iter().filter(|(_, risky, _)| *risky).map(|(path, _, _)| shown(path)).collect();
            if !risky.is_empty() {
                expected.push(Reason::RiskyPaths(risky));
            }
            let locks: Vec<String> = names.iter().filter(|(_, _, lock)| *lock).map(|(path, _, _)| shown(path)).collect();
            if !locks.is_empty() {
                expected.push(Reason::LockFiles(locks));
            }
            if files > light {
                expected.push(Reason::Files { count: files, light });
            }
            if !mapped.is_empty() {
                expected.push(Reason::MappedTests(mapped));
            }
            if !unreadable.is_empty() {
                expected.push(Reason::UnreadableMaps(unreadable));
            }
            if given {
                expected.push(Reason::BaseGiven("main".to_owned()));
            }
            if other {
                expected.push(Reason::OtherBase { checked: "side".to_owned(), policy: "main".to_owned() });
            }
            let level = if expected.is_empty() { Level::Light } else { Level::Full };
            prop_assert_eq!(risk.level(), level);
            prop_assert_eq!(risk, Risk { basis: Basis::Diff, reasons: expected });
            let plain = |names: &[&str], given_base: Option<&str>, other_base: Option<(&str, &str)>| {
                judged(&setup_with(&[]), &DiffFacts {
                    names: names.iter().map(|name| (*name).to_owned()).collect(),
                    files: u64::try_from(names.len()).unwrap(),
                    mapped: Vec::new(),
                    unreadable: Vec::new(),
                    given_base: given_base.map(str::to_owned),
                    other_base: other_base.map(|(a, b)| (a.to_owned(), b.to_owned())),
                })
            };
            let lock = plain(&["src/a.rs", "Cargo.lock"], None, None);
            prop_assert_eq!((lock.level(), lock.reasons_text()), (Level::Full, "lock file Cargo.lock".to_owned()));
            let given = plain(&["src/a.rs"], Some("main"), None);
            prop_assert_eq!(given.reasons_text(), "check base given by hand: main");
            let other = plain(&["src/a.rs"], None, Some(("side", "main")));
            prop_assert_eq!(other.reasons_text(), "checked against side, not main");
        }
    }
}
