use std::ffi::{OsStr, OsString};
use std::io::Write;

use serde::Serialize;

use crate::commands::setup::load_setup_table;
use crate::edge::check_env::branch_name;
use crate::edge::clock::now_utc;
use crate::edge::git::Git;
use crate::edge::records::{kept_metrics, metric_record_path, metric_records, store_dir};
use crate::edge::slice_git::refuse_unclean_work;
use crate::edge::store::write_json_atomic;
use crate::pure::branch::parse_slice_branch;
use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::metric::{MetricName, MetricValue, history, outlier, read_record};
use crate::pure::review::tool;
use crate::pure::roadmap::SliceName;
use crate::pure::setup::HEAD;

pub(crate) const HIDDEN: bool = false;
pub(crate) const NEEDS_SETUP: bool = true;
pub(crate) const USAGE: &str = "usage: shipit metric <name> <value>";
pub(crate) const SUMMARY: &str = "Records one measured number for this version of the work and says when it is an outlier against the project's history; it never fails on an outlier.";
pub(crate) const EXITS: [&str; 4] = [
    "recorded",
    "",
    "usage or setup error",
    "uncommitted changes",
];

#[derive(Serialize)]
struct MetricFile<'a> {
    kind: &'static str,
    version: u8,
    key: String,
    goal: Option<&'a GoalName>,
    slice: Option<&'a SliceName>,
    name: &'a MetricName,
    value: MetricValue,
    time: String,
    tool: String,
}

pub(crate) fn run(
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let git = Git::find()?;
    let (_head, _table) = load_setup_table(&git, OsStr::new(HEAD))?;
    let [name, number] = args else {
        return Err(Failure::usage(USAGE));
    };
    let name = name.to_str().and_then(MetricName::parse).ok_or_else(|| {
        Failure::usage(format!(
            "'{}' is not a metric name (lowercase letters, digits and \"-\", starting with a \
                 letter); {USAGE}",
            name.to_string_lossy()
        ))
    })?;
    let value = number
        .to_str()
        .and_then(MetricValue::parse)
        .ok_or_else(|| {
            Failure::usage(format!(
                "'{}' is not a number that is finite and not negative; {USAGE}",
                number.to_string_lossy()
            ))
        })?;
    refuse_unclean_work(&git)?;

    let tree = git.tree_of(OsStr::new(HEAD))?;
    let on = branch_name(&git).as_deref().and_then(parse_slice_branch);
    let store = store_dir(&git.common_dir()?);
    let (kept, skipped) = kept_metrics(&store);
    for goal in skipped {
        writeln!(
            err,
            "shipit: metrics/{goal}.toml is not a metrics file; skipped"
        )
        .ok();
    }
    let others = history(
        &metric_records(&store)
            .iter()
            .filter_map(read_record)
            .collect::<Vec<_>>(),
        &kept,
        &name,
        on.as_ref().map(|(goal, slice)| (goal, slice)),
    );
    let file = MetricFile {
        kind: "metric",
        version: 1,
        key: tree.to_string(),
        goal: on.as_ref().map(|(goal, _)| goal),
        slice: on.as_ref().map(|(_, slice)| slice),
        name: &name,
        value,
        time: now_utc(),
        tool: tool(),
    };
    write_json_atomic(&metric_record_path(&store, &tree, &name), &file)?;
    let flag = outlier(value, &others).map_or_else(String::new, |fence| {
        format!(
            " (outlier: above {} from {} slices, local history)",
            fence.above, fence.from
        )
    });
    writeln!(out, "metric: {name} {value}{flag}").ok();
    Ok(())
}
