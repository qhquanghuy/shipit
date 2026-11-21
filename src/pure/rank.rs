use std::cmp::Ordering;
use std::fmt;

use serde::Serialize;

use crate::pure::failure::Failure;
use crate::pure::setup::{Effort, Role, Setup, Tier};

pub(crate) const fn effort_name(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "xhigh",
        Effort::Max => "max",
    }
}

// `<model>:<effort>`, or `<model>:<effort>@<tier>` for a stand-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Worker {
    pub(crate) model: String,
    pub(crate) effort: Effort,
    pub(crate) stands_in: Option<u64>,
}

impl fmt::Display for Worker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.model, effort_name(self.effort))?;
        match self.stands_in {
            Some(tier) => write!(f, "@{tier}"),
            None => Ok(()),
        }
    }
}

pub(crate) fn parse_worker(text: &str) -> Option<Worker> {
    let (head, tier) = match text.split_once('@') {
        Some((head, tier)) => (head, Some(tier)),
        None => (text, None),
    };
    let stands_in = match tier {
        Some(digits) if digits.bytes().all(|b| b.is_ascii_digit()) => Some(digits.parse().ok()?),
        Some(_) => return None,
        None => None,
    };
    let (model, effort) = head.split_once(':')?;
    let plain = !model.is_empty() && !model.contains(char::is_whitespace);
    plain.then_some(Worker {
        model: model.to_owned(),
        effort: Effort::parse(effort)?,
        stands_in,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Rank {
    pub(crate) model: String,
    pub(crate) effort: Effort,
    pub(crate) tier: u64,
    pub(crate) stand_in: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Basis {
    Tier,
    Effort,
    Fresh,
}

impl fmt::Display for Basis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tier => "tier",
            Self::Effort => "effort",
            Self::Fresh => "fresh",
        })
    }
}

fn rank_of(policy: &Setup, worker: &Worker) -> Result<Rank, Failure> {
    let holders: Vec<&Tier> = policy
        .agents
        .tiers
        .iter()
        .filter(|tier| tier.model.as_str() == worker.model)
        .collect();
    let own = match holders.as_slice() {
        [] => {
            return Err(Failure::usage(format!(
                "model \"{}\" is not in the setup's tiers",
                worker.model
            )));
        }
        [only] => only,
        [_, _, ..] => {
            return Err(Failure::usage(format!(
                "model \"{}\" is in several tiers",
                worker.model
            )));
        }
    };
    let (tier, stand_in) = match worker.stands_in {
        None => (u64::from(&own.tier), false),
        Some(number) => {
            let is_it = policy.agents.tiers.iter().any(|tier| {
                u64::from(&tier.tier) == number
                    && tier.stand_in.as_ref().is_some_and(|stand_in| {
                        stand_in.model.as_str() == worker.model && stand_in.effort == worker.effort
                    })
            });
            if !is_it {
                return Err(Failure::usage(format!(
                    "{}:{} is not the stand-in for tier {number}",
                    worker.model,
                    effort_name(worker.effort)
                )));
            }
            (number, true)
        }
    };
    Ok(Rank {
        model: worker.model.clone(),
        effort: worker.effort,
        tier,
        stand_in,
    })
}

fn fresh_allowed(policy: &Setup, author: &Rank, by: &Rank) -> bool {
    let top = policy
        .agents
        .tiers
        .iter()
        .map(|tier| u64::from(&tier.tier))
        .max();
    author.tier == by.tier
        && author.effort == by.effort
        && Some(author.tier) == top
        && author.effort == policy.agents.effort_cap
}

// The order of the rules is the order of the messages a worker sees (docs: `shipit help receipt`).
pub(crate) fn rank_verdict(
    policy: &Setup,
    author: &Worker,
    by: &Worker,
    fresh: bool,
) -> Result<(Rank, Rank, Basis), Failure> {
    let author_rank = rank_of(policy, author)?;
    let by_rank = rank_of(policy, by)?;
    let cap = policy.agents.effort_cap;
    for rank in [&author_rank, &by_rank] {
        if rank.effort > cap {
            return Err(Failure::no(format!(
                "effort {} is above the setup's effort cap ({})",
                effort_name(rank.effort),
                effort_name(cap)
            )));
        }
    }
    if fresh && !fresh_allowed(policy, &author_rank, &by_rank) {
        return Err(Failure::usage(
            "--fresh is only for a reviewer at the author's rank when the author is at the top \
             tier and the effort cap",
        ));
    }
    if author.model == by.model && author.effort == by.effort {
        return if fresh {
            Ok((author_rank, by_rank, Basis::Fresh))
        } else {
            Err(Failure::no(format!(
                "{by} is the same model and effort as the author ({author}): use another model \
                 or a higher effort"
            )))
        };
    }
    let basis = match (
        by_rank.tier.cmp(&author_rank.tier),
        by_rank.effort.cmp(&author_rank.effort),
    ) {
        (Ordering::Greater, _) => Some(Basis::Tier),
        (Ordering::Equal, Ordering::Greater) => Some(Basis::Effort),
        (Ordering::Equal, Ordering::Equal) if fresh => Some(Basis::Fresh),
        (Ordering::Equal, Ordering::Equal | Ordering::Less) | (Ordering::Less, _) => None,
    };
    match basis {
        Some(basis) => Ok((author_rank, by_rank, basis)),
        None => Err(Failure::no(format!(
            "{by} does not rank above {author} (tier {}, effort {}): use a higher tier or effort",
            author_rank.tier,
            effort_name(author_rank.effort)
        ))),
    }
}

