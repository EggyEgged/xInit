#!/usr/bin/env bash
set -euo pipefail
# check-hygiene.sh: grep-based lint for rules that rustc and clippy do not enforce.
# Reports every violation as path:line: text, then exits 1 if there were any.
#
# Scope:
#   tests/, holdout/ and $XINIT_HOLDOUT_DIR (if set) : test-honesty rules
#   crates/xinit-core/src                            : PID 1 never-exit / never-panic rules
#   every *.rs file in all of the above and crates/  : #[allow( justification rule
#
# Known limits of a grep-based lint, stated so nobody over-trusts it:
#   - Lines that are only a // comment are skipped in the Core scan, so docs may
#     name a forbidden macro. Trailing comments on code lines are still scanned.
#   - #[cfg(test)] items are skipped by counting braces after the attribute.
#     Braces inside string or char literals can confuse the count.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE_SRC="$ROOT/crates/xinit-core/src"

violations=0
report() {  # report <rule> <lines...>
    local rule="$1"; shift
    [[ -n "$1" ]] || return 0
    echo "hygiene: $rule" >&2
    printf '%s\n' "$1" | sed "s|^$ROOT/||; s/^/    /" >&2
    violations=$((violations + $(printf '%s\n' "$1" | grep -c .)))
}

rs_files() {  # rs_files <dir>...: NUL-free list of *.rs files under existing dirs
    local d
    for d in "$@"; do
        [[ -d "$d" ]] && find "$d" -type f -name '*.rs'
    done
    return 0
}

[[ -d "$CORE_SRC" ]] || { echo "check-hygiene: FAILED: $CORE_SRC is missing" >&2; exit 1; }

# ---------------------------------------------------------------- test dirs
test_dirs=("$ROOT/tests" "$ROOT/holdout")
if [[ -n "${XINIT_HOLDOUT_DIR:-}" ]]; then
    [[ -d "$XINIT_HOLDOUT_DIR" ]] || { echo "check-hygiene: FAILED: XINIT_HOLDOUT_DIR=$XINIT_HOLDOUT_DIR is not a directory" >&2; exit 1; }
    test_dirs+=("$XINIT_HOLDOUT_DIR")
fi
mapfile -t test_rs < <(rs_files "${test_dirs[@]}")

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
# should_panic without expected = "..." passes on any panic, including the wrong one.
if [[ ${#test_rs[@]} -gt 0 ]]; then
    report 'should_panic without expected = "...": passes on any panic' \
        "$(grep -nHE 'should_panic' "${test_rs[@]}" | grep -v 'expected' || true)"
fi

# ---------------------------------------------------------------- Core src
# Strip #[cfg(test)] items and pure comment lines, keep path:line: prefixes.
core_code="$(rs_files "$CORE_SRC" | while read -r f; do
    awk -v F="$f" '
        skip == 0 && /^[[:space:]]*#\[cfg\(test\)\]/ { skip = 1; depth = 0; opened = 0; next }
        skip == 1 {
            l = $0
            o = gsub(/\{/, "{", l); c = gsub(/\}/, "}", l)
            depth += o - c
            if (o > 0) opened = 1
            if ((opened && depth <= 0) || (!opened && $0 ~ /;[[:space:]]*$/)) skip = 0
            next
        }
        /^[[:space:]]*\/\// { next }
        { print F ":" FNR ":" $0 }
    ' "$f"
done)"

scan_core() {  # scan_core <ERE> <rule text>
    report "$2" "$(printf '%s\n' "$core_code" | grep -E -- "$1" || true)"
}
# Core is PID 1. If it panics or exits, the kernel panics. Every fallible call
# must be handled with a typed error, never unwrapped.
scan_core '\.unwrap\(\)' 'unwrap() in xinit-core: PID 1 must not panic; handle the error'
scan_core '\.expect\(' 'expect( in xinit-core: PID 1 must not panic; handle the error'
scan_core 'panic!' 'panic! in xinit-core: PID 1 must not panic'
# PID 1 must never exit. Only the shutdown path may end in reboot(2), never exit.
scan_core 'process::exit' 'process::exit in xinit-core: PID 1 must never exit'
# Placeholders panic when reached. Nothing that panics may ship in PID 1.
scan_core 'todo!' 'todo! in xinit-core: placeholder panics at runtime'
scan_core 'unimplemented!' 'unimplemented! in xinit-core: placeholder panics at runtime'

# ---------------------------------------------------------------- #[allow(
# A lint suppression without a written reason hides the warning for good.
mapfile -t all_rs < <(rs_files "$ROOT/crates" "${test_dirs[@]}")
if [[ ${#all_rs[@]} -gt 0 ]]; then
    report '#[allow( without "// JUSTIFIED:" on the previous line' "$(
        for f in "${all_rs[@]}"; do
            awk -v F="$f" '
                /#!?\[allow\(/ && prev !~ /\/\/ JUSTIFIED:/ { print F ":" FNR ":" $0 }
                { prev = $0 }
            ' "$f"
        done
    )"
fi

if [[ $violations -gt 0 ]]; then
    echo "check-hygiene: FAILED: $violations violation(s)" >&2
    exit 1
fi
echo "check-hygiene: OK (${#test_rs[@]} test file(s), $(rs_files "$CORE_SRC" | grep -c .) core file(s), ${#all_rs[@]} file(s) checked for #[allow()"
