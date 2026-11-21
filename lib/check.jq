# The rules of `shipit check` that are decisions. Pure: JSON in, JSON out.
#
# baseline_errors: input is an array of the JSON documents in a base branch's
# setup file (jq -s); output is an array of "<path>: <problem>" strings, empty
# when the file is usable as a baseline. Only `cap` and `check` are looked at,
# so growing the setup schema never breaks the comparison.
#
# weakenings: input {"base": {cap, check}, "branch": {cap, check}}; output an
# array of strings naming how the branch weakens the base's checks (raises the
# cap, drops a check command); empty when it does not.
#
# change_verdict: input {dirty, tree_before, tree_after, commit_before,
# commit_after}, where dirty is true when `git status` lists anything after the
# checks ran; output {"ok": bool, "kind": "none" | "dirty" | "committed" |
# "moved_head"}.

def baseline_errors:
  if length != 1 then
    [".: must hold exactly one JSON document, found \(length)"]
  elif (.[0] | type) != "object" then
    [".: must be a JSON object"]
  else
    .[0]
    | (if (.cap | type) == "number" and .cap == (.cap | floor) and .cap >= 1
       then [] else [".cap: must be an integer of at least 1"] end)
    + (if (.check | type) == "array" and all(.check[]; type == "string")
       then [] else [".check: must be an array of strings"] end)
  end;

def weakenings:
  .base as $base
  | .branch as $branch
  | (if $branch.cap > $base.cap
     then ["cap raised from \($base.cap) to \($branch.cap)"] else [] end)
  + [$base.check | unique[] as $cmd
     | select(($branch.check | index($cmd)) == null)
     | "check command removed: \($cmd)"];

def change_verdict:
  if .dirty then {ok: false, kind: "dirty"}
  elif .tree_after != .tree_before then {ok: false, kind: "committed"}
  elif .commit_after != .commit_before then {ok: false, kind: "moved_head"}
  else {ok: true, kind: "none"} end;
