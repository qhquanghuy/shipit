use crate::pure::base::BranchName;
use crate::pure::receipt::Oid;

const HEADS: &str = "refs/heads/";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Local {
    Delete,
    Commit(Oid),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PushLine {
    pub(crate) local: Local,
    pub(crate) remote_ref: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PushTarget {
    pub(crate) branch: String,
    pub(crate) commit: Oid,
}

fn all_zeros(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|byte| byte == b'0')
}

// git writes `<local ref> <local oid> <remote ref> <remote oid>`; a delete has the zero oid.
fn parse_line(line: &str) -> Option<PushLine> {
    let mut fields = line.split(' ');
    let (_local_ref, local, remote_ref, _remote) = (
        fields.next()?,
        fields.next()?,
        fields.next()?,
        fields.next()?,
    );
    if fields.next().is_some() {
        return None;
    }
    let local = if all_zeros(local) {
        Local::Delete
    } else {
        Local::Commit(Oid::parse(local)?)
    };
    Some(PushLine {
        local,
        remote_ref: remote_ref.to_owned(),
    })
}

// The Err is the first line that does not read.
pub(crate) fn parse_push(text: &str) -> Result<Vec<PushLine>, String> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| parse_line(line).ok_or_else(|| line.to_owned()))
        .collect()
}

pub(crate) fn branch_pushes(lines: &[PushLine]) -> Vec<PushTarget> {
    lines
        .iter()
        .filter_map(|line| match &line.local {
            Local::Delete => None,
            Local::Commit(commit) => line
                .remote_ref
                .strip_prefix(HEADS)
                .map(|branch| PushTarget {
                    branch: branch.to_owned(),
                    commit: commit.clone(),
                }),
        })
        .collect()
}

// Matters when the base itself is named `slice/...`, as a stacked base can be.
pub(crate) fn is_base(target: &PushTarget, base: &BranchName) -> bool {
    target.branch == base.as_str()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::name;

    fn oid_text() -> impl Strategy<Value = String> {
        prop_oneof!["[0-9a-f]{40}", "[0-9a-f]{64}"]
            .prop_filter("not a delete", |text| !all_zeros(text))
    }

    fn remote_ref() -> impl Strategy<Value = String> {
        prop_oneof![
            "refs/heads/[a-z][a-z/]{0,8}",
            "refs/tags/[a-z]{1,6}",
            "refs/notes/[a-z]{1,6}",
            Just("refs/heads/main".to_owned()),
        ]
    }

    fn render(lines: &[(Option<String>, String)]) -> String {
        let zero = "0".repeat(40);
        let mut text = String::new();
        for (oid, remote) in lines {
            let local = oid.as_deref().unwrap_or(&zero);
            writeln!(text, "HEAD {local} {remote} {zero}").ok();
        }
        text
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn parse_round_trips(
            lines in proptest::collection::vec(
                (proptest::option::of(oid_text()), remote_ref()),
                0..6,
            ),
        ) {
            let parsed = parse_push(&render(&lines));
            let expected: Vec<PushLine> = lines
                .iter()
                .map(|(oid, remote)| PushLine {
                    local: oid.as_deref().map_or(Local::Delete, |text| {
                        Local::Commit(Oid::parse(text).expect("an oid"))
                    }),
                    remote_ref: remote.clone(),
                })
                .collect();
            prop_assert_eq!(parsed, Ok(expected));
        }

        #[test]
        fn a_line_without_four_fields_is_returned(
            fields in prop_oneof![1_usize..4, 5_usize..8]
                .prop_flat_map(|count| proptest::collection::vec("[a-z0-9/]{1,8}", count)),
        ) {
            let line = fields.join(" ");
            prop_assert_eq!(parse_push(&format!("{line}\n")), Err(line));
        }

        #[test]
        fn zero_input_is_no_lines(blanks in "\n{0,5}") {
            prop_assert_eq!(parse_push(&blanks), Ok(Vec::new()));
        }

        #[test]
        fn targets_skip_deletes_tags_and_the_base(
            lines in proptest::collection::vec(
                (proptest::option::of(oid_text()), remote_ref()),
                0..8,
            ),
        ) {
            let parsed = parse_push(&render(&lines)).expect("parses");
            let targets = branch_pushes(&parsed);
            let expected: Vec<&str> = lines
                .iter()
                .filter(|(oid, _)| oid.is_some())
                .filter_map(|(_, remote)| remote.strip_prefix(HEADS))
                .collect();
            let branches: Vec<&str> = targets.iter().map(|t| t.branch.as_str()).collect();
            prop_assert_eq!(&branches, &expected);
            let base = name("main");
            for target in &targets {
                prop_assert_eq!(is_base(target, &base), target.branch == "main");
            }
        }

        #[test]
        fn the_remote_ref_decides(branch in "[a-z]{1,6}", oid in oid_text()) {
            let local_name = format!("refs/heads/{branch}");
            let text = format!("{local_name} {oid} refs/tags/{branch} {}\n", "0".repeat(40));
            prop_assert_eq!(branch_pushes(&parse_push(&text).expect("parses")), Vec::new());
            let text = format!("HEAD {oid} {local_name} {}\n", "0".repeat(40));
            let targets = branch_pushes(&parse_push(&text).expect("parses"));
            prop_assert_eq!(targets.first().map(|t| t.branch.as_str()), Some(branch.as_str()));
        }
    }
}
