#!/usr/bin/env bash
set -euo pipefail
# check-frozen.sh: verify that tests/ matches tests/MANIFEST.sha256 exactly.
#
# Fails when:
#   - any file listed in the manifest is missing or has a different hash
#     (sha256sum -c --strict);
#   - any file under tests/ is missing from the manifest (a new, unfrozen test);
#   - the manifest is missing once any milestone has been frozen (FROZEN not empty);
#   - tests/ holds files but nothing has been frozen yet.
# Passes, and says so, only when nothing is frozen and tests/ is empty or absent.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TESTS="$ROOT/tests"
MANIFEST="$TESTS/MANIFEST.sha256"
FROZEN="$ROOT/FROZEN"

fail() { echo "check-frozen: FAILED: $*" >&2; exit 1; }

frozen_list=""
if [[ -f "$FROZEN" ]]; then
    frozen_list="$(grep -v '^[[:space:]]*$' "$FROZEN" || true)"
fi

list_test_files() {
    [[ -d "$TESTS" ]] || return 0
    find "$TESTS" -type f ! -path "$MANIFEST" -printf '%P\n' | LC_ALL=C sort
}

if [[ ! -f "$MANIFEST" ]]; then
    if [[ -n "$frozen_list" ]]; then
        fail "tests/MANIFEST.sha256 is missing but FROZEN lists: $(echo "$frozen_list" | tr '\n' ' ')"
    fi
    count="$(list_test_files | grep -c . || true)"
    if [[ "$count" -gt 0 ]]; then
        fail "tests/ holds $count file(s) but no milestone is frozen; run scripts/freeze-tests.sh M<n>"
    fi
    echo "check-frozen: no milestone frozen and tests/ is empty; nothing to check"
    exit 0
fi

# 1. Every listed file exists and matches its hash.
if ! (cd "$TESTS" && sha256sum --quiet --strict -c MANIFEST.sha256); then
    fail "command 'sha256sum --strict -c MANIFEST.sha256' (in tests/) reported mismatched or missing files"
fi

# 2. Every file under tests/ is listed in the manifest.
listed="$(sed -E 's/^[0-9a-f]{64}  //' "$MANIFEST" | LC_ALL=C sort)"
unlisted="$(LC_ALL=C comm -23 <(list_test_files) <(printf '%s\n' "$listed"))"
if [[ -n "$unlisted" ]]; then
    echo "$unlisted" | sed 's/^/  unlisted: tests\//' >&2
    fail "files under tests/ are missing from MANIFEST.sha256"
fi

echo "check-frozen: OK ($(grep -c . "$MANIFEST") files match; frozen: $(echo "$frozen_list" | tr '\n' ' '))"
