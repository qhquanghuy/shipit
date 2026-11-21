use crate::pure::receipt::Oid;
use crate::pure::text::comma_list;

pub(crate) const UNCOMMITTED: &str =
    "uncommitted changes: commit first, a receipt is per commit content";

const LISTED: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkChange {
    None,
    Dirty,
    Hidden,
    Committed,
    MovedHead,
}

#[derive(Debug)]
pub(crate) struct WorkFacts<'a> {
    pub(crate) dirty: bool,
    pub(crate) hidden: bool,
    pub(crate) tree_before: &'a Oid,
    pub(crate) tree_after: &'a Oid,
    pub(crate) commit_before: &'a Oid,
    pub(crate) commit_after: &'a Oid,
}

pub(crate) fn change_verdict(facts: &WorkFacts<'_>) -> WorkChange {
    if facts.dirty {
        WorkChange::Dirty
    } else if facts.hidden {
        WorkChange::Hidden
    } else if facts.tree_after != facts.tree_before {
        WorkChange::Committed
    } else if facts.commit_after != facts.commit_before {
        WorkChange::MovedHead
    } else {
        WorkChange::None
    }
}

pub(crate) fn first_five<S: AsRef<str>>(lines: impl IntoIterator<Item = S>) -> String {
    comma_list(
        lines
            .into_iter()
            .take(LISTED)
            .map(|line| line.as_ref().to_owned()),
    )
}

fn entries(z: &[u8]) -> impl Iterator<Item = &[u8]> {
    z.split(|byte| *byte == 0).filter(|entry| !entry.is_empty())
}

// From `git status --porcelain -z`: a rename or copy has two paths, the new one in its entry
// and the old one in the next entry, which has no status columns.
pub(crate) fn dirty_paths(status_z: &[u8]) -> String {
    let mut paths: Vec<String> = Vec::new();
    let mut old_path_next = false;
    for entry in entries(status_z) {
        if old_path_next {
            old_path_next = false;
            paths.push(String::from_utf8_lossy(entry).into_owned());
        } else {
            let columns = entry.get(..2).unwrap_or_default();
            old_path_next = columns.iter().any(|byte| matches!(byte, b'R' | b'C'));
            let path = entry.get(3..).unwrap_or_default();
            paths.push(String::from_utf8_lossy(path).into_owned());
        }
    }
    first_five(paths)
}

// From `git ls-files -v -z`: the tag is lower case for assume-unchanged, `S` for skip-worktree.
pub(crate) fn hidden_paths(ls_files_v_z: &[u8]) -> Vec<String> {
    entries(ls_files_v_z)
        .filter_map(|entry| {
            let (tag, rest) = entry.split_first()?;
            let path = rest.strip_prefix(b" ")?;
            (tag.is_ascii_lowercase() || *tag == b'S')
                .then(|| String::from_utf8_lossy(path).into_owned())
        })
        .collect()
}

