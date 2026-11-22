#!/usr/bin/env bash
set -euo pipefail
# make-blind-workspace.sh <dest>: build the workspace the blind test author works in.
#
# It holds the specs, the xinit-api stubs and the root test package (tests/), but
# no Core or Manager source, so tests are written against the spec and not the
# implementation. The test author's red check runs there:
#     cd <dest> && cargo test --offline --test m<n>
# That must build and then FAIL, because the API bodies are unimplemented!().
#
# The blind copy runs without --locked: dropping two members prunes Cargo.lock,
# and cargo keeps the locked versions of everything that remains.
# Copy the finished tests back with: rsync -a --delete <dest>/tests/ tests/

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail() { echo "make-blind-workspace: FAILED: $*" >&2; exit 1; }

[[ $# -eq 1 ]] || fail "usage: scripts/make-blind-workspace.sh <dest>"
DEST="$1"
[[ ! -e "$DEST" ]] || fail "$DEST already exists; refusing to overwrite it"
case "$(realpath -m "$DEST")/" in "$ROOT"/*) fail "$DEST is inside the repository; the copy must live outside it" ;; esac

mkdir -p "$DEST/crates"
cp -a "$ROOT/Cargo.lock" "$ROOT/rust-toolchain.toml" "$ROOT/CLAUDE.md" "$DEST/"
cp -a "$ROOT/src" "$ROOT/specs" "$ROOT/features" "$DEST/"
cp -a "$ROOT/crates/xinit-api" "$DEST/crates/"
if [[ -d "$ROOT/tests" ]]; then cp -a "$ROOT/tests" "$DEST/"; else mkdir "$DEST/tests"; fi

# Cut the workspace down to the test package and xinit-api. Each pattern must
# match exactly once; if Cargo.toml changed shape, stop instead of producing a
# workspace that silently still points at Core or Manager.
blind='[".", "crates/xinit-api"]'
expect_once() {  # expect_once <ERE> : the root Cargo.toml has exactly one matching line
    local n; n="$(grep -cE "$1" "$ROOT/Cargo.toml" || true)"
    [[ "$n" -eq 1 ]] || fail "expected exactly one line matching '$1' in Cargo.toml, found $n"
}
expect_once '^members = \['
expect_once '^default-members = \['
expect_once '# blind-drop$'
sed -E \
    -e "s|^members = \[.*\]$|members = $blind|" \
    -e "s|^default-members = \[.*\]$|default-members = $blind|" \
    -e '/# blind-drop$/d' \
    "$ROOT/Cargo.toml" > "$DEST/Cargo.toml"

if grep -qE 'xinit-(core|manager)' "$DEST/Cargo.toml" "$DEST/crates/xinit-api/Cargo.toml"; then
    fail "blind Cargo.toml still references xinit-core or xinit-manager"
fi
(cd "$DEST" && cargo metadata --offline --format-version 1 >/dev/null) \
    || fail "command 'cargo metadata --offline' failed in $DEST; the blind workspace does not resolve"

echo "make-blind-workspace: created $DEST (packages: xinit-tests, xinit-api)"
echo "red check: cd $DEST && cargo test --offline --test m<n>   # must build, then fail"
