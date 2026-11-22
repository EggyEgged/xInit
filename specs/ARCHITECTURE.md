# xInit architecture decisions

Normative behaviour belongs in per-milestone specs. This file records structural choices only.

## Crates
| Crate | Kind | Role |
|---|---|---|
| `xinit-api` | lib | Public types and function signatures shared by Core, Manager and tests. The spec writer adds signatures with `unimplemented!()` bodies. |
| `xinit-core` | bin `xrginit` | Core: PID 1 path, signal handling, child reaping, Manager supervision, degraded-mode shutdown. |
| `xinit-tests` (repo root) | lib placeholder + tests/ | Owns the frozen suite. Depends on `xinit-api` only. Workspace lints apply, the no-panic forbid does not. |
| `xinit-manager` | lib | Manager: unit loading, service supervision, re-adoption from `/sys/fs/cgroup/xinit/<unit>` and `/run/xinit/state`. |

## Decision: Manager is a subcommand of the same binary
Core starts the Manager by fork + exec of `/proc/self/exe manager`. `xinit-manager` is a library linked into `xrginit`.

Why:
- One binary to install at `/sbin/xrginit`. Core and Manager can never be different versions.
- Exec gives the Manager its own address space. A Manager panic or crash cannot corrupt Core, and Core restarts it with bounded retries.
- Keeping the Manager in its own crate keeps the Core source small. `check-hygiene.sh` applies the no-panic/no-exit rules to `crates/xinit-core/src` only.

Constraint this creates: everything in `xinit-core/src` runs in PID 1. Manager logic must live in `xinit-manager`, not in Core modules.

## Holdout suite contract
`scripts/verify-milestone.sh` runs the holdout suite only when `XINIT_HOLDOUT_DIR` is set. That directory must be a standalone Cargo package:
- `Cargo.toml` with an empty `[workspace]` table, and path dependencies written as `../../crates/<crate>`;
- a committed `Cargo.lock` (the run uses `--locked`).

verify-milestone copies it to `target/xinit-holdout/` (so the relative paths resolve to this repository) and runs `cargo test --locked` there. Zero holdout tests count toward the zero-tests failure.

## Test suite layout
Cargo builds only top-level `tests/*.rs` (and `tests/*/main.rs`) as test targets. In a virtual workspace it builds none of them. So the workspace root is also the `xinit-tests` package, and each milestone has one entry file:

```
tests/m0.rs            #[path = "m0/mod.rs"] mod m0;
tests/m0/mod.rs        mod supervise; mod reap; #[path = "../common/mod.rs"] mod common;
tests/m0/supervise.rs
tests/common/mod.rs    shared helpers
```

verify-milestone M<n> proves the suite ran:
- `cargo test --test m<n> -- --list` must exit 0 and list at least one test.
- Every `tests/**/*.rs` must appear in rustc's dep-info for some test target of `xinit-tests`. A file no `mod` reaches fails as an orphan.

### Finding the xrginit binary: XRGINIT_BIN
Integration tests in `xinit-tests` cannot use `env!("CARGO_BIN_EXE_xrginit")`. Cargo sets that variable only for tests in the binary's own package, which is `xinit-core`. Instead:

- The harness sets `XRGINIT_BIN` to the absolute path of the built `xrginit` binary.
- Tests get the path only through one helper in `tests/common/mod.rs`, written by the test author and frozen with the suite:

  ```rust
  /// Path to the xrginit binary under test, from XRGINIT_BIN.
  pub fn xrginit_bin() -> std::path::PathBuf
  ```

  It reads `XRGINIT_BIN` at run time, not with `env!`, so the blind workspace still compiles. It panics with a message naming `XRGINIT_BIN` when the variable is unset, the path is relative, or the path is not an executable file.
- No fallback to `target/debug/xrginit` or `$PATH`. A guess can pick up a stale or wrong binary and make a test pass against the wrong code. A test that cannot find the binary must fail and say why.
- In the blind workspace there is no binary, so tests that need it fail red at the helper. That is the expected red result.


## Blind workspace
`scripts/make-blind-workspace.sh <dest>` copies the root package (Cargo.toml with members cut to `.` and `crates/xinit-api`, `src/lib.rs`, `tests/`), `crates/xinit-api`, `specs/`, `features/`, `CLAUDE.md`, `Cargo.lock` and `rust-toolchain.toml`. Core and Manager source are left out. The red check there is `cargo test --offline --test m<n>`.
