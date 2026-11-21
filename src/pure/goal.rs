use std::fmt;

use serde::{Serialize, Serializer};

const HEADING: &str = "## Done-when";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DoneWhen(u64);

impl fmt::Display for DoneWhen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DW{}", self.0)
    }
}

impl Serialize for DoneWhen {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub(crate) struct GoalName(String);

impl GoalName {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        let valid = chars.next().is_some_and(|first| first.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        valid.then(|| Self(text.to_owned()))
    }
}

impl fmt::Display for GoalName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// The form for views that span goals; inside one goal a plain `DW<n>` means its own.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct QualifiedDoneWhen {
    pub(crate) goal: GoalName,
    pub(crate) id: DoneWhen,
}

impl QualifiedDoneWhen {
    pub(crate) fn of(goal: &GoalName, id: DoneWhen) -> Self {
        Self {
            goal: goal.clone(),
            id,
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        let (goal, id) = text.split_once(':')?;
        Some(Self {
            goal: GoalName::parse(goal)?,
            id: parse_done_when(id)?,
        })
    }
}

impl fmt::Display for QualifiedDoneWhen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.goal, self.id)
    }
}

impl Serialize for QualifiedDoneWhen {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

// No sign, no leading zero: "01" and "1" must not name the same item.
fn number(digits: &str) -> Option<u64> {
    let first = digits.chars().next()?;
    if first == '0' || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

pub(crate) fn parse_done_when(text: &str) -> Option<DoneWhen> {
    number(text.strip_prefix("DW")?).map(DoneWhen)
}

fn item(line: &str) -> Option<DoneWhen> {
    let (digits, _text) = line.split_once(". ")?;
    number(digits).map(DoneWhen)
}

// Items come only from the lines of the "## Done-when" section that are outside fenced blocks.
pub(crate) fn goal_items(text: &str) -> Result<Vec<DoneWhen>, String> {
    let mut fenced = false;
    let mut items: Vec<DoneWhen> = Vec::new();
    for line in text.lines().skip_while(|line| *line != HEADING).skip(1) {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            if line.starts_with("## ") {
                break;
            }
            if let Some(found) = item(line) {
                if items.contains(&found) {
                    return Err(format!("goal: {found} is numbered twice"));
                }
                items.push(found);
            }
        }
    }
    if items.is_empty() {
        Err(format!("goal: no numbered items under \"{HEADING}\""))
    } else {
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use super::*;

    fn distinct_numbers() -> impl Strategy<Value = Vec<u64>> {
        proptest::collection::btree_set(1_u64..1000, 1..8)
            .prop_map(|numbers| numbers.into_iter().collect::<Vec<_>>())
            .prop_shuffle()
    }

    fn fence_with(numbers: &[u64]) -> String {
        let mut text = String::from("```\n");
        for n in numbers {
            writeln!(text, "{n}. inside").ok();
        }
        text.push_str("```\n");
        text
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn items_are_read_only_under_done_when(
            numbers in distinct_numbers(),
            noise in proptest::collection::vec(1_u64..1000, 0..4),
        ) {
            let mut text = format!("# Goal\n\n1. before the heading\n{HEADING}\n\n");
            for n in &numbers {
                writeln!(text, "{n}. item").ok();
                text.push_str(&fence_with(&noise));
            }
            text.push_str("## Next\n\n");
            text.push_str(&fence_with(&noise));
            text.push_str("1. after the section\n");
            let wanted: Vec<DoneWhen> = numbers.iter().copied().map(DoneWhen).collect();
            prop_assert_eq!(goal_items(&text), Ok(wanted));
        }

        #[test]
        fn done_when_prints_and_serializes_as_dw_n(n in 1_u64..u64::MAX, m in 1_u64..u64::MAX) {
            let id = format!("DW{n}");
            prop_assert_eq!(parse_done_when(&id), Some(DoneWhen(n)));
            prop_assert_eq!(DoneWhen(n).to_string(), id.clone());
            prop_assert_eq!(serde_json::to_string(&DoneWhen(n)).ok(), Some(format!("\"{id}\"")));
            prop_assert_eq!(DoneWhen(n) < DoneWhen(m), n < m);
            let map = BTreeMap::from([(DoneWhen(n.min(m)), 0_u8)]);
            prop_assert_eq!(
                serde_json::to_string(&map).ok(),
                Some(format!("{{\"DW{}\":0}}", n.min(m)))
            );
        }

        #[test]
        fn ids_outside_the_pattern_are_refused(n in 1_u64..1000, junk in "[a-z]{1,3}") {
            for text in [
                format!("dw{n}"),
                format!("DW0{n}"),
                format!("DW{n}{junk}"),
                format!("DW-{n}"),
                format!(" DW{n}"),
                "DW".to_owned(),
                "DW0".to_owned(),
                "DW99999999999999999999999".to_owned(),
            ] {
                prop_assert_eq!(parse_done_when(&text), None, "{}", text);
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn goal_names_and_qualified_ids_round_trip(
            goal in "[a-z][a-z0-9-]{0,8}",
            n in 1_u64..u64::MAX,
            bad in "[^a-z0-9:-]",
            digit_first in "[0-9-][a-z0-9-]{0,5}",
        ) {
            let text = format!("{goal}:DW{n}");
            let parsed = QualifiedDoneWhen::parse(&text).expect("a qualified id");
            prop_assert_eq!(parsed.to_string(), text.clone());
            prop_assert_eq!(parsed.goal.to_string(), goal.clone());
            prop_assert_eq!(
                serde_json::to_string(&parsed).ok(),
                Some(format!("\"{text}\""))
            );
            prop_assert_eq!(
                QualifiedDoneWhen::of(&GoalName::parse(&goal).expect("a goal"), DoneWhen(n)),
                parsed
            );
            for refused in [
                format!("{goal}{bad}DW{n}"),
                format!("{bad}{goal}:DW{n}"),
                format!("{digit_first}:DW{n}"),
                format!("{goal}:{n}"),
                format!("{goal}:DW0"),
                format!(":DW{n}"),
                format!("{goal}:{goal}:DW{n}"),
                format!("DW{n}"),
            ] {
                prop_assert_eq!(QualifiedDoneWhen::parse(&refused), None, "{}", refused);
            }
            prop_assert_eq!(GoalName::parse(&format!("{goal}{bad}")), None);
            prop_assert_eq!(GoalName::parse(&digit_first), None);
        }
    }

    #[test]
    fn a_repeated_number_is_refused() {
        let text = format!("{HEADING}\n\n1. a\n2. b\n1. c\n2. d\n");
        assert_eq!(
            goal_items(&text),
            Err("goal: DW1 is numbered twice".to_owned())
        );
    }

    #[test]
    fn no_numbered_items_are_refused() {
        let text = format!("{HEADING}\n\n- a bullet\n## Next\n1. elsewhere\n");
        let message = Err("goal: no numbered items under \"## Done-when\"".to_owned());
        assert_eq!(goal_items(&text), message);
        assert_eq!(goal_items("1. no heading\n"), message);
    }
}
