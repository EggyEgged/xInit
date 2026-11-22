#!/usr/bin/env bash
set -euo pipefail
# verify-milestone.sh M<n>: the full gate for one milestone.
#
# Runs every step even after a failure so one run shows all problems, names each
# failing command, prints test counts, and exits 1 if anything failed.
# It also fails when zero tests ran or any test was ignored: a gate that passes
# with nothing tested hides exactly the problem it exists to catch.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

die() { echo "verify-milestone: FAILED: $*" >&2; exit 1; }

[[ $# -eq 1 ]] || die "usage: scripts/verify-milestone.sh M<n>   (M0..M4 or boot)"
MILESTONE="$1"
[[ "$MILESTONE" =~ ^(M[0-4]|boot)$ ]] || die "unknown milestone '$MILESTONE' (expected M0..M4 or boot)"
for tool in cargo jq sha256sum; do
    command -v "$tool" >/dev/null || die "required tool '$tool' is not on PATH"
done

LOGDIR="$(mktemp -d)"
trap 'rm -rf "$LOGDIR"' EXIT
failed_steps=()

step() {  # step <label> <command...>
    local label="$1"; shift
    echo "==> $label: $*"
    if "$@"; then
        echo "    ok: $label"
    else
        echo "    FAILED: $label (command: $*)" >&2
        failed_steps+=("$label: $*")
    fi
}

check_features() {  # the feature file exists, is a non-empty array, and is well formed
    local f="features/$MILESTONE.json"
    [[ -f "$f" ]] || { echo "missing $f" >&2; return 1; }
    jq -e --arg m "$MILESTONE" '
        type == "array" and length > 0 and all(.[];
            type == "object"
            and (keys == ["clause_ids","description","id","milestone","passes"])
            and (.id | type == "string" and length > 0)
            and .milestone == $m
            and (.clause_ids | type == "array")
            and (.description | type == "string" and length > 0)
            and (.passes | type == "boolean"))
    ' "$f" >/dev/null || { echo "$f is empty or malformed (needs >= 1 feature: id, milestone=$MILESTONE, clause_ids, description, passes)" >&2; return 1; }
}

milestone_frozen() {  # the tests for this milestone have been frozen
    [[ -f FROZEN ]] && grep -qx "$MILESTONE" FROZEN && return 0
    echo "$MILESTONE is not listed in FROZEN; run scripts/freeze-tests.sh $MILESTONE after the tests are written" >&2
    return 1
}

total_passed=0 total_failed=0 total_ignored=0
count_results() {  # count_results <log>: sum cargo's "test result:" lines
    local p f i
    read -r p f i < <(awk '/^test result:/ {
            for (n = 1; n <= NF; n++) {
                if ($(n+1) ~ /^passed/)  p += $n
                if ($(n+1) ~ /^failed/)  f += $n
                if ($(n+1) ~ /^ignored/) i += $n
            } }
        END { print p + 0, f + 0, i + 0 }' "$1")
    total_passed=$((total_passed + p)); total_failed=$((total_failed + f)); total_ignored=$((total_ignored + i))
    echo "    counts: passed=$p failed=$f ignored=$i"
}

run_tests() {  # run_tests <log> <cargo test args...>
    local log="$1"; shift
    local rc=0
    cargo test "$@" 2>&1 | tee "$log" || rc=$?
    count_results "$log"
    return "$rc"
}

run_holdout() {  # copy the holdout package next to the workspace and test it
    [[ -f "$XINIT_HOLDOUT_DIR/Cargo.toml" ]] || { echo "$XINIT_HOLDOUT_DIR has no Cargo.toml (see specs/ARCHITECTURE.md, holdout contract)" >&2; return 1; }
    [[ -f "$XINIT_HOLDOUT_DIR/Cargo.lock" ]] || { echo "$XINIT_HOLDOUT_DIR has no Cargo.lock; --locked needs one" >&2; return 1; }
    local dest="$ROOT/target/xinit-holdout"
    rm -rf "$dest" && mkdir -p "$dest" && cp -a "$XINIT_HOLDOUT_DIR/." "$dest/"
    run_tests "$LOGDIR/holdout.log" --locked --manifest-path "$dest/Cargo.toml"
}

step "features/$MILESTONE.json" check_features
step "frozen $MILESTONE" milestone_frozen
step "fmt" cargo fmt --all --check
step "clippy" cargo clippy --locked --workspace --all-targets -- -D warnings
step "hygiene" scripts/check-hygiene.sh
step "frozen tests" scripts/check-frozen.sh
step "cargo test" run_tests "$LOGDIR/test.log" --locked --workspace
if [[ -n "${XINIT_HOLDOUT_DIR:-}" ]]; then
    step "holdout" run_holdout
else
    echo "==> holdout: SKIPPED (XINIT_HOLDOUT_DIR is not set)"
fi

echo
echo "==== verify-milestone $MILESTONE summary ===="
echo "tests: passed=$total_passed failed=$total_failed ignored=$total_ignored"
if [[ $total_passed -eq 0 && $total_failed -eq 0 ]]; then
    failed_steps+=("zero tests ran")
fi
if [[ $total_ignored -gt 0 ]]; then
    failed_steps+=("$total_ignored ignored test(s); ignoring a test is forbidden")
fi
if [[ ${#failed_steps[@]} -gt 0 ]]; then
    echo "RESULT: FAILED (${#failed_steps[@]})"
    printf '  - %s\n' "${failed_steps[@]}"
    exit 1
fi
echo "RESULT: PASSED"
