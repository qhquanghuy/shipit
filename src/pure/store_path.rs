use std::fmt;

use crate::pure::messages::SLICE_NAME;
use crate::pure::roadmap::SliceName;

pub(crate) const REVIEW_KINDS: [&str; 4] =
    ["explanation", "informed-review", "second-opinion", "review"];

const NOT_THE_OWNERS: &str =
    "a goal version is the owner's: copy it into the store by hand (docs/store.md)";
const ROADMAP_NOT_PUT: &str = "a roadmap version is the owner's decision: the lead copies it into the store by hand once the owner approves it (docs/store.md)";
const ROUND_ONLY_FOR_REVIEWS: &str =
    "--round is only for explanation, informed-review, second-opinion and review";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Round(u32);

impl Round {
    // A plain number from 1: no sign, no leading zero, and it fits.
    fn parse(text: &str) -> Option<Self> {
        let plain = !text.starts_with('0') && text.bytes().all(|byte| byte.is_ascii_digit());
        plain.then(|| text.parse().ok()).flatten().map(Self)
    }
}

impl fmt::Display for Round {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Place {
    BuildPlan(SliceName),
    PlanReview(SliceName),
    Review {
        slice: SliceName,
        round: Round,
        kind: &'static str,
    },
}

// The kind as typed, then the values of --slice and --round when given.
pub(crate) fn place(kind: &str, slice: Option<&str>, round: Option<&str>) -> Result<Place, String> {
    let review_kind = REVIEW_KINDS.iter().find(|known| **known == kind);
    let plan_kind = matches!(kind, "build-plan" | "plan-review");
    if kind == "goal" {
        return Err(NOT_THE_OWNERS.to_owned());
    }
    if kind == "roadmap" {
        return Err(ROADMAP_NOT_PUT.to_owned());
    }
    if !plan_kind && review_kind.is_none() {
        return Err(format!(
            "shipit store put takes build-plan, plan-review, explanation, informed-review, second-opinion or review, not {kind}"
        ));
    }
    let slice = slice.ok_or_else(|| format!("{kind} needs --slice <name>"))?;
    if plan_kind && round.is_some() {
        return Err(ROUND_ONLY_FOR_REVIEWS.to_owned());
    }
    let reviewed = match (review_kind, round) {
        (Some(known), Some(text)) => Some((*known, text)),
        (Some(_), None) => return Err(format!("{kind} needs --round <n>")),
        (None, Some(_) | None) => None,
    };
    let named = SliceName::parse(slice).ok_or_else(|| format!("--slice {slice}: {SLICE_NAME}"))?;
    match reviewed {
        Some((kind, text)) => {
            let round = Round::parse(text).ok_or_else(|| {
                format!("--round {text}: a number from 1 up, with no sign and no leading zero")
            })?;
            Ok(Place::Review {
                slice: named,
                round,
                kind,
            })
        }
        None if kind == "build-plan" => Ok(Place::BuildPlan(named)),
        None => Ok(Place::PlanReview(named)),
    }
}

impl Place {
    pub(crate) const fn slice(&self) -> &SliceName {
        match self {
            Self::BuildPlan(slice) | Self::PlanReview(slice) | Self::Review { slice, .. } => slice,
        }
    }

    // A build plan and its plan review may be put again while the slice is not started.
    pub(crate) const fn replaceable(&self) -> bool {
        match self {
            Self::BuildPlan(_) | Self::PlanReview(_) => true,
            Self::Review { .. } => false,
        }
    }

