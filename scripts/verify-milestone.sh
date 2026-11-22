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

SUITE="$(echo "$MILESTONE" | tr '[:upper:]' '[:lower:]')"   # M0 -> m0, boot -> boot

# Tests find the binary only through XRGINIT_BIN (see specs/ARCHITECTURE.md).
# Never trust a value from the caller's shell: a leftover export points at a
# stale binary and gives a false green. The gate always builds and sets it.
unset XRGINIT_BIN

build_xrginit() {  # build xrginit (dev profile, --locked) and export its absolute path
    local json exe
    # json-render-diagnostics: artifacts as JSON on stdout, compiler errors readable on stderr.
    if ! json="$(cargo build --locked -p xinit-core --bin xrginit --message-format=json-render-diagnostics)"; then
        echo "command 'cargo build --locked -p xinit-core --bin xrginit' failed" >&2
        return 1
    fi
    # Take the path from cargo's own artifact record, not from a guessed target/ layout.
    exe="$(printf '%s\n' "$json" | jq -r 'select(.reason == "compiler-artifact" and .target.name == "xrginit" and (.target.kind | index("bin"))) | .executable // empty' | tail -n 1)"
    if [[ -z "$exe" || "$exe" != /* || ! -x "$exe" ]]; then
        echo "cargo reported no usable xrginit executable (got '${exe:-nothing}')" >&2
        return 1
    fi
    export XRGINIT_BIN="$exe"
    echo "    XRGINIT_BIN=$XRGINIT_BIN"
    echo "    sha256=$(sha256sum "$XRGINIT_BIN" | cut -d' ' -f1)"
}

suite_listed() {  # the frozen suite for this milestone is a real target with >= 1 test
    # "Some tests passed" is not enough: unit tests in the crates could mask a
    # suite that is missing or not compiled. Ask the suite itself.
    local log="$LOGDIR/list.log" rc=0 n
    cargo test --locked --test "$SUITE" -- --list > "$log" 2>"$LOGDIR/list.err" || rc=$?
    if [[ $rc -ne 0 ]]; then
        cat "$LOGDIR/list.err" >&2
        echo "test target '$SUITE' failed to build or list (exit $rc); expected entry file tests/$SUITE.rs" >&2
        return 1
    fi
    n="$(grep -c ': test$' "$log" || true)"
    echo "    tests/$SUITE.rs lists $n test(s)"
    [[ "$n" -gt 0 ]] || { echo "test target '$SUITE' lists zero tests" >&2; return 1; }
}

tests_all_compiled() {  # every .rs file under tests/ is compiled into some test target
    # A file under tests/<suite>/ that no mod declaration reaches is never built,
    # so its tests vanish without an error. rustc's dep-info names every source
    # file it read; any tests/*.rs missing from it is an orphan.
    [[ -d tests ]] || { echo "    tests/ absent; nothing to check"; return 0; }
    local exes used orphans
    exes="$(cargo test --locked --no-run --message-format=json 2>/dev/null \
        | jq -r 'select(.reason == "compiler-artifact" and (.package_id | test("xinit-tests")) and .target.kind == ["test"]) | .executable')" \
        || { echo "cargo test --no-run failed" >&2; return 1; }
    used="$(for exe in $exes; do
                [[ -f "$exe.d" ]] || { echo "missing dep-info $exe.d" >&2; continue; }
                head -1 "$exe.d" | cut -d: -f2- | tr ' ' '\n'
            done | grep '\.rs$' \
                 | xargs -r -d '\n' realpath -m --relative-to="$ROOT" \
                 | LC_ALL=C sort -u)"   # normalises #[path = "../x"] entries such as tests/m0/../common/mod.rs
    orphans="$(LC_ALL=C comm -23 <(find tests -type f -name '*.rs' | LC_ALL=C sort) <(printf '%s\n' "$used"))"
    if [[ -n "$orphans" ]]; then
        printf '    orphan (never compiled): %s\n' $orphans >&2
        echo "declare each file as a module reachable from tests/<suite>.rs (see CLAUDE.md, Test layout)" >&2
        return 1
    fi
    echo "    $(find tests -type f -name '*.rs' | grep -c . || true) file(s) under tests/, all compiled"
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
tests_ran=0
step "build xrginit" build_xrginit
if [[ -z "${XRGINIT_BIN:-}" ]]; then
    # Without a fresh binary every binary test would fail at xrginit_bin() and
    # bury the real cause, so stop here and report the build failure alone.
    echo "==> tests: NOT RUN (xrginit did not build; see 'build xrginit' above)" >&2
else
    tests_ran=1
    step "cargo test" run_tests "$LOGDIR/test.log" --locked --workspace
    step "suite tests/$SUITE.rs" suite_listed
    step "no orphan test files" tests_all_compiled
    if [[ -n "${XINIT_HOLDOUT_DIR:-}" ]]; then
        echo "    holdout uses the same XRGINIT_BIN=$XRGINIT_BIN"
        step "holdout" run_holdout
    else
        echo "==> holdout: SKIPPED (XINIT_HOLDOUT_DIR is not set)"
    fi
fi

echo
echo "==== verify-milestone $MILESTONE summary ===="
echo "binary: ${XRGINIT_BIN:-none (build failed)}"
if [[ $tests_ran -eq 0 ]]; then
    echo "tests: NOT RUN"
else
    echo "tests: passed=$total_passed failed=$total_failed ignored=$total_ignored"
    if [[ $total_passed -eq 0 && $total_failed -eq 0 ]]; then
        failed_steps+=("zero tests ran")
    fi
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
