use crate::pure::goal::GoalName;
use crate::pure::roadmap::SliceName;

const PREFIX: &str = "slice/";

pub(crate) fn slice_branch(goal: &GoalName, slice: &SliceName) -> String {
    format!("{PREFIX}{goal}/{slice}")
}

// Any `slice/` prefix, so a malformed `slice/x` is judged by the slice rules and never waved
// through as some other branch.
pub(crate) fn is_slice_branch(branch: &str) -> bool {
    branch.starts_with(PREFIX)
}

pub(crate) fn parse_slice_branch(branch: &str) -> Option<(GoalName, SliceName)> {
    let (goal, slice) = branch.strip_prefix(PREFIX)?.split_once('/')?;
    Some((GoalName::parse(goal)?, SliceName::parse(slice)?))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_slice_branch_parses_back_to_its_goal_and_slice(
            goal in "[a-z][a-z0-9-]{0,8}",
            slice in "[a-z][a-z0-9-]{0,8}",
        ) {
            prop_assume!(slice != "roadmap");
            let goal = GoalName::parse(&goal).expect("a goal name");
            let slice = SliceName::parse(&slice).expect("a slice name");
            let branch = slice_branch(&goal, &slice);
            prop_assert!(is_slice_branch(&branch));
            prop_assert_eq!(parse_slice_branch(&branch), Some((goal, slice)));
        }

        #[test]
        fn every_slice_prefix_is_a_slice_branch_but_only_the_exact_form_parses(
            rest in "[A-Za-z0-9/._-]{0,12}",
            goal in "[a-z][a-z0-9-]{0,5}",
            slice in "[a-z][a-z0-9-]{0,5}",
        ) {
            prop_assume!(slice != "roadmap");
            let prefixed = format!("slice/{rest}");
            prop_assert!(is_slice_branch(&prefixed));
            for malformed in [
                format!("slice/{slice}"),
                format!("slice/{goal}/{slice}/extra"),
                format!("slice/{goal}/"),
                format!("slice//{slice}"),
                format!("slice/{goal}/Roadmap-{slice}"),
                format!("slice/{goal}/roadmap"),
            ] {
                prop_assert!(is_slice_branch(&malformed));
                prop_assert_eq!(parse_slice_branch(&malformed), None);
            }
            for other in ["main", "slices/a/b", "origin/slice/a/b", "feature/slice/a", ""] {
                prop_assert!(!is_slice_branch(other));
                prop_assert_eq!(parse_slice_branch(other), None);
            }
        }
    }
}
