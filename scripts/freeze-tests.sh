#!/usr/bin/env bash
set -euo pipefail
# freeze-tests.sh M<n>: write tests/MANIFEST.sha256 covering every file under
# tests/ except the manifest itself, sorted by path (LC_ALL=C), in plain
# sha256sum text format so a human can check it with `sha256sum -c`.
# Also records the milestone in FROZEN so check-frozen.sh knows a manifest
# must exist from now on. Commit the result as its own test-freeze commit.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TESTS="$ROOT/tests"
MANIFEST="$TESTS/MANIFEST.sha256"
FROZEN="$ROOT/FROZEN"

fail() { echo "freeze-tests: FAILED: $*" >&2; exit 1; }

[[ $# -eq 1 ]] || fail "usage: scripts/freeze-tests.sh M<n>   (M0..M4 or boot)"
MILESTONE="$1"
[[ "$MILESTONE" =~ ^(M[0-4]|boot)$ ]] || fail "unknown milestone '$MILESTONE' (expected M0..M4 or boot)"
[[ -d "$TESTS" ]] || fail "tests/ does not exist; there is nothing to freeze"

mapfile -d '' files < <(find "$TESTS" -type f ! -path "$MANIFEST" -printf '%P\0' | LC_ALL=C sort -z)
[[ ${#files[@]} -gt 0 ]] || fail "tests/ holds no files; freezing an empty suite would hide missing tests"

for f in "${files[@]}"; do
    # sha256sum escapes these characters, which would break the plain format.
    [[ "$f" != *$'\n'* && "$f" != *\\* ]] || fail "file name contains a newline or backslash: $f"
done

tmp="$(mktemp "$TESTS/.MANIFEST.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
if ! (cd "$TESTS" && sha256sum -- "${files[@]}") > "$tmp"; then
    fail "command 'sha256sum' failed while hashing tests/"
fi
mv "$tmp" "$MANIFEST"
trap - EXIT

touch "$FROZEN"
grep -qx "$MILESTONE" "$FROZEN" || echo "$MILESTONE" >> "$FROZEN"

echo "freeze-tests: wrote tests/MANIFEST.sha256 (${#files[@]} files) and recorded $MILESTONE in FROZEN"
