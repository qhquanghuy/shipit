#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that lists the goals of the store"
)]

use std::fs;
use std::path::{Path, PathBuf};

use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;

// `goal.v<N>.md`, N with no sign and no leading zero.
fn version_of(file_name: &str) -> Option<u64> {
    let digits = file_name.strip_prefix("goal.v")?.strip_suffix(".md")?;
    if digits.starts_with('0') || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

// The goal's file with the highest version: a changed goal is a new file, never an edit.
pub(crate) fn newest_goal(common: &Path, goal: &GoalName) -> Result<PathBuf, Failure> {
    let missing = || Failure::usage(format!("no goal \"{goal}\" in the store"));
    let dir = common.join("shipit").join("goals").join(goal.to_string());
    let entries = fs::read_dir(&dir).map_err(|_why| missing())?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version = version_of(entry.file_name().to_str()?)?;
            Some((version, entry.path()))
        })
        .max_by_key(|(version, _path)| *version)
        .map(|(_version, path)| path)
        .ok_or_else(missing)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::edge::testing::scratch;

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_version_has_digits_and_no_leading_zero(n in 0_u64..100_000, pad in 0_usize..3) {
            let plain = format!("goal.v{n}.md");
            prop_assert_eq!(version_of(&plain), (n > 0).then_some(n));
            let padded = format!("goal.v{}{n}.md", "0".repeat(pad));
            prop_assert_eq!(version_of(&padded), (pad == 0 && n > 0).then_some(n));
        }

        #[test]
        fn other_names_are_not_versions(name in "[a-z0-9.+-]{0,12}") {
            prop_assume!(!name.starts_with("goal.v"));
            prop_assert_eq!(version_of(&name), None);
        }
    }

    #[test]
    fn the_highest_version_wins_and_other_files_are_ignored() {
        let common = scratch("goal-file");
        let goal = GoalName::parse("demo").expect("a goal name");
        let dir = common.join("shipit").join("goals").join("demo");
        fs::create_dir_all(&dir).expect("a folder");
        for name in [
            "goal.v2.md",
            "goal.v10.md",
            "goal.v9.md",
            "goal.vx.md",
            "roadmap.v11.md",
        ] {
            fs::write(dir.join(name), "x").expect("a file");
        }
        assert_eq!(newest_goal(&common, &goal), Ok(dir.join("goal.v10.md")));
    }

    #[test]
    fn a_goal_without_a_version_is_named() {
        let common = scratch("goal-file-none");
        let goal = GoalName::parse("demo").expect("a goal name");
        let missing = Err(Failure::usage("no goal \"demo\" in the store"));
        assert_eq!(newest_goal(&common, &goal), missing);
        let dir = common.join("shipit").join("goals").join("demo");
        fs::create_dir_all(&dir).expect("a folder");
        fs::write(dir.join("roadmap.v1.md"), "x").expect("a file");
        assert_eq!(newest_goal(&common, &goal), missing);
    }
}
