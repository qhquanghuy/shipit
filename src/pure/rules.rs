use std::fmt;

use toml::{Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    path: String,
    message: String,
}

impl Problem {
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

// An Err with no problems is a missing key: the shape rule reports it.
pub(crate) type Parsed<T> = Result<T, Vec<Problem>>;

pub(crate) fn fail<T>(path: &str, message: &str) -> Parsed<T> {
    Err(vec![Problem::new(path, message)])
}

// Collects problems in the order the rules ran; builds the value only when there are none.
#[derive(Debug, Default)]
pub(crate) struct Problems(Vec<Problem>);

impl Problems {
    pub(crate) fn push(&mut self, problem: Problem) {
        self.0.push(problem);
    }

    pub(crate) fn add(&mut self, more: Vec<Problem>) {
        self.0.extend(more);
    }

    pub(crate) fn take<T>(&mut self, parsed: Parsed<T>) -> Option<T> {
        match parsed {
            Ok(value) => Some(value),
            Err(more) => {
                self.0.extend(more);
                None
            }
        }
    }

    pub(crate) fn finish<T>(self, build: impl FnOnce() -> Option<T>) -> Parsed<T> {
        if self.0.is_empty() {
            build().ok_or(self.0)
        } else {
            Err(self.0)
        }
    }
}

// TOML integers are i64: a float is the wrong kind, and so is a negative number.
pub(crate) fn whole(value: &Value) -> Option<u64> {
    u64::try_from(value.as_integer()?).ok()
}

pub(crate) fn non_empty(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

// For the rule "then.tier must be above scout.tier": any sign counts.
pub(crate) fn integer_at_most(value: &Value, bound: &Value) -> bool {
    matches!((value.as_integer(), bound.as_integer()), (Some(a), Some(b)) if a <= b)
}

// Unknown keys in file order, then missing keys in schema order.
pub(crate) fn shape(
    path: &str,
    object: &Table,
    required: &[&str],
    optional: &[&str],
) -> Vec<Problem> {
    let unknown = object
        .keys()
        .filter(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
        .map(|key| Problem::new(format!("{path}.{key}"), "unknown key"));
    let missing = required
        .iter()
        .filter(|key| !object.contains_key(**key))
        .map(|key| Problem::new(format!("{path}.{key}"), "missing"));
    unknown.chain(missing).collect()
}

pub(crate) fn object_at<'a>(value: &'a Value, path: &str) -> Parsed<&'a Table> {
    value
        .as_table()
        .ok_or_else(|| vec![Problem::new(path, "must be an object")])
}

// The key `key` of `object`, which `rule` must accept.
pub(crate) fn at<T>(
    object: &Table,
    key: &str,
    path: &str,
    rule: impl FnOnce(&Value) -> Option<T>,
    message: &str,
) -> Parsed<T> {
    match object.get(key) {
        None => Err(Vec::new()),
        Some(value) => {
            rule(value).ok_or_else(|| vec![Problem::new(format!("{path}.{key}"), message)])
        }
    }
}

pub(crate) fn present<T>(
    object: &Table,
    key: &str,
    parse: impl FnOnce(&Value) -> Parsed<T>,
) -> Parsed<T> {
    object.get(key).map_or_else(|| Err(Vec::new()), parse)
}

pub(crate) fn invalid(label: &str, problems: &[Problem]) -> String {
    let mut message = format!("{label} is invalid:");
    for problem in problems {
        message.push('\n');
        message.push_str(&problem.to_string());
    }
    message
}

pub(crate) fn optional<T>(
    object: &Table,
    key: &str,
    parse: impl FnOnce(&Value) -> Parsed<T>,
) -> Parsed<Option<T>> {
    object
        .get(key)
        .map_or(Ok(None), |value| parse(value).map(Some))
}