    // The path parts below goals/<goal>/.
    pub(crate) fn parts(&self) -> Vec<String> {
        let slice = ["slices".to_owned(), self.slice().to_string()];
        let rest = match self {
            Self::BuildPlan(_) => vec!["build-plan.md".to_owned()],
            Self::PlanReview(_) => vec!["plan-review.md".to_owned()],
            Self::Review { round, kind, .. } => {
                vec![
                    "reviews".to_owned(),
                    format!("round-{round}"),
                    format!("{kind}.md"),
                ]
            }
        };
        slice.into_iter().chain(rest).collect()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const KINDS: [&str; 6] = [
        "build-plan",
        "plan-review",
        "explanation",
        "informed-review",
        "second-opinion",
        "review",
    ];

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn a_place_is_named_by_its_kind_and_flags(
            kind in prop::sample::select(KINDS.to_vec()),
            slice in "[a-z][a-z0-9-]{0,8}",
            round in 1_u32..,
        ) {
            prop_assume!(slice != "roadmap");
            let is_review = REVIEW_KINDS.contains(&kind);
            let found = place(kind, Some(&slice), is_review.then(|| round.to_string()).as_deref());
            let expected: Vec<String> = match kind {
                "build-plan" => vec!["slices".into(), slice.clone(), "build-plan.md".into()],
                "plan-review" => vec!["slices".into(), slice.clone(), "plan-review.md".into()],
                _ => vec![
                    "slices".into(),
                    slice.clone(),
                    "reviews".into(),
                    format!("round-{round}"),
                    format!("{kind}.md"),
                ],
            };
            let found = found.expect("a place");
            prop_assert_eq!(found.parts(), expected);
            prop_assert_eq!(found.slice().to_string(), slice);
            prop_assert_eq!(found.replaceable(), !is_review);
        }

        #[test]
        fn no_place_leaves_the_goal_folder(
            kind in prop::sample::select(KINDS.to_vec()),
            slice in prop_oneof!["[ -~]{0,8}", Just("..".to_owned()), Just(".".to_owned()),
                Just("a/b".to_owned()), Just(String::new())],
            round in prop_oneof!["[ -~]{0,8}", Just("..".to_owned()), Just("1/../2".to_owned()),
                Just(String::new())],
        ) {
            let is_review = REVIEW_KINDS.contains(&kind);
            if let Ok(found) = place(kind, Some(&slice), is_review.then_some(round.as_str())) {
                for part in found.parts() {
                    prop_assert!(!part.is_empty());
                    prop_assert!(!part.contains('/'));
                    prop_assert!(!part.starts_with('.'));
                }
            }
        }

        #[test]
        fn the_flags_must_fit_the_kind(
            kind in prop_oneof![
                prop::sample::select(KINDS.to_vec()),
                Just("goal"), Just("roadmap"), Just("other"),
            ],
            with_slice in any::<bool>(),
            with_round in any::<bool>(),
        ) {
            let found = place(
                kind,
                with_slice.then_some("alpha"),
                with_round.then_some("2"),
            );
            let is_review = REVIEW_KINDS.contains(&kind);
            let known = is_review || matches!(kind, "build-plan" | "plan-review");
            let expected = match kind {
                "goal" => Err(NOT_THE_OWNERS.to_owned()),
                "roadmap" => Err(ROADMAP_NOT_PUT.to_owned()),
                _ if !known => Err(format!("shipit store put takes build-plan, plan-review, explanation, informed-review, second-opinion or review, not {kind}")),
                _ if !with_slice => Err(format!("{kind} needs --slice <name>")),
                _ if !is_review && with_round => Err(ROUND_ONLY_FOR_REVIEWS.to_owned()),
                _ if is_review && !with_round => Err(format!("{kind} needs --round <n>")),
                _ => Ok(()),
            };
            prop_assert_eq!(found.map(|_place| ()), expected);
        }

        #[test]
        fn a_slice_that_is_not_a_name_and_a_bad_round_are_named(
            slice in "[A-Z ./][ -~]{0,5}",
            round in "[+\\-0 a-z.][ -~]{0,3}",
        ) {
            prop_assert_eq!(
                place("build-plan", Some(&slice), None),
                Err(format!("--slice {slice}: {SLICE_NAME}"))
            );
            prop_assert_eq!(
                place("review", Some("alpha"), Some(&round)),
                Err(format!("--round {round}: a number from 1 up, with no sign and no leading zero"))
            );
        }

        #[test]
        fn a_round_is_a_plain_number_from_one(text in "[0-9+a-z-]{0,12}") {
            let plain = text.starts_with(|c: char| ('1'..='9').contains(&c))
                && text.chars().all(|c| c.is_ascii_digit())
                && text.parse::<u32>().is_ok();
            match Round::parse(&text) {
                Some(round) => {
                    prop_assert!(plain);
                    prop_assert_eq!(round.to_string(), text);
                }
                None => prop_assert!(!plain),
            }
        }
    }
}
