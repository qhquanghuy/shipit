# shellcheck shell=bash
# Shared helpers. Sourced by bin/shipit and by libexec scripts.

# die <code> <message>: print "shipit: <message>" to stderr and exit.
die() {
  local code=$1
  shift
  printf 'shipit: %s\n' "$*" >&2
  exit "$code"
}

# require_bash <major> <minor> <patch>: die 2 unless that bash version is 4.4 or
# newer (inherit_errexit needs it). Called with BASH_VERSINFO below; taking the
# version as arguments is what lets a test try old versions.
require_bash() {
  local major=$1 minor=$2 patch=$3
  if ((major < 4 || (major == 4 && minor < 4))); then
    die 2 "shipit needs bash 4.4 or newer (found $major.$minor.$patch)"
  fi
}

require_bash "${BASH_VERSINFO[0]}" "${BASH_VERSINFO[1]}" "${BASH_VERSINFO[2]}"

# A failure inside $(...) must end that substitution, so a die in a function
# called as x=$(f) stops it before it can print a second message.
shopt -s inherit_errexit

# repo_top: print the top folder of the current git repository.
repo_top() {
  git rev-parse --show-toplevel 2>/dev/null ||
    die 2 "not inside a git repository; run shipit from inside the project"
}

# jq_lib <jq args>: run jq with the modules in lib/ on the include path.
jq_lib() {
  jq -L "$SHIPIT_ROOT/lib" "$@"
}
