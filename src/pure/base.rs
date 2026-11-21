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

// A local branch that has everything origin has (ahead or equal, so the owner's unpushed work
// counts) wins; with both and the local behind or diverged, origin.
pub(crate) fn choose_base_ref(
    base: &BranchName,
    origin: bool,
    local: bool,
    local_has_origin: bool,
) -> Option<BaseRef> {
    match (origin, local) {
        (true, true) if local_has_origin => Some(BaseRef::Local(base.clone())),
        (true, _) => Some(BaseRef::Origin(base.clone())),
        (false, true) => Some(BaseRef::Local(base.clone())),
        (false, false) => None,
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

    #[test]
    fn the_base_ref_truth_table() {
        let main = name("main");
        let local = Some(BaseRef::Local(name("main")));
        let origin = Some(BaseRef::Origin(name("main")));
        assert_eq!(choose_base_ref(&main, true, true, true), local);
        assert_eq!(choose_base_ref(&main, true, true, false), origin);
        assert_eq!(choose_base_ref(&main, true, false, false), origin);
        assert_eq!(choose_base_ref(&main, false, true, false), local);
        assert_eq!(choose_base_ref(&main, false, false, false), None);
    }

    #[test]
    fn a_base_ref_is_shown_as_git_names_it() {
        assert_eq!(BaseRef::Local(name("main")).to_string(), "main");
        assert_eq!(BaseRef::Origin(name("main")).to_string(), "origin/main");
    }

    #[test]
    fn the_full_ref_names_a_branch_never_a_tag() {
        assert_eq!(BaseRef::Local(name("main")).full(), "refs/heads/main");
        assert_eq!(
            BaseRef::Origin(name("a/b")).full(),
            "refs/remotes/origin/a/b"
        );
    }

    #[test]
    fn names_git_accepts_as_branches_parse() {
        for text in ["main", "a/b", "@", "@x", "a@b", "é", "release-1.0", "x.y/z"] {
            assert_eq!(BranchName::parse(text), Some(name(text)), "{text}");
        }
    }

    #[test]
    fn revisions_and_malformed_names_are_not_branches() {
        let bad = [
            "",
            "-x",
            "HEAD",
            "HEAD~1",
            "HEAD^",
            "main~2",
            "a..b",
            "a//b",
            "/a",
            "a/",
            "a.",
            ".a",
            "a/.b",
            "a.lock",
            "a/b.lock/c",
            "a b",
            "a:b",
            "a?",
            "a*",
            "a[",
            "a\\b",
            "a@{b",
            "@{-1}",
            "a\tb",
            "a\u{7f}b",
        ];
        for text in bad {
            assert_eq!(BranchName::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_base_verdict_cases() {
        let main = name("main");
        let (a, b) = (oid('a'), oid('b'));
        assert_eq!(base_verdict(false, &main, Some("feature"), &a, &b), Ok(()));
        assert_eq!(
            base_verdict(false, &main, Some("main"), &a, &b),
            Err(SelfBase::TheBranch)
        );
        assert_eq!(
            base_verdict(false, &main, Some("feature"), &b, &b),
            Err(SelfBase::TheCommit)
        );
        assert_eq!(base_verdict(false, &main, None, &a, &b), Ok(()));
        assert_eq!(base_verdict(true, &main, Some("main"), &b, &b), Ok(()));
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
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

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
    }
}
