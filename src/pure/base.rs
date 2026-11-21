use std::fmt;

use serde::Serialize;

use crate::pure::receipt::Oid;

// A branch name as `git check-ref-format --branch` accepts it, and never a revision such as
// `HEAD~1` or a tag: the setup's base is the branch's own word, so it must name a branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct BranchName(String);

const FORBIDDEN: &[char] = &[' ', '~', '^', ':', '?', '*', '[', '\\'];

fn is_branch_name(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && text != "HEAD"
        && !text.ends_with(['/', '.'])
        && !text.contains("..")
        && !text.contains("@{")
        && !text.contains(|c: char| c.is_ascii_control() || FORBIDDEN.contains(&c))
        && text.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .rsplit_once('.')
                    .is_none_or(|(_, extension)| extension != "lock")
        })
}

impl BranchName {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        is_branch_name(text).then(|| Self(text.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// The full ref name is what git is asked for: a short name could resolve to a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BaseRef {
    Local(BranchName),
    Origin(BranchName),
}

impl BaseRef {
    pub(crate) fn full(&self) -> String {
        match self {
            Self::Local(name) => format!("refs/heads/{name}"),
            Self::Origin(name) => format!("refs/remotes/origin/{name}"),
        }
    }
}

impl fmt::Display for BaseRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(name) => write!(f, "{name}"),
            Self::Origin(name) => write!(f, "origin/{name}"),
        }
    }
}

// Owner's decision (2026-11-21): origin wins whenever it exists, so a commit on the local base
// never silently becomes the base.
pub(crate) fn choose_base_ref(base: &BranchName, origin: bool, local: bool) -> Option<BaseRef> {
    if origin {
        Some(BaseRef::Origin(base.clone()))
    } else if local {
        Some(BaseRef::Local(base.clone()))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelfBase {
    TheBranch,
    TheCommit,
}

impl fmt::Display for SelfBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TheBranch => "is the branch being checked",
            Self::TheCommit => "points at the commit being checked",
        })
    }
}

// A base named by the setup is the branch's own word: it must not be the branch itself or sit at
// the commit being checked, or it could not vouch for the checks.
pub(crate) fn base_verdict(
    trusted: bool,
    base: &BranchName,
    current: Option<&str>,
    ref_commit: &Oid,
    head: &Oid,
) -> Result<(), SelfBase> {
    if trusted {
        Ok(())
    } else if current == Some(base.0.as_str()) {
        Err(SelfBase::TheBranch)
    } else if ref_commit == head {
        Err(SelfBase::TheCommit)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::{name, oid};

    fn config() -> ProptestConfig {
        ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    // Names git accepts, with the odd ones (`@`, `@x`, a dot inside, non-ASCII) in the alphabet.
    fn valid_name() -> impl Strategy<Value = String> {
        let part = "[a-zA-Z0-9é@_-]{1,4}(\\.[0-9]{1,2})?";
        ("[a-zA-Z0-9é@_]", proptest::collection::vec(part, 0..3)).prop_map(|(first, rest)| {
            let mut text = first;
            text.push_str(&rest.join("/"));
            text
        })
    }

    #[test]
    fn a_base_ref_names_a_branch_never_a_tag_and_is_shown_as_git_shows_it() {
        let main = name("main");
        assert_eq!(BaseRef::Local(main.clone()).full(), "refs/heads/main");
        assert_eq!(BaseRef::Local(main.clone()).to_string(), "main");
        assert_eq!(
            BaseRef::Origin(name("a/b")).full(),
            "refs/remotes/origin/a/b"
        );
        assert_eq!(BaseRef::Origin(main).to_string(), "origin/main");
    }

    #[test]
    fn the_reasons_read_as_the_shell_said_them() {
        assert_eq!(
            SelfBase::TheBranch.to_string(),
            "is the branch being checked"
        );
        assert_eq!(
            SelfBase::TheCommit.to_string(),
            "points at the commit being checked"
        );
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn names_git_accepts_as_branches_parse(text in valid_name()) {
            prop_assert_eq!(BranchName::parse(&text), Some(name(&text)));
        }

        #[test]
        fn a_forbidden_character_or_sequence_is_never_a_branch(
            before in "[a-z]{0,4}",
            bad in proptest::sample::select(vec![
                " ", "~", "^", ":", "?", "*", "[", "\\", "..", "@{", "\t", "\u{7f}",
            ]),
            after in "[a-z]{0,4}",
        ) {
            prop_assert_eq!(BranchName::parse(&format!("{before}{bad}{after}")), None);
        }

        #[test]
        fn a_malformed_shape_is_never_a_branch(
            x in "[a-z]{1,4}",
            shape in proptest::sample::select(vec![
                "", "HEAD", "-X", "/X", "X/", "X.", ".X", "X.lock", "X/.X", "X//X", "X/X.lock/X",
            ]),
        ) {
            prop_assert_eq!(BranchName::parse(&shape.replace('X', &x)), None);
        }

        #[test]
        fn origin_wins_whenever_it_exists(origin in any::<bool>(), local in any::<bool>()) {
            let main = name("main");
            let expected = if origin {
                Some(BaseRef::Origin(main.clone()))
            } else if local {
                Some(BaseRef::Local(main.clone()))
            } else {
                None
            };
            prop_assert_eq!(choose_base_ref(&main, origin, local), expected);
        }

        #[test]
        fn a_trusted_base_is_always_ok(
            base in "[a-z]{1,8}",
            current in proptest::option::of(".{0,8}"),
            same in any::<bool>(),
        ) {
            let base = name(&base);
            let head = oid('b');
            let ref_commit = if same { oid('b') } else { oid('a') };
            prop_assert_eq!(
                base_verdict(true, &base, current.as_deref(), &ref_commit, &head),
                Ok(())
            );
        }

        #[test]
        fn the_branch_being_checked_is_refused_whatever_the_commits_are(
            base in "[a-z]{1,8}",
            same in any::<bool>(),
        ) {
            let head = oid('b');
            let ref_commit = if same { oid('b') } else { oid('a') };
            prop_assert_eq!(
                base_verdict(false, &name(&base), Some(&base), &ref_commit, &head),
                Err(SelfBase::TheBranch)
            );
        }

        #[test]
        fn another_branch_at_the_commit_being_checked_is_refused(
            base in "[a-z]{1,8}",
            current in proptest::option::of("[A-Z]{1,8}"),
        ) {
            let head = oid('b');
            prop_assert_eq!(
                base_verdict(false, &name(&base), current.as_deref(), &head, &head),
                Err(SelfBase::TheCommit)
            );
        }

        #[test]
        fn another_branch_at_another_commit_is_ok(
            base in "[a-z]{1,8}",
            current in proptest::option::of("[A-Z]{1,8}"),
        ) {
            prop_assert_eq!(
                base_verdict(false, &name(&base), current.as_deref(), &oid('a'), &oid('b')),
                Ok(())
            );
        }
    }
}
