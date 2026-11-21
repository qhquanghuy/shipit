# The receipt rule. Pure: JSON in, JSON out.
#
# receipt_valid($tree): input is the content of a receipt file, or null when
# the file is missing or unreadable; output {"ok": bool, "reason": "…"}. Valid
# only for an object with kind "check", version 1 and key equal to $tree.

def receipt_valid($tree):
  if type == "object" and .kind == "check" and .version == 1 and .key == $tree
  then {ok: true, reason: "valid receipt"}
  else {ok: false, reason: "no receipt for this content: run shipit check"}
  end;
