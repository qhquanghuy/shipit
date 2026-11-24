use std::fmt;

use crate::pure::goal::GoalName;
use crate::pure::messages::SLICE_NAME;
use crate::pure::roadmap::SliceName;

pub(crate) const REVIEW_KINDS: [&str; 4] =
    ["explanation", "informed-review", "second-opinion", "review"];

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
    if !plan_kind && review_kind.is_none() {
        return Err(format!(
            "shipit store put takes goal, roadmap, build-plan, plan-review, explanation, informed-review, second-opinion or review, not {kind}"
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OwnerFile {
    Goal,
    Roadmap,
}

impl OwnerFile {
    pub(crate) const fn stem(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Roadmap => "roadmap",
        }
    }

    pub(crate) fn name(self, version: u64) -> String {
        format!("{}.v{version}.md", self.stem())
    }

    // The version of a file name `<stem>.v<N>.md`, N with no sign and no leading zero.
    pub(crate) fn version(self, file_name: &str) -> Option<u64> {
        let digits = file_name
            .strip_prefix(self.stem())?
            .strip_prefix(".v")?
            .strip_suffix(".md")?;
        let plain = !digits.starts_with('0') && digits.bytes().all(|byte| byte.is_ascii_digit());
        if plain { digits.parse().ok() } else { None }
    }

    // At u64::MAX the name is taken, so the put is refused.
    pub(crate) fn next(newest: Option<u64>) -> u64 {
        newest.map_or(1, |n| n.saturating_add(1))
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        [Self::Goal, Self::Roadmap]
            .into_iter()
            .find(|kind| kind.stem() == text)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PutKind {
    Owner(OwnerFile),
    Placed(Place),
}

fn no_flags(kind: &str) -> String {
    format!("{kind} takes no --slice or --round")
}

// The kind as typed, then the values of --slice and --round when given.
pub(crate) fn put_kind(
    kind: &str,
    slice: Option<&str>,
    round: Option<&str>,
) -> Result<PutKind, String> {
    match OwnerFile::parse(kind) {
        Some(_) if slice.is_some() || round.is_some() => Err(no_flags(kind)),
        Some(owned) => Ok(PutKind::Owner(owned)),
        None => place(kind, slice, round).map(PutKind::Placed),
    }
}

// The goal a roadmap names in its Slices block; None when it cannot be read.
pub(crate) fn roadmap_goal(text: &str) -> Option<GoalName> {
    let block = crate::pure::block::toml_block(text, "## Slices").ok()??;
    let table = crate::pure::setup::toml_table(block.as_bytes()).ok()?;
    Some(crate::pure::roadmap::parse_roadmap(&table).ok()?.goal)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Shown {
    Goal,
    Roadmap,
    Placed(Place),
}

// The kind as typed, then the values of --slice and --round when given. A goal and a roadmap are
// shown whole; the other kinds are placed as a put places them.
pub(crate) fn shown(kind: &str, slice: Option<&str>, round: Option<&str>) -> Result<Shown, String> {
    let whole = match kind {
        "goal" => Some(Shown::Goal),
        "roadmap" => Some(Shown::Roadmap),
        _ => None,
    };
    let known = REVIEW_KINDS.contains(&kind) || matches!(kind, "build-plan" | "plan-review");
    match whole {
        Some(_) if slice.is_some() || round.is_some() => Err(no_flags(kind)),
        Some(found) => Ok(found),
        None if known => place(kind, slice, round).map(Shown::Placed),
        None => Err(format!(
            "shipit store show takes goal, roadmap, build-plan, plan-review, explanation, informed-review, second-opinion or review, not {kind}"
        )),
    }
}

impl Shown {
    // The folder of a copy in the drafts folder: the slice, or the goal for the two that
    // belong to no slice.
    pub(crate) fn folder_name(&self, goal: &GoalName) -> String {
        match self {
            Self::Goal | Self::Roadmap => goal.to_string(),
            Self::Placed(place) => place.slice().to_string(),
        }
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

    // The name of a copy: the rounds of one slice share its folder, so the round comes first.
    pub(crate) fn copy_name(&self) -> String {
        match self {
            Self::BuildPlan(_) => "build-plan.md".to_owned(),
            Self::PlanReview(_) => "plan-review.md".to_owned(),
            Self::Review { round, kind, .. } => format!("round-{round}-{kind}.md"),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::goal_pick::roadmap_version;

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
        fn a_copy_is_filed_by_its_slice_or_goal_and_named_by_its_round(
            kind in prop::sample::select(KINDS.to_vec()),
            slice in "[a-z][a-z0-9-]{0,8}",
            round in 1_u32..,
        ) {
            prop_assume!(slice != "roadmap");
            let is_review = REVIEW_KINDS.contains(&kind);
            let found = place(kind, Some(&slice), is_review.then(|| round.to_string()).as_deref())
                .expect("a place");
            let expected = if is_review {
                format!("round-{round}-{kind}.md")
            } else {
                format!("{kind}.md")
            };
            prop_assert_eq!(found.copy_name(), expected);
            let goal = GoalName::parse("demo").expect("a goal name");
            prop_assert_eq!(Shown::Placed(found).folder_name(&goal), slice);
            prop_assert_eq!(Shown::Goal.folder_name(&goal), "demo".to_owned());
            prop_assert_eq!(Shown::Roadmap.folder_name(&goal), "demo".to_owned());
        }

        #[test]
        fn a_shown_kind_names_its_store_file(
            kind in prop_oneof![
                prop::sample::select(KINDS.to_vec()),
                Just("goal"), Just("roadmap"), Just("other"),
            ],
            slice in prop::option::of("[a-z][a-z0-9-]{0,8}"),
            round in prop::option::of(1_u32..),
        ) {
            prop_assume!(slice.as_deref() != Some("roadmap"));
            let round = round.map(|round| round.to_string());
            let found = shown(kind, slice.as_deref(), round.as_deref());
            let flagged = slice.is_some() || round.is_some();
            match kind {
                "goal" | "roadmap" if flagged => prop_assert_eq!(
                    found,
                    Err(format!("{kind} takes no --slice or --round"))
                ),
                "goal" => prop_assert_eq!(found, Ok(Shown::Goal)),
                "roadmap" => prop_assert_eq!(found, Ok(Shown::Roadmap)),
                "other" => prop_assert_eq!(
                    found,
                    Err("shipit store show takes goal, roadmap, build-plan, plan-review, explanation, informed-review, second-opinion or review, not other".to_owned())
                ),
                _ => prop_assert_eq!(
                    found,
                    place(kind, slice.as_deref(), round.as_deref()).map(Shown::Placed)
                ),
            }
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
            let found = put_kind(
                kind,
                with_slice.then_some("alpha"),
                with_round.then_some("2"),
            );
            let is_review = REVIEW_KINDS.contains(&kind);
            let known = is_review || matches!(kind, "build-plan" | "plan-review");
            let owned = matches!(kind, "goal" | "roadmap");
            let expected = match kind {
                _ if owned && (with_slice || with_round) => Err(format!("{kind} takes no --slice or --round")),
                _ if owned => Ok(()),
                _ if !known => Err(format!("shipit store put takes goal, roadmap, build-plan, plan-review, explanation, informed-review, second-opinion or review, not {kind}")),
                _ if !with_slice => Err(format!("{kind} needs --slice <name>")),
                _ if !is_review && with_round => Err(ROUND_ONLY_FOR_REVIEWS.to_owned()),
                _ if is_review && !with_round => Err(format!("{kind} needs --round <n>")),
                _ => Ok(()),
            };
            prop_assert_eq!(found.map(|_kind| ()), expected);
        }

        #[test]
        fn an_owner_version_is_one_above_the_newest(
            n in 1_u64..,
            rest in "[a-z0-9.]{0,12}",
        ) {
            for kind in [OwnerFile::Goal, OwnerFile::Roadmap] {
                prop_assert_eq!(kind.version(&kind.name(n)), Some(n));
                prop_assert_eq!(kind.version(&format!("{}.v0{n}.md", kind.stem())), None);
                prop_assert_eq!(OwnerFile::parse(kind.stem()), Some(kind));
            }
            prop_assert_eq!(OwnerFile::Goal.version(&OwnerFile::Roadmap.name(n)), None);
            prop_assert_eq!(OwnerFile::next(None), 1);
            prop_assert_eq!(OwnerFile::next(Some(n)), n.saturating_add(1));
            prop_assert_eq!(
                OwnerFile::Goal.version(&format!("goal{rest}")),
                roadmap_version(&format!("roadmap{rest}"))
            );
            prop_assert_eq!(
                OwnerFile::Roadmap.version(&format!("roadmap{rest}")),
                roadmap_version(&format!("roadmap{rest}"))
            );
        }

        #[test]
        fn a_roadmap_names_its_goal(
            goal in "[a-z][a-z0-9-]{0,8}",
            prose in "[a-z ]{0,20}",
        ) {
            let text = format!(
                "# Roadmap\n\n{prose}\n\n## Slices\n\n```toml\ngoal = \"{goal}\"\n\n[[slice]]\nname = \"a\"\nafter = []\nmeets = [\"DW1\"]\nowns = [\"f/\"]\ntest_drive = false\n```\n"
            );
            prop_assert_eq!(roadmap_goal(&text), GoalName::parse(&goal));
            prop_assert_eq!(roadmap_goal(&text.replace("## Slices", "## Other")), None);
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
