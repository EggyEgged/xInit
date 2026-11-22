#!/usr/bin/env bash
set -euo pipefail
# check-hygiene.sh: lint for rules that rustc and clippy do not enforce.
# Reports every violation as path:line: text, then exits 1 if there were any.
#
# Division of labour:
#   - The no-panic / no-exit rules for Core and Manager are enforced by clippy,
#     which understands the code (strings, comments, #[cfg(test)]). The crate
#     roots carry #![cfg_attr(not(test), forbid(clippy::unwrap_used, ...))].
#     forbid, not deny: under deny a local #[allow(..., reason)] would switch it off.
#     The workspace denies clippy::allow_attributes_without_reason.
#     This script only checks that that enforcement is still in place, because
#     deleting one attribute would silently disable it.
#   - Test code (tests/, holdout/, $XINIT_HOLDOUT_DIR) is checked here by text
#     matching. The patterns are distinctive enough that a grep is reliable.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

violations=0
report() {  # report <rule> <lines>
    [[ -n "$2" ]] || return 0
    echo "hygiene: $1" >&2
    printf '%s\n' "$2" | sed "s|^$ROOT/||; s/^/    /" >&2
    violations=$((violations + $(printf '%s\n' "$2" | grep -c .)))
}

# ------------------------------------------------- clippy enforcement present
FORBIDDEN_LINTS=(unwrap_used expect_used panic exit todo unimplemented)
CRATE_ROOTS=("$ROOT/crates/xinit-core/src/main.rs" "$ROOT/crates/xinit-manager/src/lib.rs")
for f in "${CRATE_ROOTS[@]}"; do
    [[ -f "$f" ]] || { report "crate root missing (no-panic lints cannot apply)" "$f"; continue; }
    # Join the file so a multi-line attribute can be matched as one string.
    flat="$(tr -d ' \n\t' < "$f")"
    if [[ "$flat" != *'#![cfg_attr(not(test),forbid('* ]]; then
        report 'crate root lacks #![cfg_attr(not(test), forbid(...))]: PID 1 / Manager no-panic rules are off' "$f"
        continue
    fi
    for lint in "${FORBIDDEN_LINTS[@]}"; do
        grep -qE "clippy::$lint\b" "$f" || report "crate root no longer forbids clippy::$lint" "$f"
    done
done
grep -qE '^allow_attributes_without_reason *= *"(deny|forbid)"' "$ROOT/Cargo.toml" \
    || report 'workspace no longer denies clippy::allow_attributes_without_reason' "$ROOT/Cargo.toml"
for toml in "$ROOT"/crates/*/Cargo.toml; do
    tr -d ' \n\t' < "$toml" | grep -q '\[lints\]workspace=true' \
        || report 'crate does not inherit workspace lints ([lints] workspace = true)' "$toml"
done

# ------------------------------------------------------------- test code
test_dirs=("$ROOT/tests" "$ROOT/holdout")
if [[ -n "${XINIT_HOLDOUT_DIR:-}" ]]; then
    [[ -d "$XINIT_HOLDOUT_DIR" ]] || { echo "check-hygiene: FAILED: XINIT_HOLDOUT_DIR=$XINIT_HOLDOUT_DIR is not a directory" >&2; exit 1; }
    test_dirs+=("$XINIT_HOLDOUT_DIR")
fi
test_rs=()
for d in "${test_dirs[@]}"; do
    [[ -d "$d" ]] && mapfile -t -O "${#test_rs[@]}" test_rs < <(find "$d" -type f -name '*.rs')
done

scan_tests() {  # scan_tests <ERE> <rule text>
    [[ ${#test_rs[@]} -gt 0 ]] || return 0
    report "$2" "$(grep -nHE -- "$1" "${test_rs[@]}" || true)"
}
# An ignored test is a skipped test, and skipping silently is forbidden.
scan_tests '#\[ignore' '#[ignore] in tests: a test that cannot run must fail and say why'
# assert!(true) asserts nothing and makes an empty test look like a passing one.
scan_tests 'assert!\(\s*true' 'assert!(true) in tests: asserts nothing'
# Sleeping to wait for a condition is flaky: too short fails on slow CI, too long hides races.
scan_tests 'thread::sleep' 'thread::sleep in tests: wait on the condition (pidfd, poll, channel), not on time'
scan_tests 'tokio::time::sleep' 'tokio::time::sleep in tests: wait on the condition, not on time'
if [[ ${#test_rs[@]} -gt 0 ]]; then
    # should_panic without expected = "..." passes on any panic, including the wrong one.
    report 'should_panic without expected = "...": passes on any panic' \
        "$(grep -nHE 'should_panic' "${test_rs[@]}" | grep -v 'expected' || true)"
    # Workspace lints do not reach a separate holdout package, so check the reason
    # by text here. A suppression without a written reason hides the warning for good.
    report '#[allow( without reason = "..." in test code' \
        "$(grep -nHE '#!?\[allow\(' "${test_rs[@]}" | grep -v 'reason *=' || true)"
fi

if [[ $violations -gt 0 ]]; then
    echo "check-hygiene: FAILED: $violations violation(s)" >&2
    exit 1
fi
echo "check-hygiene: OK (clippy enforcement present in ${#CRATE_ROOTS[@]} crate roots; ${#test_rs[@]} test file(s) scanned)"
