#!/usr/bin/env bash
set -euo pipefail
# mutants-runner.sh <test-executable> [args...]: cargo target runner for mutation testing.
#
# Most tests run the xrginit binary through XRGINIT_BIN, but `cargo build --tests`
# and `cargo test` (all cargo-mutants runs) never rebuild that binary. Without this
# runner every binary-driven test would run against a stale, unmutated build and
# every mutant in the CLI would count as missed.
#
# Cargo calls this script for each test executable. It rebuilds xrginit from the
# tree that holds the executable (the cargo-mutants scratch copy, with the mutant
# applied), into a separate target dir so it never waits on cargo's build lock,
# points XRGINIT_BIN at that build, and execs the test executable.
#
# Use it through scripts/run-mutants.sh, which installs it as the runner for the
# host target triple.

fail() { echo "mutants-runner: FAILED: $*" >&2; exit 101; }

[[ $# -ge 1 ]] || fail "usage: mutants-runner.sh <test-executable> [args...]"
exe="$1"
shift
case "$exe" in
    /*/target/debug/deps/*) ;;
    *) fail "unexpected executable path $exe (want <workspace>/target/debug/deps/<name>)" ;;
esac
target="${exe%/debug/deps/*}"
ws="$(dirname "$target")"
[[ -f "$ws/Cargo.toml" ]] || fail "no Cargo.toml at $ws"
bin_target="$target/xrginit-bin"

cargo build --quiet --locked --offline --manifest-path "$ws/Cargo.toml" \
    -p xinit-core --bin xrginit --target-dir "$bin_target" >&2 \
    || fail "building xrginit from $ws failed"

export XRGINIT_BIN="$bin_target/debug/xrginit"
[[ -x "$XRGINIT_BIN" ]] || fail "$XRGINIT_BIN missing after build"
exec "$exe" "$@"