// The author is the setup's role when it has the role's tier, and its effort when the role sets one.
pub(crate) fn role_notice(name: &str, role: &Role, author: &Worker, rank: &Rank) -> Option<String> {
    let tier = u64::from(&role.tier);
    if rank.tier == tier && role.effort.is_none_or(|effort| effort == rank.effort) {
        return None;
    }
    let effort = role
        .effort
        .map(|effort| format!(", effort {}", effort_name(effort)))
        .unwrap_or_default();
    Some(format!(
        "--author {author} is not the setup's {name} (tier {tier}{effort})"
    ))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use toml::Value;

    use super::*;
    use crate::pure::setup::{parse_setup, toml_table};
    use crate::pure::testing::setup_with;

    const MODELS: [&str; 4] = ["haiku", "sonnet", "opus", "fable"];
    const EFFORTS: [&str; 3] = ["low", "medium", "high"];

    fn worker(text: &str) -> Worker {
        parse_worker(text).expect("a worker")
    }

    fn tier_of(model: &str) -> u64 {
        MODELS
            .iter()
            .position(|name| *name == model)
            .map_or(0, |index| u64::try_from(index).unwrap_or(0) + 1)
    }

    fn effort_rank(effort: &str) -> usize {
        EFFORTS.iter().position(|name| *name == effort).unwrap_or(0)
    }

    fn some_worker() -> impl Strategy<Value = String> {
        (0..MODELS.len(), 0..EFFORTS.len())
            .prop_map(|(model, effort)| format!("{}:{}", MODELS[model], EFFORTS[effort]))
    }

    fn exit_of(failure: &Failure) -> u8 {
        failure.exit.code()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn rank_is_tier_then_effort(author in some_worker(), by in some_worker()) {
            let policy = setup_with(&[]);
            let (author_model, author_effort) = author.split_once(':').expect("a pair");
            let (by_model, by_effort) = by.split_once(':').expect("a pair");
            let author_key = (tier_of(author_model), effort_rank(author_effort));
            let by_key = (tier_of(by_model), effort_rank(by_effort));
            let verdict = rank_verdict(&policy, &worker(&author), &worker(&by), false);
            match by_key.cmp(&author_key) {
                Ordering::Greater => {
                    let expected = if by_key.0 > author_key.0 { Basis::Tier } else { Basis::Effort };
                    prop_assert_eq!(verdict.map(|(_, _, basis)| basis), Ok(expected));
                }
                Ordering::Equal | Ordering::Less => {
                    prop_assert_eq!(verdict.map_err(|failure| exit_of(&failure)), Err(1));
                }
            }
        }

        #[test]
        fn any_effort_above_the_cap_is_refused(
            high in prop_oneof![Just("xhigh"), Just("max")],
            model in 0..MODELS.len(),
            other in some_worker(),
            first in any::<bool>(),
        ) {
            let policy = setup_with(&[]);
            let loud = format!("{}:{high}", MODELS[model]);
            let (author, by) = if first { (loud, other) } else { (other, loud) };
            let verdict = rank_verdict(&policy, &worker(&author), &worker(&by), false);
            let failure = verdict.expect_err("above the cap");
            prop_assert_eq!(exit_of(&failure), 1);
            prop_assert!(failure.message.contains("is above the setup's effort cap (high)"));
        }

        #[test]
        fn a_pass_with_the_same_real_model_and_effort_needs_fresh_top_tier_and_cap(
            same in some_worker(),
        ) {
            let policy = setup_with(&[]);
            let one = worker(&same);
            let plain = rank_verdict(&policy, &one, &one, false).expect_err("same worker");
            prop_assert_eq!(exit_of(&plain), 1);
            let fresh = rank_verdict(&policy, &one, &one, true);
            if same == "fable:high" {
                prop_assert_eq!(fresh.map(|(_, _, basis)| basis), Ok(Basis::Fresh));
            } else {
                prop_assert_eq!(fresh.map_err(|failure| exit_of(&failure)), Err(2));
            }
        }

        #[test]
        fn fresh_needs_the_authors_exact_rank_at_top_tier_and_cap(
            author in prop_oneof![some_worker(), Just("opus:high@4".to_owned())],
            by in prop_oneof![some_worker(), Just("opus:high@4".to_owned())],
        ) {
            let policy = setup_with(&[]);
            let verdict = rank_verdict(&policy, &worker(&author), &worker(&by), true);
            let at_the_top = |text: &str| text == "fable:high" || text == "opus:high@4";
            if at_the_top(&author) && at_the_top(&by) {
                prop_assert_eq!(verdict.map(|(_, _, basis)| basis), Ok(Basis::Fresh));
            } else {
                prop_assert_eq!(verdict.map_err(|failure| exit_of(&failure)), Err(2));
            }
        }

        #[test]
        fn a_stand_in_counts_as_its_tier_and_is_recorded(
            author in (0..2_usize, 0..EFFORTS.len())
                .prop_map(|(model, effort)| format!("{}:{}", MODELS[model], EFFORTS[effort])),
        ) {
            let policy = setup_with(&[]);
            let (_, by, basis) = rank_verdict(&policy, &worker(&author), &worker("opus:high@4"), false)
                .expect("a tier 4 stand-in is above tier 1 and 2");
            prop_assert_eq!((by.tier, by.stand_in, basis), (4, true, Basis::Tier));
        }

        #[test]
        fn worker_text_parses_iff_model_colon_effort_with_optional_tier(
            model in "[a-z]{1,6}",
            effort in 0..5_usize,
            tier in proptest::option::of(1..9_u64),
        ) {
            let names = ["low", "medium", "high", "xhigh", "max"];
            let text = match tier {
                Some(tier) => format!("{model}:{}@{tier}", names[effort]),
                None => format!("{model}:{}", names[effort]),
            };
            let parsed = parse_worker(&text).expect("parses");
            prop_assert_eq!(parsed.stands_in, tier);
            prop_assert_eq!(parsed.to_string(), text);
            for bad in [format!("{model}{}", names[effort]), format!("{model}:fast"), format!("{model}:high@x"), format!(":{}", names[effort])] {
                prop_assert_eq!(parse_worker(&bad), None);
            }
        }
    }

    #[test]
    fn a_model_in_two_tiers_is_refused() {
        let text = include_str!("../../tests/fixtures/setup-valid.toml")
            .replace("model = \"fable\"", "model = \"opus\"");
        let table = toml_table(text.as_bytes()).expect("toml");
        let policy = parse_setup(&table).expect("a valid setup");
        let failure = rank_verdict(&policy, &worker("opus:low"), &worker("sonnet:low"), false)
            .expect_err("two tiers");
        assert_eq!(
            failure,
            Failure::usage("model \"opus\" is in several tiers")
        );
        let none = rank_verdict(
            &setup_with(&[]),
            &worker("gpt:low"),
            &worker("opus:low"),
            false,
        );
        assert_eq!(
            none.expect_err("no tier"),
            Failure::usage("model \"gpt\" is not in the setup's tiers")
        );
    }

    #[test]
    fn only_the_setups_stand_in_may_use_a_tier_tag() {
        let policy = setup_with(&[]);
        for text in ["opus:medium@4", "opus:high@3", "sonnet:high@4"] {
            let failure = rank_verdict(&policy, &worker("haiku:low"), &worker(text), false)
                .expect_err("not a stand-in");
            assert_eq!(exit_of(&failure), 2, "{text}");
        }
        let failure = rank_verdict(
            &policy,
            &worker("haiku:low"),
            &worker("opus:medium@4"),
            false,
        )
        .expect_err("not a stand-in");
        assert_eq!(
            failure.message,
            "opus:medium is not the stand-in for tier 4"
        );
    }

    #[test]
    fn the_notice_for_an_author_who_is_not_the_planner() {
        let policy = setup_with(&[]);
        let author = worker("sonnet:high");
        let (rank, _, _) =
            rank_verdict(&policy, &author, &worker("fable:low"), false).expect("fable ranks above");
        assert_eq!(
            role_notice("planner", &policy.agents.roles.planner, &author, &rank),
            Some(
                "--author sonnet:high is not the setup's planner (tier 3, effort high)".to_owned()
            )
        );
        let planner = worker("opus:high");
        let (rank, _, _) = rank_verdict(&policy, &planner, &worker("fable:low"), false)
            .expect("fable ranks above");
        assert_eq!(
            role_notice("planner", &policy.agents.roles.planner, &planner, &rank),
            None
        );
        let loose = setup_with(&[("agents.roles.reader.tier", Value::Integer(2))]);
        let (rank, _, _) =
            rank_verdict(&loose, &author, &worker("fable:low"), false).expect("fable ranks above");
        assert_eq!(
            role_notice("reader", &loose.agents.roles.reader, &author, &rank),
            None,
            "a role without an effort takes any effort of its tier"
        );
    }
}
