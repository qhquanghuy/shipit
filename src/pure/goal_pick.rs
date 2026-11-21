use crate::pure::branch::parse_slice_branch;
use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::roadmap::{Element, Roadmap, SliceName};
use crate::pure::text::comma_list;

// A named goal is taken as given: the loader says when the store has no folder for it.
pub(crate) fn which_goal(
    flag: Option<&GoalName>,
    branch: Option<&str>,
    goals: &[GoalName],
) -> Result<GoalName, Failure> {
    if let Some(goal) = flag {
        return Ok(goal.clone());
    }
    if let Some((goal, _slice)) = branch.and_then(parse_slice_branch) {
        return Ok(goal);
    }
    match goals {
        [] => Err(Failure::usage("no goal in the store")),
        [only] => Ok(only.clone()),
        [_, _, ..] => Err(Failure::usage(format!(
            "more than one goal in the store ({}): pass --goal <name>",
            comma_list(goals)
        ))),
    }
}

pub(crate) const fn element_name(element: &Element) -> &SliceName {
    match element {
        Element::Slice(slice) => &slice.name,
        Element::Proof(proof) => &proof.name,
    }
}

pub(crate) fn element_named<'a>(roadmap: &'a Roadmap, name: &SliceName) -> Option<&'a Element> {
    roadmap
        .elements
        .iter()
        .find(|element| element_name(element) == name)
}

// The newest roadmap is the one with the highest version number: `roadmap.v<N>.md`, N without a
// sign or a leading zero.
pub(crate) fn roadmap_version(file_name: &str) -> Option<u64> {
    let digits = file_name.strip_prefix("roadmap.v")?.strip_suffix(".md")?;
    let plain = !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit());
    if plain { digits.parse().ok() } else { None }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::branch::slice_branch;

    fn goal(text: &str) -> GoalName {
        GoalName::parse(text).expect("a goal name")
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn the_flag_beats_the_branch_which_beats_the_only_goal(
            names in proptest::collection::btree_set("[a-z][a-z0-9-]{0,5}", 0..4),
            flag in proptest::option::of("[a-z][a-z0-9-]{0,5}"),
            on_branch in proptest::option::of("[a-z][a-z0-9-]{0,5}"),
        ) {
            let goals: Vec<GoalName> = names.iter().map(|name| goal(name)).collect();
            let flag = flag.map(|name| goal(&name));
            let branch = on_branch.as_ref().map(|name| {
                slice_branch(&goal(name), &SliceName::parse("x").expect("a slice name"))
            });
            let picked = which_goal(flag.as_ref(), branch.as_deref(), &goals);
            match (&flag, &on_branch, goals.as_slice()) {
                (Some(flag), _, _) => prop_assert_eq!(picked, Ok(flag.clone())),
                (None, Some(name), _) => prop_assert_eq!(picked, Ok(goal(name))),
                (None, None, [only]) => prop_assert_eq!(picked, Ok(only.clone())),
                (None, None, []) => {
                    prop_assert_eq!(picked, Err(Failure::usage("no goal in the store")));
                }
                (None, None, [_, _, ..]) => {
                    let message = format!(
                        "more than one goal in the store ({}): pass --goal <name>",
                        comma_list(&goals)
                    );
                    prop_assert_eq!(picked, Err(Failure::usage(message)));
                }
            }
        }

        #[test]
        fn a_roadmap_file_name_gives_its_version_or_nothing(
            n in 1..u64::MAX,
            junk in "[a-z0-9.]{0,8}",
        ) {
            prop_assert_eq!(roadmap_version(&format!("roadmap.v{n}.md")), Some(n));
            prop_assert_eq!(roadmap_version(&format!("roadmap.v0{n}.md")), None);
            prop_assert_eq!(
                roadmap_version(&format!("roadmap.v{n}.md{junk}")).is_some(),
                junk.is_empty()
            );
            prop_assert_eq!(roadmap_version(&format!("goal.v{n}.md")), None);
        }

        #[test]
        fn a_branch_that_is_not_a_slice_branch_names_no_goal(
            branch in "(main|feature/[a-z]{1,5}|slices/a/b)",
        ) {
            let goals = [goal("one")];
            prop_assert_eq!(which_goal(None, Some(&branch), &goals), Ok(goal("one")));
        }
    }
}
