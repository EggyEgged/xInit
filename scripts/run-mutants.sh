#!/usr/bin/env bash
set -euo pipefail
# run-mutants.sh <output-dir> [extra cargo-mutants args...]
#
# Runs cargo-mutants over the whole workspace with every test in the workspace,
# using scripts/mutants-runner.sh so binary-driven tests see the mutated xrginit.
# Results land in <output-dir>/mutants.out (missed.txt, caught.txt, ...).
# Exit codes are cargo-mutants' own: 0 all caught, 2 some missed, 3 timeouts,
# 4 baseline tests failed (nothing was measured).
#
# Sandbox: a mutant can make xrginit report any pid (for example pid=1), and a test
# that signals that pid could then run kill(-1, SIGKILL), which kills every process
# the user owns. That took down the whole desktop session twice during M0. So the
# run always happens inside a new user + PID namespace with its own /proc: there,
# kill(-1) reaches only processes inside the namespace. This script is PID 1 of the
# namespace; it runs cargo-mutants as a child (not exec) so that it reaps orphans,
# and kill(-1) never reaches it.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail() { echo "run-mutants: FAILED: $*" >&2; exit 1; }

[[ $# -ge 1 ]] || fail "usage: scripts/run-mutants.sh <output-dir> [extra cargo-mutants args...]"

if [[ "${XINIT_MUTANTS_SANDBOX:-}" != "1" ]]; then
    command -v unshare >/dev/null || fail "unshare (util-linux) is required for the sandbox"
    export XINIT_MUTANTS_SANDBOX=1
    exec unshare --user --map-current-user --pid --fork --mount-proc --kill-child \
        "$ROOT/scripts/run-mutants.sh" "$@"
fi
[[ $$ -eq 1 ]] || fail "expected to run as PID 1 of the sandbox namespace, got PID $$"

OUT="$(realpath -m "$1")"
shift
command -v cargo-mutants >/dev/null || fail "cargo-mutants is not installed"
mkdir -p "$OUT"

triple="$(rustc -vV | sed -n 's/^host: //p')"
[[ -n "$triple" ]] || fail "could not read the host triple from rustc -vV"

cd "$ROOT"
status=0
cargo mutants \
    --workspace \
    --test-workspace=true \
    --cap-lints=true \
    --output "$OUT" \
    "--cargo-arg=--config=target.$triple.runner=\"$ROOT/scripts/mutants-runner.sh\"" \
    "$@" || status=$?
exit "$status"
