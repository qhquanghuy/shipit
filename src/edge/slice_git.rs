use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;

use crate::commands::check::settle_base;
use crate::edge::git::Git;
use crate::edge::records::{build_plan_path, store_dir};
use crate::edge::store::{check_receipt_path, read_json};
use crate::pure::base::BaseRef;
use crate::pure::branch::slice_branch;
use crate::pure::failure::Failure;
use crate::pure::goal::GoalName;
use crate::pure::messages::{NO_FILE, say};
use crate::pure::receipt::{Oid, receipt_validity};
use crate::pure::review::{ReceiptKind, check_base};
use crate::pure::roadmap::SliceName;
use crate::pure::rules::invalid;
use crate::pure::setup::{Setup, parse_setup, setup_label};
use crate::pure::work::{UNCOMMITTED, hidden_paths, hidden_refusal};

// The hash git gives the bytes of the file as they are, so a plan or a roadmap is keyed by what
// the reviewer read and not by any filter of the project.
pub(crate) fn file_hash(git: &Git, path: &Path) -> Result<Oid, Failure> {
    git.output(&[
        OsStr::new("hash-object"),
        OsStr::new("--no-filters"),
        OsStr::new("--"),
        path.as_os_str(),
    ])
    .ok()
    .and_then(|bytes| Oid::parse(String::from_utf8_lossy(&bytes).trim()))
    .ok_or_else(|| Failure::usage(say(NO_FILE, &[("path", &path.display())])))
}

// The ranks and limits are the base branch's word: a branch never judges itself with its own.
// A base with no setup file is the adoption, and the branch's own setup is the policy.
pub(crate) fn load_policy(
    git: &Git,
    head: Setup,
    err: &mut dyn Write,
) -> Result<(Setup, BaseRef), Failure> {
    let (_base, base_ref, flow) = settle_base(git, head.gate.base.clone(), false, err)?;
    let Some(flow) = flow else {
        writeln!(
            err,
            "shipit: the policy is this branch's own setup (adoption)"
        )
        .ok();
        return Ok((head, base_ref));
    };
    let policy = parse_setup(flow.table()).map_err(|problems| {
        Failure::usage(invalid(&setup_label(&base_ref.to_string()), &problems))
    })?;
    Ok((policy, base_ref))
}

// The local branch, else origin's: a slice another clone pushed can be reviewed here.
pub(crate) fn branch_tip(git: &Git, branch: &str) -> Option<Oid> {
    ["refs/heads/", "refs/remotes/origin/"]
        .iter()
        .find_map(|prefix| git.commit_of(OsStr::new(&format!("{prefix}{branch}"))))
}

pub(crate) fn merge_base(git: &Git, a: &Oid, b: &Oid) -> Option<Oid> {
    let bytes = git
        .output(&[
            OsStr::new("merge-base"),
            OsStr::new("--end-of-options"),
            OsStr::new(a.as_str()),
            OsStr::new(b.as_str()),
        ])
        .ok()?;
    Oid::parse(String::from_utf8_lossy(&bytes).trim())
}

// The commit, then its parents.
pub(crate) fn parents(git: &Git, commit: &Oid) -> Result<Vec<Oid>, Failure> {
    let bytes = git
        .output(&[
            OsStr::new("rev-list"),
            OsStr::new("--parents"),
            OsStr::new("-n"),
            OsStr::new("1"),
            OsStr::new(commit.as_str()),
        ])
        .map_err(|_why| Failure::usage(format!("cannot read the parents of {commit}")))?;
    Ok(String::from_utf8_lossy(&bytes)
        .split_whitespace()
        .skip(1)
        .filter_map(Oid::parse)
        .collect())
}

