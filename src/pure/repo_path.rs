use std::fmt;

use serde::Serialize;

// A path as git lists it: relative, normal, no glob. A path holds itself and everything below it,
// so `docs/` and `docs` are one path, and `src/a.rs/` is read as `src/a.rs`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub(crate) struct RepoPath(String);

impl RepoPath {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let path = text.strip_suffix('/').unwrap_or(text);
        let normal = path.split('/').all(|segment| {
            !segment.is_empty() && segment != "." && segment != ".." && segment.trim() == segment
        });
        let plain = path
            .chars()
            .all(|c| (c.is_ascii_graphic() || c == ' ') && !matches!(c, '*' | '?' | '[' | '\\'));
        (normal && plain).then(|| Self(path.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    // The path itself or anything below it, by the bytes git lists.
    pub(crate) fn covers(&self, name: &[u8]) -> bool {
        name.strip_prefix(self.0.as_bytes())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(b"/"))
    }

    // The path itself and every folder above it, shortest first.
    pub(crate) fn self_and_parents(&self) -> impl Iterator<Item = &str> {
        self.0
            .match_indices('/')
            .filter_map(|(at, _)| self.0.get(..at))
            .chain(std::iter::once(self.0.as_str()))
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn path() -> impl Strategy<Value = String> {
        proptest::collection::vec("[a-z0-9_-][a-z0-9._-]{0,5}", 1..5)
            .prop_map(|segments| segments.join("/"))
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_normal_path_is_kept_and_a_trailing_slash_is_not_a_second_path(text in path()) {
            let plain = RepoPath::parse(&text).expect("a normal path");
            let folder = RepoPath::parse(&format!("{text}/")).expect("a folder");
            prop_assert_eq!(plain.to_string(), text.clone());
            prop_assert_eq!(&folder, &plain);
            prop_assert_eq!(serde_json::to_string(&plain).ok(), Some(format!("\"{text}\"")));
        }

        #[test]
        fn a_path_covers_itself_and_what_is_below_it_and_nothing_else(
            text in path(),
            below in path(),
            tail in "[a-z0-9._-]{1,3}",
        ) {
            let parsed = RepoPath::parse(&text).expect("a normal path");
            prop_assert!(parsed.covers(text.as_bytes()));
            let under = format!("{text}/{below}");
            let longer = format!("{text}{tail}");
            prop_assert!(parsed.covers(under.as_bytes()));
            prop_assert!(!parsed.covers(longer.as_bytes()));
            prop_assert!(!parsed.covers(b""));
            if let Some((parent, _last)) = text.rsplit_once('/') {
                prop_assert!(!parsed.covers(parent.as_bytes()));
            }
        }

        #[test]
        fn every_spelling_of_something_else_is_refused(
            text in path(),
            other in path(),
            bad in prop::sample::select(vec![
                '*', '?', '[', '\\', '\0', '\n', '\t', '\u{7f}', 'é', '\u{2028}', '\u{a0}',
            ]),
        ) {
            for refused in [
                format!("{text} "),
                format!(" {text}"),
                format!("{text} /{other}"),
                format!("{text}/ {other}"),
                format!("{text}\\{other}"),
                format!("/{text}"),
                format!("./{text}"),
                format!("{text}/./{other}"),
                format!("{text}/../{other}"),
                format!("../{text}"),
                format!("{text}//{other}"),
                format!("{text}//"),
                format!("{text}/{bad}"),
                format!("{bad}{text}"),
                format!("{text}{bad}x"),
                format!("{text}/."),
                format!("{text}/.."),
                String::new(),
                "/".to_owned(),
                ".".to_owned(),
                "..".to_owned(),
            ] {
                prop_assert_eq!(RepoPath::parse(&refused), None, "{}", refused.escape_default());
            }
        }

        #[test]
        fn the_path_and_its_folders_are_listed_shortest_first(text in path()) {
            let parsed = RepoPath::parse(&text).expect("a normal path");
            let listed: Vec<&str> = parsed.self_and_parents().collect();
            let mut wanted: Vec<String> = Vec::new();
            let mut so_far = String::new();
            for segment in text.split('/') {
                if !so_far.is_empty() {
                    so_far.push('/');
                }
                so_far.push_str(segment);
                wanted.push(so_far.clone());
            }
            prop_assert_eq!(listed, wanted);
            prop_assert_eq!(parsed.as_str(), text.as_str());
        }
    }
}