pub(crate) fn hidden_refusal(paths: &[String]) -> String {
    format!(
        "tracked files are marked skip-worktree or assume-unchanged, which hides edits from git \
         ({}); clear the flag with 'git update-index --no-skip-worktree --no-assume-unchanged \
         <file>' and run shipit again",
        first_five(paths)
    )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::oid;

    fn verdicts(edit: impl Fn(&mut Facts)) -> WorkChange {
        let mut facts = Facts {
            dirty: false,
            hidden: false,
            tree_after: 'a',
            commit_after: 'c',
        };
        edit(&mut facts);
        let (tree_before, commit_before) = (oid('a'), oid('c'));
        let (tree_after, commit_after) = (oid(facts.tree_after), oid(facts.commit_after));
        change_verdict(&WorkFacts {
            dirty: facts.dirty,
            hidden: facts.hidden,
            tree_before: &tree_before,
            tree_after: &tree_after,
            commit_before: &commit_before,
            commit_after: &commit_after,
        })
    }

    struct Facts {
        dirty: bool,
        hidden: bool,
        tree_after: char,
        commit_after: char,
    }

    #[test]
    fn the_change_verdict_cases() {
        assert_eq!(verdicts(|_| {}), WorkChange::None);
        assert_eq!(verdicts(|f| f.dirty = true), WorkChange::Dirty);
        assert_eq!(verdicts(|f| f.hidden = true), WorkChange::Hidden);
        assert_eq!(verdicts(|f| f.tree_after = 'b'), WorkChange::Committed);
        assert_eq!(verdicts(|f| f.commit_after = 'd'), WorkChange::MovedHead);
        assert_eq!(
            verdicts(|f| {
                f.dirty = true;
                f.tree_after = 'b';
            }),
            WorkChange::Dirty
        );
    }

    #[test]
    fn a_rename_lists_its_new_then_its_old_path() {
        assert_eq!(dirty_paths(b"R  g1\0f1\0"), "g1, f1");
        assert_eq!(dirty_paths(b" C g1\0f1\0?? z\0"), "g1, f1, z");
    }

    #[test]
    fn at_most_five_paths_are_listed() {
        let status = b"?? a\0?? b\0?? c\0?? d\0?? e\0?? f\0?? g\0";
        assert_eq!(dirty_paths(status), "a, b, c, d, e");
        assert_eq!(dirty_paths(b""), "");
    }

    #[test]
    fn names_with_spaces_tabs_and_commas_are_listed_plainly() {
        assert_eq!(
            dirty_paths(b"?? a b\0?? tab\there\0?? c,d\0"),
            "a b, tab\there, c,d"
        );
    }

    #[test]
    fn hidden_paths_are_the_tagged_entries() {
        let listing = b"H f0\0h f1\0S f2\0s f3\0M f4\0? f5\0K f6\0h with space\0";
        assert_eq!(
            hidden_paths(listing),
            ["f1", "f2", "f3", "with space"].map(String::from)
        );
        assert_eq!(hidden_paths(b"H a\0"), Vec::<String>::new());
        assert_eq!(hidden_paths(b""), Vec::<String>::new());
    }

    #[test]
    fn the_refusal_lists_five_paths_and_the_remedy() {
        let paths: Vec<String> = ["a", "b", "c", "d", "e", "f"].map(String::from).to_vec();
        assert_eq!(
            hidden_refusal(&paths),
            "tracked files are marked skip-worktree or assume-unchanged, which hides edits from \
             git (a, b, c, d, e); clear the flag with 'git update-index --no-skip-worktree \
             --no-assume-unchanged <file>' and run shipit again"
        );
    }

    #[test]
    fn first_five_joins_with_a_comma_and_a_space() {
        assert_eq!(first_five(["a", "b"]), "a, b");
        assert_eq!(first_five(Vec::<String>::new()), "");
    }

    fn no_comma_bytes() -> impl Strategy<Value = Vec<u8>> {
        proptest::collection::vec(
            any::<u8>().prop_filter("no comma", |byte| *byte != b','),
            0..200,
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_dirty_work_is_always_dirty(
            hidden in any::<bool>(),
            tree in "[a-f]",
            commit in "[a-f]",
        ) {
            let (tree_before, commit_before) = (oid('1'), oid('2'));
            let tree_after = oid(tree.chars().next().unwrap_or('a'));
            let commit_after = oid(commit.chars().next().unwrap_or('a'));
            let facts = WorkFacts {
                dirty: true,
                hidden,
                tree_before: &tree_before,
                tree_after: &tree_after,
                commit_before: &commit_before,
                commit_after: &commit_after,
            };
            prop_assert_eq!(change_verdict(&facts), WorkChange::Dirty);
        }

        #[test]
        fn dirty_paths_never_panics_and_lists_at_most_five(bytes in no_comma_bytes()) {
            prop_assert!(dirty_paths(&bytes).split(", ").count() <= LISTED);
        }

        #[test]
        fn hidden_paths_never_panics_and_keeps_only_tagged_entries(bytes in no_comma_bytes()) {
            let tagged = entries(&bytes)
                .filter(|entry| {
                    entry.first().is_some_and(|t| t.is_ascii_lowercase() || *t == b'S')
                        && entry.get(1) == Some(&b' ')
                })
                .count();
            let paths = hidden_paths(&bytes);
            prop_assert_eq!(paths.len(), tagged);
            prop_assert!(first_five(&paths).split(", ").count() <= LISTED);
        }

        #[test]
        fn dirty_paths_lists_at_most_five_names(
            names in proptest::collection::vec("[a-z]{1,5}", 0..12),
        ) {
            let status: Vec<u8> = names
                .iter()
                .flat_map(|name| format!("?? {name}\0").into_bytes())
                .collect();
            let expected: Vec<&str> = names.iter().take(LISTED).map(String::as_str).collect();
            prop_assert_eq!(dirty_paths(&status), expected.join(", "));
        }
    }
}
