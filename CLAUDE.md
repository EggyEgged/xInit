# xInit (xrginit): project instructions

Keep this file under 200 lines. Everything here is something you cannot infer from the code.

## What this is
xInit is the init system for xergOS. The binary is xrginit. It is pure Rust, Rust 2024 edition, Linux only, cgroups v2 only, with no SysV compatibility and no journald. It must work as PID 1 (init=/sbin/xrginit) and as an ordinary supervisor process in milestones M0 to M2.
Architecture: Core is PID 1. It is small, synchronous, has no async runtime, and never exits. Manager is a child of Core, restarted with bounded retries. After a Manager crash it re-adopts services by reading /sys/fs/cgroup/xinit/<unit> and /run/xinit/state. Core can shut down everything alone in degraded mode.

## Commands
- Build: cargo build --locked
- Musl build: cargo build --locked --target x86_64-unknown-linux-musl
- Full gate for a milestone: scripts/verify-milestone.sh M<n>
- One test: cargo test --locked -p xinit-core <name> -- --exact
- Frozen-test check: scripts/check-frozen.sh
- Hygiene lint: scripts/check-hygiene.sh
- Boot test (Debian phase only): scripts/qemu-boot-test.sh <image>

## Invariants (each has a reason)
1. Core never exits and never panics. A PID 1 exit makes the kernel panic or tear down the namespace.
2. Install a signal handler only for signals Core acts on. Signals to PID 1 without a handler are ignored by the kernel.
3. Every unsafe block has a // SAFETY: comment. Every FFI declaration sits in an unsafe extern block (Rust 2024).
4. Track children with pidfds and cgroups. Never track a unit by PID files, and never reuse a PID after it has been reaped.
5. Between fork and exec, call only async-signal-safe functions. Do not allocate or take locks there.
6. Syscall wrappers return typed errors.

## Forbidden
- Editing, deleting, moving or adding files under tests/ or holdout/, or tests/MANIFEST.sha256. The tests are the frozen contract.
- #[ignore], assert!(true), sleeps used to wait for a condition, should_panic without an expected message.
- Special-casing test inputs, copying expected output from tests into code, or reading tests from src.
- Skipping a test silently. A test that cannot run must fail and say why.
- #[allow(...)] without a // JUSTIFIED: line above it.
- New dependencies beyond those named in the milestone brief, unless you ask first.
- Work outside the current milestone's scope.

## Milestone protocol
1. Read PROGRESS.md and features/M<n>.json first.
2. Flip a feature's passes flag from false to true only after verify-milestone exits 0 and you have pasted its summary into PROGRESS.md. Nothing else in the feature file may change.
3. Commit implementation separately from test-freeze commits.
4. If the spec is silent or a frozen test looks wrong, do not guess. Write it under "Spec disputes" in PROGRESS.md and leave that clause unpassed.

## Reporting
- Show command output as evidence. Label anything you did not run as NOT VERIFIED.
- Keep final summaries under 15 lines. Long summaries bury the failures the next session needs to see.
