# shellcheck shell=bash
# The store (docs/store.md): where ShipIt keeps receipts and records. Needs
# lib/common.sh (die).

# store_path <relative path>: print the absolute path of <relative path> in the
# store. Creates nothing.
store_path() {
  local rel=${1-} common
  case "/$rel/" in
    //* | */../* | */./* | *//*) die 2 "bad store path '$rel' (a bug in shipit)" ;;
  esac
  [[ -n $rel ]] || die 2 "bad store path '' (a bug in shipit)"
  common=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) ||
    die 2 "not inside a git repository; run shipit from inside the project"
  printf '%s/shipit/%s\n' "$common" "$rel"
}

# write_json_atomic <path>: read JSON on stdin and write it to <path>, creating
# the folder. The text goes to a temporary file in the same folder first, then
# is moved into place, so a reader never sees half a file.
write_json_atomic() {
  local path=$1 dir tmp
  dir=$(dirname "$path")
  mkdir -p "$dir"
  tmp=$(mktemp "$dir/.tmp.XXXXXX")
  if ! jq . >"$tmp"; then
    rm -f "$tmp"
    die 2 "refused to write $path: the text is not JSON (a bug in shipit)"
  fi
  mv -f "$tmp" "$path"
}
