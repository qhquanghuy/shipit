# shellcheck shell=bash
# Shared helpers. Sourced by bin/shipit and by libexec scripts.

# A failure inside $(...) must end that substitution, so a die in a function
# called as x=$(f) stops it before it can print a second message. Needs bash 4.4.
shopt -s inherit_errexit

# die <code> <message>: print "shipit: <message>" to stderr and exit.
die() {
  local code=$1
  shift
  printf 'shipit: %s\n' "$*" >&2
  exit "$code"
}

# repo_top: print the top folder of the current git repository.
repo_top() {
  git rev-parse --show-toplevel 2>/dev/null ||
    die 2 "not inside a git repository; run shipit from inside the project"
}

# jq_lib <jq args>: run jq with the modules in lib/ on the include path.
jq_lib() {
  jq -L "$SHIPIT_ROOT/lib" "$@"
}
