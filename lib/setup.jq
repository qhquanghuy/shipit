# The setup file schema (docs/setup.md). Pure: JSON in, JSON out.
#
# setup_errors: input is an array of the JSON documents in the file (jq -s);
# output is an array of "<path>: <problem>" strings, empty when valid.
# effort_rank($e): index of $e on the effort scale, null if not on it.

def efforts: ["low", "medium", "high", "xhigh", "max"];

def effort_rank($e):
  if ($e | type) == "string" then efforts | index($e) else null end;

def is_int: type == "number" and . == floor;
def is_int_min($min): is_int and . >= $min;
def non_empty_string: type == "string" and length > 0;

# Errors of the value on input: none if f is true on it.
def rule($path; f; $msg):
  if f then [] else ["\($path): \($msg)"] end;

# Same, for the key $k of the object on input. A missing key is reported by
# shape, so it is skipped here.
def at($k; $path; f; $msg):
  if has($k) then .[$k] | rule("\($path).\($k)"; f; $msg) else [] end;

# The value on input must be an object; then f gives its errors.
def object_at($path; f):
  if type == "object" then f else ["\($path): must be an object"] end;

# The object on input has exactly the required keys plus some optional ones.
def shape($path; $required; $optional):
  (keys_unsorted - ($required + $optional) | map("\($path).\(.): unknown key"))
  + ($required - keys_unsorted | map("\($path).\(.): missing"));

# The effort on input must be on the scale and, given a cap rank, not above it.
def effort_errors($path; $cap):
  if effort_rank(.) == null then
    ["\($path): must be one of \(efforts | join(", "))"]
  elif $cap != null and effort_rank(.) > $cap then
    ["\($path): must not be above effort_cap"]
  else [] end;

# The tier number on input must name one of $count tiers (null: count unknown).
def tier_ref_errors($path; $count):
  if is_int_min(1) and ($count == null or . <= $count) then []
  else ["\($path): must be the number of an existing tier"] end;

# The optional effort of the object on input.
def effort_at($path; $cap):
  if has("effort") then .effort | effort_errors("\($path).effort"; $cap) else [] end;

# The tier of the role object on input.
def tier_at($path; $count):
  if has("tier") then .tier | tier_ref_errors("\($path).tier"; $count) else [] end;

def stand_in_errors($path; $cap):
  object_at($path;
    shape($path; ["model", "effort"]; [])
    + at("model"; $path; non_empty_string; "must be a non-empty string")
    + effort_at($path; $cap));

# Element $i of tiers.
def tier_errors($i; $cap):
  ".tiers[\($i)]" as $path
  | object_at($path;
      shape($path; ["tier", "model"]; ["stand_in"])
      + at("tier"; $path; . == $i + 1; "must be \($i + 1) (tiers are numbered 1..n in order)")
      + at("model"; $path; non_empty_string; "must be a non-empty string")
      + (if has("stand_in") then .stand_in | stand_in_errors("\($path).stand_in"; $cap) else [] end));

def tiers_errors($cap):
  if type != "array" or length == 0 then [".tiers: must be a non-empty array of objects"]
  else [to_entries[] | .key as $i | .value | tier_errors($i; $cap)] | add end;

# The scout's own fields, on the scout object: retries and then.
def scout_errors($count):
  .tier as $tier
  | at("retries"; ".roles.scout"; is_int_min(0); "must be an integer of at least 0")
  + (if has("then") then
      .["then"]
      | object_at(".roles.scout.then";
          shape(".roles.scout.then"; ["tier"]; [])
          + (if has("tier") then
              .tier
              | tier_ref_errors(".roles.scout.then.tier"; $count)
                + (if is_int and ($tier | is_int) and . <= $tier
                   then [".roles.scout.then.tier: must be above roles.scout.tier"]
                   else [] end)
            else [] end))
    else [] end);

def role_errors($name; $cap; $count):
  ".roles.\($name)" as $path
  | object_at($path;
      shape($path; ["tier"] + (if $name == "scout" then ["retries", "then"] else [] end); ["effort"])
      + tier_at($path; $count)
      + effort_at($path; $cap)
      + (if $name == "scout" then scout_errors($count) else [] end));

def roles_errors($cap; $count):
  ["scout", "maker", "tester"] as $names
  | object_at(".roles";
      shape(".roles"; $names; [])
      + ([$names[] as $n | select(has($n)) | .[$n] | role_errors($n; $cap; $count)] | add // []));

def strings_errors($path):
  if type != "array" then ["\($path): must be an array of non-empty strings"]
  else [to_entries[] | select(.value | non_empty_string | not)
        | "\($path)[\(.key)]: must be a non-empty string"] end;

# The environment object on input: the command prefix every check runs behind.
def environment_errors:
  object_at(".environment";
    shape(".environment"; ["run"]; [])
    + (if has("run") then .run | strings_errors(".environment.run") else [] end));

def packs_errors:
  object_at(".packs";
    shape(".packs"; ["skill", "domain", "model"]; [])
    + ([("skill", "domain", "model") as $k
        | at($k; ".packs"; non_empty_string; "must be a non-empty string")] | add));

# The errors of one setup object.
# The messages_to_owner object on input: two line limits, the update limit within the first.
def messages_errors:
  object_at(".messages_to_owner";
    shape(".messages_to_owner"; ["max_lines", "status_update_max_lines"]; [])
    + ([("max_lines", "status_update_max_lines") as $k
        | at($k; ".messages_to_owner"; is_int_min(1); "must be an integer of at least 1")] | add)
    + (if (.max_lines | is_int_min(1)) and (.status_update_max_lines | is_int_min(1))
          and .status_update_max_lines > .max_lines
       then [".messages_to_owner.status_update_max_lines: must not be above max_lines"]
       else [] end));

def setup_object_errors:
  effort_rank(.effort_cap) as $cap
  | (if (.tiers | type) == "array" then .tiers | length else null end) as $count
  | shape(""; ["version", "base", "cap", "check", "environment", "fix_rounds", "effort_cap", "tiers", "roles", "packs", "messages_to_owner"]; [])
  + at("version"; ""; . == 1; "must be the integer 1")
  + at("base"; ""; non_empty_string; "must be a non-empty string")
  + at("cap"; ""; is_int_min(1); "must be an integer of at least 1")
  + at("fix_rounds"; ""; is_int_min(0); "must be an integer of at least 0")
  + at("effort_cap"; ""; effort_rank(.) != null; "must be one of \(efforts | join(", "))")
  + (if has("check") then .check | strings_errors(".check") else [] end)
  + (if has("environment") then .environment | environment_errors else [] end)
  + (if has("tiers") then .tiers | tiers_errors($cap) else [] end)
  + (if has("roles") then .roles | roles_errors($cap; $count) else [] end)
  + (if has("packs") then .packs | packs_errors else [] end)
  + (if has("messages_to_owner") then .messages_to_owner | messages_errors else [] end);

def setup_errors:
  if length != 1 then
    [".: must hold exactly one JSON document, found \(length)"]
  elif (.[0] | type) != "object" then
    [".: must be a JSON object"]
  else .[0] | setup_object_errors end;
