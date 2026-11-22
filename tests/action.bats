#!/usr/bin/env bats

bats_require_minimum_version 1.5.0

# This test reads action.yml and builds nothing, so it needs no shipit binary. It proves the text
# of the build step only; the run on GitHub is proven by the proof step gates-github-proof.
# It holds the whole step, not a pattern: patterns missed a subshell, `builtin cd`, `CARGO_HOME`
# and `--config`. So any edit of the step fails here, and whoever edits the step edits this test.
# Blank and comment lines are left out: they change nothing, and a comment must not end the step.

ACTION="$BATS_TEST_DIRNAME/../action.yml"

@test "action: shipit is built in the action's own folder with the stable toolchain, and the step is held word for word" {
  actual=$(awk '/^    - name: Build ShipIt$/ { on = 1; print; next } on && /^    - / { exit } on && NF && $1 !~ /^#/' "$ACTION")
  expected=$(cat <<'STEP'
    - name: Build ShipIt
      shell: bash
      env:
        RUSTUP_TOOLCHAIN: stable
        RUSTFLAGS: --cap-lints=warn
        CARGO_PROFILE_RELEASE_LTO: "off"
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "16"
        CARGO_TARGET_DIR: ${{ runner.temp }}/shipit
      run: |
        cd "$GITHUB_ACTION_PATH"
        cargo build --release --locked --manifest-path "$GITHUB_ACTION_PATH/Cargo.toml"
        echo "$RUNNER_TEMP/shipit/release" >> "$GITHUB_PATH"
STEP
  )
  [ "$actual" = "$expected" ] || { printf 'the step in action.yml is now:\n%s\n' "$actual"; false; }
}
