# xInit progress

## Milestone table
| Milestone | Spec | Tests frozen | Implemented | verify-milestone | Notes |
|---|---|---|---|---|---|
| M0 | no | no | no | not run | |
| M1 | no | no | no | not run | |
| M2 | no | no | no | not run | |
| M3 | no | no | no | not run | |
| M4 | no | no | no | not run | |
| boot | no | no | no | not run | Debian image; scripts/qemu-boot-test.sh is a stub (exit 2) |

## Decisions
- 2026-11-22: Manager runs as `xrginit manager` (exec of /proc/self/exe); `xinit-manager` is a library crate. See specs/ARCHITECTURE.md.
- 2026-11-22: Toolchain pinned to Rust 1.96.0 in rust-toolchain.toml (rustfmt, clippy, musl target).
- 2026-11-22: Release profile uses `panic = "abort"`; any panic that slips through is immediate and loud.
- 2026-11-22: Feature files start as `[]`. The spec writer adds features; verify-milestone fails on an empty or malformed file.
- 2026-11-22: `FROZEN` (repo root) lists frozen milestones, one per line, written by scripts/freeze-tests.sh. Once non-empty, check-frozen.sh requires tests/MANIFEST.sha256. verify-milestone M<n> fails until M<n> is in FROZEN.
- 2026-11-22: verify-milestone fails if any test is ignored, as well as when zero tests run.
- 2026-11-22: check-hygiene.sh also rejects `should_panic` without `expected` (from the Forbidden list in CLAUDE.md).
- 2026-11-22: The no-panic/no-exit rules moved from grep to clippy. The Core and Manager crate roots carry `#![cfg_attr(not(test), forbid(clippy::unwrap_used, expect_used, panic, exit, todo, unimplemented))]`. It is forbid, not deny, because under deny a local `#[allow(..., reason)]` turns the rule off (tested). The workspace denies `clippy::allow_attributes_without_reason`, which replaces the `// JUSTIFIED:` grep. check-hygiene.sh now checks that this enforcement is still present, and keeps only its text rules for test code.

## Spec disputes
(none)

## Verification log
- 2026-11-22 scaffolding: `scripts/verify-milestone.sh M0` on the empty repository, rustc 1.96.0. Expected failure, which shows the gate does not pass silently:
  ```
  tests: passed=0 failed=0 ignored=0
  RESULT: FAILED (3)
    - features/M0.json: check_features
    - frozen M0: milestone_frozen
    - zero tests ran
  ```
  fmt, clippy -D warnings, check-hygiene and check-frozen passed. cargo build --locked passed for both the gnu and musl targets.
- 2026-11-22 hygiene→clippy: tested on scratch copies.
  - clippy rejects: unwrap after a `#[cfg(test)]` + `"{"` decoy (which the old grep missed), a local `#[allow(clippy::unwrap_used, reason)]` (E0453), `#[allow]` without a reason, and `panic!` in Manager.
  - clippy accepts: unwrap inside a `#[cfg(test)]` module.
  - check-hygiene rejects: the crate-root attribute removed, and a crate missing `[lints] workspace = true`.
  - OPEN DEFECT: files in the root `tests/` are never compiled. The workspace root is virtual, so no package owns them. A test there did not run; cargo test reported 0 passed. Must be fixed before M0 tests are written.
