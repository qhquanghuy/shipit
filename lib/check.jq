# The rules of `shipit check` that are decisions. Pure: JSON in, JSON out.
#
# baseline_errors: input is an array of the JSON documents in a base branch's
# setup file (jq -s); output is an array of "<path>: <problem>" strings, empty
# when the file is usable as a baseline. Only `base`, `cap`, `check` and
# `environment` are looked at, so growing the setup schema never breaks the
# comparison.
#
# setup_changes: input {"base": {base, cap, check, environment}, "branch": {…}}
# (the base branch's setup and the slice's, flow-critical fields only); output
# an array of the field names that are not identical, in the order base, cap,
# check, environment; empty when the slice leaves them all as they are.
#
# base_verdict: input {trusted, base, current, ref_commit, head_commit};
# trusted is true when the base name came from `--base`, current is the name of
# the checked-out branch (null when HEAD is detached); output {"ok": bool,
# "reason": "…"}. A base named by the setup, which the branch controls, must not
# be the branch itself or any ref at the commit being checked: it could not
# vouch for the checks.
#
# change_verdict: input {dirty, hidden, tree_before, tree_after, commit_before,
# commit_after}, where dirty is true when `git status` lists anything after the
# checks ran and hidden is true when a tracked file is marked skip-worktree or
# assume-unchanged; output {"ok": bool, "kind": "none" | "dirty" | "hidden" |
# "committed" | "moved_head"}.

def flow_fields: ["base", "cap", "check", "environment"];

def baseline_errors:
  if length != 1 then
    [".: must hold exactly one JSON document, found \(length)"]
  elif (.[0] | type) != "object" then
    [".: must be a JSON object"]
  else
    .[0]
    | (if (.base | type) == "string" and (.base | length) > 0
       then [] else [".base: must be a non-empty string"] end)
    + (if (.cap | type) == "number" and .cap == (.cap | floor) and .cap >= 1
       then [] else [".cap: must be an integer of at least 1"] end)
    + (if (.check | type) == "array" and all(.check[]; type == "string")
       then [] else [".check: must be an array of strings"] end)
    + (if (.environment | type) == "object"
          and (.environment.run | type) == "array"
          and all(.environment.run[]; type == "string")
       then [] else [".environment.run: must be an array of strings"] end)
  end;

def setup_changes:
  .base as $base
  | .branch as $branch
  | [flow_fields[] | select($base[.] != $branch[.])];

def base_verdict:
  if .trusted then {ok: true, reason: ""}
  elif .current == .base then
    {ok: false, reason: "is the branch being checked"}
  elif .ref_commit == .head_commit then
    {ok: false, reason: "points at the commit being checked"}
  else {ok: true, reason: ""} end;

def change_verdict:
  if .dirty then {ok: false, kind: "dirty"}
  elif (.hidden // false) then {ok: false, kind: "hidden"}
  elif .tree_after != .tree_before then {ok: false, kind: "committed"}
  elif .commit_after != .commit_before then {ok: false, kind: "moved_head"}
  else {ok: true, kind: "none"} end;
