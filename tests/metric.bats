#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load helpers
load slice-helpers

metric() {
  run --separate-stderr "$SHIPIT_BIN" metric "$@"
}

# other_slices <count>: a metric record of clone-ratio for each of <count> other slices, valued 1, 2, ...
other_slices() {
  local i dir
  for i in $(seq 1 "$1"); do
    dir="$(store)/receipts/metric/$(printf '%040d' "$i")"
    mkdir -p "$dir"
    printf '{"kind":"metric","version":1,"key":"%040d","goal":"demo","slice":"other-%s","name":"clone-ratio","value":%s,"time":"2026-11-21T10:00:00Z","tool":"shipit 0.1.0"}\n' \
      "$i" "$i" "$i" >"$dir/clone-ratio.json"
  done
}

@test "metric: records the value for the current tree and slice" {
  make_slice_repo
  make_work slice/demo/beta 1
  metric clone-ratio 2.1
  [ "$status" -eq 0 ]
  [ "$output" = "metric: clone-ratio 2.1" ]
  local tree
  tree=$(git rev-parse 'HEAD^{tree}')
  [ "$(jq -c '[.kind, .key, .goal, .slice, .name, .value]' "$(store)/receipts/metric/$tree/clone-ratio.json")" = \
    "[\"metric\",\"$tree\",\"demo\",\"beta\",\"clone-ratio\",2.1]" ]
  git checkout -q main
  metric cognitive-complexity 31
  [ "$status" -eq 0 ]
  [ "$(jq -c '[.goal, .slice]' "$(store)/receipts/metric/$(git rev-parse 'HEAD^{tree}')/cognitive-complexity.json")" = "[null,null]" ]
}

@test "metric: flags an outlier against ten other slices" {
  make_slice_repo
  make_work slice/demo/beta 1
  other_slices 9
  metric clone-ratio 99
  [ "$output" = "metric: clone-ratio 99" ]
  other_slices 10
  metric clone-ratio 15
  [ "$output" = "metric: clone-ratio 15" ]
  metric clone-ratio 16
  [ "$status" -eq 0 ]
  [ "$output" = "metric: clone-ratio 16 (outlier: above 15.5 from 10 slices, local history)" ]
}

@test "metric: history counts the kept metrics of a completed goal" {
  make_slice_repo
  make_work slice/demo/beta 1
  mkdir -p "$(store)/metrics"
  local i
  for i in $(seq 1 10); do
    printf '[slices.s%s]\nclone-ratio = %s\n' "$i" "$i"
  done >"$(store)/metrics/old.toml"
  printf '[complete]\nissues = []\n' >>"$(store)/metrics/old.toml"
  metric clone-ratio 16
  [ "$status" -eq 0 ]
  [ "$output" = "metric: clone-ratio 16 (outlier: above 15.5 from 10 slices, local history)" ]
  printf 'not toml [\n' >"$(store)/metrics/broken.toml"
  metric clone-ratio 16
  [ "$status" -eq 0 ]
  [ "$stderr" = "shipit: metrics/broken.toml is not a metrics file; skipped" ]
}

@test "metric: a dirty tree is refused" {
  make_slice_repo
  make_work slice/demo/beta 1
  printf 'more\n' >>beta-1
  metric clone-ratio 1
  [ "$status" -eq 3 ]
  [ "$stderr" = "shipit: uncommitted changes: commit first, a receipt is per commit content" ]
  [ ! -e "$(store)/receipts/metric" ]
}

@test "metric: a name or a value that is not allowed exits 2" {
  make_slice_repo
  metric Clone 1
  [ "$status" -eq 2 ]
  metric clone-ratio -1
  [ "$status" -eq 2 ]
  metric clone-ratio nan
  [ "$status" -eq 2 ]
  metric clone-ratio
  [ "$status" -eq 2 ]
  [ ! -e "$(store)/receipts/metric" ]
}
