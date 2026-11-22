# xInit progress

## Milestone table
| Milestone | Spec | Tests frozen | Implemented | verify-milestone | Notes |
|---|---|---|---|---|---|
| M0 | spec final, 32 clauses, 0 OPEN | no | no | not run | specs/M0.md; xinit-api stub matches final spec 3ca7927 |
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
- 2026-11-22: The workspace root is also the package `xinit-tests`, which owns tests/. It has one entry file per milestone (tests/m<n>.rs), inherits workspace lints but not the no-panic forbid, and depends only on xinit-api. verify-milestone lists the milestone's suite and checks for orphan files. See specs/ARCHITECTURE.md, Test suite layout.
- 2026-11-22: The blind test workspace is built by scripts/make-blind-workspace.sh rather than ad hoc sed commands, and fails if the Cargo.toml shape changes.
- 2026-11-22: Tests find the binary only through XRGINIT_BIN (absolute path), read by the helper `xrginit_bin()` in tests/common/mod.rs, which panics loudly if it is unusable. There is no fallback path. See specs/ARCHITECTURE.md.
- 2026-11-22: verify-milestone always builds xrginit (--locked, dev profile) and overwrites XRGINIT_BIN with its absolute path. It prints the path and sha256. A failed build stops the gate before any test runs. The holdout suite gets the same XRGINIT_BIN.
- 2026-11-22: check-hygiene.sh also rejects `should_panic` without `expected` (from the Forbidden list in CLAUDE.md).
- 2026-11-22: The no-panic/no-exit rules moved from grep to clippy. The Core and Manager crate roots carry `#![cfg_attr(not(test), forbid(clippy::unwrap_used, expect_used, panic, exit, todo, unimplemented))]`. It is forbid, not deny, because under deny a local `#[allow(..., reason)]` turns the rule off (tested). The workspace denies `clippy::allow_attributes_without_reason`, which replaces the `// JUSTIFIED:` grep. check-hygiene.sh now checks that this enforcement is still present, and keeps only its text rules for test code.

## Autopilot
Driver: docs/build-prompts.md section 4. Roles launch only through `scripts/run-role.sh <dir> <prompt-file> <output-file> <allowed-tools> [holdout-dir]`.

Flag spellings checked with `claude --help` (Claude Code 2.1.330), 2026-11-22: `-p` (`--print`), `--model`, `--permission-mode`, `--allowedTools` (alias `--allowed-tools`), `--disallowedTools` (alias `--disallowed-tools`), `--add-dir`, `--output-format`. run-role.sh uses `-p`, `--model claude-opus-5-5`, `--permission-mode acceptEdits`, `--allowedTools`, `--disallowedTools`, `--add-dir`, `--output-format json`.

Setup checks, 2026-11-22: main clean and equal to origin/main (77b8833); gh auth OK; cargo 1.96.0 from ~/.cargo/bin matches rust-toolchain.toml.

### Session log
| Date | Milestone | Role | Exit | Outcome |
|---|---|---|---|---|

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
  - FIXED in the next entry. Was: files in the root `tests/` are never compiled. The workspace root is virtual, so no package owns them. A test there did not run; cargo test reported 0 passed. Must be fixed before M0 tests are written.
- 2026-11-22 root test package: tested on a scratch copy with a planted feature, an API stub and tests/m0.rs + tests/m0/{mod,ping}.rs + tests/common/mod.rs.
  - Blind workspace: `cargo test --offline --test m0` built and failed red ("not implemented", exit 101).
  - After implementing the stub, verify-milestone M0 printed RESULT: PASSED (m0 listed 1 test; 4 files under tests/, all compiled).
  - An unreached tests/m0/forgotten.rs failed the gate as an orphan.
  - Removing tests/m0.rs while a crate unit test still passed failed the gate at `suite tests/m0.rs`.
  - Real repository, verify-milestone M0: exit 1, RESULT: FAILED (4): features/M0.json, frozen M0, suite tests/m0.rs, zero tests ran.
- 2026-11-22 XRGINIT_BIN in the gate: tested on a scratch copy with a test that runs xrginit_bin() and checks the stub's exact stderr.
  - Clean run: RESULT: PASSED, XRGINIT_BIN=<copy>/target/debug/xrginit.
  - With a stale XRGINIT_BIN exported (a script printing "STALE BINARY"), the gate tested the fresh build: PASSED, same path. Control: the same test run directly against the stale binary FAILED (left "STALE BINARY").
  - Broken build (unclosed delimiter in main.rs, stale export still set): exit 1. The log shows "tests: NOT RUN" and "binary: none (build failed)", with the compiler error. No tests ran.
