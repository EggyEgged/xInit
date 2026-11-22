# xInit progress

## Milestone table
| Milestone | Spec | Tests frozen | Implemented | verify-milestone | Notes |
|---|---|---|---|---|---|
| M0 | spec final, 32 clauses, 0 OPEN | yes (111 tests, e6bc4df) | yes (m0/impl) | FAILED (2): fmt, clippy on frozen tests only; 114/114 tests pass | specs/M0.md; see Spec disputes D-1, D-2 |
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
- 2026-11-22 (M0 impl): Dependencies pinned after `cargo info`: libc 0.2.191 (1.0 is alpha only), toml 1.2.2 with features std, parse, serde. `toml::Table`/`Value` are gated behind toml's `serde` feature, which pulls in serde_core (brief allows serde).
- 2026-11-22 (M0 impl): `Service` lives in xinit-api (the tests use it in-process); the `run` CLI and log format live in xinit-manager (`cli`, `log`); Core's main only forwards argv and the exit code.
- 2026-11-22 (M0 impl): The child is created with raw clone3(CLONE_PIDFD), so pid and pidfd come from one syscall and no pid is ever signalled. Between clone3 and exec the child uses only raw syscalls (rt_sigaction, rt_sigprocmask, setsid, setgroups, setgid, setuid, close_range(CLOEXEC), execve), never glibc wrappers that consult process-wide state. A close-on-exec pipe reports (stage, errno); EOF means exec succeeded, so `running` is logged only after exec (B-M0-013).
- 2026-11-22 (M0 impl): Child setup failures other than credentials and exec (signal reset, setsid, close_range), and an argv element with an interior NUL / empty argv given directly to the API, map to: NUL or empty command -> `Exec{EINVAL}`/`Exec{ENOENT}`; setup syscalls -> `Spawn{errno}`. /etc/passwd missing -> `UserUnknown`; unreadable for another reason -> `Credentials{errno}`.
- 2026-11-22 (M0 impl): Supplementary groups for a credential change are exactly the /etc/group entries that list the user as a member (spec wording "groups from /etc/group"); the primary gid is not added. Malformed /etc/group lines are skipped. Untested unprivileged (B-M0-026 root path is Untested).
- 2026-11-22 (M0 impl): B-M0-027 note: xrginit resets the Rust runtime's SIGSEGV and SIGBUS handlers to SIG_DFL at start of `run`, and keeps the runtime's ignored SIGPIPE (allowed). SIGINT/SIGTERM are blocked and read from a signalfd; SIGCHLD is untouched (the pidfd is polled).
- 2026-11-22 (M0 impl): `unit-invalid` carries `key=` only for the variants with a key field (unknown-key, missing-key, invalid-type, invalid-value); schema-version and unsupported-service-type have none. Non-UTF-8 unit file content is `Syntax`.
- 2026-11-22 (M0 impl): `Service` implements `AsFd` (its pidfd) so the CLI can poll it with the signalfd; the pidfd stays open until drop, but `signal` after the reap returns `Exited` without touching it. This is extra public surface not listed in the spec.
- 2026-11-22 (M0 impl): A failed write to stderr is ignored: logging is xrginit's only channel, so there is nowhere to report it, and supervision continues.
- 2026-11-22: The no-panic/no-exit rules moved from grep to clippy. The Core and Manager crate roots carry `#![cfg_attr(not(test), forbid(clippy::unwrap_used, expect_used, panic, exit, todo, unimplemented))]`. It is forbid, not deny, because under deny a local `#[allow(..., reason)]` turns the rule off (tested). The workspace denies `clippy::allow_attributes_without_reason`, which replaces the `// JUSTIFIED:` grep. check-hygiene.sh now checks that this enforcement is still present, and keeps only its text rules for test code.

## Autopilot
Driver: docs/build-prompts.md section 4. Roles launch only through `scripts/run-role.sh <dir> <prompt-file> <output-file> <allowed-tools> [holdout-dir]`.

