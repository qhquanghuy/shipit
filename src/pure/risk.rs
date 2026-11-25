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
    // A `**` would read as crossing folders, which a `*` never does, so it is refused.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        if text.contains("**") {
            return None;
        }
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
    UnreadableFiles(Vec<String>),
    BaseGiven(String),
    OtherBase { checked: String, policy: String },
    Raised(String),
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
            Self::UnreadableFiles(paths) => {
                let mut text = listed(paths, "changed file", "changed files");
                text.push_str(" could not be read at the fork");
                text
            }
            Self::BaseGiven(name) => format!("check base given by hand: {name}"),
            Self::OtherBase { checked, policy } => {
                format!("checked against {checked}, not {policy}")
            }
            Self::Raised(reason) => format!("raised: {reason}"),
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

// The reasons are short texts; the edited mapped tests are also listed whole, for tools.
#[derive(Serialize)]
struct Shown<'a> {
    level: Level,
    from: Basis,
    reasons: Vec<String>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    mapped_tests: &'a [String],
}

impl Serialize for Risk {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Shown {
            level: self.level(),
            from: self.basis,
            reasons: self.reasons.iter().map(Reason::text).collect(),
            mapped_tests: self
                .reasons
                .iter()
                .find_map(|reason| match reason {
                    Reason::MappedTests(names) => Some(names.as_slice()),
                    Reason::RiskyPaths(_)
                    | Reason::LockFiles(_)
                    | Reason::Files { .. }
                    | Reason::UnreadableMaps(_)
                    | Reason::UnreadableFiles(_)
                    | Reason::BaseGiven(_)
                    | Reason::OtherBase { .. }
                    | Reason::Raised(_) => None,
                })
                .unwrap_or_default(),
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

// A raise only adds a reason, so no entry can lower the level.
fn with_raises(mut reasons: Vec<Reason>, raised: &[String]) -> Vec<Reason> {
    reasons.extend(raised.iter().cloned().map(Reason::Raised));
    reasons
}

// Before the work: owns name files and folders, not tests, so no mapped test can count here.
pub(crate) fn estimate(setup: &Setup, owns: &[RepoPath], raised: &[String]) -> Risk {
    let names: Vec<String> = owns.iter().map(ToString::to_string).collect();
    let count = u64::try_from(owns.len()).unwrap_or(u64::MAX);
    Risk {
        basis: Basis::Owns,
        reasons: with_raises(path_reasons(setup, &names, count), raised),
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
    pub(crate) unread_files: Vec<String>,
    pub(crate) given_base: Option<String>,
    pub(crate) other_base: Option<(String, String)>,
}

// After `shipit check`: the changed names, the receipt's file count, and what status could not
// judge, each make the slice full.
pub(crate) fn judged(setup: &Setup, diff: &DiffFacts, raised: &[String]) -> Risk {
    let mut reasons = path_reasons(setup, &diff.names, diff.files);
    if !diff.mapped.is_empty() {
        reasons.push(Reason::MappedTests(diff.mapped.clone()));
    }
    if !diff.unreadable.is_empty() {
        reasons.push(Reason::UnreadableMaps(diff.unreadable.clone()));
    }
    if !diff.unread_files.is_empty() {
        reasons.push(Reason::UnreadableFiles(diff.unread_files.clone()));
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
        reasons: with_raises(reasons, raised),
    }
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

// A name on its own argument line (`it(\n  "name",`) belongs to the call that opens above it.
fn anchor(lines: &[Line<'_>], at: usize) -> usize {
    let Some(named) = lines.get(at) else {
        return at;
    };
    if named.text.ends_with('(') || named.text.ends_with('{') {
        return at;
    }
    (0..at)
        .rev()
        .find(|index| {
            lines
                .get(*index)
                .is_some_and(|line| !line.blank() && line.indent < named.indent)
        })
        .filter(|index| {
            lines
                .get(*index)
                .is_some_and(|line| line.text.ends_with('('))
        })
        .unwrap_or(at)
}

// Indexes from 0; the span is (first, last), both included.
fn span(lines: &[Line<'_>], named_at: usize) -> (usize, usize) {
    let at = anchor(lines, named_at);
    let Some(named) = lines.get(at) else {
        return (at, at);
    };
    let indent = named.indent;
    let at_indent = |index: &usize, prefix: &[&str]| {
        lines.get(*index).is_some_and(|line| {
            line.indent == indent && prefix.iter().any(|start| line.text.starts_with(start))
        })
    };
    // Up over attributes, decorators and comments at the test's indent, the lines of a
    // multi-line attribute, and blank lines only when such a line sits above them.
    let opens_attribute = |index: &usize| {
        at_indent(index, &["#[", "@"])
            && lines
                .get(*index)
                .is_some_and(|line| line.text.ends_with('('))
    };
    let mut first = at;
    let mut probe = at;
    while let Some(above) = probe.checked_sub(1) {
        let Some(line) = lines.get(above) else {
            break;
        };
        let comment = ["//", "--", "#"]
            .iter()
            .any(|start| line.text.starts_with(start));
        probe = if line.blank() {
            above
        } else if line.indent != indent {
            break;
        } else if comment || line.text.starts_with('@') {
            first = above;
            above
        } else if line.text.starts_with(')') {
            let Some(opener) = (0..above)
                .rev()
                .find(|index| {
                    lines
                        .get(*index)
                        .is_some_and(|line| !line.blank() && line.indent <= indent)
                })
                .filter(opens_attribute)
            else {
                break;
            };
            first = opener;
            opener
        } else {
            break;
        };
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
    (first, last.max(signature).max(named_at))
}

// The text of each span of the name, in file order.
fn spans_of(lines: &[Line<'_>], name: &str) -> Vec<Vec<String>> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| names_test(line.text, name))
        .map(|(at, _)| {
            let (first, last) = span(lines, at);
            lines
                .get(first..=last)
                .unwrap_or_default()
                .iter()
                .map(|line| format!("{}{}", " ".repeat(line.indent), line.text))
                .collect()
        })
        .collect()
}

// A mapped test is edited or removed when one of its spans at the fork is not found, line for
// line, at the tip; a new line that names it elsewhere edits nothing.
pub(crate) fn touched_tests(fork: &str, tip: &str, mapped: &BTreeSet<String>) -> Vec<String> {
    let (old, new) = (lines_of(fork), lines_of(tip));
    mapped
        .iter()
        .filter(|name| {
            let mut left = spans_of(&new, name);
            spans_of(&old, name).into_iter().any(|span| {
                match left.iter().position(|kept| *kept == span) {
                    Some(found) => {
                        left.swap_remove(found);
                        false
                    }
                    None => true,
                }
            })
        })
        .cloned()
        .collect()
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
                !text.contains("**") && RepoPath::parse(&text.replace('*', "a")).is_some()
            );
            for good in ["src/", "claude_*.rs"] {
                prop_assert!(RiskPath::parse(good).is_some(), "{}", good);
            }
            for bad in ["", "/a", "a//b", "a/..", "a?", "[a]", "**", "src/**/a.rs", "a**"] {
                prop_assert!(RiskPath::parse(bad).is_none(), "{}", bad);
            }
        }

        #[test]
        fn the_estimate_names_each_reason_from_owns(
            owns in proptest::sample::subsequence(UNIVERSE.to_vec(), 0..=UNIVERSE.len()),
            light in 0_u64..6,
            raised in proptest::collection::vec("[a-z ]{1,6}", 0..=3),
        ) {
            let paths: Vec<RepoPath> = owns.iter().map(|(path, _, _)| RepoPath::parse(path).unwrap()).collect();
            let risk = estimate(&test_setup(light), &paths, &raised);
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
            expected.extend(raised.iter().cloned().map(Reason::Raised));
            let level = if expected.is_empty() { Level::Light } else { Level::Full };
            prop_assert_eq!(risk.level(), level);
            prop_assert_eq!(risk, Risk { basis: Basis::Owns, reasons: expected });
            let light = estimate(&test_setup(10), &[RepoPath::parse("src/a.rs").unwrap()], &[]);
            prop_assert_eq!(light.level(), Level::Light);
            prop_assert_eq!(light.reasons_text(), "");
        }
    }

    // The walk up from a test's line passes comments, blank lines between attributes and the
    // lines of a multi-line attribute, so a switch-off above them still edits the test.
    #[test]
    fn a_switch_off_above_comments_blanks_or_a_long_attribute_edits_the_test() {
        let mapped = BTreeSet::from(["t_one".to_owned()]);
        let rust = "    #[test]\n    fn t_one() {\n    }\n";
        let cases = [
            (
                "    // why\n    #[test]\n    fn t_one() {\n    }\n",
                "    #[ignore]\n    // why\n    #[test]\n    fn t_one() {\n    }\n",
            ),
            (
                rust,
                "    #[ignore]\n\n    #[test]\n    fn t_one() {\n    }\n",
            ),
            (
                rust,
                "    #[cfg_attr(\n        unix,\n        ignore\n    )]\n    #[test]\n    fn t_one() {\n    }\n",
            ),
            (
                "# why\ndef t_one():\n    x\n",
                "@pytest.mark.skip\n# why\ndef t_one():\n    x\n",
            ),
        ];
        for (fork, tip) in cases {
            assert_eq!(
                touched_tests(fork, tip, &mapped),
                vec!["t_one".to_owned()],
                "{tip}"
            );
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
            Reason::UnreadableFiles(names(&["a.rs", "b.rs"])).text(),
            "changed files a.rs, b.rs could not be read at the fork"
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
        assert_eq!(Reason::Raised("x".to_owned()).text(), "raised: x");
    }

    // One test block of a file: its kind (0 Rust-like, 1 bats-like, 2 indent-only, 3 a JS call
    // with the name on its own argument line, 4 the same in Scala with a braced body), its name,
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
                2 => {
                    lines.push(format!("def {name}():"));
                    lines.extend(body("    "));
                }
                3 => {
                    lines.push("it(".to_owned());
                    lines.push(format!("  \"{name} x\","));
                    lines.push("  () => {".to_owned());
                    lines.extend(body("    "));
                    lines.push("  }".to_owned());
                    lines.push(");".to_owned());
                }
                _ => {
                    lines.push("test(".to_owned());
                    lines.push(format!("  \"{name} x\""));
                    lines.push(") {".to_owned());
                    lines.extend(body("  "));
                    lines.push("}".to_owned());
                }
            }
            lines
        }

        // The line a switch-off adds above the test.
        fn attribute(&self) -> String {
            if self.kind == 0 {
                "    #[ignore]".to_owned()
            } else {
                "@skip".to_owned()
            }
        }
    }

    // Blocks with unique names, in kind order. A Rust-like block starts at indent 4, so it comes
    // before an indent-only block, whose span ends only at a line at its own indent or less.
    fn blocks() -> impl Strategy<Value = Vec<Block>> {
        proptest::collection::btree_set("t_[a-z]{3,6}", 1..=5).prop_flat_map(|names| {
            let count = names.len();
            let parts = proptest::collection::vec(
                (
                    0_u8..5,
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

    // The file's lines and each block's first and last line (from 0), blank lines after each.
    fn file_of(blocks: &[Block]) -> (Vec<String>, Vec<(usize, usize)>) {
        let mut lines = Vec::new();
        let mut spans = Vec::new();
        for block in blocks {
            let first = lines.len();
            lines.extend(block.lines());
            spans.push((first, lines.len() - 1));
            lines.extend(std::iter::repeat_n(String::new(), block.blanks));
        }
        (lines, spans)
    }

    fn text_of(lines: &[String]) -> String {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    fn universe_names(picked: &[(&str, bool, bool)]) -> Vec<String> {
        picked.iter().map(|(path, _, _)| shown(path)).collect()
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn a_mapped_test_is_edited_when_its_span_changes(
            blocks in blocks(),
            picks in proptest::collection::vec(any::<bool>(), 5),
        ) {
            let (lines, spans) = file_of(&blocks);
            let fork = text_of(&lines);
            let mapped: BTreeSet<String> = blocks
                .iter()
                .zip(&picks)
                .filter(|(_, pick)| **pick)
                .map(|(block, _)| block.name.clone())
                .collect();
            let none = Vec::<String>::new();
            let touched = |tip: &[String]| touched_tests(&fork, &text_of(tip), &mapped);
            for (block, (first, last)) in blocks.iter().zip(&spans) {
                let only = if mapped.contains(&block.name) { vec![block.name.clone()] } else { Vec::new() };
                for at in *first..=*last {
                    let mut tip = lines.clone();
                    if let Some(line) = tip.get_mut(at) {
                        line.push('x');
                    }
                    prop_assert_eq!(touched(&tip), only.clone(), "{}", at);
                }
                let mut tip = lines.clone();
                tip.insert(*first, block.attribute());
                prop_assert_eq!(touched(&tip), only.clone());
                let mut tip = lines.clone();
                tip.drain(*first..=*last);
                prop_assert_eq!(touched(&tip), only);
                // A line just above the test, or a blank one after it, edits nothing.
                let mut tip = lines.clone();
                tip.insert(*first, "x9".to_owned());
                tip.insert(last + 2, String::new());
                prop_assert_eq!(touched(&tip), none.clone());
            }
            let mut tip = lines.clone();
            tip.push(blocks.iter().map(|block| block.name.as_str()).collect::<Vec<_>>().join(" "));
            prop_assert_eq!(touched(&tip), none.clone());
            prop_assert_eq!(touched(&lines), none);
            prop_assert_eq!(touched(&[]), mapped.iter().cloned().collect::<Vec<_>>());
        }

        #[test]
        fn the_diff_adds_its_own_reasons_after_the_path_reasons(
            names in proptest::sample::subsequence(UNIVERSE.to_vec(), 0..=UNIVERSE.len()),
            files in 0_u64..15,
            light in 0_u64..6,
            mapped in proptest::collection::vec("[a-z]{1,4}", 0..=4),
            unreadable in proptest::collection::vec("[a-z]{1,4}", 0..=4),
            unread_files in proptest::collection::vec("[a-z]{1,4}", 0..=4),
            given in any::<bool>(),
            other in any::<bool>(),
            raised in proptest::collection::vec("[a-z ]{1,6}", 0..=3),
        ) {
            let setup = test_setup(light);
            let names = universe_names(&names);
            let diff = DiffFacts {
                names: names.clone(),
                files,
                mapped: mapped.clone(),
                unreadable: unreadable.clone(),
                unread_files: unread_files.clone(),
                given_base: given.then(|| "main".to_owned()),
                other_base: other.then(|| ("side".to_owned(), "main".to_owned())),
            };
            let risk = judged(&setup, &diff, &raised);
            let mut expected = path_reasons(&setup, &names, files);
            if !mapped.is_empty() {
                expected.push(Reason::MappedTests(mapped.clone()));
            }
            if !unreadable.is_empty() {
                expected.push(Reason::UnreadableMaps(unreadable));
            }
            if !unread_files.is_empty() {
                expected.push(Reason::UnreadableFiles(unread_files));
            }
            if given {
                expected.push(Reason::BaseGiven("main".to_owned()));
            }
            if other {
                expected.push(Reason::OtherBase { checked: "side".to_owned(), policy: "main".to_owned() });
            }
            expected.extend(raised.iter().cloned().map(Reason::Raised));
            let shown = serde_json::to_value(&risk).unwrap();
            prop_assert_eq!(&shown["level"], if expected.is_empty() { "light" } else { "full" });
            prop_assert_eq!(&shown["from"], "diff");
            if mapped.is_empty() {
                prop_assert!(shown.get("mapped_tests").is_none());
            } else {
                prop_assert_eq!(&shown["mapped_tests"], &serde_json::json!(mapped));
            }
            prop_assert_eq!(risk, Risk { basis: Basis::Diff, reasons: expected });
        }
    }
}
