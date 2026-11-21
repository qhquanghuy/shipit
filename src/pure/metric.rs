use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use serde_json::Value;
use toml::Table;

use crate::pure::goal::GoalName;
use crate::pure::roadmap::SliceName;

// Below this many other slices there is no history worth comparing with.
pub(crate) const MIN_HISTORY: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub(crate) struct MetricName(String);

impl MetricName {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        let plain = chars.next().is_some_and(|first| first.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        plain.then(|| Self(text.to_owned()))
    }
}

impl fmt::Display for MetricName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// A measured number: finite and not negative, so it has an order but no `Eq`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub(crate) struct MetricValue(f64);

impl MetricValue {
    pub(crate) fn from_number(value: f64) -> Option<Self> {
        (value.is_finite() && value >= 0.0 && !value.is_sign_negative()).then_some(Self(value))
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        Self::from_number(text.parse().ok()?)
    }
}

impl fmt::Display for MetricValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fence {
    pub(crate) above: f64,
    pub(crate) from: usize,
}

// Higher is worse for every metric: only a value above Q3 + 1.5 times the spread is flagged.
pub(crate) fn outlier(value: MetricValue, history: &[MetricValue]) -> Option<Fence> {
    let n = history.len();
    if n < MIN_HISTORY {
        return None;
    }
    let mut sorted: Vec<f64> = history.iter().map(|kept| kept.0).collect();
    sorted.sort_by(f64::total_cmp);
    let at = |rank: usize| sorted.get(rank.saturating_sub(1)).copied();
    let q1 = at(n.div_ceil(4))?;
    let q3 = at((3 * n).div_ceil(4))?;
    let above = 1.5_f64.mul_add(q3 - q1, q3);
    (value.0 > above).then_some(Fence { above, from: n })
}

// One measurement of a slice's work, as `receipts/metric/<tree>/<name>.json` says it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetricRecord {
    pub(crate) key: String,
    pub(crate) goal: GoalName,
    pub(crate) slice: SliceName,
    pub(crate) name: MetricName,
    pub(crate) value: MetricValue,
    pub(crate) time: String,
}

// A record of work that is on no slice branch has no slice to count, so it is not read.
pub(crate) fn read_record(doc: &Value) -> Option<MetricRecord> {
    if doc.get("kind")?.as_str()? != "metric" {
        return None;
    }
    Some(MetricRecord {
        key: doc.get("key")?.as_str()?.to_owned(),
        goal: GoalName::parse(doc.get("goal")?.as_str()?)?,
        slice: SliceName::parse(doc.get("slice")?.as_str()?)?,
        name: MetricName::parse(doc.get("name")?.as_str()?)?,
        value: MetricValue::from_number(doc.get("value")?.as_f64()?)?,
        time: doc.get("time")?.as_str()?.to_owned(),
    })
}

pub(crate) type SliceMetrics = BTreeMap<SliceName, BTreeMap<MetricName, MetricValue>>;

// The numbers `shipit complete` keeps for a goal: one table per slice.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KeptMetrics {
    pub(crate) goal: GoalName,
    pub(crate) slices: SliceMetrics,
}

fn number(value: &toml::Value) -> Option<MetricValue> {
    let text = match value {
        toml::Value::Integer(whole) => whole.to_string(),
        toml::Value::Float(fraction) => fraction.to_string(),
        toml::Value::String(_)
        | toml::Value::Boolean(_)
        | toml::Value::Datetime(_)
        | toml::Value::Array(_)
        | toml::Value::Table(_) => return None,
    };
    MetricValue::parse(&text)
}

// The `[complete]` table holds the issues made on the way out, which are text and not numbers.
pub(crate) fn parse_kept(goal: GoalName, table: &Table) -> Option<KeptMetrics> {
    let mut slices = SliceMetrics::new();
    for (key, value) in table {
        if key == "complete" {
            continue;
        }
        let mut numbers = BTreeMap::new();
        for (name, kept) in value.as_table()? {
            numbers.insert(MetricName::parse(name)?, number(kept)?);
        }
        slices.insert(SliceName::parse(key)?, numbers);
    }
    Some(KeptMetrics { goal, slices })
}

