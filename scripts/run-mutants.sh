#!/usr/bin/env bash
set -euo pipefail
# run-mutants.sh <output-dir> [extra cargo-mutants args...]
#
# Runs cargo-mutants over the whole workspace with every test in the workspace,
# using scripts/mutants-runner.sh so binary-driven tests see the mutated xrginit.
# Results land in <output-dir>/mutants.out (missed.txt, caught.txt, ...).
# Exit codes are cargo-mutants' own: 0 all caught, 2 some missed, 3 timeouts,
# 4 baseline tests failed (nothing was measured).

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail() { echo "run-mutants: FAILED: $*" >&2; exit 1; }

[[ $# -ge 1 ]] || fail "usage: scripts/run-mutants.sh <output-dir> [extra cargo-mutants args...]"
OUT="$(realpath -m "$1")"
shift
command -v cargo-mutants >/dev/null || fail "cargo-mutants is not installed"
mkdir -p "$OUT"

triple="$(rustc -vV | sed -n 's/^host: //p')"
[[ -n "$triple" ]] || fail "could not read the host triple from rustc -vV"

cd "$ROOT"
exec cargo mutants \
    --workspace \
    --test-workspace=true \
    --cap-lints=true \
    --output "$OUT" \
    "--cargo-arg=--config=target.$triple.runner=\"$ROOT/scripts/mutants-runner.sh\"" \
    "$@"
