use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

use crate::pure::base::BranchName;
use crate::pure::json::{
    Parsed, Problem, Problems, at, fail, integer_at_most, non_empty, object_at, optional, present,
    shape, whole,
};

pub(crate) const SETUP_PATH: &str = ".config/shipit.json";
pub(crate) const HEAD: &str = "HEAD";

const ONE_OF: &str = "must be one of low, medium, high, xhigh, max";
const TIER_REF: &str = "must be the number of an existing tier";
const NOT_EMPTY: &str = "must be a non-empty string";
const BRANCH: &str = "must be a branch name (see git check-ref-format --branch)";
const TOP_KEYS: [&str; 11] = [
    "version",
    "base",
    "cap",
    "check",
    "environment",
    "fix_rounds",
    "effort_cap",
    "tiers",
    "roles",
    "packs",
    "messages_to_owner",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::Xhigh),
            "max" => Some(Self::Max),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Version;

impl Serialize for Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(1)
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct NonEmpty(String);

impl NonEmpty {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct CheckCommand(String);

impl CheckCommand {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct Cap(u64);

impl Cap {
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct Lines(u64);

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct TierNumber(u64);

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Environment {
    pub(crate) run: Vec<NonEmpty>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct StandIn {
    pub(crate) model: NonEmpty,
    pub(crate) effort: Effort,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[expect(clippy::struct_field_names, reason = "the field is the file's own key")]
pub(crate) struct Tier {
    pub(crate) tier: TierNumber,
    pub(crate) model: NonEmpty,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stand_in: Option<StandIn>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Role {
    pub(crate) tier: TierNumber,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) effort: Option<Effort>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Then {
    pub(crate) tier: TierNumber,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Scout {
    pub(crate) tier: TierNumber,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) effort: Option<Effort>,
    pub(crate) retries: u64,
    pub(crate) then: Then,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Roles {
    pub(crate) scout: Scout,
    pub(crate) maker: Role,
    pub(crate) tester: Role,
    pub(crate) reader: Role,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Packs {
    pub(crate) skill: NonEmpty,
    pub(crate) domain: NonEmpty,
    pub(crate) model: NonEmpty,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct MessagesToOwner {
    pub(crate) max_lines: Lines,
    pub(crate) status_update_max_lines: Lines,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Setup {
    pub(crate) version: Version,
    pub(crate) base: BranchName,
    pub(crate) cap: Cap,
    pub(crate) check: Vec<CheckCommand>,
    pub(crate) environment: Environment,
    pub(crate) fix_rounds: u64,
    pub(crate) effort_cap: Effort,
    pub(crate) tiers: Vec<Tier>,
    pub(crate) roles: Roles,
    pub(crate) packs: Packs,
    pub(crate) messages_to_owner: MessagesToOwner,
}

// What the rules need from other fields, read even when those fields are themselves wrong.
#[derive(Clone, Copy)]
struct Context {
    cap: Option<Effort>,
    count: Option<u64>,
}

pub(crate) fn setup_label(rev: &str) -> String {
    if rev == HEAD {
        SETUP_PATH.to_owned()
    } else {
        format!("{rev}:{SETUP_PATH}")
    }
}

pub(crate) fn json_values(bytes: &[u8]) -> Result<Vec<Value>, String> {
    serde_json::Deserializer::from_slice(bytes)
        .into_iter::<Value>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|why| why.to_string())
}

fn non_empty_value(value: &Value) -> Option<NonEmpty> {
    non_empty(value).map(|text| NonEmpty(text.to_owned()))
}

fn strings<T>(value: &Value, path: &str, make: fn(String) -> T) -> Parsed<Vec<T>> {
    let Some(items) = value.as_array() else {
        return fail(path, "must be an array of non-empty strings");
    };
    let mut problems = Vec::new();
    let mut found = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match non_empty(item) {
            Some(text) => found.push(make(text.to_owned())),
            None => problems.push(Problem::new(format!("{path}[{index}]"), NOT_EMPTY)),
        }
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

fn effort_value(value: &Value, path: &str, cap: Option<Effort>) -> Parsed<Effort> {
    match value.as_str().and_then(Effort::parse) {
        None => fail(path, ONE_OF),
        Some(effort) if cap.is_some_and(|cap| effort > cap) => {
            fail(path, "must not be above effort_cap")
        }
        Some(effort) => Ok(effort),
    }
}

fn effort_at(
    object: &Map<String, Value>,
    path: &str,
    cap: Option<Effort>,
) -> Parsed<Option<Effort>> {
    optional(object, "effort", |value| {
        effort_value(value, &format!("{path}.effort"), cap)
    })
}

fn tier_ref(value: &Value, count: Option<u64>) -> Option<TierNumber> {
    whole(value)
        .filter(|n| *n >= 1 && count.is_none_or(|count| *n <= count))
        .map(TierNumber)
}

fn tier_at(object: &Map<String, Value>, path: &str, context: Context) -> Parsed<TierNumber> {
    at(
        object,
        "tier",
        path,
        |value| tier_ref(value, context.count),
        TIER_REF,
    )
}

fn parse_stand_in(value: &Value, path: &str, cap: Option<Effort>) -> Parsed<StandIn> {
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &["model", "effort"], &[]));
    let model = problems.take(at(object, "model", path, non_empty_value, NOT_EMPTY));
    let effort = problems.take(present(object, "effort", |value| {
        effort_value(value, &format!("{path}.effort"), cap)
    }));
    problems.finish(|| {
        Some(StandIn {
            model: model?,
            effort: effort?,
        })
    })
}

fn parse_tier(value: &Value, number: u64, index: usize, cap: Option<Effort>) -> Parsed<Tier> {
    let path = format!(".tiers[{index}]");
    let object = object_at(value, &path)?;
    let mut problems = Problems::default();
    problems.add(shape(&path, object, &["tier", "model"], &["stand_in"]));
    let tier = problems.take(at(
        object,
        "tier",
        &path,
        |value| (whole(value) == Some(number)).then_some(TierNumber(number)),
        &format!("must be {number} (tiers are numbered 1..n in order)"),
    ));
    let model = problems.take(at(object, "model", &path, non_empty_value, NOT_EMPTY));
    let stand_in = problems.take(optional(object, "stand_in", |value| {
        parse_stand_in(value, &format!("{path}.stand_in"), cap)
    }));
    problems.finish(|| {
        Some(Tier {
            tier: tier?,
            model: model?,
            stand_in: stand_in?,
        })
    })
}

fn parse_tiers(value: &Value, cap: Option<Effort>) -> Parsed<Vec<Tier>> {
    let Some(items) = value.as_array().filter(|items| !items.is_empty()) else {
        return fail(".tiers", "must be a non-empty array of objects");
    };
    let mut problems = Problems::default();
    let tiers: Vec<Option<Tier>> = items
        .iter()
        .zip(1_u64..)
        .enumerate()
        .map(|(index, (item, number))| problems.take(parse_tier(item, number, index, cap)))
        .collect();
    problems.finish(|| tiers.into_iter().collect())
}

fn parse_role(value: &Value, path: &str, context: Context) -> Parsed<Role> {
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &["tier"], &["effort"]));
    let tier = problems.take(tier_at(object, path, context));
    let effort = problems.take(effort_at(object, path, context.cap));
    problems.finish(|| {
        Some(Role {
            tier: tier?,
            effort: effort?,
        })
    })
}

fn parse_then(value: &Value, scout_tier: Option<&Value>, context: Context) -> Parsed<Then> {
    let path = ".roles.scout.then";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &["tier"], &[]));
    let tier = object.get("tier").and_then(|value| {
        let found = tier_ref(value, context.count);
        let at = format!("{path}.tier");
        if found.is_none() {
            problems.push(Problem::new(&at, TIER_REF));
        }
        if scout_tier.is_some_and(|scout| integer_at_most(value, scout)) {
            problems.push(Problem::new(&at, "must be above roles.scout.tier"));
        }
        found
    });
    problems.finish(|| Some(Then { tier: tier? }))
}

fn parse_scout(value: &Value, context: Context) -> Parsed<Scout> {
    let path = ".roles.scout";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(
        path,
        object,
        &["tier", "retries", "then"],
        &["effort"],
    ));
    let tier = problems.take(tier_at(object, path, context));
    let effort = problems.take(effort_at(object, path, context.cap));
    let retries = problems.take(at(
        object,
        "retries",
        path,
        whole,
        "must be an integer of at least 0",
    ));
    let then = problems.take(present(object, "then", |value| {
        parse_then(value, object.get("tier"), context)
    }));
    problems.finish(|| {
        Some(Scout {
            tier: tier?,
            effort: effort?,
            retries: retries?,
            then: then?,
        })
    })
}

fn parse_roles(value: &Value, context: Context) -> Parsed<Roles> {
    let object = object_at(value, ".roles")?;
    let mut problems = Problems::default();
    problems.add(shape(
        ".roles",
        object,
        &["scout", "maker", "tester", "reader"],
        &[],
    ));
    let scout = problems.take(present(object, "scout", |value| {
        parse_scout(value, context)
    }));
    let maker = problems.take(present(object, "maker", |value| {
        parse_role(value, ".roles.maker", context)
    }));
    let tester = problems.take(present(object, "tester", |value| {
        parse_role(value, ".roles.tester", context)
    }));
    let reader = problems.take(present(object, "reader", |value| {
        parse_role(value, ".roles.reader", context)
    }));
    problems.finish(|| {
        Some(Roles {
            scout: scout?,
            maker: maker?,
            tester: tester?,
            reader: reader?,
        })
    })
}

fn parse_environment(value: &Value) -> Parsed<Environment> {
    let path = ".environment";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &["run"], &[]));
    let run = problems.take(present(object, "run", |value| {
        strings(value, ".environment.run", NonEmpty)
    }));
    problems.finish(|| Some(Environment { run: run? }))
}

fn parse_packs(value: &Value) -> Parsed<Packs> {
    let path = ".packs";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &["skill", "domain", "model"], &[]));
    let skill = problems.take(at(object, "skill", path, non_empty_value, NOT_EMPTY));
    let domain = problems.take(at(object, "domain", path, non_empty_value, NOT_EMPTY));
    let model = problems.take(at(object, "model", path, non_empty_value, NOT_EMPTY));
    problems.finish(|| {
        Some(Packs {
            skill: skill?,
            domain: domain?,
            model: model?,
        })
    })
}