// One number for each other slice, whichever goal it belongs to: its newest record, and the
// number a completed goal kept for it, so that the history outlives the goal. A record of a goal
// that has a kept file is skipped, because the kept numbers stand for it.
pub(crate) fn history(
    records: &[MetricRecord],
    kept: &[KeptMetrics],
    name: &MetricName,
    except: Option<(&GoalName, &SliceName)>,
) -> Vec<MetricValue> {
    let mut newest: BTreeMap<(&GoalName, &SliceName), &MetricRecord> = BTreeMap::new();
    for record in records.iter().filter(|record| record.name == *name) {
        let slice = (&record.goal, &record.slice);
        let gone = kept.iter().any(|goal| goal.goal == record.goal);
        if except == Some(slice) || gone {
            continue;
        }
        let replaces = newest
            .get(&slice)
            .is_none_or(|older| older.time <= record.time);
        if replaces {
            newest.insert(slice, record);
        }
    }
    let mut values: Vec<MetricValue> = newest.values().map(|record| record.value).collect();
    values.extend(
        kept.iter()
            .flat_map(|goal| goal.slices.values())
            .filter_map(|numbers| numbers.get(name).copied()),
    );
    values
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn value(n: u32) -> MetricValue {
        MetricValue::parse(&n.to_string()).expect("a number")
    }

    fn values(numbers: &[u32]) -> Vec<MetricValue> {
        numbers.iter().map(|n| value(*n)).collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn fewer_than_ten_slices_never_flag(
            numbers in proptest::collection::vec(0..1000_u32, 0..MIN_HISTORY),
            seen in 0..1_000_000_u32,
        ) {
            prop_assert_eq!(outlier(value(seen), &values(&numbers)), None);
        }

        #[test]
        fn a_value_at_or_below_the_median_is_never_an_outlier(
            numbers in proptest::collection::vec(0..1000_u32, MIN_HISTORY..30),
        ) {
            let mut sorted = numbers.clone();
            sorted.sort_unstable();
            let median = sorted.get(sorted.len() / 2).copied().expect("a median");
            prop_assert_eq!(outlier(value(median), &values(&numbers)), None);
        }

        #[test]
        fn outliers_are_monotone(
            numbers in proptest::collection::vec(0..1000_u32, MIN_HISTORY..30),
            low in 0..2000_u32,
            more in 0..2000_u32,
        ) {
            let history = values(&numbers);
            if outlier(value(low), &history).is_some() {
                prop_assert!(outlier(value(low + more), &history).is_some());
            }
        }

        #[test]
        fn metric_values_parse_iff_finite_and_not_negative(x in any::<f64>()) {
            prop_assert_eq!(
                MetricValue::parse(&x.to_string()).is_some(),
                x.is_finite() && x >= 0.0 && !x.is_sign_negative()
            );
        }
    }

    #[test]
    fn the_fence_is_q3_plus_one_and_a_half_spreads() {
        let history = values(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let fence = Fence {
            above: 8.0 + 1.5 * 5.0,
            from: 10,
        };
        assert_eq!(outlier(value(15), &history), None);
        assert_eq!(outlier(value(16), &history), Some(fence));
    }

    #[test]
    fn history_takes_one_number_per_other_slice_and_the_kept_ones() {
        let goal = |text: &str| GoalName::parse(text).expect("a goal name");
        let slice = |text: &str| SliceName::parse(text).expect("a slice name");
        let record = |g: &str, s: &str, n: u32, time: &str| MetricRecord {
            key: "k".to_owned(),
            goal: goal(g),
            slice: slice(s),
            name: MetricName::parse("clone-ratio").expect("a name"),
            value: value(n),
            time: time.to_owned(),
        };
        let records = [
            record("v1", "a", 1, "2026-01-01T00:00:00Z"),
            record("v1", "a", 2, "2026-01-02T00:00:00Z"),
            record("v1", "b", 3, "2026-01-01T00:00:00Z"),
            record("v1", "me", 99, "2026-01-01T00:00:00Z"),
            record("old", "z", 98, "2026-01-01T00:00:00Z"),
        ];
        let kept = [KeptMetrics {
            goal: goal("old"),
            slices: SliceMetrics::from([(
                slice("z"),
                BTreeMap::from([(MetricName::parse("clone-ratio").expect("a name"), value(7))]),
            )]),
        }];
        let name = MetricName::parse("clone-ratio").expect("a name");
        assert_eq!(
            history(&records, &kept, &name, Some((&goal("v1"), &slice("me")))),
            values(&[2, 3, 7])
        );
    }

    #[test]
    fn a_kept_file_reads_its_slice_tables_and_skips_the_issues() {
        let table: Table = toml::from_str(
            "[alpha]\nclone-ratio = 2.5\ncognitive-complexity = 31\n[complete]\nissues = [\"u\"]\n",
        )
        .expect("toml");
        let kept = parse_kept(GoalName::parse("v1").expect("a goal"), &table).expect("kept");
        assert_eq!(kept.slices.len(), 1);
        let bad: Table = toml::from_str("[alpha]\nnote = \"text\"\n").expect("toml");
        assert_eq!(
            parse_kept(GoalName::parse("v1").expect("a goal"), &bad),
            None
        );
    }
}
