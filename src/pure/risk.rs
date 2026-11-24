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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reason {
    RiskyPaths(Vec<String>),
    LockFiles(Vec<String>),
    Files { count: u64, light: u64 },
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Risk {
    basis: Basis,
    reasons: Vec<Reason>,
}

impl Risk {
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
    }
}