fn lines(value: &Value) -> Option<Lines> {
    whole(value).filter(|n| *n >= 1).map(Lines)
}

fn parse_messages(value: &Value) -> Parsed<MessagesToOwner> {
    let path = ".messages_to_owner";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(
        path,
        object,
        &["max_lines", "status_update_max_lines"],
        &[],
    ));
    let message = "must be an integer of at least 1";
    let max_lines = problems.take(at(object, "max_lines", path, lines, message));
    let status = problems.take(at(object, "status_update_max_lines", path, lines, message));
    if let (Some(max), Some(update)) = (&max_lines, &status)
        && update.0 > max.0
    {
        problems.push(Problem::new(
            format!("{path}.status_update_max_lines"),
            "must not be above max_lines",
        ));
    }
    problems.finish(|| {
        Some(MessagesToOwner {
            max_lines: max_lines?,
            status_update_max_lines: status?,
        })
    })
}

pub(crate) fn parse_setup(docs: &[Value]) -> Parsed<Setup> {
    let [doc] = docs else {
        return fail(
            ".",
            &format!("must hold exactly one JSON document, found {}", docs.len()),
        );
    };
    let Some(object) = doc.as_object() else {
        return fail(".", "must be a JSON object");
    };
    let context = Context {
        cap: object
            .get("effort_cap")
            .and_then(Value::as_str)
            .and_then(Effort::parse),
        count: object
            .get("tiers")
            .and_then(Value::as_array)
            .and_then(|tiers| u64::try_from(tiers.len()).ok()),
    };
    let mut problems = Problems::default();
    problems.add(shape("", object, &TOP_KEYS, &[]));
    let version = problems.take(at(
        object,
        "version",
        "",
        |value| (whole(value) == Some(1)).then_some(Version),
        "must be the integer 1",
    ));
    let base = problems.take(at(
        object,
        "base",
        "",
        |value| value.as_str().and_then(BranchName::parse),
        BRANCH,
    ));
    let cap = problems.take(at(
        object,
        "cap",
        "",
        |value| whole(value).filter(|n| *n >= 1).map(Cap),
        "must be an integer of at least 1",
    ));
    let fix_rounds = problems.take(at(
        object,
        "fix_rounds",
        "",
        whole,
        "must be an integer of at least 0",
    ));
    let effort_cap = problems.take(at(
        object,
        "effort_cap",
        "",
        |value| value.as_str().and_then(Effort::parse),
        ONE_OF,
    ));
    let check = problems.take(present(object, "check", |value| {
        strings(value, ".check", CheckCommand)
    }));
    let environment = problems.take(present(object, "environment", parse_environment));
    let tiers = problems.take(present(object, "tiers", |value| {
        parse_tiers(value, context.cap)
    }));
    let roles = problems.take(present(object, "roles", |value| {
        parse_roles(value, context)
    }));
    let packs = problems.take(present(object, "packs", parse_packs));
    let messages_to_owner = problems.take(present(object, "messages_to_owner", parse_messages));
    problems.finish(|| {
        Some(Setup {
            version: version?,
            base: base?,
            cap: cap?,
            check: check?,
            environment: environment?,
            fix_rounds: fix_rounds?,
            effort_cap: effort_cap?,
            tiers: tiers?,
            roles: roles?,
            packs: packs?,
            messages_to_owner: messages_to_owner?,
        })
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/setup-valid.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).unwrap()
    }

    fn lines_of_docs(docs: &[Value]) -> Vec<String> {
        match parse_setup(docs) {
            Ok(_setup) => Vec::new(),
            Err(problems) => problems.iter().map(ToString::to_string).collect(),
        }
    }

    fn lines_of(value: Value) -> Vec<String> {
        lines_of_docs(&[value])
    }

    fn config() -> ProptestConfig {
        ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    // Every value of the fixture below the top: its path as the problems write it, and its JSON
    // pointer.
    fn places() -> Vec<(String, String)> {
        fn walk(value: &Value, path: &str, pointer: &str, found: &mut Vec<(String, String)>) {
            let children: Vec<(String, String, &Value)> = match value {
                Value::Object(map) => map
                    .iter()
                    .map(|(key, inner)| {
                        (format!("{path}.{key}"), format!("{pointer}/{key}"), inner)
                    })
                    .collect(),
                Value::Array(items) => items
                    .iter()
                    .enumerate()
                    .map(|(index, inner)| {
                        (
                            format!("{path}[{index}]"),
                            format!("{pointer}/{index}"),
                            inner,
                        )
                    })
                    .collect(),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
            };
            for (child_path, child_pointer, inner) in children {
                found.push((child_path.clone(), child_pointer.clone()));
                walk(inner, &child_path, &child_pointer, found);
            }
        }
        let mut found = Vec::new();
        walk(&fixture(), "", "", &mut found);
        found
    }

    fn places_where(keep: impl Fn(&str, &Value) -> bool) -> Vec<(String, String)> {
        let value = fixture();
        places()
            .into_iter()
            .filter(|(path, pointer)| value.pointer(pointer).is_some_and(|v| keep(path, v)))
            .collect()
    }

    fn put(value: &mut Value, pointer: &str, new: Value) {
        if let Some(slot) = value.pointer_mut(pointer) {
            *slot = new;
        } else if let Some((parent, key)) = pointer.rsplit_once('/')
            && let Some(object) = value.pointer_mut(parent).and_then(Value::as_object_mut)
        {
            object.insert(key.to_owned(), new);
        }
    }

    fn remove(value: &mut Value, pointer: &str) {
        if let Some((parent, key)) = pointer.rsplit_once('/')
            && let Some(object) = value.pointer_mut(parent).and_then(Value::as_object_mut)
        {
            object.shift_remove(key);
        }
    }

    fn json_integer() -> impl Strategy<Value = Value> {
        prop_oneof![
            (-3_i64..0).prop_map(|n| json!(n)),
            (0_u64..4).prop_map(|n| json!(n)),
            any::<u64>().prop_map(|n| json!(n)),
        ]
    }

    const SCALE: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

    #[test]
    fn the_fixture_parses_and_serializes_back_to_itself() {
        let setup = parse_setup(&[fixture()]).unwrap();
        assert_eq!(
            serde_json::to_string(&setup).unwrap(),
            serde_json::to_string(&fixture()).unwrap()
        );
    }

    #[test]
    fn efforts_are_the_five_names_in_ascending_order() {
        let scale: Vec<Option<Effort>> = SCALE.into_iter().map(Effort::parse).collect();
        assert_eq!(
            scale,
            vec![
                Some(Effort::Low),
                Some(Effort::Medium),
                Some(Effort::High),
                Some(Effort::Xhigh),
                Some(Effort::Max)
            ]
        );
        assert!(Effort::Low < Effort::Medium);
        assert!(Effort::Medium < Effort::High);
        assert!(Effort::High < Effort::Xhigh);
        assert!(Effort::Xhigh < Effort::Max);
    }

    // A value the proptest found: without serde_json's float_roundtrip the parser reads this as
    // a nearby non-integer.
    #[test]
    fn a_large_integer_written_with_a_zero_fraction_reads_back_exactly() {
        let value: Value = serde_json::from_str("2215942996861907.0").unwrap();
        assert_eq!(whole(&value), Some(2_215_942_996_861_907));
    }

    #[test]
    fn the_upper_bound_of_a_whole_number_is_exclusive() {
        let two_to_the_64: Value = serde_json::from_str("18446744073709551616").unwrap();
        assert_eq!(whole(&two_to_the_64), None);
        let as_float: Value = serde_json::from_str("1.8446744073709552e19").unwrap();
        assert_eq!(whole(&as_float), None);
        assert_eq!(whole(&json!(u64::MAX)), Some(u64::MAX));
        let largest_float: Value = serde_json::from_str("18446744073709549568.0").unwrap();
        assert_eq!(whole(&largest_float), Some(18_446_744_073_709_549_568));
    }

    #[test]
    fn no_tiers_is_reported_with_every_role_that_names_one() {
        let mut value = fixture();
        value["tiers"] = json!([]);
        assert_eq!(
            lines_of(value),
            [
                ".tiers: must be a non-empty array of objects",
                ".roles.scout.tier: must be the number of an existing tier",
                ".roles.scout.then.tier: must be the number of an existing tier",
                ".roles.maker.tier: must be the number of an existing tier",
                ".roles.tester.tier: must be the number of an existing tier",
                ".roles.reader.tier: must be the number of an existing tier",
            ]
        );
    }

    #[test]
    fn every_value_null() {
        let mut value = fixture();
        for (_key, slot) in value.as_object_mut().unwrap() {
            *slot = Value::Null;
        }
        assert_eq!(
            lines_of(value),
            [
                ".version: must be the integer 1",
                ".base: must be a branch name (see git check-ref-format --branch)",
                ".cap: must be an integer of at least 1",
                ".fix_rounds: must be an integer of at least 0",
                ".effort_cap: must be one of low, medium, high, xhigh, max",
                ".check: must be an array of non-empty strings",
                ".environment: must be an object",
                ".tiers: must be a non-empty array of objects",
                ".roles: must be an object",
                ".packs: must be an object",
                ".messages_to_owner: must be an object",
            ]
        );
    }

    #[test]
    fn several_broken_rules() {
        let mut value = fixture();
        value["cap"] = json!(0);
        value["base"] = json!("");
        value["roles"]["maker"]["effort"] = json!("max");
        value["packs"]["model"] = json!(3);
        assert_eq!(
            lines_of(value),
            [
                ".base: must be a branch name (see git check-ref-format --branch)",
                ".cap: must be an integer of at least 1",
                ".roles.maker.effort: must not be above effort_cap",
                ".packs.model: must be a non-empty string",
            ]
        );
    }

    #[test]
    fn the_reader_is_required_and_has_the_role_shape() {
        let mut value = fixture();
        value["roles"]
            .as_object_mut()
            .unwrap()
            .shift_remove("reader");
        assert_eq!(lines_of(value), [".roles.reader: missing"]);
        let mut value = fixture();
        value["roles"]["reader"] = json!({"tier": 5, "extra": 1});
        assert_eq!(
            lines_of(value),
            [
                ".roles.reader.extra: unknown key",
                ".roles.reader.tier: must be the number of an existing tier"
            ]
        );
    }

    #[test]
    fn a_text_that_is_not_json_is_an_error_and_an_empty_one_has_no_documents() {
        assert_eq!(json_values(b"").unwrap(), Vec::<Value>::new());
        assert!(json_values(b"{\"a\":").is_err());
    }

    #[test]
    fn the_label_names_the_ref_unless_it_is_head() {
        assert_eq!(setup_label("HEAD"), ".config/shipit.json");
        assert_eq!(setup_label("main"), "main:.config/shipit.json");
    }

    fn arb_effort() -> impl Strategy<Value = Effort> {
        proptest::sample::select(vec![
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::Xhigh,
            Effort::Max,
        ])
    }

    fn arb_effort_to(cap: Effort) -> impl Strategy<Value = Effort> {
        let allowed: Vec<Effort> = [
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::Xhigh,
            Effort::Max,
        ]
        .into_iter()
        .filter(|effort| *effort <= cap)
        .collect();
        proptest::sample::select(allowed)
    }

    fn arb_text() -> impl Strategy<Value = String> {
        "[a-z]{1,8}"
    }

    fn arb_role(cap: Effort, n: u64) -> impl Strategy<Value = Role> {
        (0..n, proptest::option::of(arb_effort_to(cap))).prop_map(|(tier, effort)| Role {
            tier: TierNumber(tier + 1),
            effort,
        })
    }

    fn arb_setup() -> impl Strategy<Value = Setup> {
        let scalars = (
            arb_text(),
            prop_oneof![1_u64..1000, 1_u64..=u64::MAX],
            0_u64..10,
            proptest::collection::vec(arb_text(), 0..3),
            proptest::collection::vec(arb_text(), 0..3),
        );
        let strings = (arb_text(), arb_text(), arb_text());
        let limits = (1_u64..50, 0_u64..50);
        (scalars, strings, limits, arb_effort(), 2_u64..=5)
            .prop_flat_map(|(scalars, strings, limits, cap, n)| {
                let tiers = proptest::collection::vec(
                    (
                        arb_text(),
                        proptest::option::of((arb_text(), arb_effort_to(cap))),
                    ),
                    usize::try_from(n).unwrap()..=usize::try_from(n).unwrap(),
                );
                let scout = (
                    1..n,
                    0_u64..8,
                    proptest::option::of(arb_effort_to(cap)),
                    0_u64..5,
                );
                (
                    Just((scalars, strings, limits, cap, n)),
                    tiers,
                    scout,
                    (arb_role(cap, n), arb_role(cap, n), arb_role(cap, n)),
                )
            })
            .prop_map(
                |(
                    (
                        (base, cap_value, fix_rounds, check, run),
                        (skill, domain, model),
                        limits,
                        cap,
                        n,
                    ),
                    tiers,
                    (scout_tier, offset, scout_effort, retries),
                    (maker, tester, reader),
                )| {
                    let (max_lines, update_offset) = limits;
                    let then_tier = scout_tier + 1 + offset % (n - scout_tier);
                    Setup {
                        version: Version,
                        base: BranchName::parse(&base).unwrap(),
                        cap: Cap(cap_value),
                        check: check.into_iter().map(CheckCommand).collect(),
                        environment: Environment {
                            run: run.into_iter().map(NonEmpty).collect(),
                        },
                        fix_rounds,
                        effort_cap: cap,
                        tiers: tiers
                            .into_iter()
                            .zip(1_u64..)
                            .map(|((name, stand_in), number)| Tier {
                                tier: TierNumber(number),
                                model: NonEmpty(name),
                                stand_in: stand_in.map(|(model, effort)| StandIn {
                                    model: NonEmpty(model),
                                    effort,
                                }),
                            })
                            .collect(),
                        roles: Roles {
                            scout: Scout {
                                tier: TierNumber(scout_tier),
                                effort: scout_effort,
                                retries,
                                then: Then {
                                    tier: TierNumber(then_tier),
                                },
                            },
                            maker,
                            tester,
                            reader,
                        },
                        packs: Packs {
                            skill: NonEmpty(skill),
                            domain: NonEmpty(domain),
                            model: NonEmpty(model),
                        },
                        messages_to_owner: MessagesToOwner {
                            max_lines: Lines(max_lines),
                            status_update_max_lines: Lines(1 + update_offset % max_lines),
                        },
                    }
                },
            )
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn whole_accepts_every_u64_as_an_integer(n in any::<u64>()) {
            prop_assert_eq!(whole(&json!(n)), Some(n));
        }

        #[test]
        fn whole_accepts_a_u64_written_with_a_zero_fraction(n in any::<u64>()) {
            let value: Value = serde_json::from_str(&format!("{n}.0")).unwrap();
            // Within 1024 of 2^64 the text rounds up to 2^64, which is refused.
            prop_assert_eq!(whole(&value).is_some(), n < u64::MAX - 1024);
            if n < 1 << 53 {
                prop_assert_eq!(whole(&value), Some(n));
            }
        }

        #[test]
        fn whole_refuses_a_number_with_a_fraction_part(i in any::<i32>(), step in 1_u32..1000) {
            let x = f64::from(i) + f64::from(step) / 1024.0;
            prop_assert_eq!(whole(&json!(x)), None);
        }

        #[test]
        fn a_text_off_the_scale_is_not_an_effort(text in "[a-zA-Z ]{0,7}") {
            prop_assume!(!SCALE.contains(&text.as_str()));
            prop_assert_eq!(Effort::parse(&text), None);
        }

        #[test]
        fn a_file_of_any_other_count_than_one_document_is_refused_with_the_count(
            count in prop_oneof![Just(0_usize), 2_usize..6],
        ) {
            let docs = vec![json!({}); count];
            prop_assert_eq!(
                lines_of_docs(&docs),
                vec![format!(".: must hold exactly one JSON document, found {count}")]
            );
        }

        #[test]
        fn a_document_that_is_not_an_object_is_refused(
            doc in proptest::sample::select(vec![
                json!(null), json!(true), json!(5), json!("x"), json!([]), json!([{}]),
            ]),
        ) {
            prop_assert_eq!(lines_of_docs(&[doc]), vec![".: must be a JSON object".to_owned()]);
        }

        #[test]
        fn json_values_counts_the_documents(numbers in proptest::collection::vec(any::<u8>(), 0..6)) {
            let text = numbers.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
            prop_assert_eq!(json_values(text.as_bytes()).map(|docs| docs.len()), Ok(numbers.len()));
        }

        #[test]
        fn a_value_of_the_wrong_kind_is_reported_at_its_place(
            (path, pointer) in proptest::sample::select(places()),
            wrong in proptest::sample::select(vec![json!(true), json!(null), json!(5), json!("7")]),
        ) {
            let mut value = fixture();
            let current = value.pointer(&pointer).cloned().unwrap_or(Value::Null);
            prop_assume!(!(wrong.is_number() && current.is_number()));
            prop_assume!(!(wrong.is_string() && current.is_string()));
            put(&mut value, &pointer, wrong);
            let lines = lines_of(value);
            let prefix = format!("{path}: ");
            prop_assert!(lines.iter().any(|line| line.starts_with(&prefix)), "{:?}", lines);
        }

        #[test]
        fn a_missing_required_key_is_the_only_problem_and_a_missing_optional_one_is_none(
            (path, pointer) in proptest::sample::select(places()),
        ) {
            let mut value = fixture();
            let Some((parent, key)) = pointer.rsplit_once('/') else {
                return Err(TestCaseError::fail("a pointer below the top"));
            };
            prop_assume!(value.pointer(parent).is_some_and(Value::is_object));
            remove(&mut value, &pointer);
            let optional = key == "stand_in" || (key == "effort" && !path.contains("stand_in"));
            let expected = if optional {
                Vec::new()
            } else {
                vec![format!("{path}: missing")]
            };
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn an_unknown_key_is_the_only_problem_in_any_object(
            (path, pointer) in proptest::sample::select(
                [(String::new(), String::new())]
                    .into_iter()
                    .chain(places_where(|_, v| v.is_object()))
                    .collect::<Vec<_>>()
            ),
            key in "[a-z_]{1,10}",
        ) {
            prop_assume!(!["effort", "stand_in"].contains(&key.as_str()));
            let mut value = fixture();
            prop_assume!(value.pointer(&pointer).and_then(Value::as_object)
                .is_some_and(|object| !object.contains_key(&key)));
            put(&mut value, &format!("{pointer}/{key}"), json!(1));
            prop_assert_eq!(lines_of(value), vec![format!("{path}.{key}: unknown key")]);
        }

        #[test]
        fn an_empty_text_is_refused_where_a_non_empty_one_is_needed(
            (path, pointer) in proptest::sample::select(places_where(|path, v| {
                v.is_string()
                    && path != ".base"
                    && path != ".effort_cap"
                    && !path.ends_with(".effort")
            })),
        ) {
            let mut value = fixture();
            put(&mut value, &pointer, json!(""));
            prop_assert_eq!(
                lines_of(value),
                vec![format!("{path}: must be a non-empty string")]
            );
        }

        #[test]
        fn command_lists_hold_only_non_empty_strings(
            (path, pointer) in proptest::sample::select(vec![
                (".check", "/check"),
                (".environment.run", "/environment/run"),
            ]),
            entries in proptest::collection::vec(
                prop_oneof![
                    Just(json!("")),
                    Just(json!(3)),
                    "[a-z]{1,4}".prop_map(|text| json!(text)),
                ],
                0..5,
            ),
        ) {
            let expected: Vec<String> = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.as_str().is_none_or(str::is_empty))
                .map(|(index, _)| format!("{path}[{index}]: must be a non-empty string"))
                .collect();
            let mut value = fixture();
            put(&mut value, pointer, Value::Array(entries));
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn the_base_is_a_branch_name(text in prop_oneof!["\\PC{0,12}", "[a-z.~^/ -]{0,6}"]) {
            let mut value = fixture();
            put(&mut value, "/base", json!(text));
            let expected = if BranchName::parse(&text).is_some() {
                Vec::new()
            } else {
                vec![".base: must be a branch name (see git check-ref-format --branch)".to_owned()]
            };
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn the_version_is_the_integer_1(n in prop_oneof![-2_i64..4, any::<i64>()]) {
            let mut value = fixture();
            put(&mut value, "/version", json!(n));
            let expected = if n == 1 {
                Vec::new()
            } else {
                vec![".version: must be the integer 1".to_owned()]
            };
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn a_count_is_accepted_from_its_minimum(
            (path, pointer, min) in proptest::sample::select(vec![
                (".cap", "/cap", 1_u64),
                (".fix_rounds", "/fix_rounds", 0),
                (".roles.scout.retries", "/roles/scout/retries", 0),
                (".messages_to_owner.max_lines", "/messages_to_owner/max_lines", 1),
                (
                    ".messages_to_owner.status_update_max_lines",
                    "/messages_to_owner/status_update_max_lines",
                    1,
                ),
            ]),
            n in json_integer(),
        ) {
            let mut value = fixture();
            put(&mut value, "/messages_to_owner/max_lines", json!(u64::MAX));
            put(&mut value, "/messages_to_owner/status_update_max_lines", json!(1));
            let expected = if n.as_u64().is_some_and(|n| n >= min) {
                Vec::new()
            } else {
                vec![format!("{path}: must be an integer of at least {min}")]
            };
            put(&mut value, pointer, n);
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn a_number_with_a_fraction_part_is_refused_where_an_integer_is_needed(
            (path, pointer) in proptest::sample::select(vec![
                (".version", "/version"),
                (".cap", "/cap"),
                (".fix_rounds", "/fix_rounds"),
                (".tiers[0].tier", "/tiers/0/tier"),
                (".roles.scout.tier", "/roles/scout/tier"),
                (".roles.scout.retries", "/roles/scout/retries"),
                (".roles.scout.then.tier", "/roles/scout/then/tier"),
                (".roles.maker.tier", "/roles/maker/tier"),
                (".messages_to_owner.max_lines", "/messages_to_owner/max_lines"),
                (
                    ".messages_to_owner.status_update_max_lines",
                    "/messages_to_owner/status_update_max_lines",
                ),
            ]),
            i in 0_u32..3,
            step in 1_u32..1000,
        ) {
            let mut value = fixture();
            put(&mut value, pointer, json!(f64::from(i) + f64::from(step) / 1024.0));
            let lines = lines_of(value);
            let prefix = format!("{path}: ");
            prop_assert!(lines.iter().any(|line| line.starts_with(&prefix)), "{:?}", lines);
        }

        #[test]
        fn the_status_limit_is_not_above_the_max_lines(max in 1_u64..40, status in 1_u64..40) {
            let mut value = fixture();
            put(&mut value, "/messages_to_owner/max_lines", json!(max));
            put(&mut value, "/messages_to_owner/status_update_max_lines", json!(status));
            let expected = if status <= max {
                Vec::new()
            } else {
                vec![
                    ".messages_to_owner.status_update_max_lines: must not be above max_lines"
                        .to_owned(),
                ]
            };
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn an_effort_off_the_scale_is_refused_wherever_one_is_written(
            (path, pointer) in proptest::sample::select(vec![
                (".effort_cap", "/effort_cap"),
                (".tiers[3].stand_in.effort", "/tiers/3/stand_in/effort"),
                (".roles.scout.effort", "/roles/scout/effort"),
                (".roles.maker.effort", "/roles/maker/effort"),
                (".roles.tester.effort", "/roles/tester/effort"),
                (".roles.reader.effort", "/roles/reader/effort"),
            ]),
            text in "[a-zA-Z ]{0,7}",
        ) {
            prop_assume!(!SCALE.contains(&text.as_str()));
            let mut value = fixture();
            put(&mut value, pointer, json!(text));
            prop_assert_eq!(
                lines_of(value),
                vec![format!("{path}: must be one of low, medium, high, xhigh, max")]
            );
        }

        #[test]
        fn an_effort_is_accepted_when_it_is_not_above_the_cap(
            (path, pointer) in proptest::sample::select(vec![
                (".tiers[3].stand_in.effort", "/tiers/3/stand_in/effort"),
                (".roles.scout.effort", "/roles/scout/effort"),
                (".roles.maker.effort", "/roles/maker/effort"),
                (".roles.tester.effort", "/roles/tester/effort"),
                (".roles.reader.effort", "/roles/reader/effort"),
            ]),
            effort in arb_effort(),
            cap in arb_effort(),
        ) {
            let mut value = fixture();
            put(&mut value, "/effort_cap", serde_json::to_value(cap).unwrap());
            put(&mut value, "/tiers/3/stand_in/effort", json!("low"));
            remove(&mut value, "/roles/maker/effort");
            remove(&mut value, "/roles/tester/effort");
            put(&mut value, pointer, serde_json::to_value(effort).unwrap());
            let expected = if effort <= cap {
                Vec::new()
            } else {
                vec![format!("{path}: must not be above effort_cap")]
            };
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn a_role_names_an_existing_tier(
            (path, pointer) in proptest::sample::select(vec![
                (".roles.maker.tier", "/roles/maker/tier"),
                (".roles.tester.tier", "/roles/tester/tier"),
                (".roles.reader.tier", "/roles/reader/tier"),
            ]),
            n in -1_i64..8,
        ) {
            let mut value = fixture();
            put(&mut value, pointer, json!(n));
            let expected = if (1..=4).contains(&n) {
                Vec::new()
            } else {
                vec![format!("{path}: must be the number of an existing tier")]
            };
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn the_scouts_next_tier_exists_and_is_above_the_scouts_own(
            scout in -1_i64..8,
            then in -1_i64..8,
        ) {
            let mut value = fixture();
            put(&mut value, "/roles/scout/tier", json!(scout));
            put(&mut value, "/roles/scout/then/tier", json!(then));
            let exists = |n: i64| (1..=4).contains(&n);
            let mut expected = Vec::new();
            if !exists(scout) {
                expected.push(".roles.scout.tier: must be the number of an existing tier");
            }
            if !exists(then) {
                expected.push(".roles.scout.then.tier: must be the number of an existing tier");
            }
            if then <= scout {
                expected.push(".roles.scout.then.tier: must be above roles.scout.tier");
            }
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn the_tiers_are_numbered_one_to_n_in_order(
            numbers in proptest::collection::vec(0_u64..7, 4),
        ) {
            let mut value = fixture();
            let mut expected = Vec::new();
            for (index, (number, want)) in numbers.iter().zip(1_u64..).enumerate() {
                put(&mut value, &format!("/tiers/{index}/tier"), json!(number));
                if *number != want {
                    expected.push(format!(
                        ".tiers[{index}].tier: must be {want} (tiers are numbered 1..n in order)"
                    ));
                }
            }
            prop_assert_eq!(lines_of(value), expected);
        }

        #[test]
        fn a_valid_setup_survives_serializing_and_parsing(setup in arb_setup()) {
            let value = serde_json::to_value(&setup).unwrap();
            prop_assert_eq!(parse_setup(&[value]), Ok(setup));
        }
    }
}