Flag spellings checked with `claude --help` (Claude Code 2.1.330), 2026-11-22: `-p` (`--print`), `--model`, `--permission-mode`, `--allowedTools` (alias `--allowed-tools`), `--disallowedTools` (alias `--disallowed-tools`), `--add-dir`, `--output-format`. run-role.sh uses `-p`, `--model claude-opus-5-5`, `--permission-mode acceptEdits`, `--allowedTools`, `--disallowedTools`, `--add-dir`, `--output-format json`.

Setup checks, 2026-11-22: main clean and equal to origin/main (77b8833); gh auth OK; cargo 1.96.0 from ~/.cargo/bin matches rust-toolchain.toml.

### Session log
| Date | Milestone | Role | Exit | Outcome |
|---|---|---|---|---|
| 2026-11-22 | M0 | test author (run 1) | 1 | Usage limit hit after 65 turns; 12 Bash calls refused by the tool limits. Workspace discarded; partial holdout kept for run 2 to review. |
| 2026-11-22 | M0 | test author (run 2) | 0 | 111 tests, 0 passed / 111 failed on stub; holdout 19 tests; 3 wrong stop impls caught. Prompt had an added harness note (one cargo command per call). |
| 2026-11-22 | M0 | orchestrator Phase 2 | 0 | Only tests/, specs/red, specs/trace changed; 111/111 runtime failures, 0 compile errors; 111 tests cite a clause; hygiene OK; 32/32 clauses traced; frozen (9 files). |

### Spec notes from the M0 test author (not blocking, not OPEN)
1. B-M0-026: a root-only success test and the unprivileged EPERM test cannot both pass in one environment. The root test is omitted and listed Untested.
2. B-M0-027: Rust's runtime installs SIGSEGV and SIGBUS handlers by default, which the clause forbids. The implementation must remove them, or the spec must allow them.
3. B-M0-018: the spec's example command races (xrginit may log `running` before the shell sets its trap). The tests use a FIFO readiness signal; the spec example should too.

## Spec disputes
These are gate disputes, not behaviour disputes: every M0 test passes. They block verify-milestone M0 from exiting 0, so no passes flag in features/M0.json was flipped.
- D-1 (all M0 clauses; gate step `fmt`): the frozen files under tests/ are not rustfmt-clean, so `cargo fmt --all --check` fails on tests/common/mod.rs and every tests/m0/*.rs. No rustfmt.toml fixes it (tried max_width = 120 with use_small_heuristics = "Max": still diffs in both directions, so the files follow no single width). The implementation crates are fmt-clean. Fix needs a human: re-freeze the tests after `cargo fmt`, or scope the gate's fmt step to the crates.
- D-2 (all M0 clauses; gate step `clippy`): `cargo clippy --workspace --all-targets -D warnings` fails with 8 lints in frozen test code: sliced_string_as_bytes (tests/common/mod.rs:385), collapsible_if (tests/common/mod.rs:613, :685; tests/m0/cli_lifecycle.rs:454), doc_lazy_continuation (tests/m0/api_model.rs:13-16). `cargo clippy -p xinit-api -p xinit-manager -p xinit-core --all-targets -- -D warnings` is clean. Fix needs a human: re-freeze with the lints fixed, or allow them for xinit-tests.

## Verification log
- 2026-11-22 M0 baseline (before impl): `scripts/verify-milestone.sh M0` exit 1. fmt failed on frozen tests (D-1); the 111 m0 tests were red on the `unimplemented!()` stub (0 passed).
- 2026-11-22 M0 impl: `scripts/verify-milestone.sh M0` exit 1. Summary:
  ```
  binary: /home/egg/Documents/Projects/xInit/target/debug/xrginit
  tests: passed=114 failed=0 ignored=0
  RESULT: FAILED (2)
    - fmt: cargo fmt --all --check
    - clippy: cargo clippy --locked --workspace --all-targets -- -D warnings
  ```
  Both failures are in frozen test files only (D-1, D-2). hygiene, frozen tests, build, suite listing (111 tests) and orphan check passed. Holdout SKIPPED (XINIT_HOLDOUT_DIR not set). `cargo build --locked --target x86_64-unknown-linux-musl` exit 0. Manual run of `xrginit run` on a sleep-1 unit printed starting, running pid=…, exited code=0.
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
