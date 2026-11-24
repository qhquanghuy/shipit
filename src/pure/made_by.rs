use crate::pure::claude_hook::MAKER;

pub(crate) const OWNER: &str = "owner";
pub(crate) const TRAILER_KEY: &str = "ShipIt-Made-By";

const SHORT: usize = 12;

// The first commit of `git log` lines `<oid>\t<values>` whose values are empty or hold anything
// but MAKER or OWNER, shortened to 12 characters.
pub(crate) fn unmade(log: &str) -> Option<String> {
    log.lines().find_map(|line| {
        let (oid, values) = line.split_once('\t')?;
        let made = !values.is_empty()
            && values
                .split(',')
                .all(|value| matches!(value.trim(), MAKER | OWNER));
        (!made).then(|| oid.chars().take(SHORT).collect())
    })
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use super::*;

    fn values() -> impl Strategy<Value = Vec<&'static str>> {
        proptest::collection::vec(
            prop_oneof![
                Just(MAKER),
                Just(OWNER),
                Just("shipit:lead"),
                Just("x"),
                Just("")
            ],
            0..3,
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn every_commit_of_the_work_must_carry_a_makers_or_owners_trailer(
            commits in proptest::collection::vec(("[0-9a-f]{40}", values()), 0..6),
        ) {
            let mut log = String::new();
            for (oid, values) in &commits {
                writeln!(log, "{oid}\t{}", values.join(",")).ok();
            }
            let expected = commits
                .iter()
                .find(|(_oid, values)| {
                    values.is_empty() || values.iter().any(|value| *value != MAKER && *value != OWNER)
                })
                .map(|(oid, _values)| oid.chars().take(12).collect::<String>());
            prop_assert_eq!(unmade(&log), expected);
        }
    }
}
