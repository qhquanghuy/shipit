use serde::{Serialize, Serializer};
use toml::{Table, Value};

use crate::pure::base::BranchName;
use crate::pure::rules::{
    Parsed, Problem, Problems, at, fail, integer_at_most, non_empty, object_at, optional, present,
    shape, whole,
};

pub(crate) const SETUP_PATH: &str = ".config/shipit.toml";
pub(crate) const HEAD: &str = "HEAD";

const ONE_OF: &str = "must be one of low, medium, high, xhigh, max";
const TIER_REF: &str = "must be the number of an existing tier";
const NOT_EMPTY: &str = "must be a non-empty string";
const BRANCH: &str = "must be a branch name (see git check-ref-format --branch)";
pub(crate) const TOP_KEYS: [&str; 5] = ["version", "gate", "agents", "packs", "owner"];
pub(crate) const GATE_KEYS: [&str; 4] = ["base", "max_files", "check", "environment"];
pub(crate) const AGENTS_KEYS: [&str; 4] = ["max_fix_rounds", "effort_cap", "tiers", "roles"];
pub(crate) const AGENTS_OPTIONAL: [&str; 1] = ["parallel"];
pub(crate) const PACKS_KEYS: [&str; 3] = ["skill", "domain", "model"];
pub(crate) const OWNER_KEYS: [&str; 2] = ["max_lines", "status_update_max_lines"];
const PARALLEL_OPTIONAL: [&str; 2] = ["slices", "agents"];
const DEFAULT_SLICES: u64 = 3;
const DEFAULT_AGENTS: u64 = 4;
const ROLE_NAMES: [&str; 5] = ["scout", "planner", "maker", "tester", "reader"];

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
    pub(crate) fn parse(text: &str) -> Option<Self> {
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

impl From<&Lines> for u64 {
    fn from(lines: &Lines) -> Self {
        lines.0
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct TierNumber(u64);

impl From<&TierNumber> for u64 {
    fn from(tier: &TierNumber) -> Self {
        tier.0
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct SliceCap(u64);

impl From<&SliceCap> for u64 {
    fn from(cap: &SliceCap) -> Self {
        cap.0
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct AgentCap(u64);

impl From<&AgentCap> for u64 {
    fn from(cap: &AgentCap) -> Self {
        cap.0
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Parallel {
    pub(crate) slices: SliceCap,
    pub(crate) agents: AgentCap,
}

impl Default for Parallel {
    fn default() -> Self {
        Self {
            slices: SliceCap(DEFAULT_SLICES),
            agents: AgentCap(DEFAULT_AGENTS),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Environment {
    pub(crate) prefix: Vec<NonEmpty>,
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
pub(crate) struct EscalateTo {
    pub(crate) tier: TierNumber,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Scout {
    pub(crate) tier: TierNumber,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) effort: Option<Effort>,
    pub(crate) retries: u64,
    pub(crate) escalate_to: EscalateTo,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Roles {
    pub(crate) scout: Scout,
    pub(crate) planner: Role,
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
pub(crate) struct Owner {
    pub(crate) max_lines: Lines,
    pub(crate) status_update_max_lines: Lines,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Gate {
    pub(crate) base: BranchName,
    pub(crate) max_files: Cap,
    pub(crate) check: Vec<CheckCommand>,
    pub(crate) environment: Environment,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Agents {
    pub(crate) max_fix_rounds: u64,
    pub(crate) effort_cap: Effort,
    pub(crate) tiers: Vec<Tier>,
    pub(crate) roles: Roles,
    pub(crate) parallel: Parallel,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Setup {
    pub(crate) version: Version,
    pub(crate) gate: Gate,
    pub(crate) agents: Agents,
    pub(crate) packs: Packs,
    pub(crate) owner: Owner,
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

pub(crate) fn toml_table(bytes: &[u8]) -> Result<Table, String> {
    let text = std::str::from_utf8(bytes).map_err(|why| why.to_string())?;
    toml::from_str(text).map_err(|why| why.to_string())
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
            fail(path, "must not be above agents.effort_cap")
        }
        Some(effort) => Ok(effort),
    }
}

fn effort_at(object: &Table, path: &str, cap: Option<Effort>) -> Parsed<Option<Effort>> {
    optional(object, "effort", |value| {
        effort_value(value, &format!("{path}.effort"), cap)
    })
}

fn tier_ref(value: &Value, count: Option<u64>) -> Option<TierNumber> {
    whole(value)
        .filter(|n| *n >= 1 && count.is_none_or(|count| *n <= count))
        .map(TierNumber)
}

fn tier_at(object: &Table, path: &str, context: Context) -> Parsed<TierNumber> {
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
    let path = format!(".agents.tiers[{index}]");
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
        return fail(".agents.tiers", "must be a non-empty array of objects");
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

fn parse_escalate_to(
    value: &Value,
    scout_tier: Option<&Value>,
    context: Context,
) -> Parsed<EscalateTo> {
    let path = ".agents.roles.scout.escalate_to";
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
            problems.push(Problem::new(&at, "must be above agents.roles.scout.tier"));
        }
        found
    });
    problems.finish(|| Some(EscalateTo { tier: tier? }))
}

fn parse_scout(value: &Value, context: Context) -> Parsed<Scout> {
    let path = ".agents.roles.scout";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(
        path,
        object,
        &["tier", "retries", "escalate_to"],
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
    let escalate_to = problems.take(present(object, "escalate_to", |value| {
        parse_escalate_to(value, object.get("tier"), context)
    }));
    problems.finish(|| {
        Some(Scout {
            tier: tier?,
            effort: effort?,
            retries: retries?,
            escalate_to: escalate_to?,
        })
    })
}

fn parse_roles(value: &Value, context: Context) -> Parsed<Roles> {
    let object = object_at(value, ".agents.roles")?;
    let mut problems = Problems::default();
    problems.add(shape(".agents.roles", object, &ROLE_NAMES, &[]));
    let scout = problems.take(present(object, "scout", |value| {
        parse_scout(value, context)
    }));
    let planner = problems.take(present(object, "planner", |value| {
        parse_role(value, ".agents.roles.planner", context)
    }));
    let maker = problems.take(present(object, "maker", |value| {
        parse_role(value, ".agents.roles.maker", context)
    }));
    let tester = problems.take(present(object, "tester", |value| {
        parse_role(value, ".agents.roles.tester", context)
    }));
    let reader = problems.take(present(object, "reader", |value| {
        parse_role(value, ".agents.roles.reader", context)
    }));
    problems.finish(|| {
        Some(Roles {
            scout: scout?,
            planner: planner?,
            maker: maker?,
            tester: tester?,
            reader: reader?,
        })
    })
}

fn parse_environment(value: &Value) -> Parsed<Environment> {
    let path = ".gate.environment";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &["prefix"], &[]));
    let prefix = problems.take(present(object, "prefix", |value| {
        strings(value, ".gate.environment.prefix", NonEmpty)
    }));
    problems.finish(|| Some(Environment { prefix: prefix? }))
}

fn parse_packs(value: &Value) -> Parsed<Packs> {
    let path = ".packs";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &PACKS_KEYS, &[]));
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

fn parse_owner(value: &Value) -> Parsed<Owner> {
    let path = ".owner";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &OWNER_KEYS, &[]));
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
        Some(Owner {
            max_lines: max_lines?,
            status_update_max_lines: status?,
        })
    })
}

fn parse_gate(value: &Value) -> Parsed<Gate> {
    let path = ".gate";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &GATE_KEYS, &[]));
    let base = problems.take(at(
        object,
        "base",
        path,
        |value| value.as_str().and_then(BranchName::parse),
        BRANCH,
    ));
    let max_files = problems.take(at(
        object,
        "max_files",
        path,
        |value| whole(value).filter(|n| *n >= 1).map(Cap),
        "must be an integer of at least 1",
    ));
    let check = problems.take(present(object, "check", |value| {
        strings(value, ".gate.check", CheckCommand)
    }));
    let environment = problems.take(present(object, "environment", parse_environment));
    problems.finish(|| {
        Some(Gate {
            base: base?,
            max_files: max_files?,
            check: check?,
            environment: environment?,
        })
    })
}

fn at_least_one<T>(value: &Value, path: &str, make: fn(u64) -> T) -> Parsed<T> {
    whole(value)
        .filter(|n| *n >= 1)
        .map(make)
        .ok_or_else(|| vec![Problem::new(path, "must be an integer of at least 1")])
}

// The table and each cap are optional: a setup written before the table existed means the
// defaults, so the schema grows without a break.
fn parse_parallel(value: &Value) -> Parsed<Parallel> {
    let path = ".agents.parallel";
    let object = object_at(value, path)?;
    let mut problems = Problems::default();
    problems.add(shape(path, object, &[], &PARALLEL_OPTIONAL));
    let slices = problems.take(optional(object, "slices", |value| {
        at_least_one(value, &format!("{path}.slices"), SliceCap)
    }));
    let agents = problems.take(optional(object, "agents", |value| {
        at_least_one(value, &format!("{path}.agents"), AgentCap)
    }));
    problems.finish(|| {
        Some(Parallel {
            slices: slices?.unwrap_or(SliceCap(DEFAULT_SLICES)),
            agents: agents?.unwrap_or(AgentCap(DEFAULT_AGENTS)),
        })
    })
}

fn parse_agents(value: &Value) -> Parsed<Agents> {
    let path = ".agents";
    let object = object_at(value, path)?;
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
    problems.add(shape(path, object, &AGENTS_KEYS, &AGENTS_OPTIONAL));
    let max_fix_rounds = problems.take(at(
        object,
        "max_fix_rounds",
        path,
        whole,
        "must be an integer of at least 0",
    ));
    let effort_cap = problems.take(at(
        object,
        "effort_cap",
        path,
        |value| value.as_str().and_then(Effort::parse),
        ONE_OF,
    ));
    let tiers = problems.take(present(object, "tiers", |value| {
        parse_tiers(value, context.cap)
    }));
    let roles = problems.take(present(object, "roles", |value| {
        parse_roles(value, context)
    }));
    let parallel = problems.take(optional(object, "parallel", parse_parallel));
    problems.finish(|| {
        Some(Agents {
            max_fix_rounds: max_fix_rounds?,
            effort_cap: effort_cap?,
            tiers: tiers?,
            roles: roles?,
            parallel: parallel?.unwrap_or_default(),
        })
    })
}

pub(crate) fn parse_setup(object: &Table) -> Parsed<Setup> {
    let mut problems = Problems::default();
    problems.add(shape("", object, &TOP_KEYS, &[]));
    let version = problems.take(at(
        object,
        "version",
        "",
        |value| (whole(value) == Some(1)).then_some(Version),
        "must be the integer 1",
    ));
    let gate = problems.take(present(object, "gate", parse_gate));
    let agents = problems.take(present(object, "agents", parse_agents));
    let packs = problems.take(present(object, "packs", parse_packs));
    let owner = problems.take(present(object, "owner", parse_owner));
    problems.finish(|| {
        Some(Setup {
            version: version?,
            gate: gate?,
            agents: agents?,
            packs: packs?,
            owner: owner?,
        })
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/setup-valid.toml");

    fn fixture() -> Value {
        toml::from_str(FIXTURE).unwrap()
    }

    fn lines_of(value: &Value) -> Vec<String> {
        match value.as_table().map(parse_setup) {
            Some(Err(problems)) => problems.iter().map(ToString::to_string).collect(),
            Some(Ok(_)) | None => Vec::new(),
        }
    }

    fn lines_of_text(text: &str) -> Vec<String> {
        lines_of(&Value::Table(toml_table(text.as_bytes()).unwrap()))
    }

    fn config() -> ProptestConfig {
        ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    fn int(n: i64) -> Value {
        Value::Integer(n)
    }

    fn text(s: &str) -> Value {
        Value::String(s.to_owned())
    }

    // One value of every kind TOML has; a test picks the ones that differ from the current one.
    fn kinds() -> Vec<Value> {
        vec![
            Value::Boolean(true),
            int(5),
            text("7"),
            Value::Float(1.5),
            Value::Datetime("1979-05-27T07:32:00Z".parse().unwrap()),
            Value::Array(Vec::new()),
            Value::Table(Table::new()),
        ]
    }

    // The value at a pointer: "" is the document, "/a/0/b" is a key, an index, a key.
    fn look<'a>(value: &'a Value, pointer: &str) -> Option<&'a Value> {
        pointer
            .split('/')
            .skip(1)
            .try_fold(value, |here, key| match here {
                Value::Table(table) => table.get(key),
                Value::Array(items) => {
                    let index = key.parse::<usize>().ok()?;
                    items.get(index)
                }
                Value::String(_)
                | Value::Integer(_)
                | Value::Float(_)
                | Value::Boolean(_)
                | Value::Datetime(_) => None,
            })
    }

    fn look_mut<'a>(value: &'a mut Value, pointer: &str) -> Option<&'a mut Value> {
        pointer
            .split('/')
            .skip(1)
            .try_fold(value, |here, key| match here {
                Value::Table(table) => table.get_mut(key),
                Value::Array(items) => {
                    let index = key.parse::<usize>().ok()?;
                    items.get_mut(index)
                }
                Value::String(_)
                | Value::Integer(_)
                | Value::Float(_)
                | Value::Boolean(_)
                | Value::Datetime(_) => None,
            })
    }

    // Every value of the fixture below the top: its path as the problems write it, and its
    // pointer.
    fn places() -> Vec<(String, String)> {
        fn walk(value: &Value, path: &str, pointer: &str, found: &mut Vec<(String, String)>) {
            let children: Vec<(String, String, &Value)> = match value {
                Value::Table(table) => table
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
                Value::String(_)
                | Value::Integer(_)
                | Value::Float(_)
                | Value::Boolean(_)
                | Value::Datetime(_) => Vec::new(),
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
            .filter(|(path, pointer)| look(&value, pointer).is_some_and(|v| keep(path, v)))
            .collect()
    }

    fn put(value: &mut Value, pointer: &str, new: Value) {
        if let Some(slot) = look_mut(value, pointer) {
            *slot = new;
        } else if let Some((parent, key)) = pointer.rsplit_once('/')
            && let Some(Value::Table(table)) = look_mut(value, parent)
        {
            table.insert(key.to_owned(), new);
        }
    }

    fn remove(value: &mut Value, pointer: &str) {
        if let Some((parent, key)) = pointer.rsplit_once('/')
            && let Some(Value::Table(table)) = look_mut(value, parent)
        {
            table.remove(key);
        }
    }

    fn integer() -> impl Strategy<Value = i64> {
        prop_oneof![-3_i64..4, any::<i64>()]
    }

    const SCALE: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

    #[test]
    fn the_fixture_parses_and_serializes_back_to_itself_with_the_default_caps() {
        let setup = parse_setup(fixture().as_table().unwrap()).unwrap();
        let mut expected = fixture();
        put(
            &mut expected,
            "/agents/parallel",
            Value::Table(Table::new()),
        );
        put(&mut expected, "/agents/parallel/slices", int(3));
        put(&mut expected, "/agents/parallel/agents", int(4));
        assert_eq!(
            serde_json::to_value(&setup).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[test]
    fn a_parallel_table_that_is_not_an_object_is_refused() {
        for wrong in kinds().into_iter().filter(|kind| !kind.is_table()) {
            let mut value = fixture();
            put(&mut value, "/agents/parallel", wrong);
            assert_eq!(lines_of(&value), [".agents.parallel: must be an object"]);
        }
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

    #[test]
    fn no_tiers_is_reported_with_every_role_that_names_one() {
        let mut value = fixture();
        put(&mut value, "/agents/tiers", Value::Array(Vec::new()));
        assert_eq!(
            lines_of(&value),
            [
                ".agents.tiers: must be a non-empty array of objects",
                ".agents.roles.scout.tier: must be the number of an existing tier",
                ".agents.roles.scout.escalate_to.tier: must be the number of an existing tier",
                ".agents.roles.planner.tier: must be the number of an existing tier",
                ".agents.roles.maker.tier: must be the number of an existing tier",
                ".agents.roles.tester.tier: must be the number of an existing tier",
                ".agents.roles.reader.tier: must be the number of an existing tier",
            ]
        );
    }

    #[test]
    fn every_top_level_value_of_the_wrong_kind() {
        let mut value = fixture();
        for (_key, slot) in value.as_table_mut().unwrap() {
            *slot = Value::Boolean(false);
        }
        assert_eq!(
            lines_of(&value),
            [
                ".version: must be the integer 1",
                ".gate: must be an object",
                ".agents: must be an object",
                ".packs: must be an object",
                ".owner: must be an object",
            ]
        );
    }

    // The order of the messages inside `gate` and `agents` is the order the rules run in.
    #[test]
    fn every_value_inside_gate_and_agents_of_the_wrong_kind() {
        let mut value = fixture();
        for table in ["gate", "agents"] {
            if let Some(Value::Table(inner)) = look_mut(&mut value, &format!("/{table}")) {
                for (_key, slot) in inner {
                    *slot = Value::Boolean(false);
                }
            }
        }
        assert_eq!(
            lines_of(&value),
            [
                ".gate.base: must be a branch name (see git check-ref-format --branch)",
                ".gate.max_files: must be an integer of at least 1",
                ".gate.check: must be an array of non-empty strings",
                ".gate.environment: must be an object",
                ".agents.max_fix_rounds: must be an integer of at least 0",
                ".agents.effort_cap: must be one of low, medium, high, xhigh, max",
                ".agents.tiers: must be a non-empty array of objects",
                ".agents.roles: must be an object",
            ]
        );
    }

    #[test]
    fn several_broken_rules() {
        let mut value = fixture();
        put(&mut value, "/gate/max_files", int(0));
        put(&mut value, "/gate/base", text(""));
        put(&mut value, "/agents/roles/maker/effort", text("max"));
        put(&mut value, "/packs/model", int(3));
        assert_eq!(
            lines_of(&value),
            [
                ".gate.base: must be a branch name (see git check-ref-format --branch)",
                ".gate.max_files: must be an integer of at least 1",
                ".agents.roles.maker.effort: must not be above agents.effort_cap",
                ".packs.model: must be a non-empty string",
            ]
        );
    }

    #[test]
    fn the_reader_is_required_and_has_the_role_shape() {
        let mut value = fixture();
        remove(&mut value, "/agents/roles/reader");
        assert_eq!(lines_of(&value), [".agents.roles.reader: missing"]);
        let mut value = fixture();
        let mut reader = Table::new();
        reader.insert("tier".to_owned(), int(5));
        reader.insert("extra".to_owned(), int(1));
        put(&mut value, "/agents/roles/reader", Value::Table(reader));
        assert_eq!(
            lines_of(&value),
            [
                ".agents.roles.reader.extra: unknown key",
                ".agents.roles.reader.tier: must be the number of an existing tier"
            ]
        );
    }

    // An empty file is an empty table: the schema's own rules report what is missing.
    #[test]
    fn an_empty_file_lists_every_key_as_missing() {
        assert_eq!(
            lines_of_text(""),
            TOP_KEYS.map(|key| format!(".{key}: missing"))
        );
    }

    // The shape before the reshape: every old key is refused where it stands, and the new tables
    // are missing; `version` and `packs` kept their place and are accepted.
    #[test]
    fn the_old_shape_is_refused_key_by_key() {
        let old = include_str!("../../tests/fixtures/setup-old-shape.toml");
        let mut expected: Vec<String> = [
            "base",
            "cap",
            "check",
            "fix_rounds",
            "effort_cap",
            "environment",
            "tiers",
            "roles",
            "messages_to_owner",
        ]
        .map(|key| format!(".{key}: unknown key"))
        .into();
        expected.extend(["gate", "agents", "owner"].map(|key| format!(".{key}: missing")));
        assert_eq!(lines_of_text(old), expected);
    }

    #[test]
    fn stand_in_and_escalate_to_inline_or_as_a_table_are_the_same_setup() {
        let tables = FIXTURE
            .replace(
                "stand_in = { model = \"opus\", effort = \"high\" }\n",
                "\n[agents.tiers.stand_in]\nmodel = \"opus\"\neffort = \"high\"\n",
            )
            .replace(
                "escalate_to = { tier = 2 }\n",
                "\n[agents.roles.scout.escalate_to]\ntier = 2\n",
            );
        assert_ne!(tables, FIXTURE);
        let parsed = |text: &str| parse_setup(&toml_table(text.as_bytes()).unwrap());
        assert_eq!(parsed(&tables), parsed(FIXTURE));
        assert!(parsed(FIXTURE).is_ok());
    }

    #[test]
    fn the_label_names_the_ref_unless_it_is_head() {
        assert_eq!(setup_label("HEAD"), ".config/shipit.toml");
        assert_eq!(setup_label("main"), "main:.config/shipit.toml");
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

    // TOML integers stop at i64::MAX, so a cap never goes above it.
    #[expect(
        clippy::too_many_lines,
        reason = "one generator builds the whole setup; splitting it would only pass tuples around"
    )]
    fn arb_setup() -> impl Strategy<Value = Setup> {
        let scalars = (
            arb_text(),
            prop_oneof![1_u64..1000, 1_u64..=i64::MAX.unsigned_abs()],
            0_u64..10,
            proptest::collection::vec(arb_text(), 0..3),
            proptest::collection::vec(arb_text(), 0..3),
        );
        let strings = (arb_text(), arb_text(), arb_text());
        let limits = (
            1_u64..50,
            0_u64..50,
            1_u64..=i64::MAX.unsigned_abs(),
            1_u64..=i64::MAX.unsigned_abs(),
        );
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
                    (
                        arb_role(cap, n),
                        arb_role(cap, n),
                        arb_role(cap, n),
                        arb_role(cap, n),
                    ),
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
                    (planner, maker, tester, reader),
                )| {
                    let (max_lines, update_offset, slices, agents) = limits;
                    let then_tier = scout_tier + 1 + offset % (n - scout_tier);
                    Setup {
                        version: Version,
                        gate: Gate {
                            base: BranchName::parse(&base).unwrap(),
                            max_files: Cap(cap_value),
                            check: check.into_iter().map(CheckCommand).collect(),
                            environment: Environment {
                                prefix: run.into_iter().map(NonEmpty).collect(),
                            },
                        },
                        agents: Agents {
                            max_fix_rounds: fix_rounds,
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
                                    escalate_to: EscalateTo {
                                        tier: TierNumber(then_tier),
                                    },
                                },
                                planner,
                                maker,
                                tester,
                                reader,
                            },
                            parallel: Parallel {
                                slices: SliceCap(slices),
                                agents: AgentCap(agents),
                            },
                        },
                        packs: Packs {
                            skill: NonEmpty(skill),
                            domain: NonEmpty(domain),
                            model: NonEmpty(model),
                        },
                        owner: Owner {
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
        fn whole_accepts_every_non_negative_integer(n in 0_i64..=i64::MAX) {
            prop_assert_eq!(whole(&int(n)), u64::try_from(n).ok());
        }

        #[test]
        fn whole_refuses_a_negative_integer(n in i64::MIN..0) {
            prop_assert_eq!(whole(&int(n)), None);
        }

        #[test]
        fn a_text_off_the_scale_is_not_an_effort(word in "[a-zA-Z ]{0,7}") {
            prop_assume!(!SCALE.contains(&word.as_str()));
            prop_assert_eq!(Effort::parse(&word), None);
        }

        #[test]
        fn a_text_that_is_not_toml_is_an_error(
            bad in proptest::sample::select(vec![
                "{\"version\": 1}",
                "cap =",
                "cap = 1\ncap = 2",
                "[a",
                "a = 'b",
                "a = 1 b = 2",
            ]),
        ) {
            prop_assert!(toml_table(bad.as_bytes()).is_err());
        }

        // The bad byte sits inside an otherwise valid file, so a lossy read would accept it.
        #[test]
        fn bytes_that_are_not_utf8_are_an_error(byte in 0x80_u8..=0xff) {
            let mut bytes = b"base = \"".to_vec();
            bytes.push(byte);
            bytes.extend_from_slice(b"\"");
            prop_assert!(toml_table(&bytes).is_err());
        }

        // TOML tells floats from integers: a whole float is not an integer either.
        #[test]
        fn whole_refuses_a_float(x in proptest::num::f64::NORMAL | proptest::num::f64::ZERO) {
            prop_assert_eq!(whole(&Value::Float(x)), None);
        }

        // Kept by the toml crate's preserve_order feature; without it the keys come out sorted.
        #[test]
        fn unknown_keys_are_reported_in_file_order(
            keys in proptest::collection::btree_set("x_[a-z]{1,6}", 2..6)
                .prop_map(|keys| keys.into_iter().collect::<Vec<_>>())
                .prop_shuffle(),
        ) {
            let mut unknown = keys.join(" = 1\n");
            unknown.push_str(" = 1\n");
            let expected: Vec<String> =
                keys.iter().map(|key| format!(".{key}: unknown key")).collect();
            prop_assert_eq!(lines_of_text(&format!("{unknown}{FIXTURE}")), expected);
        }

        // The integers of the file are i64: a bigger one is not a TOML value at all.
        #[test]
        fn an_integer_above_i64_is_not_toml(n in (i64::MAX.unsigned_abs() + 1)..=u64::MAX) {
            let written = format!("cap = {n}");
            prop_assert!(toml_table(written.as_bytes()).is_err());
        }

        #[test]
        fn a_value_of_the_wrong_kind_is_reported_at_its_place(
            (path, pointer) in proptest::sample::select(places()),
            wrong in proptest::sample::select(kinds()),
        ) {
            let mut value = fixture();
            let current = look(&value, &pointer).map(Value::type_str);
            prop_assume!(current != Some(wrong.type_str()));
            put(&mut value, &pointer, wrong);
            let lines = lines_of(&value);
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
            prop_assume!(look(&value, parent).is_some_and(Value::is_table));
            remove(&mut value, &pointer);
            let optional = key == "stand_in" || (key == "effort" && !path.contains("stand_in"));
            let expected = if optional {
                Vec::new()
            } else {
                vec![format!("{path}: missing")]
            };
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn an_unknown_key_is_the_only_problem_in_any_table(
            (path, pointer) in proptest::sample::select(
                [(String::new(), String::new())]
                    .into_iter()
                    .chain(places_where(|_, v| v.is_table()))
                    .collect::<Vec<_>>()
            ),
            key in "[a-z_]{1,10}",
        ) {
            prop_assume!(![ "effort", "stand_in", "parallel"].contains(&key.as_str()));
            let mut value = fixture();
            prop_assume!(look(&value, &pointer).and_then(Value::as_table)
                .is_some_and(|table| !table.contains_key(&key)));
            put(&mut value, &format!("{pointer}/{key}"), int(1));
            prop_assert_eq!(lines_of(&value), vec![format!("{path}.{key}: unknown key")]);
        }

        #[test]
        fn an_empty_text_is_refused_where_a_non_empty_one_is_needed(
            (path, pointer) in proptest::sample::select(places_where(|path, v| {
                v.is_str()
                    && path != ".gate.base"
                    && path != ".agents.effort_cap"
                    && !path.ends_with(".effort")
            })),
        ) {
            let mut value = fixture();
            put(&mut value, &pointer, text(""));
            prop_assert_eq!(
                lines_of(&value),
                vec![format!("{path}: must be a non-empty string")]
            );
        }

        #[test]
        fn command_lists_hold_only_non_empty_strings(
            (path, pointer) in proptest::sample::select(vec![
                (".gate.check", "/gate/check"),
                (".gate.environment.prefix", "/gate/environment/prefix"),
            ]),
            entries in proptest::collection::vec(
                prop_oneof![
                    Just(text("")),
                    Just(int(3)),
                    "[a-z]{1,4}".prop_map(|word| text(&word)),
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
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn the_base_is_a_branch_name(word in prop_oneof!["\\PC{0,12}", "[a-z.~^/ -]{0,6}"]) {
            let mut value = fixture();
            put(&mut value, "/gate/base", text(&word));
            let expected = if BranchName::parse(&word).is_some() {
                Vec::new()
            } else {
                vec![".gate.base: must be a branch name (see git check-ref-format --branch)".to_owned()]
            };
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn the_version_is_the_integer_1(n in integer()) {
            let mut value = fixture();
            put(&mut value, "/version", int(n));
            let expected = if n == 1 {
                Vec::new()
            } else {
                vec![".version: must be the integer 1".to_owned()]
            };
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn a_count_is_accepted_from_its_minimum(
            (path, pointer, min) in proptest::sample::select(vec![
                (".gate.max_files", "/gate/max_files", 1_i64),
                (".agents.max_fix_rounds", "/agents/max_fix_rounds", 0),
                (".agents.roles.scout.retries", "/agents/roles/scout/retries", 0),
                (".owner.max_lines", "/owner/max_lines", 1),
                (
                    ".owner.status_update_max_lines",
                    "/owner/status_update_max_lines",
                    1,
                ),
            ]),
            n in integer(),
        ) {
            let mut value = fixture();
            put(&mut value, "/owner/max_lines", int(i64::MAX));
            put(&mut value, "/owner/status_update_max_lines", int(1));
            let expected = if n >= min {
                Vec::new()
            } else {
                vec![format!("{path}: must be an integer of at least {min}")]
            };
            put(&mut value, pointer, int(n));
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn the_status_limit_is_not_above_the_max_lines(max in 1_i64..40, status in 1_i64..40) {
            let mut value = fixture();
            put(&mut value, "/owner/max_lines", int(max));
            put(&mut value, "/owner/status_update_max_lines", int(status));
            let expected = if status <= max {
                Vec::new()
            } else {
                vec![
                    ".owner.status_update_max_lines: must not be above max_lines"
                        .to_owned(),
                ]
            };
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn an_effort_off_the_scale_is_refused_wherever_one_is_written(
            (path, pointer) in proptest::sample::select(vec![
                (".agents.effort_cap", "/agents/effort_cap"),
                (".agents.tiers[3].stand_in.effort", "/agents/tiers/3/stand_in/effort"),
                (".agents.roles.scout.effort", "/agents/roles/scout/effort"),
                (".agents.roles.planner.effort", "/agents/roles/planner/effort"),
                (".agents.roles.maker.effort", "/agents/roles/maker/effort"),
                (".agents.roles.tester.effort", "/agents/roles/tester/effort"),
                (".agents.roles.reader.effort", "/agents/roles/reader/effort"),
            ]),
            word in "[a-zA-Z ]{0,7}",
        ) {
            prop_assume!(!SCALE.contains(&word.as_str()));
            let mut value = fixture();
            put(&mut value, pointer, text(&word));
            prop_assert_eq!(
                lines_of(&value),
                vec![format!("{path}: must be one of low, medium, high, xhigh, max")]
            );
        }

        #[test]
        fn an_effort_is_accepted_when_it_is_not_above_the_cap(
            (path, pointer) in proptest::sample::select(vec![
                (".agents.tiers[3].stand_in.effort", "/agents/tiers/3/stand_in/effort"),
                (".agents.roles.scout.effort", "/agents/roles/scout/effort"),
                (".agents.roles.planner.effort", "/agents/roles/planner/effort"),
                (".agents.roles.maker.effort", "/agents/roles/maker/effort"),
                (".agents.roles.tester.effort", "/agents/roles/tester/effort"),
                (".agents.roles.reader.effort", "/agents/roles/reader/effort"),
            ]),
            effort in arb_effort(),
            cap in arb_effort(),
        ) {
            let mut value = fixture();
            put(&mut value, "/agents/effort_cap", Value::try_from(cap).unwrap());
            put(&mut value, "/agents/tiers/3/stand_in/effort", text("low"));
            remove(&mut value, "/agents/roles/planner/effort");
            remove(&mut value, "/agents/roles/maker/effort");
            remove(&mut value, "/agents/roles/tester/effort");
            put(&mut value, pointer, Value::try_from(effort).unwrap());
            let expected = if effort <= cap {
                Vec::new()
            } else {
                vec![format!("{path}: must not be above agents.effort_cap")]
            };
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn a_role_names_an_existing_tier(
            (path, pointer) in proptest::sample::select(vec![
                (".agents.roles.planner.tier", "/agents/roles/planner/tier"),
                (".agents.roles.maker.tier", "/agents/roles/maker/tier"),
                (".agents.roles.tester.tier", "/agents/roles/tester/tier"),
                (".agents.roles.reader.tier", "/agents/roles/reader/tier"),
            ]),
            n in -1_i64..8,
        ) {
            let mut value = fixture();
            put(&mut value, pointer, int(n));
            let expected = if (1..=4).contains(&n) {
                Vec::new()
            } else {
                vec![format!("{path}: must be the number of an existing tier")]
            };
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn the_scouts_next_tier_exists_and_is_above_the_scouts_own(
            scout in -1_i64..8,
            then in -1_i64..8,
        ) {
            let mut value = fixture();
            put(&mut value, "/agents/roles/scout/tier", int(scout));
            put(&mut value, "/agents/roles/scout/escalate_to/tier", int(then));
            let exists = |n: i64| (1..=4).contains(&n);
            let mut expected = Vec::new();
            if !exists(scout) {
                expected.push(".agents.roles.scout.tier: must be the number of an existing tier");
            }
            if !exists(then) {
                expected.push(".agents.roles.scout.escalate_to.tier: must be the number of an existing tier");
            }
            if then <= scout {
                expected.push(".agents.roles.scout.escalate_to.tier: must be above agents.roles.scout.tier");
            }
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn the_tiers_are_numbered_one_to_n_in_order(
            numbers in proptest::collection::vec(0_i64..7, 4),
        ) {
            let mut value = fixture();
            let mut expected = Vec::new();
            for (index, (number, want)) in numbers.iter().zip(1_i64..).enumerate() {
                put(&mut value, &format!("/agents/tiers/{index}/tier"), int(*number));
                if *number != want {
                    expected.push(format!(
                        ".agents.tiers[{index}].tier: must be {want} (tiers are numbered 1..n in order)"
                    ));
                }
            }
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn parallel_caps_accepted_iff_whole_at_least_1(
            key in proptest::sample::select(vec!["slices", "agents"]),
            cap in prop_oneof![integer().prop_map(int), proptest::sample::select(kinds())],
        ) {
            let mut value = fixture();
            put(&mut value, "/agents/parallel", Value::Table(Table::new()));
            put(&mut value, &format!("/agents/parallel/{key}"), cap.clone());
            let accepted = whole(&cap).is_some_and(|n| n >= 1);
            let expected = if accepted {
                Vec::new()
            } else {
                vec![format!(".agents.parallel.{key}: must be an integer of at least 1")]
            };
            prop_assert_eq!(lines_of(&value), expected);
        }

        #[test]
        fn no_parallel_table_means_3_and_4(
            table in proptest::option::of((
                proptest::option::of(1_i64..1000),
                proptest::option::of(1_i64..1000),
            )),
        ) {
            let mut value = fixture();
            if let Some((slices, agents)) = table {
                put(&mut value, "/agents/parallel", Value::Table(Table::new()));
                if let Some(n) = slices {
                    put(&mut value, "/agents/parallel/slices", int(n));
                }
                if let Some(n) = agents {
                    put(&mut value, "/agents/parallel/agents", int(n));
                }
            }
            let setup = parse_setup(value.as_table().unwrap()).unwrap();
            let (slices, agents) = table.unwrap_or((None, None));
            prop_assert_eq!(u64::from(&setup.agents.parallel.slices), slices.map_or(3, i64::unsigned_abs));
            prop_assert_eq!(u64::from(&setup.agents.parallel.agents), agents.map_or(4, i64::unsigned_abs));
        }

        #[test]
        fn unknown_key_in_parallel_refused(key in "[a-z_]{1,10}") {
            prop_assume!(!["slices", "agents"].contains(&key.as_str()));
            let mut value = fixture();
            put(&mut value, "/agents/parallel", Value::Table(Table::new()));
            put(&mut value, &format!("/agents/parallel/{key}"), int(1));
            prop_assert_eq!(
                lines_of(&value),
                vec![format!(".agents.parallel.{key}: unknown key")]
            );
        }

        #[test]
        fn setup_numbers_read_back_as_u64(setup in arb_setup()) {
            let json = serde_json::to_value(&setup).unwrap();
            let number = |pointer: &str| json.pointer(pointer).and_then(serde_json::Value::as_u64);
            prop_assert_eq!(number("/agents/roles/maker/tier"), Some(u64::from(&setup.agents.roles.maker.tier)));
            prop_assert_eq!(number("/agents/tiers/0/tier"), setup.agents.tiers.first().map(|tier| u64::from(&tier.tier)));
            prop_assert_eq!(number("/owner/max_lines"), Some(u64::from(&setup.owner.max_lines)));
            prop_assert_eq!(
                number("/owner/status_update_max_lines"),
                Some(u64::from(&setup.owner.status_update_max_lines))
            );
            prop_assert_eq!(number("/agents/parallel/slices"), Some(u64::from(&setup.agents.parallel.slices)));
            prop_assert_eq!(number("/agents/parallel/agents"), Some(u64::from(&setup.agents.parallel.agents)));
        }

        #[test]
        fn a_valid_setup_survives_being_written_as_toml_and_parsed(setup in arb_setup()) {
            let written = toml::to_string(&setup).unwrap();
            let table = toml_table(written.as_bytes()).unwrap();
            prop_assert_eq!(parse_setup(&table), Ok(setup));
        }
    }
}
