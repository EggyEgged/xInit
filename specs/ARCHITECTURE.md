# xInit architecture decisions

Normative behaviour belongs in per-milestone specs. This file records structural choices only.

## Crates
| Crate | Kind | Role |
|---|---|---|
| `xinit-api` | lib | Public types and function signatures shared by Core, Manager and tests. The spec writer adds signatures with `unimplemented!()` bodies. |
| `xinit-core` | bin `xrginit` | Core: PID 1 path, signal handling, child reaping, Manager supervision, degraded-mode shutdown. |
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