// Every option is given, so that no setting of the user's (prefixes, renames, an external
// diff program, colour, a relative path) changes the bytes the id is made from. None for no
// change at all.
pub(crate) fn patch_id(git: &Git, from: &Oid, to: &Oid) -> Result<Option<Oid>, Failure> {
    let cannot = || Failure::usage(format!("cannot read the change from {from} to {to}"));
    let diff = git
        .output(&[
            OsStr::new("-c"),
            OsStr::new("core.quotePath=true"),
            OsStr::new("diff"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--no-textconv"),
            OsStr::new("--no-color"),
            OsStr::new("--no-relative"),
            OsStr::new("--no-renames"),
            OsStr::new("--ignore-submodules=none"),
            OsStr::new("--submodule=short"),
            OsStr::new("--binary"),
            OsStr::new("--full-index"),
            OsStr::new("-U3"),
            OsStr::new("--inter-hunk-context=0"),
            OsStr::new("--diff-algorithm=myers"),
            OsStr::new("--indent-heuristic"),
            OsStr::new("--src-prefix=a/"),
            OsStr::new("--dst-prefix=b/"),
            OsStr::new(from.as_str()),
            OsStr::new(to.as_str()),
        ])
        .map_err(|_why| cannot())?;
    if diff.is_empty() {
        return Ok(None);
    }
    let ids = git
        .output_fed(
            &[OsStr::new("patch-id"), OsStr::new("--stable")],
            Some(&diff),
        )
        .map_err(|_why| cannot())?;
    let text = String::from_utf8_lossy(&ids);
    Ok(text.split_whitespace().next().and_then(Oid::parse))
}

// How many commits the tip has that the base does not.
pub(crate) fn commits_beyond(git: &Git, tip: &Oid, base: &BaseRef) -> Result<u64, Failure> {
    let cannot = || Failure::usage(format!("cannot count the commits of {tip} beyond {base}"));
    let range = format!("{}..{tip}", base.full());
    let bytes = git
        .output(&[
            OsStr::new("rev-list"),
            OsStr::new("--count"),
            OsStr::new(&range),
        ])
        .map_err(|_why| cannot())?;
    String::from_utf8_lossy(&bytes)
        .trim()
        .parse()
        .map_err(|_why| cannot())
}

// The branch names under `slice/`, local or on origin (no network), each once.
pub(crate) fn slice_branches(git: &Git) -> Result<Vec<String>, Failure> {
    let bytes = git
        .output(&[
            OsStr::new("for-each-ref"),
            OsStr::new("--format=%(refname)"),
            OsStr::new("refs/heads/slice"),
            OsStr::new("refs/remotes/origin/slice"),
        ])
        .map_err(|_why| Failure::usage("cannot list the slice branches"))?;
    let mut names: Vec<String> = String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|name| {
            name.strip_prefix("refs/heads/")
                .or_else(|| name.strip_prefix("refs/remotes/origin/"))
        })
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

// The branch starts at the base's tip with no upstream, so a push never lands on the base.
pub(crate) fn create_branch(git: &Git, branch: &str, base: &BaseRef) -> Result<(), Failure> {
    git.output(&[
        OsStr::new("branch"),
        OsStr::new("--no-track"),
        OsStr::new("--end-of-options"),
        OsStr::new(branch),
        OsStr::new(&base.full()),
    ])
    .map(|_bytes| ())
    .map_err(|_why| Failure::usage(format!("cannot create the branch {branch} at {base}")))
}

// A remote-tracking branch of the goal (no network: the last fetch decides), if there is one.
pub(crate) fn origin_branch_of(git: &Git, goal: &GoalName) -> Result<Option<String>, Failure> {
    let prefix = format!("refs/remotes/origin/slice/{goal}/");
    let bytes = git
        .output(&[
            OsStr::new("for-each-ref"),
            OsStr::new("--count=1"),
            OsStr::new("--format=%(refname:short)"),
            OsStr::new(&prefix),
        ])
        .map_err(|_why| Failure::usage("cannot list the remote branches"))?;
    let name = String::from_utf8_lossy(&bytes).trim().to_owned();
    Ok((!name.is_empty()).then_some(name))
}

// The paths of the coverage maps committed at the revision.
pub(crate) fn map_paths_at(git: &Git, rev: &OsStr) -> Result<Vec<String>, Failure> {
    let bytes = git
        .output(&[
            OsStr::new("ls-tree"),
            OsStr::new("-r"),
            OsStr::new("-z"),
            OsStr::new("--name-only"),
            OsStr::new("--end-of-options"),
            rev,
            OsStr::new("--"),
            OsStr::new(".shipit/coverage/"),
        ])
        .map_err(|_why| Failure::usage("cannot list the coverage maps"))?;
    Ok(bytes
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect())
}

// (status, path) of every file the change touches; a rename is a deletion and an addition.
pub(crate) fn changed_with_status(
    git: &Git,
    from: &Oid,
    to: &OsStr,
) -> Result<Vec<(char, String)>, Failure> {
    let bytes = git
        .output(&[
            OsStr::new("diff"),
            OsStr::new("--name-status"),
            OsStr::new("-z"),
            OsStr::new("--no-renames"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--ignore-submodules=none"),
            OsStr::new("--end-of-options"),
            OsStr::new(from.as_str()),
            to,
        ])
        .map_err(|_why| Failure::usage(format!("cannot list the changes since {from}")))?;
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut changes = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        if let Some(letter) = status.first() {
            changes.push((
                char::from(*letter),
                String::from_utf8_lossy(path).into_owned(),
            ));
        }
    }
    Ok(changes)
}

// A record is of a commit's content, so work that is not committed has none.
pub(crate) fn refuse_unclean_work(git: &Git) -> Result<(), Failure> {
    let hidden = hidden_paths(&git.ls_files_v_z()?);
    if !hidden.is_empty() {
        return Err(Failure::unclean(hidden_refusal(&hidden)));
    }
    if !git.status_porcelain()?.is_empty() {
        return Err(Failure::unclean(UNCOMMITTED));
    }
    Ok(())
}

// What `shipit check` judged: the base it used (its short name), the commit where the work left
// it, and the id of the change since.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) base: String,
    pub(crate) merge_base: Oid,
    pub(crate) patch_id: Option<Oid>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Tip {
    pub(crate) commit: Oid,
    pub(crate) tree: Oid,
    pub(crate) checked: Option<Checked>,
}

// The key a receipt of this kind would have for the slice's current version; None when the
// version has none yet (no plan file, no branch, no checked change).
pub(crate) fn current_key(
    git: &Git,
    common: &Path,
    (goal, slice): (&GoalName, &SliceName),
    kind: ReceiptKind,
) -> Result<Option<Oid>, Failure> {
    let store = store_dir(common);
    match kind {
        ReceiptKind::PlanReview => Ok(file_hash(git, &build_plan_path(&store, goal, slice)).ok()),
        ReceiptKind::InformedReview | ReceiptKind::SecondOpinion | ReceiptKind::TestDrive => {
            let Some(commit) = branch_tip(git, &slice_branch(goal, slice)) else {
                return Ok(None);
            };
            let tip = tip_of(git, common, commit)?;
            Ok(match kind {
                ReceiptKind::TestDrive => Some(tip.tree),
                ReceiptKind::PlanReview
                | ReceiptKind::InformedReview
                | ReceiptKind::SecondOpinion => tip.checked.and_then(|checked| checked.patch_id),
            })
        }
    }
}

// A review is keyed by the change `check` judged, so a version with no valid check receipt has
// no key.
pub(crate) fn tip_of(git: &Git, common: &Path, commit: Oid) -> Result<Tip, Failure> {
    let tree = git.tree_of(OsStr::new(commit.as_str()))?;
    let docs = read_json(&check_receipt_path(common, &tree));
    let base = receipt_validity(docs.as_deref(), &tree)
        .and(docs.as_deref())
        .and_then(<[_]>::first)
        .and_then(check_base);
    let checked = match base {
        None => None,
        Some(base) => {
            let fork = merge_base(git, &base.commit, &commit)
                .ok_or_else(|| Failure::usage(format!("no shared history with {}", base.name)))?;
            Some(Checked {
                patch_id: patch_id(git, &fork, &commit)?,
                base: base.name,
                merge_base: fork,
            })
        }
    };
    Ok(Tip {
        commit,
        tree,
        checked,
    })
}
