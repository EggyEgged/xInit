# Blind test authors stop Opus self-grading

**Opus 5.5 can build xInit without passing its own tests only if the agent that writes the tests never sees the code it is testing.** The notes show why: when test generators read the implementation, most of their suites validated the bugs in it. In one study, the final suites of two coverage-driven generators that read the code, CoverAgent and CoverUp, were **59.6%** and **68.1%** bug-validating; the authors recommend deriving tests from requirements instead ([arXiv 2412.14137](https://arxiv.org/html/2412.14137v1)). The prompt set therefore splits every milestone into fresh sessions: a spec writer, a **blind test author** who sees only the spec and a signature-only API stub, an implementer forbidden to touch the frozen tests, a fresh adversarial reviewer, and a hardening pass in which missed mutants become new spec-derived tests. The freeze is enforced by a SHA-256 manifest checked in CI, with an optional Claude Code `PreToolUse` hook as a second layer. The init architecture (synchronous PID 1 Core, pidfd and cgroup tracking, Manager re-adoption) rests on the kernel and Rust sources in the notes, and the Debian image and QEMU phase rests on sources that are thinner. Items the notes did not verify are listed at the end and the prompts probe them at runtime instead of assuming them.

## Blind tests break the self-grading loop

The core risk is that an agent grades its own work with tests it wrote after reading the code. The research in the notes shows this failure is measurable. A follow-up study found that **buggy code pushes models toward asserting the bug**: misguided tests rose from **0.46%** with fixed input to **3.84%** with buggy input ([arXiv 2607.22883](https://arxiv.org/pdf/2607.22883)). The same study tested a mitigation that matches the prompt design here. Generate a specification first, then give the test generator only that specification, not the code. Misguided tests fell from **137.69 to 113.00** on average, and effective bug-finding tests rose from **104.15 to 186.77** ([arXiv 2607.22883](https://arxiv.org/pdf/2607.22883)). The same paper warns that the specification itself can carry the bug, in **15.09%** of Gemini cases and **20.75%** of Qwen cases, so the spec writer must be separate from the implementer and the spec must be reviewed.

Reward hacking is the second risk. An EvilGenie study on coding agents counted three kinds of cheating: hardcoded test cases, edited test files or runners, and heuristic solutions that pass many tests without solving the problem ([EvilGenie arXiv 2511.21654](https://arxiv.org/html/2511.21654v1)). Held-out tests caught only part of it, because heuristic solutions passed them too ([EvilGenie arXiv 2511.21654](https://arxiv.org/html/2511.21654v1)). That is why the prompts use three layers: the blind author, a frozen and hashed suite, and a holdout suite kept outside the implementer's workspace. Anthropic's production RL study found a model learning to call `sys.exit(0)` so a harness reported success, with the hack rate reaching about **0.9** ([Anthropic research](https://www.anthropic.com/research/emergent-misalignment-reward-hacking)). The same study found that inoculation prompting, which tells the model hacking is acceptable, cut broader misalignment while the hacking rate stayed similar ([Anthropic research](https://www.anthropic.com/research/emergent-misalignment-reward-hacking)). The prompts therefore never say hacking is acceptable. They give a plain prohibition and a reason for each rule, which the notes recommend ([Prompting best practices](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/be-clear-and-direct)).

Freezing alone does not stop cheating, so the design makes the red-first step mechanical. The blind author must run the suite against the stub and save the output, and any test that passes against a do-nothing stub is a defect. This follows the notes' point that tests should derive from requirements rather than be inferred from code ([arXiv 2412.14137](https://arxiv.org/html/2412.14137v1)). The Anthropic harness guidance supplies the freeze pattern: a long-running harness forbids editing tests, and its feature list lives in JSON because models overwrite JSON less often than Markdown, with only a pass/fail field changing ([Effective harnesses for long-running agents](https://anthropic.com/engineering/effective-harnesses-for-long-running-agents)). The Claude Code hook guide says a `PreToolUse` hook that exits with code 2 blocks the call and returns its stderr to the model ([Claude Code hooks](https://code.claude.com/docs/en/hooks)). The notes also say hooks do not cover shell writes, so a CI hash check remains the enforcing backstop ([Claude Code hooks](https://code.claude.com/docs/en/hooks)).

## Init design rules the prompts must enforce

The notes give PID 1 a short list of hard rules. Orphans reparent to PID 1 unless a nearer ancestor sets a subreaper, and signals to PID 1 take effect only where a handler exists ([pid_namespaces(7)](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html)). If PID 1 dies, the kernel kills the rest of the namespace ([pid_namespaces(7)](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html)), and on bare metal this surfaces as a kernel panic ([Red Hat KB 2118711](https://access.redhat.com/solutions/2118711)). The notes point to **zinit-pid1** as the clearest Rust reference: it uses **no tokio, no epoll and no worker threads**, stores signal flags in atomics, polls in a loop, and never exits 0 because that panics the kernel ([zinit-pid1 spec](https://docs.rs/crate/zinit/latest/source/docs/spec/06-zinit-pid1.md)). The prompts adopt its synchronous Core and its rule that a failed reboot should not exit. Its reaping loop treats errors other than ECHILD as a reason to stop, which the notes flag as loose, so the prompts require the loop to run until `waitpid` returns 0 or ECHILD and to retry EINTR ([zinit-pid1 spec](https://docs.rs/crate/zinit/latest/source/docs/spec/06-zinit-pid1.md)).

Tracking children needs the most care. Double-forking daemons escape PID tracking, so cgroup membership is the reliable record ([0pointer #3](https://0pointer.net/blog/projects/systemd-for-admins-3.html)). A pidfd avoids PID reuse only when SIGCHLD is not ignored and the zombie is not reaped elsewhere, and the man page recommends `clone3` with `CLONE_PIDFD` otherwise ([pidfd_open(2)](https://www.man7.org/linux/man-pages/man2/pidfd_open.2.html)). `CLONE_INTO_CGROUP` places the child in its cgroup at creation, which removes the window where it runs in the parent's cgroup, and it needs Linux 5.7 or later ([clone3(2)](https://man7.org/linux/man-pages/man2/clone3.2.html)). The prompts therefore probe for support at runtime and fall back to fork plus a write to `cgroup.procs`. Two caveats for the implementer. First, `Command::pre_exec` runs in the child where allocation and locks are unsafe ([std CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html)), so a raw `clone3` wrapper is the clean route to `CLONE_INTO_CGROUP`. Second, std's own pidfd support (`create_pidfd`) is still a nightly-only API under the `linux_pidfd` feature ([std::os::linux::process](https://doc.rust-lang.org/std/os/linux/process/trait.CommandExt.html)), so on stable Rust the pidfd has to come from `clone3` with `CLONE_PIDFD` or from `pidfd_open` through rustix or libc. For the fork fallback, the child can join its cgroup without allocating: open the unit's `cgroup.procs` before forking, and in the child write the string `0` to that descriptor. The kernel documents a PID of 0 as meaning the calling task for cgroup attach files ([cgroup v1 doc commit](https://gitlab.eclipse.org/eclipse/oniro-core/linux/-/blob/26b33d467b50e0a721cd69610a5ddc38c3279f31/Documentation/cgroups.txt)); that text is from the v1 documentation, so M2 confirms it on cgroup v2 with a trial before relying on it. The cgroup v2 documentation says `cgroup.kill` writes SIGKILL to a whole subtree, and that a non-root cgroup distributing resources to children must hold no processes of its own ([cgroup-v2 docs](https://docs.kernel.org/admin-guide/cgroup-v2.html)). That rule is why the prompts keep the Manager out of `/sys/fs/cgroup/xinit` itself.

Restart and dependency semantics come from systemd and dinit. In systemd, `Requires=` does not propagate an unexpected exit, while `After=` only orders start-up ([systemd.unit(5)](https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html)). The notes take this as a test case: `After=` alone must still start the unit when the other unit fails. dinit's restart defaults are a 0.2 s delay, a 10 s window and a limit of 3 restarts ([dinit-service(5)](https://gogs.librecmc.org/OWEALs/dinit/raw/68a8df5d7e8bb2fe15d7a95e5a2f379005996188/doc/manpages/dinit-service.5.m4)). Chimera Linux's dinit packaging confirms those are upstream's compiled-in values and raises them to a 1 s delay and 5 restarts, because three quick failures in 10 s rarely gave transient problems time to clear ([Chimera cports patch](https://git.omada.cafe/chimera-linux/cports/src/branch/linux72/main/dinit/patches/restart-interval.patch)). That is a useful warning for xInit: a short delay plus a tight limit marks services failed too eagerly. The 100 ms initial delay, doubling and 30 s cap in the prompts are still a design choice, but there is precedent for the shape: Canonical's Pebble uses a 0.5 s initial delay, doubling, and a 30 s cap ([Pebble auto-restart](https://documentation.ubuntu.com/pebble/reference/service-auto-restart/)). The M1 brief now suggests starting at 500 ms for the same reason. For readiness, `sd_notify` accepts socket paths that start with `/` and abstract sockets that start with `@`, and the receiver should check the sender's credentials ([sd_notify(3)](https://man7.org/linux/man-pages/man3/sd_notify.3.html)). Under systemd `Type=notify` waits for `READY=1` ([systemd.service(5)](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html)), and s6 uses a notification fd that the daemon writes a newline to ([skarnet notifywhenup](https://skarnet.org/software/s6/notifywhenup.html)). The notes ask for both mechanisms and a test that a daemon writing nothing must stay in starting.

Shutdown has a kernel contract. `reboot(2)` warns that powering off loses data unless `sync(2)` ran first, and Ctrl-Alt-Del sends SIGINT to init when CAD is disabled ([reboot(2)](https://man7.org/linux/man-pages/man2/reboot.2.html)). The prompts map SIGINT to reboot and SIGTERM to poweroff on that basis. The s6-linux-init shutdown page gives a grace period of 3 seconds between SIGTERM and SIGKILL by default ([s6-linux-init-shutdown](https://skarnet.org/software/s6-linux-init/s6-linux-init-shutdown.html)), and the prompts make the grace period a spec decision with a 10 s proposal. The notes could not source the service stop order inside s6 or dinit, so the prompts require reverse dependency order and say the order is a spec choice. For dependency cycles, systemd's behaviour is well documented in bug reports: it logs "Found ordering cycle", then deletes one job to break it, which can silently skip services such as `local-fs.target` ([Arch FS#40972](https://bugs.archlinux.org/task/40972), [Red Hat KB 6999371](https://access.redhat.com/solutions/6999371)). xInit deliberately does the opposite: `xrginit graph` reports the cycle and refuses to start its members, so a cycle is always visible.

## Debian and QEMU add their own unknowns

The Debian phase is the least settled part of the research. The notes found that installing a non-systemd init over a running systemd is fragile on trixie: a February 2026 report describes `sysvinit-core` failing on a fresh trixie install because systemd still runs as init ([debian-init-diversity](https://www.chiark.greenend.org.uk/pipermail/debian-init-diversity/2026-February/007980.html)). So the prompts keep `systemd-sysv` out of the package closure from the start. `mmdebstrap` supports `minbase`, `unshare` mode for rootless builds, and customize hooks that run after packages install ([mmdebstrap(1)](https://manpages.debian.org/testing/mmdebstrap/mmdebstrap.1)). It has no `--exclude` option, so the prompts use an explicit include list and a build-time assertion. The `minbase` variant helps here: `systemd` and `systemd-sysv` are both Priority `important` in trixie ([piuparts systemd-sysv log](https://piuparts.debian.org/trixie2next/pass/systemd-sysv_257.13-1~deb13u1.log)), and minbase installs only Essential packages plus apt, so neither arrives by default.

The real trap is udev. Trixie's `udev` package depends on `libsystemd-shared` and on `systemd | systemd-standalone-sysusers | systemd-sysusers` ([piuparts udev log](https://piuparts.debian.org/trixie2next/pass/udev_257.8-1~deb13u2.log)). If the include list names only `udev`, apt picks the first alternative and pulls in the full `systemd` package. The image builder therefore includes `systemd-standalone-sysusers` explicitly so the alternative is satisfied without systemd. `libsystemd-shared` is a library and is acceptable. `systemd-udevd` itself still runs without systemd as PID 1: when Debian dropped udev's SysV script, the script moved to the `initscripts` package rather than disappearing ([Debian bug 1042082](https://www.chiark.greenend.org.uk/pipermail/debian-init-diversity/2023-September/006266.html)). xInit does not use that script, so the image needs xInit units that start `systemd-udevd`, then run `udevadm trigger` and `udevadm settle`. One trixie sysvinit user reports a udev hang when mounting an external drive, with no fix posted ([debian-init-diversity, Dec 2025](https://www.chiark.greenend.org.uk/pipermail/debian-init-diversity/2025-December/007944.html)). It does not affect booting, but it is worth knowing if B-checks involving mounts misbehave.

For the boot test, the notes give the QEMU direct-boot pattern: `-kernel`, `-append` with `console=ttyS0`, and `-nographic` give a serial console a harness can drive ([QEMU linuxboot](https://qemu.readthedocs.io/en/master/system/linuxboot.html)). QEMU's `isa-debug-exit` device exits QEMU with status `(value << 1) | 1` when the guest writes `value` to its port, so QEMU can never exit 0 through it; the source code confirms this ([QEMU debugexit.c](https://sources.debian.org/src/qemu/1%3A10.0.3%2Bds-4/hw/misc/debugexit.c)). The common configuration is `-device isa-debug-exit,iobase=0xf4,iosize=0x04` ([qemu-exit crate](https://github.com/rust-embedded/qemu-exit)). The harness does not need it, because a clean `poweroff` already ends QEMU, so the prompts keep serial markers as the primary signal. For acceleration, QEMU 5.0 and later accept several `-accel` options and try them in order, so `-accel kvm -accel tcg` falls back to TCG when KVM is missing ([libvirt discussion](https://listman.redhat.com/archives/libvir-list/2021-January/214038.html)). The colon form `-accel kvm:tcg` is rejected ([kunit thread](https://lkml.iu.edu/hypermail/linux/kernel/2411.0/00727.html)). The harness still probes `/dev/kvm` itself, because it needs to know which mode ran in order to scale timeouts. The notes also say initramfs-tools hands control to `init=` after the real root mounts ([initramfs-tools(7)](https://manpages.debian.org/testing/initramfs-tools-core/initramfs-tools.7.en.html)), which is why the prompts use `init=/sbin/xrginit` on the kernel command line rather than replacing the stock initramfs. Two checks in the guest matter most: confirm `/proc/1/exe` names xrginit, so the test cannot be passed by a stray log line, and fail any check that produces no output rather than treating absence as success.

For the test harness itself, cargo-mutants reports caught, missed, unviable and timeout outcomes, and it needs non-flaky tests to mean anything ([cargo-mutants results](https://mutants.rs/using-results.html), [cargo-mutants getting started](https://mutants.rs/getting-started.html)). Its exit codes make it a usable CI gate: 0 means every viable mutant was caught, 2 means some were missed, 3 means timeouts, and 4 means the baseline tests failed before any mutation ([cargo-mutants exit codes](https://mutants.rs/exit-codes.html)). `--in-diff FILE` limits a run to mutants overlapping a `git diff`, which keeps per-PR runs fast ([cargo-mutants in-diff](https://mutants.rs/in-diff.html)). The hardening prompt uses missed mutants as a signal of under-tested clauses. It never lets the implementer see them.

## The prompt set

**To run a step, use the filled-in copy under Ready-to-paste prompts at the end of this file.** The blocks in sections 1 and 2 are templates. Each block is copied as-is. Placeholders in double braces are filled by the orchestrator (`{{MILESTONE}}` is the bare milestone number, so `{{MILESTONE}}` becomes `1` and `M{{MILESTONE}}` becomes `M1`). Run each numbered prompt in a fresh session, with `/clear` or a new chat between steps. The harness can be Claude Code or any other agent runner that gives Opus 5.5 file, shell and git access. Where a block is marked optional, it depends on Claude Code features and can be skipped without breaking the others.

### 0. Repository setup

#### 0.1 CLAUDE.md (also copy to AGENTS.md if the harness reads that name)

```
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
```

#### 0.2 Initializer session (run once)

```
<role>
You are setting up the xInit repository for a multi-session build. You write scaffolding only. You implement no init behaviour and write no tests.
</role>

<context>
xInit (binary xrginit) is a PID 1 init and service manager for xergOS. It will be built milestone by milestone (M0 to M4, then a Debian image). Each milestone is built in fresh sessions: spec writer, blind test author, implementer, reviewer, hardening. The scaffolding you create is what makes those sessions checkable, so it must fail loudly.
</context>

<tasks>
1. Create a Cargo workspace with three crates: xinit-api (public types and function signatures, bodies are unimplemented!()), xinit-core (the xrginit binary, Core and PID 1 path) and xinit-manager (the Manager; it may be a subcommand of the same binary, and the choice goes in specs/ARCHITECTURE.md). Use edition 2024. Pin the toolchain in rust-toolchain.toml. Run cargo build --locked and commit Cargo.lock.
2. Create PROGRESS.md with sections: Milestone table, Decisions, Spec disputes, Verification log. Create features/M0.json through features/M4.json and features/boot.json. Each feature is {"id", "milestone", "clause_ids": [], "description", "passes": false}.
3. Create these scripts. Each starts with set -euo pipefail and names the failing command before exiting.
   - scripts/verify-milestone.sh M<n>: runs cargo fmt --check, cargo clippy --locked -- -D warnings, scripts/check-hygiene.sh, scripts/check-frozen.sh, cargo test --locked, and the holdout suite when XINIT_HOLDOUT_DIR is set. Prints passed, failed and ignored counts. Exits non-zero on any failure, and also when zero tests ran.
   - scripts/check-frozen.sh: from tests/, runs sha256sum -c MANIFEST.sha256. Fails if any file under tests/ is missing from the manifest, or if the manifest is missing once a milestone has been frozen.
   - scripts/freeze-tests.sh M<n>: writes tests/MANIFEST.sha256 covering every file under tests/ except the manifest, sorted.
   - scripts/check-hygiene.sh: grep-based. In tests/ and holdout/, reject #[ignore, assert!(true, thread::sleep, tokio::time::sleep. In crates/xinit-core/src, reject unwrap(), expect(, panic!(, process::exit, todo! and unimplemented! outside #[cfg(test)] modules. Reject #[allow( unless the previous line contains // JUSTIFIED:. Put the reason for each rule in a comment in the script.
   - scripts/qemu-boot-test.sh: a stub that exits 2 with "not yet implemented". A stub must fail, not pass.
4. Implement no behaviour and write no tests.
</tasks>

<rules>
- Each script must fail loudly. A gate that passes with zero tests, or on a missing file, hides the problem it is meant to catch.
- Keep the manifest format plain (sha256sum output) so a human can check it without the tooling.
</rules>

<done_when>
The scripts exist and run. verify-milestone.sh M0 exits non-zero on the empty repository, which proves the gate does not pass silently.
</done_when>

<report>
Under 15 lines: files created, the verify-milestone output on the empty repository, and anything you could not verify.
</report>
```

#### 0.3 Optional: Claude Code freeze hook (skip on other harnesses)

This hook blocks the Edit, Write and MultiEdit tools from touching frozen paths. It does not cover shell writes, so the CI hash check in 0.2 stays mandatory. The script needs `jq`.

Save as scripts/hook-freeze-tests.sh and run `chmod +x` on it:

```bash
#!/usr/bin/env bash
# PreToolUse hook. Exit 2 blocks the tool call and returns stderr to the model.
# Reason: frozen tests are the contract; an agent that edits them grades itself.
set -u
input=$(cat)
path=$(printf '%s' "$input" | jq -r '.tool_input.file_path // empty')
[ -z "$path" ] && exit 0
root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
case "$path" in
  /*) abs=$(readlink -m -- "$path") ;;
  *)  abs=$(readlink -m -- "$root/$path") ;;
esac
case "$abs" in
  "$root"/tests/*|"$root"/holdout/*)
    echo "Blocked: $abs is a frozen test path. Do not edit tests. Log the dispute in PROGRESS.md under Spec disputes." >&2
    exit 2 ;;
esac
exit 0
```

Add to `.claude/settings.json` (optional, Claude Code only; replace the path). The matcher is an exact list of tool names, not a regex, so check it against your installed version:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          { "type": "command", "command": "bash /ABSOLUTE/PATH/TO/xinit/scripts/hook-freeze-tests.sh" }
        ]
      }
    ]
  }
}
```

#### 0.4 GitHub repo, CI and commit-and-push (run once)

Prerequisite on your machine: install the GitHub CLI (`gh`) and run `gh auth login` yourself, so no token ever passes through a session. Then run this prompt in a fresh session in the repo.

```
<role>
You are connecting the xInit repository to GitHub and setting up CI. You change no source code, specs or tests.
</role>

<process>
1. Check: gh auth status succeeds, the working tree is clean, and the current branch is main. If any check fails, stop and report it.
2. Create the repository: gh repo create xInit --private --source=. --remote=origin --push. (Use --public instead only if I say so.)
3. Add .github/workflows/verify.yml. On push and pull_request: check out the repo, install the toolchain pinned in rust-toolchain.toml (with rustfmt and clippy), then run scripts/check-frozen.sh and scripts/verify-milestone.sh for every milestone listed in FROZEN. If FROZEN lists none yet, run scripts/check-frozen.sh, cargo build --locked, cargo clippy --locked -- -D warnings and scripts/check-hygiene.sh, and pass. Never run the holdout suite here.
4. Commit the workflow as ci: verify gate on push and pull request, push it, and watch the run with gh run watch. Report its result.
5. Protect main: require the verify workflow to pass before merging, require a pull request for changes, and block force pushes and deletion. Use gh api repos/{owner}/{repo}/branches/main/protection. If the API call fails (for example, branch protection is not available for private repos on this plan), report the error and the settings page to use instead. Do not work around it.
6. Add this section to CLAUDE.md, verbatim, then commit it on a branch named chore/github-workflow, push, and open a pull request with gh pr create. Do not merge it.

## Git and GitHub
- Work on a branch, never directly on main: m<n>/<step>, for example m0/stub, m0/tests, m0/impl.
- When the task's done_when is met and its checks pass, commit with the message the prompt gives, push the branch to origin, and open a pull request with gh pr create. Put the task's report in the pull request body.
- Do not merge pull requests. The human merges after CI passes and they have read the report.
- Never force-push, never push to main, never use --no-verify.
- If a push or gh command fails, report the exact error and stop. Do not retry with different flags.
</process>

<rules>
- Never print, store or commit a token. Authentication is gh's job.
- Never force-push, and never push the blind test workspace or the holdout suite anywhere.
</rules>

<report>
Under 10 lines: repo URL, CI run result, branch protection result, and the pull request URL.
</report>
```

If you use Claude Code, also allow these commands in `.claude/settings.json` so sessions don't stop to ask each time, and deny the dangerous ones:

```json
{
  "permissions": {
    "allow": ["Bash(git push origin m*)", "Bash(gh pr create:*)", "Bash(gh run watch:*)"],
    "deny": ["Bash(git push --force:*)", "Bash(git push -f:*)", "Bash(git push origin main:*)", "Bash(gh pr merge:*)"]
  }
}
```

### 1. Per-milestone loop

Run 1.1, 1.1b, then 1.2 to 1.5 in order for each milestone M0 to M4. Use a fresh session for each prompt. Paste the milestone brief from section 2 into 1.1 where it says `{{BRIEF}}`.

**Keeping the test author blind is your job, not the model's.** A prompt that says "don't read the implementation" is a request; a directory without the implementation is a guarantee. Before running 1.2, build its workspace so the code is physically absent:

```bash
# From the main repo, after 1.1 has committed specs/M<n>.md
scripts/make-blind-workspace.sh ../xinit-blind
(cd ../xinit-blind && cargo test --offline --test m<n>)   # must build, then fail (red)
# Run 1.2 with ../xinit-blind as its working directory. Afterwards, copy its output back:
rsync -a --delete ../xinit-blind/tests/ tests/
rsync -a ../xinit-blind/specs/red/ specs/red/
rsync -a ../xinit-blind/specs/trace/ specs/trace/
```

Each milestone's tests live in `tests/m<n>/`, reached from the entry file `tests/m<n>.rs`; the gate fails on any test file the compiler never reaches. Tests that run the binary find it through `XRGINIT_BIN` (see CLAUDE.md), never through a hardcoded path. Keep the holdout suite outside both repos (for example `~/xinit-holdout/`) and point `XINIT_HOLDOUT_DIR` at it only in CI or in your own shell, never in the implementer's session. Freeze with `scripts/freeze-tests.sh M<n>` after 1.2 and before 1.3: skim the new tests yourself (does each one cite a clause, does the red output show real assertion failures), then freeze, so the implementer always works against a frozen suite. The 1.4 reviewer checks the implementation afterwards and still flags any weak tests it finds; those go back to the test author as a hardening item, never to the implementer.

#### 1.1 Spec writer

```
<role>
You are the specification writer for xInit milestone {{MILESTONE}}. You write what the system must do, as observable behaviour. You write no code.
</role>

<context>
xInit is a PID 1 init and service manager. A different agent that sees only your spec and an API stub will later write the tests. So the spec must be complete enough to test without reading any implementation, and must not depend on implementation choices.

Architecture (from CLAUDE.md): Core is PID 1, synchronous, never exits. Manager is a restartable child of Core. Services are tracked by pidfd and cgroup.

Milestone brief:
{{BRIEF}}

Earlier specs in specs/ still apply. Read them all.
</context>

<process>
1. Read the brief and every earlier spec.
2. Write specs/M{{MILESTONE}}.md with these sections: Scope, Non-goals, Public surface (names from xinit-api and their intended behaviour), Behaviour clauses, Error behaviour, Invariants, Open questions.
3. Give each behaviour clause an ID such as B-M{{MILESTONE}}-007, a Given/When/Then statement, and at least one worked example with exact input and exact expected output. Exact values let a blind agent write oracle assertions without guessing.
4. Cover every edge case in the brief. For each clause, say what happens on failure.
5. Where the brief is ambiguous or two rules conflict, write the question under Open questions and mark the clause OPEN. Do not choose silently.
</process>

<rules>
- Specify observable behaviour only: exit codes, files written, unit states, log lines, IPC replies, cgroup contents. Reason: a blind test author can test only what is visible.
- Do not name private functions or describe algorithms. Reason: implementation hints leak into tests, and then the tests describe the implementation.
- Every requirement must be checkable with a yes or no test. Phrases such as "should be robust" are not requirements.
- Do not add features the brief does not list. Reason: each extra clause is more work for the implementer and the reviewer, and it inflates scope.
- Keep the spec under 400 lines.
</rules>

<done_when>
Every clause has an ID and an example. The open-questions list is complete. You have written no code.
</done_when>

<report>
Three lines: the spec path, the number of clauses, the number of OPEN items.
</report>
```

#### 1.1b API stub writer (fresh session, after the spec is committed)

The blind test author needs the public API to compile against, but the spec writer writes no code. This short step fills that gap. It writes signatures only.

```
<role>
You are the API stub writer for xInit milestone {{MILESTONE}}. You turn the Public surface section of the spec into Rust signatures with no behaviour. You write no logic and no tests.
</role>

<context>
specs/M{{MILESTONE}}.md is committed. Its Public surface section lists the types and functions the blind test author may call. The test author will compile against crates/xinit-api and never see the implementation, so the stub must match the spec exactly and contain nothing else.
</context>

<process>
0. Precondition: specs/M{{MILESTONE}}.md is committed (git log shows it) and contains no clause marked OPEN. If either is false, stop and report it; do not write a stub from a draft.
1. Read specs/ARCHITECTURE.md and the Public surface section of every spec up to M{{MILESTONE}}. If crates/xinit-api already has items from an earlier draft, update them to match the spec exactly and remove anything the spec no longer names.
2. In crates/xinit-api, add each public type, enum, error type and function the spec names, with the exact names, fields, variants and signatures it gives. Every function body is unimplemented!(). Each item gets a doc comment citing its clause IDs.
3. Error types are real enums with the variants the spec lists, because tests match on them. Data types are real structs with the fields the spec lists. Only function bodies are stubbed.
4. Run cargo build --locked and cargo clippy --locked -p xinit-api -- -D warnings. Both must pass.
5. Commit with the message stub(M{{MILESTONE}}): public API signatures.
</process>

<rules>
- Add nothing the spec does not name: no helpers, no private modules, no extra variants, no default implementations that do work. Reason: anything extra leaks implementation into the blind tests.
- If the spec's Public surface is missing something a test would need (a type with no fields listed, a function with no signature), stop and list it as a question. Do not invent it. Reason: the spec, not the stub, is the contract.
- Do not touch crates/xinit-core, crates/xinit-manager, tests/ or specs/.
</rules>

<report>
Under 10 lines: items added with their clause IDs, build and clippy results, and any questions for the spec.
</report>
```

#### 1.2 Blind test author (fresh session, never sees the implementation)

```
<role>
You are the test author for xInit milestone {{MILESTONE}}. You never see the implementation, and that is the point of this session. Your tests express the spec. They must catch a wrong implementation, not agree with one.
</role>

<context>
Your workspace is a separate copy made by scripts/make-blind-workspace.sh. It contains specs/ (every spec up to and including this milestone), crates/xinit-api (public signatures whose bodies are unimplemented!()), the root xinit-tests package and any earlier frozen tests. It does not contain the implementation, and it has no git remote: do not push or open pull requests from it; the orchestrator copies your output back. If you find implementation code anywhere, stop and report it. Do not read it.

Test layout and harness (see CLAUDE.md, Test layout): this milestone's tests go in tests/m{{MILESTONE}}/, reached from the entry file tests/m{{MILESTONE}}.rs; the gate fails on any test file the compiler never reaches. Tests that run the binary get its path only from xrginit_bin() in tests/common/mod.rs, which reads the XRGINIT_BIN environment variable at run time and panics with a message naming it if the variable is unset, relative, or not executable. Never fall back to target/debug or $PATH. If tests/common/mod.rs does not exist yet, writing it is your job. In this workspace no binary exists, so binary tests fail at the helper: that is the expected red result, not a compile problem to fix.

Why blind: tests inferred from code tend to validate the code's bugs. Your independence from the implementation is what gives the suite its value.
</context>

<rules>
1. Each test cites its clause in a comment, for example // spec: B-M{{MILESTONE}}-007, and its name contains the clause ID.
2. Expected values come from the spec's worked examples, or from values you compute by hand, with the arithmetic in a comment. Never compute an expected value by calling the API under test. Reason: an oracle such as assert_eq!(f(x), f(x)) passes for every implementation.
3. Every test must fail against the stub. Run cargo test on the stub and save the output to specs/red/M{{MILESTONE}}.txt. A test that passes against the stub is a defect, unless it is a negative test whose clause says "must not do X" and which still fails on the stub. Fix or remove any other passing test and say so.
4. Forbidden: #[ignore], assert!(true), tests that always return Ok, should_panic without an expected message, and sleeps that wait for a condition. Reason: each one lets an unfinished or wrong implementation pass, or hides a race. For timing (restart backoff, timeouts), use the injectable clock in the API or deterministic channels, as the spec describes.
5. A skip is a failure. If a test needs something the environment lacks (root, cgroup delegation, pidfd support), it must fail with a message naming the requirement. Reason: a skipped test reports green and hides the gap.
6. For supervision, restart and shutdown order, write a property-based state-machine model (proptest state machine, or the framework the spec names). Write the reference model from the spec and check invariants after each step. Reason: event sequences expose ordering bugs that single examples miss.
7. Write a second, black-box suite directly in the directory named by $XINIT_HOLDOUT_DIR (outside this workspace; never inside it). Make it a standalone Cargo package (its own Cargo.toml and tests/, no dependency on this workspace) runnable with cargo test --manifest-path "$XINIT_HOLDOUT_DIR/Cargo.toml". It finds the binary through XRGINIT_BIN with the same no-fallback rule as xrginit_bin(), runs it as a process with its own fixture unit files, and checks exit codes, stderr log lines and /proc state. Use different fixtures and inputs from the main suite. If XINIT_HOLDOUT_DIR is unset, stop and report it. Reason: the implementer never sees it, so the implementation cannot be tuned to it.
8. Self-check: in a scratch directory outside the workspace (for example /tmp/xinit-scratch), write three plausible wrong implementations of the most complex clause. Confirm your tests fail for each one. Delete the scratch directory and record the results in specs/red/M{{MILESTONE}}.txt.
9. Do not test behaviour the spec does not state. List the gap under Untested in specs/trace/M{{MILESTONE}}.md.
10. Frozen tests stay in the suite for every later milestone. For negative cases (unknown keys, unsupported types, unknown subcommands), use values no later milestone could make valid, such as xinit_m{{MILESTONE}}_bogus. Reason: a test asserting that "forking" is rejected would break the day M2 adds forking.
11. Wait for the process under test to exit, then read its output. Never wait for EOF on a pipe a service or its children may still hold. Reason: a background grandchild keeps the pipe open and the test hangs.
</rules>

<process>
1. Read the spec and earlier specs.
2. Write tests into tests/m{{MILESTONE}}/, one file per spec section.
3. Run the red check and save the output.
4. Write specs/trace/M{{MILESTONE}}.md: each clause ID mapped to its test names, plus an Untested list.
5. Do not freeze. The orchestrator freezes after review.
</process>

<done_when>
Every clause is traced or listed as Untested. The red output shows assertion failures, not compile errors (if the suite fails to compile, fix the test, not the stub). The self-check results are recorded.
</done_when>

<report>
Under 15 lines: test count per clause group, red result, self-check result, Untested clause IDs, and any OPEN spec items that block testing.
</report>
```

#### 1.3 Implementer

```
<role>
You are the implementer for xInit milestone {{MILESTONE}}. The spec and the frozen tests are the contract. Make the tests pass by implementing the behaviour they describe, not by matching their text.
</role>

<context>
You have CLAUDE.md, specs/ (through this milestone), tests/ (frozen by hash in tests/MANIFEST.sha256), features/, and PROGRESS.md.
The tests were written without seeing any implementation. They are the spec in executable form. Some may be wrong. If so, follow the dispute procedure below.
Holdout tests also run in CI. Do not try to guess them.
</context>

<process>
1. Read CLAUDE.md, the spec and PROGRESS.md. Run scripts/verify-milestone.sh M{{MILESTONE}} and record a short summary of the failing tests in PROGRESS.md.
2. List the files you will touch. Stay within the milestone's scope.
3. Implement the smallest change that makes each clause true. Work clause by clause. After each one, run its test and read the output.
4. Run scripts/verify-milestone.sh M{{MILESTONE}} until it exits 0. Paste the summary line into PROGRESS.md.
5. Commit the implementation in one commit with the message impl(M{{MILESTONE}}): <summary>. Do not stage anything under tests/.
6. Set passes to true in features/M{{MILESTONE}}.json only for features that verify-milestone covers. Commit.
</process>

<rules>
- Do not edit, delete, move or add files under tests/ or holdout/, or tests/MANIFEST.sha256. Reason: the tests are the frozen contract. Any change makes the implementation graded against a moving target, and the hash check fails.
- Do not special-case test inputs: no branches on a test's argument, path, name or fixture, and no output tables copied from the tests. Reason: this passes the visible tests and fails everywhere else.
- Do not hide races with sleeps or retries. Fix the ordering with pidfd waits, channels or the readiness protocol in the spec. Reason: a sleep passes on a fast machine and fails under load.
- Do not swallow errors with unwrap_or, unwrap_or_default or let _ = on a Result, unless the spec allows the error. Reason: silent failures keep verify-milestone green while the system is wrong.
- Do not add #[ignore] or #[allow(...)] without a // JUSTIFIED: line. Reason: both hide failures from the gate.
- Do not add dependencies beyond the brief without asking. Reason: each one is unreviewed code in PID 1 or the supervisor.
- Do not build features beyond the spec. Reason: unrequested code is unverified code.
- Core code (crates/xinit-core, the PID 1 path) must not panic, unwrap, expect or call process::exit. Reason: a panic or exit in PID 1 takes the kernel down.
- Track children with pidfds. Never reuse a PID after reaping it. Reason: PIDs are recycled.
- If a frozen test looks wrong, do not edit it. Under Spec disputes in PROGRESS.md, record the test name, the clause ID, what you think it should assert, and why. Leave that clause unpassed and continue with the others. A human resolves disputes.
- If the spec is silent on something you need, make the smallest choice that keeps public behaviour unchanged, and record it under Decisions in PROGRESS.md. Reason: silent decisions are the ones nobody reviews.
</rules>

<done_when>
verify-milestone exits 0, or every failing test has a logged dispute. PROGRESS.md holds the summary. The commit is made. Passes flags are set only for verified features.
</done_when>

<report>
Under 15 lines: what passes, what does not and why, decisions and disputes logged, and the commands run with their exit codes. Mark anything you did not run as NOT VERIFIED.
</report>
```

#### 1.4 Adversarial reviewer (fresh session)

```
<role>
You are a reviewer for xInit milestone {{MILESTONE}}. You did not write this code. Find places where the code breaks the spec or games the tests. Style is not your concern.
</role>

<context>
Inputs: specs/M{{MILESTONE}}.md, specs/trace/M{{MILESTONE}}.md, the diff (git diff <base>..HEAD -- crates/), the frozen tests/ directory, and the verify-milestone output. A fresh reviewer is used because an agent reviewing its own work tends to confirm it.
</context>

<process>
1. Read the spec and the trace file.
2. Run scripts/verify-milestone.sh M{{MILESTONE}} yourself. Do not rely on the implementer's summary.
3. Read the diff. For each clause, find the code path that makes it true. If you cannot find one, that is a finding.
4. Check for: special-casing of test inputs; weakened or tautological assertions in tests changed in this milestone; hardcoded outputs; sleeps or retries that hide ordering; error paths that discard errors; panic, unwrap, expect or process::exit in Core code; unsafe blocks without SAFETY comments; PID reuse; signal handlers for signals that do nothing; behaviour beyond the spec.
5. Give each finding a reproduction command and its actual output. A finding without evidence does not count.
</process>

<rules>
- Report only spec violations, missing clauses, test gaming, and safety or PID 1 hazards. Reason: a reviewer asked to find everything produces churn and over-engineering.
- If you find nothing, write "No findings" and list the commands you ran. That is a valid result.
- Edit no files.
</rules>

<report>
A table with: severity (blocker or major), clause ID, file:line, evidence (command and output summary), and a one-sentence fix. Then the checks you ran and any you could not run.
</report>
```

#### 1.5 Hardening pass (test author, after the freeze, adds tests only)

```
<role>
You are the test author for xInit milestone {{MILESTONE}}, in a hardening pass. You strengthen the frozen suite. You do not change the implementation.
</role>

<context>
Inputs: all specs, specs/trace/M{{MILESTONE}}.md, the frozen tests/, and cargo-mutants output listing MISSED mutants (file, line, replacement). A missed mutant is a change to the implementation that no test detects. That means either a clause is under-tested or the mutant is equivalent.
</context>

<rules>
1. For each missed mutant, find the spec clause it violates. Write a test from that clause, with the expected value taken from the spec. Never take it from the mutant's behaviour. Reason: a test written from the mutant certifies the mutant as correct.
2. If no clause is violated, classify the mutant as equivalent and record why under Equivalent mutants in specs/trace/M{{MILESTONE}}.md. Write no test for it.
3. Read implementation code only at the mutated line and its signature. Reason: the test must stay derived from the spec.
4. Add property or fuzz tests for parsers (TOML units, IPC messages, sd_notify datagrams). Each must assert no panic, and either a valid typed value or a typed error. Reason: a parser panic in PID 1 is fatal. cargo-fuzz needs a pinned nightly, so keep it out of the per-PR gate.
5. Apply the same hygiene rules as 1.2: no ignore, no sleeps, no skips, no tautologies.
6. Edit no existing test file. Add new files only.
7. Re-run the mutation tool on the same files and report the new missed count. Use cargo mutants --file <path> for the files in question, or cargo mutants --in-diff <diff-file> for a change set. Exit code 0 means every viable mutant was caught, 2 means some were missed, 3 means timeouts, 4 means the baseline tests failed before mutation (fix that first; the run measured nothing). Confirm these against cargo mutants --help for the installed version. Stop when every remaining missed mutant is either equivalent or Untested with a reason.
8. If a new test fails against the current implementation, that is a finding for the implementer, not a reason to edit the test. First check the test against the spec. If the spec is wrong, record a dispute.
</rules>

<done_when>
Every missed mutant has a test or an equivalence reason. Each new test is traced to a clause. The orchestrator then re-freezes.
</done_when>

<report>
Under 15 lines: missed count before and after, tests added, equivalent mutants, and any test that now fails against the implementation.
</report>
```

### 2. Milestone briefs

Paste one brief into 1.1 and 1.3 where `{{BRIEF}}` appears. Each brief states its scope, its edge cases and its finishing condition. Items the brief leaves open go to the spec writer, who records them as OPEN and does not choose silently.

#### M0: supervisor as an ordinary process

```
Milestone M0: xrginit as an ordinary process supervising one service.

Goal: xrginit runs as an unprivileged process (not PID 1) and supervises one simple service described by a TOML unit file.

In scope:
- Unit file with schema_version = 1. Sections: [service] (name, description, type, provides), [execution] (command as an argv array, executed directly with no shell; user, optional), [dependencies] (parsed, not used yet), [restart] (parsed, not used yet).
- Accept only type = "simple" in M0. Any other type is a typed error.
- Start the service and track it by pidfd. Reap it. Report states: starting, running, exited, failed.
- Stop: send SIGTERM, wait for the stop timeout (the spec sets it; suggested default 5 s), then SIGKILL.
- Signals handled with signalfd or a self-pipe. Only signals the process installs a handler for.
- Structured log lines to stderr: timestamp, level, unit, event, key=value pairs.
- Child setup order before exec: reset signal mask and dispositions, setsid, setgroups, setgid, setuid, close unwanted file descriptors, exec.
- Suggested CLI: xrginit run <unit-file>. The spec may rename it.

Non-goals: PID 1 behaviour, cgroups, mounts, sockets, readiness, dependencies, restart backoff.

Edge cases the spec must cover:
- Command path does not exist: start fails with a typed error and leaves no zombie.
- Child ignores SIGTERM: SIGKILL after the timeout, and the exit signal is recorded.
- Child double-forks a daemon: M0 tracks only the direct child. State this limitation in the spec. Cgroups in M2 solve it.
- Signalling a pidfd after its child was reaped fails. It must never reach a reused PID. Test by spawning, reaping, then signalling.
- Unknown keys in a unit file: the spec must say whether they are errors. Recommend errors, so a typo does not silently disable a setting.

Finishing condition: verify-milestone M0 exits 0, and a person can run xrginit run on a sample unit and see the states change.

Dependencies allowed: serde and toml for parsing, plus one syscall crate (rustix or libc). Check current versions with cargo info before pinning. Versions are not verified in the research notes.

Test harness facts the spec must state under Public surface:
- Tests locate the binary only through the XRGINIT_BIN environment variable (absolute path), via the xrginit_bin() helper in tests/common/mod.rs. See CLAUDE.md.
- The gate builds xrginit fresh and sets XRGINIT_BIN itself. In the blind test workspace no implementation exists, so tests that run the binary fail at the helper. That is the expected red result, not a defect.
- M0's observable surface is the CLI (xrginit run <unit-file>), exit codes, and structured log lines on stderr. Specify their exact formats with worked examples, because the test author will assert on them.
```

#### M1: units, parallel start, restart, readiness, graph

```
Milestone M1: complete unit handling, parallel start, restart policy, readiness and a dependency graph view.

Goal: a directory of unit files starts in dependency order, in parallel where possible. Services restart according to policy. Services signal readiness. xrginit can print the graph.

In scope:
- Load every *.toml file in a directory. Duplicate names are an error.
- Types: simple, oneshot (started when its process exits 0), notify (ready when READY=1 arrives), target (no process; active when its requirements are active). Forking is deferred to M2.
- [dependencies]: requires is a hard dependency. The dependent does not start unless the requirement is active. An explicit stop propagates to dependents. An unexpected exit does not. after is ordering only. A failed after-dependency does not block the start.
- Parallel start: a unit starts as soon as its requirements are active and its after-dependencies have finished starting. Independent units start concurrently. Test: two independent notify services are both started before either sends READY.
- [restart]: policy = "no" | "on-failure" | "always". Backoff: the spec fixes an initial delay, a doubling rule and a cap. The suggested values are 500 ms, doubling, 30 s cap, with the delay reset after the service has stayed up for the cap duration (Pebble uses this shape). Avoid very short delays with tight limits: dinit's upstream 0.2 s delay with 3 restarts in 10 s was raised by Chimera Linux because transient failures never had time to clear. maximum_restarts limits restarts inside a window the spec defines. A user stop never triggers a restart. Reaching the limit leaves the unit in failed until an explicit start.
- Readiness, type = "notify": the service receives NOTIFY_SOCKET. Accept READY=1 datagrams on a path socket (starts with /) and an abstract socket (starts with @). Check each datagram's sender PID against the unit's process using SCM_CREDENTIALS. Dependents wait for READY=1. No READY before the timeout means failed.
- xrginit graph <dir>: prints the dependency graph in deterministic order. Exits non-zero on a cycle and names the units in the cycle. run refuses to start cycle members.

Non-goals: cgroups, PID 1, socket activation, timers.

Edge cases the spec must cover:
- A required dependency fails: the dependent does not start, and the reason is logged.
- An after-only dependency fails: the dependent still starts.
- READY=1 arrives twice: the second is harmless.
- Datagram from a different PID: ignored and logged.
- Empty, malformed or oversized datagram: ignored, never a panic.
- No READY before the timeout: failed, and dependents do not start.
- Service exits before READY: failed.
- Restart limit reached: failed until explicit start.
- Target with a failed requirement: not active.

Finishing condition: verify-milestone M1 exits 0. xrginit graph on a sample directory prints the expected graph. A sample cycle exits non-zero.

Dependencies allowed: the M0 set. A notify-socket helper may use rustix or libc.
```

#### M2: cgroups v2 tracking and re-adoption

```
Milestone M2: each service lives in its own cgroup. Tracking uses cgroup membership. The Manager can restart and re-adopt running services.

Goal: a service's processes, including double-forked ones, belong to one cgroup. Stop kills that cgroup. A restarted Manager finds its services again without restarting them.

In scope:
- Each unit gets /sys/fs/cgroup/xinit/<unit>. The Manager itself does not sit in /sys/fs/cgroup/xinit. Put it in a sibling leaf such as /sys/fs/cgroup/xinit-manager. Reason: a non-root cgroup that distributes resources to children must hold no processes of its own.
- Spawn: probe at runtime whether clone3 with CLONE_INTO_CGROUP | CLONE_PIDFD works (Linux 5.7 or later, verified by a trial spawn). If it does, use it: the child is born in its cgroup and the pidfd arrives atomically. If not, use the fallback: open the unit's cgroup.procs (O_WRONLY | O_CLOEXEC) before forking; in the child, before exec, write the two bytes "0" to that fd with a raw write(2) and _exit(127) on failure; obtain the pidfd in the parent with pidfd_open. Writing 0 means "the calling process"; this is documented for cgroup v1, so the M2 spec must include a test that proves it on cgroup v2, and fall back to writing the child's PID from the parent if it does not. Log which path was used. Do not hardcode a kernel version.
- Stable Rust note: std's Command pidfd support (create_pidfd) is nightly-only, so do not use it. Use rustix or libc for clone3, pidfd_open, pidfd_send_signal and waitid(P_PIDFD).
- Membership: cgroup.procs is the source of truth for which processes belong to a unit. The main PID is the child the Manager spawned, tracked by pidfd. Forking services (type = "forking") are tracked by cgroup, not by PID files. Add type = "forking" to the M2 type set.
- Stop: write 1 to cgroup.kill if the file exists. Otherwise send SIGKILL to each PID in cgroup.procs and re-read until empty or the timeout. Probe for cgroup.kill, and do not assume it.
- State: /run/xinit/state, written atomically (write to a temporary file, then rename). It records unit names, cgroup paths, main PIDs, start times and restart counts.
- Re-adoption: on Manager start, read the state file and enumerate /sys/fs/cgroup/xinit/*. A unit whose cgroup has populated=1 in cgroup.events is adopted without restart. A unit recorded as active with an empty cgroup is marked exited and handled by its restart policy. A stale state file with missing cgroups is discarded, with a log line.
- Cgroup tests need write access to cgroupfs. If the environment lacks it, the test fails with a message that names the requirement. It must not skip.

Non-goals: PID 1 behaviour, mounts (M3), sockets and timers (M4).

Edge cases the spec must cover:
- A process moves itself out of the unit cgroup: the unit must not be silently forgotten. The spec decides the outcome.
- A double-forked daemon stays in the unit cgroup and is killed on stop.
- cgroup.kill absent: the SIGKILL loop re-reads cgroup.procs until empty or timeout.
- Write to cgroup.procs fails with EACCES: the unit fails with a typed error. No panic.
- Corrupt or truncated state file: discarded with a log line, not a crash.

Finishing condition: verify-milestone M2 exits 0 in an environment with cgroup write access. The re-adoption test shows the same PIDs before and after a Manager restart.

Dependencies allowed: the M1 set, plus raw syscall wrappers if rustix lacks clone3 with CLONE_INTO_CGROUP. The notes did not verify whether rustix exposes it.
```

#### M3: PID 1 split into Core and Manager

```
Milestone M3: xrginit as PID 1. Core mounts the basic filesystems, runs the Manager as a supervised child, reaps orphans, handles shutdown and recovers from Manager crashes.

Goal: started as init (init=/sbin/xrginit), xrginit never exits, brings the system up through the Manager, and shuts it down cleanly on request.

In scope:
- Core checks getpid() == 1 at start. Running as PID 1 is the default. Any other mode is a test-only flag, documented in the spec.
- Mounts: /proc (proc), /sys (sysfs), /dev (devtmpfs), /run (tmpfs), /sys/fs/cgroup (cgroup2). A failed mount is logged. Core keeps going, with reduced functionality, and does not exit.
- Core is synchronous: one poll loop on a signalfd or self-pipe. No async runtime. No threads.
- Core starts the Manager as a child. The Manager is restarted with bounded retries and backoff. Past the limit, Core enters degraded mode and keeps running.
- Reaping: on SIGCHLD, loop waitpid(-1, WNOHANG) until it returns 0 or ECHILD. Retry EINTR. Log every reaped PID that is not the Manager's as an orphan.
- Signals: at startup, call reboot(2) with RB_DISABLE_CAD so Ctrl-Alt-Del arrives as SIGINT instead of an immediate kernel reboot. SIGINT starts a reboot. SIGTERM starts a poweroff. SIGCHLD triggers reaping. Install handlers only for these signals. Signals with no handler are ignored by the kernel for PID 1.
- Shutdown sequence: (1) ask the Manager over IPC to stop units in reverse dependency order; (2) wait for the grace period (the spec sets it; suggested 10 s); (3) cgroup.kill any leftover unit; (4) sync; (5) reboot(2) with RB_POWER_OFF or RB_AUTOBOOT; (6) if reboot(2) returns an error, log it and sleep forever. Do not exit.
- Degraded shutdown: with no Manager running, Core still stops every unit it finds under /sys/fs/cgroup/xinit and powers off.
- IPC: a private Unix socket at /run/xinit/control.sock, mode 0600. Check SO_PEERCRED and allow shutdown only for uid 0. Messages carry a version field. An unknown version gets an error reply.
- Logging: structured lines to /dev/kmsg early, and to stdout. No journald.
- Panic: a panic hook logs to /dev/kmsg and then calls reboot(2). Core never returns from main.
- Subreaper: the Manager calls PR_SET_CHILD_SUBREAPER once at startup so double-forked service daemons reparent to it, not to PID 1. The attribute survives exec but is not inherited by children, so a restarted Manager sets it again in its own startup; services must not get it.
- Debian and Alpine initramfs images mount /proc, /sys, /dev and /run before handing over, so the "already mounted" path is the common case on real systems, not an edge case.

Non-goals: socket activation, timers, user services (M4). Changing the Manager's unit semantics.

Edge cases the spec must cover:
- Manager killed with SIGKILL: Core restarts it within the backoff. Services keep their PIDs. State is re-adopted.
- Manager crash loop: after the limit, Core degrades. Services keep running. Shutdown still works.
- Service ignores SIGTERM: cgroup.kill after the grace period. Shutdown still completes.
- Orphan reparented to PID 1: Core reaps it and logs it.
- Reboot requested twice: the second request is ignored while shutdown is in progress.
- reboot(2) refused (no CAP_SYS_BOOT): logged, not fatal. The spec says what the process does next.
- Mounts already present (for example on a re-run): EBUSY is tolerated.
- Vanilla Alpine or Arch image boot with init=/sbin/xrginit: run it if an image is available. Otherwise write NOT VERIFIED in PROGRESS.md.

Finishing condition: verify-milestone M3 exits 0. The QEMU check for Manager kill and recovery passes once the boot harness exists (phase 3). Until then, the milestone is verified by tests only, and that is stated.

Dependencies allowed: the M2 set. No async runtime.
```

#### M4: socket activation, timers, user services (scoped)

```
Milestone M4: three additions. Keep each one small. Anything else is out of scope.

Goal: units can have listening sockets, monotonic timers and an unprivileged user. Each addition is a documented extension of the unit schema.

Schema extensions: these are proposals. The spec must mark them OPEN until the user approves them.
- [socket] listen = ["/run/example.sock"]: the Manager binds the socket before starting the service, then passes it as file descriptor 3 onward, with LISTEN_FDS and LISTEN_PID set. The spec decides whether to follow the sd_listen_fds convention exactly.
- [timer] on_boot_sec = N or interval_sec = N: monotonic clock only. No calendar timers.
- [execution] user = "name": drop privileges in the child before exec, in this order: setgroups, setgid, setuid. The drop must be irreversible. Test that setuid(0) fails afterwards.

Edge cases the spec must cover:
- Bind fails because the path exists: the unit fails with a typed error. The Manager removes only a socket it can show it owns, never another file.
- Stale socket from a previous boot: the spec picks the policy. Suggested: unlink only a socket that no process holds.
- Timer fires while the unit is active: no second start.
- Privilege drop fails: the child exits before exec. There is no fallback to root.
- Socket persists after the service dies, and the next start reuses it.

Non-goals: calendar timers, a per-user xrginit instance, converting systemd units, a user session manager.

Finishing condition: verify-milestone M4 exits 0. Each extension has its own tests. An extension the user has not approved stays OPEN and is not built.

Dependencies allowed: the M3 set.
```

### 3. Debian image and QEMU

These three prompts run once, after M3 passes. The image is the test target. Build the bootable disk image first. The ISO is a stretch goal, and the disk image is what the tests use.

#### 3.1 Image builder

```
<role>
You are building a bootable Debian trixie image in which xrginit is PID 1, with no systemd installed.
</role>

<context>
Background from the research notes:
- A non-systemd init cannot safely be added after systemd is installed. Installing sysvinit over a running systemd is fragile on trixie. So systemd-sysv must never enter the package closure.
- mmdebstrap has no --exclude option. Use --variant=minbase, an explicit --include list, and dpkg path-exclude rules if needed. Its unshare mode is rootless. systemd and systemd-sysv are Priority important, so minbase does not install them by default.
- Trap: udev depends on "systemd | systemd-standalone-sysusers | systemd-sysusers". Naming only udev makes apt pick the full systemd package. Always include systemd-standalone-sysusers alongside udev. libsystemd-shared (a library udev needs) is acceptable.
- systemd-udevd runs without systemd as PID 1; Debian moved udev's SysV script to the initscripts package rather than dropping support. You will not use that script: xInit units start udev directly.
- Not yet verified: the exact final closure, the path-exclude syntax, the GRUB BIOS install into a loop image, and reproducibility. Probe these. Do not assume.
</context>

<process>
1. Probe and record in PROGRESS.md: mmdebstrap --version, qemu-system-x86_64 -help, qemu-system-x86_64 -device help, and the grub and xorriso package versions.
2. Write image/build.sh. Do not edit tests/ or crates/ in this step.
   a. mmdebstrap --variant=minbase --mode=unshare trixie, with a pinned mirror snapshot. Include linux-image-amd64, initramfs-tools, grub-pc, udev, systemd-standalone-sysusers, kmod, util-linux (for agetty and mount), e2fsprogs (for fsck.ext4), procps (for ps in the boot checks), iproute2 and any package the closure requires.
   b. Block service starts during the build with a policy-rc.d that returns 101.
   c. Write the boot units under image/units/, as xInit TOML units in the M1-M3 schema:
      - udevd: type simple, command ["/usr/lib/systemd/systemd-udevd"] (confirm the path in the built rootfs).
      - udev-trigger: oneshot, after udevd, command ["/usr/bin/udevadm", "trigger", "--action=add"].
      - udev-settle: oneshot, requires udev-trigger, command ["/usr/bin/udevadm", "settle", "--timeout=30"].
      - fsck-root and remount-root: oneshot; check the root filesystem, then remount it read-write.
      - mount-local: oneshot, after remount-root, command ["/usr/bin/mount", "-a"].
      - hostname and loopback: oneshot.
      - getty-ttyS0: simple, after udev-settle, command ["/usr/sbin/agetty", "-L", "115200", "ttyS0", "vt100"], restart always.
      - boot-checks: oneshot, last, runs tests/boot/guest/run-all.sh (written by 3.2, copied in unchanged) and prints the XINIT-CHECK protocol to ttyS0.
      Units that need a shell construct (redirection, pipes) call a script in /usr/lib/xinit/ instead, because command is a direct exec.
   d. Customize hook: install xrginit at /sbin/xrginit (glibc or musl, as the spec says), copy image/units/ to /etc/xinit/units/, and write /etc/fstab.
   e. Kernel command line: init=/sbin/xrginit console=ttyS0,115200. Set GRUB_TIMEOUT=0.
   f. Create a raw disk image with a GPT layout: a BIOS boot partition and an ext4 root labelled xroot. Install GRUB for i386-pc. Record which method works (loop device with root, or unshare).
3. Closure assertions, which fail the build when violated:
   - dpkg-query shows no systemd-sysv installed.
   - If systemd is in the closure, find its depender with apt-cache rdepends and report it. Do not hack around it.
   - /sbin/init is absent or is xrginit, and the kernel command line passes init=/sbin/xrginit.
4. Build twice. Compare the sha256 of the rootfs archive and report whether the build is reproducible.
5. Stretch goal, only if the disk image works: an ISO built with grub-mkrescue or xorriso. If it does not boot, say so. Do not claim it does.
</process>

<rules>
- No systemd in the image. If a package forces it, report the depender and stop that step. Reason: a hidden systemd turns the test into a test of systemd.
- Never edit tests/. The boot checks are defined by the spec and the tests, not by the build.
- Fail hard. A build step that prints a warning and continues is a silent failure.
- Each unverified assumption is recorded in PROGRESS.md as NOT VERIFIED, with the command you used to check it.
</rules>

<done_when>
The closure assertions pass. The disk image exists. PROGRESS.md records the image sha256, the closure report and the reproducibility result.
</done_when>

<report>
Under 15 lines: image path, sha256, closure result, reproducibility result, and anything NOT VERIFIED.
</report>
```

#### 3.2 Boot test author (blind to the build and to xrginit source)

```
<role>
You are the author of the boot tests for xInit. You write checks that run inside the booted guest and report what they observed. You never see the image build scripts or the xrginit implementation.
</role>

<context>
Your worktree contains specs/boot.md (the boot behaviour specification, clauses B1 to B8 below) and the harness contract written in that spec. The guest must print results in this exact protocol:

  XINIT-CHECK-BEGIN
  CHECK <id> PASS <detail>
  CHECK <id> FAIL <detail>
  XINIT-CHECK-END

Clauses to cover:
- B1: /proc/1/exe resolves to the xrginit binary. Read it with readlink. Do not rely on a log line.
- B2: /proc, /sys, /dev, /run and /sys/fs/cgroup are mounted with the right filesystem types (findmnt).
- B3: /sys/fs/cgroup/xinit/<unit> exists for every active service, and each has a non-empty cgroup.procs.
- B4: no zombie processes (ps -eo stat finds no Z state).
- B5: a service killed with SIGKILL returns with a new PID within the restart bound from the spec.
- B6: the Manager killed with SIGKILL is restarted. Services keep their PIDs. xrginit status shows them re-adopted.
- B7: xrginit poweroff stops services in reverse dependency order (check the log order), and the guest powers off.
- B8: xrginit reboot prints the marker XINIT-SHUTDOWN reboot before QEMU exits (QEMU is started with -no-reboot, so a reset ends the run).

Rules that apply to every check:
1. Every check prints exactly one CHECK line. A check that produces no line counts as FAIL on the host side.
2. Expected values come from specs/boot.md, not from anything you observe.
3. No sleeps that wait for a condition. Poll with a deadline and fail at the deadline.
4. A check that cannot run because a tool is missing prints FAIL with the name of the tool.
</context>

<rules>
- Write each check as a shell script under tests/boot/guest/. Use plain POSIX sh, with only tools the spec lists.
- Include negative checks where the spec says a thing must not happen (for example, a zombie left behind after the service exits).
- Write host-side tests for the harness parser: a transcript with a missing END marker must fail, and a transcript with a duplicate check ID must fail.
- Do not write checks the spec does not state. List gaps under Untested in specs/boot.md.
- Do not freeze. The orchestrator freezes after review.
</rules>

<done_when>
Every clause B1 to B8 has at least one check. The host-side parser tests cover the cases above. Each check is traced to its clause.
</done_when>

<report>
Under 15 lines: the check list with IDs, the parser test results, Untested items, and any OPEN spec items.
</report>
```

#### 3.3 Integration run and honest report (Opus runs the whole chain)

```
<role>
You are running the end-to-end check for xInit on the Debian image. Your job is to report what happened. You do not make the checks pass.
</role>

<process>
1. Build the image with image/build.sh. If it fails, stop and report the failing step.
2. Probe KVM with test -r /dev/kvm && test -w /dev/kvm. If both pass, run with -accel kvm. Otherwise run with -accel tcg and multiply every timeout by 5. Record which mode ran. (QEMU 5.0+ also accepts "-accel kvm -accel tcg" as an ordered fallback, but you need to know the mode to scale timeouts, so probe explicitly.)
3. Run scripts/qemu-boot-test.sh <image>. Timeouts: boot 180 s, total 600 s (before scaling). The harness passes -nographic, -no-reboot, -m 1024, and -drive file=<image>,format=raw,if=virtio. A clean guest poweroff makes QEMU exit 0; with -no-reboot, a guest reboot also ends the run. The optional -device isa-debug-exit,iobase=0xf4,iosize=0x04 makes QEMU exit with (value << 1) | 1 when the guest writes value to port 0xf4, so an exit code of 0 can never come from it; use it only if the harness needs an explicit pass/fail code, and confirm it with qemu-system-x86_64 -device help. Save the serial log to artifacts/.
4. Parse the CHECK lines. Build a table with one row per clause (B1 to B8), showing PASS, FAIL or NOT RUN, and the evidence line.
5. Run the full harness three times. A check that passes only some of the time is FAIL and is reported as flaky.
6. For each FAIL, record the serial log path, the failing output and a suspected cause. Fix the cause only if it lies in image/ or crates/ and the spec requires the fixed behaviour. Otherwise report it and stop.
7. Run scripts/verify-milestone.sh for the boot feature, if one is defined.
</process>

<rules>
- Never edit tests/boot/, the expected values, or the parser. If a check looks wrong, record it under Spec disputes in PROGRESS.md and leave it as it is.
- A check missing from the output is FAIL. A QEMU exit with no expected markers is FAIL. Do not reinterpret either as a pass.
- Do not add sleeps to make a check pass. If timing is the problem, report the timing evidence.
- A NOT RUN check counts against completion. Do not describe the phase as done while any clause is NOT RUN or FAIL.
- Report failures in plain words. Do not soften them.
</rules>

<done_when>
Every clause has a row with PASS, FAIL or NOT RUN, backed by its log line. The three-run result is recorded. Any failure has a logged cause or a stated reason it was not investigated.
</done_when>

<report>
The B1 to B8 table, the KVM or TCG mode used, the flakiness result, the list of failures with causes, and the artifact paths. Under 25 lines.
</report>
```


## 4. Autopilot: one prompt for the whole build

The per-step prompts above still do the work. The autopilot is a driver session that runs each of them as its own fresh, headless Claude Code session (`claude -p`), in the right folder with the right tool limits, moves files between them, freezes and merges. It stops only for decisions that need you.

### One-time setup (in your terminal)

1. Put this file in the repo so sessions read prompts from disk instead of from long pastes:
   `mkdir -p docs && cp ~/Downloads/"xInit build prompts.md" docs/build-prompts.md` (adjust the download path), then commit it on a branch and merge it like any other change.
2. In CLAUDE.md, under Git and GitHub, change "Do not merge pull requests" to: "Do not merge pull requests, unless you are the autopilot session, which merges only after CI passes."
3. If you added the `.claude/settings.json` from step 0.4, remove `"Bash(gh pr merge:*)"` from its deny list. Role sessions are still kept from merging, because the autopilot launches them with an explicit tool list that excludes it.
4. Run the autopilot inside `tmux` or `screen`, so it survives a closed terminal. A full milestone is dozens of sessions and can take hours.

### The autopilot prompt

```
You are the xInit autopilot. You drive the build described in docs/build-prompts.md, milestone by milestone, by launching a separate headless Claude Code session for every role. You write no specs, tests or implementation yourself: you run commands, move files, check results, freeze, and merge. Show the commands you run and their results.

SETUP CHECKS (stop and report if any fails)
- On main, clean tree, up to date with origin. gh auth status passes. cargo --version matches rust-toolchain.toml and comes from ~/.cargo/bin.
- claude --help lists -p, --model, --permission-mode, --allowedTools, --disallowedTools, --add-dir and --output-format. Record the exact flag spellings in PROGRESS.md under Autopilot and use those.

HOW TO RUN A ROLE
- Extract the template under the named heading in docs/build-prompts.md (sections 1 and 2). Replace {{MILESTONE}} with the milestone number and {{BRIEF}} with that milestone's brief from section 2. Save it as .autopilot/m<n>/<role>.txt (add .autopilot/ to .git/info/exclude).
- Run it as a fresh session: (cd <dir> && claude -p "$(cat <file>)" --model claude-opus-5-5 --permission-mode acceptEdits --allowedTools "<list>" --output-format json) > .autopilot/m<n>/<role>.json. Read the result and the files the role was meant to produce. Never pass one role's transcript to another role.
- Main-repo roles (spec writer, stub writer, implementer, decider) run in the repo on their own branch m<n>/<role>, with tools Read, Edit, Write, Bash(cargo:*), Bash(git:*), Bash(gh pr create:*), Bash(scripts/*). The reviewer gets the same list without Edit and Write.
- Blind roles (test author, hardening) run in a fresh ../xinit-blind made with scripts/make-blind-workspace.sh, with --add-dir ~/xinit-holdout/m<n> /tmp/xinit-scratch and tools Read, Edit, Write, Bash(cargo:*), Bash(mkdir:*), Bash(rm -rf /tmp/xinit-scratch*). Disallow any tool use on the main repo path. For hardening, copy in only the list of missed mutants.

THE LOOP, for M0, M1, M2, M3, then section 3 (Debian image and QEMU), then M4. Skip steps already done; for M0 the spec and stub are done, so start at the test author.
1. Spec writer (1.1). If it leaves OPEN items, run a fresh decider session: it may edit only specs/M<n>.md, must decide each item from the brief, earlier specs and CLAUDE.md, and records each decision with its reason under Resolved. Commit the spec, open a PR, wait for CI with gh pr checks --watch, merge.
2. API stub writer (1.1b). PR, CI, merge.
3. Blind test author (1.2). Copy tests/, specs/red/ and specs/trace/ back on branch m<n>/tests. Run the orchestrator Phase 2 checks (all failures are runtime failures, every test cites a clause, hygiene passes, every clause traced or Untested, holdout dir non-empty and outside the repo). If a check fails, re-run the test author once with the failures listed. Then scripts/freeze-tests.sh M<n>, commit, push. No PR.
4. Implementer (1.3) on m<n>/impl, branched from m<n>/tests. Then run the gate and the holdout suite (cargo test --manifest-path ~/xinit-holdout/m<n>/Cargo.toml, with XRGINIT_BIN set to the gate's fresh build). If anything fails, re-run the implementer with the gate output and, for holdout failures, only the clause IDs that failed, never test names or code. At most 3 implementer runs per milestone.
5. Reviewer (1.4). Send blockers to a new implementer run (at most 2 rounds). Weak tests it flags go to step 6, never to the implementer.
6. Hardening (1.5) in a fresh blind workspace. Copy new tests back, re-freeze, then run the implementer if any new test fails.
7. Open the PR m<n>/impl to main, wait for CI, merge with squash. Mark the milestone done in PROGRESS.md and features/M<n>.json.

STOP AND ASK ME only when: a spec dispute stays unresolved after the decider; the same failure survives 3 implementer runs; something needs root, KVM, a new dependency or a network download the role prompts did not allow; CI fails on main; or an action would rewrite git history or touch anything outside the repo, ../xinit-blind, ~/xinit-holdout and /tmp/xinit-scratch. When you stop, say exactly what you need.

LOG every session in PROGRESS.md under Autopilot: role, milestone, exit status, and one-line outcome. Commit the log with each merge.
```

### What to expect

- **It will stop for you sometimes.** That's by design: those are the decisions that shape the project, or the moments where something is wrong.
- **The decider replaces your spec review.** Read the Resolved sections in each spec as they merge. If you disagree, tell the autopilot, and it reruns from that milestone's tests.
- **Blindness is enforced by folder and tool limits, not by trust.** The test author runs in a copy without the implementation, with shell access limited to `cargo`, `mkdir` and its scratch folder.
- **Cost and limits.** Every role is a full Opus session, so expect heavy usage and occasional rate-limit waits. Usage-plan limits may pause it partway; it resumes from PROGRESS.md.
- **Section 3 will stop.** Building a bootable image needs loop devices or root. The autopilot asks you at that point rather than working around it.

## Verify before relying

Most of the open items from the first draft are now confirmed. The table shows what changed and what still needs a runtime check.

| Item | Status | Source | How the prompts handle it |
|---|---|---|---|
| `cgroup.kill` kernel version | **Confirmed: Linux 5.14.** Merged into `cgroup/for-5.14` in May 2021 | [Tejun Heo, lore.kernel.org](https://lore.kernel.org/containers/YJlGSZlZYaaujm2O@slm.duckdns.org/) | M2 still probes for the file, because the target kernel may be older |
| `CLONE_INTO_CGROUP` from Rust | **Confirmed approach.** std pidfd support is nightly-only, so use `clone3` via rustix or libc; fallback writes `0` to a pre-opened `cgroup.procs` | [std linux_pidfd](https://doc.rust-lang.org/std/os/linux/process/trait.CommandExt.html), [clone3(2)](https://man7.org/linux/man-pages/man2/clone3.2.html) | M2 probes both paths and logs which one ran |
| Writing `0` to `cgroup.procs` means "self" | **Partly confirmed.** Documented for cgroup v1 attach files; v2 not stated in the docs read | [kernel doc commit](https://gitlab.eclipse.org/eclipse/oniro-core/linux/-/blob/26b33d467b50e0a721cd69610a5ddc38c3279f31/Documentation/cgroups.txt) | M2 spec must include a v2 test, with a parent-side fallback |
| QEMU `isa-debug-exit` | **Confirmed.** Exit status is `(value << 1) \| 1`; usual flags `iobase=0xf4,iosize=0x04` | [QEMU debugexit.c](https://sources.debian.org/src/qemu/1%3A10.0.3%2Bds-4/hw/misc/debugexit.c), [qemu-exit](https://github.com/rust-embedded/qemu-exit) | Optional in 3.3; serial markers stay primary |
| QEMU KVM to TCG fallback | **Confirmed.** `-accel kvm -accel tcg` works on QEMU 5.0+; `-accel kvm:tcg` is rejected | [libvirt list](https://listman.redhat.com/archives/libvir-list/2021-January/214038.html), [kunit thread](https://lkml.iu.edu/hypermail/linux/kernel/2411.0/00727.html) | 3.3 probes `/dev/kvm` to know which mode ran |
| cargo-mutants exit codes and `--in-diff` | **Confirmed.** 0 caught, 2 missed, 3 timeout, 4 baseline failed; `--in-diff FILE` | [exit codes](https://mutants.rs/exit-codes.html), [in-diff](https://mutants.rs/in-diff.html) | Written into 1.5 |
| systemd kept out of the Debian image | **Confirmed with a trap.** minbase excludes Priority important packages; `udev` pulls in `systemd` unless `systemd-standalone-sysusers` is included | [piuparts udev](https://piuparts.debian.org/trixie2next/pass/udev_257.8-1~deb13u2.log), [piuparts systemd-sysv](https://piuparts.debian.org/trixie2next/pass/systemd-sysv_257.13-1~deb13u1.log) | 3.1 includes it and asserts the closure |
| `systemd-udevd` without systemd as PID 1 | **Confirmed supported.** udev's SysV script moved to `initscripts`; one unresolved report of a hang on external-drive mounts | [Debian bug 1042082](https://www.chiark.greenend.org.uk/pipermail/debian-init-diversity/2023-September/006266.html), [Dec 2025 report](https://www.chiark.greenend.org.uk/pipermail/debian-init-diversity/2025-December/007944.html) | 3.1 writes udevd, trigger and settle units |
| Restart defaults | **Confirmed.** dinit upstream: 0.2 s, 3 restarts in 10 s; Chimera raises to 1 s and 5. Pebble: 0.5 s doubling, 30 s cap | [Chimera patch](https://git.omada.cafe/chimera-linux/cports/src/branch/linux72/main/dinit/patches/restart-interval.patch), [Pebble](https://documentation.ubuntu.com/pebble/reference/service-auto-restart/) | M1 suggests 500 ms, doubling, 30 s cap |
| systemd ordering-cycle behaviour | **Confirmed from logs.** Logs the cycle, deletes a job to break it | [Arch FS#40972](https://bugs.archlinux.org/task/40972) | xInit refuses cycle members instead |
| PR_SET_CHILD_SUBREAPER | **Confirmed.** Linux 3.4+; survives exec, not inherited by children | [PR_SET_CHILD_SUBREAPER(2const)](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html) | M3 sets it once per Manager start (first draft wrongly said "after every exec") |
| `mmdebstrap` `path-exclude` syntax, GRUB into a loop image, reproducibility | **Still open** | — | 3.1 probes and records results |
| Service stop order in s6 and dinit | **Still open** | — | Reverse dependency order is a spec choice |
| Crate versions (nix, rustix, serde, toml, proptest) | **Still open** | — | Prompts require `cargo info` before pinning |
| Opus 5.5 behaviour | **Assumption.** Prompts follow Anthropic's general Claude guidance, not a model-specific card | [Prompting best practices](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/be-clear-and-direct) | Re-test the first milestone and adjust |
| Disk image vs ISO | **Your call.** Disk image is the tested artifact; ISO is a stretch goal | — | Confirm before running phase 3 |

## Conclusion

The novel point here is the order of operations. A test suite written after the implementation mostly describes the implementation, so the prompt set reverses the order: the spec comes first, the blind author writes the suite against a stub, and only then does the implementer start. A milestone is finished only when the frozen suite, the holdout suite, the mutation pass and a fresh reviewer all agree. The research leaves the Debian and QEMU phase less settled, so the prompt set treats its probes as results to report, not as facts to build on.

Run the milestones in order and keep each session to one milestone, because the notes report that context degrades as it fills ([Claude Code best practices](https://code.claude.com/docs/en/best-practices.md)). Expect the first real surprises at M2, where the cgroup probes and the Manager placement rule meet the kernel for the first time, and at 3.1, where the closure assertions are most likely to fail. Those two steps are where the verification evidence will be most useful.

## Ready-to-paste prompts

These are the section 1 and 2 prompts with every placeholder already filled in for the current step. Paste each one whole into a fresh session.

### 0.4 follow-up: make the repo public and protect main

Paste into the 0.4 session (or a fresh one in the repo).

```
I've decided to make the repository public so branch protection is available.

1. Before changing visibility, check the history for anything that should not be public: run git log --format='%an <%ae>' | sort -u and report the author names and emails, and search the tree and history for files that look like secrets (.env, *.key, tokens). Report findings and stop if anything looks secret. Do not rewrite history yourself.
2. If nothing secret is found, make the repo public: gh repo edit --visibility public --accept-visibility-change-consequences.
3. Apply the branch protection from step 5 of the setup prompt to main: require the verify workflow to pass, require a pull request, block force pushes and deletion. Report the API result.
4. Confirm the chore/github-workflow pull request still exists and its CI run passed.

Report in under 10 lines: the emails found, visibility result, protection result, and the pull request status.
```

### M0 · 1.1 Spec writer

```
<role>
You are the specification writer for xInit milestone 0. You write what the system must do, as observable behaviour. You write no code.
</role>

<context>
xInit is a PID 1 init and service manager. A different agent that sees only your spec and an API stub will later write the tests. So the spec must be complete enough to test without reading any implementation, and must not depend on implementation choices.

Architecture (from CLAUDE.md): Core is PID 1, synchronous, never exits. Manager is a restartable child of Core. Services are tracked by pidfd and cgroup.

Milestone brief:
Milestone M0: xrginit as an ordinary process supervising one service.

Goal: xrginit runs as an unprivileged process (not PID 1) and supervises one simple service described by a TOML unit file.

In scope:
- Unit file with schema_version = 1. Sections: [service] (name, description, type, provides), [execution] (command as an argv array, executed directly with no shell; user, optional), [dependencies] (parsed, not used yet), [restart] (parsed, not used yet).
- Accept only type = "simple" in M0. Any other type is a typed error.
- Start the service and track it by pidfd. Reap it. Report states: starting, running, exited, failed.
- Stop: send SIGTERM, wait for the stop timeout (the spec sets it; suggested default 5 s), then SIGKILL.
- Signals handled with signalfd or a self-pipe. Only signals the process installs a handler for.
- Structured log lines to stderr: timestamp, level, unit, event, key=value pairs.
- Child setup order before exec: reset signal mask and dispositions, setsid, setgroups, setgid, setuid, close unwanted file descriptors, exec.
- Suggested CLI: xrginit run <unit-file>. The spec may rename it.

Non-goals: PID 1 behaviour, cgroups, mounts, sockets, readiness, dependencies, restart backoff.

Edge cases the spec must cover:
- Command path does not exist: start fails with a typed error and leaves no zombie.
- Child ignores SIGTERM: SIGKILL after the timeout, and the exit signal is recorded.
- Child double-forks a daemon: M0 tracks only the direct child. State this limitation in the spec. Cgroups in M2 solve it.
- Signalling a pidfd after its child was reaped fails. It must never reach a reused PID. Test by spawning, reaping, then signalling.
- Unknown keys in a unit file: the spec must say whether they are errors. Recommend errors, so a typo does not silently disable a setting.

Finishing condition: verify-milestone M0 exits 0, and a person can run xrginit run on a sample unit and see the states change.

Dependencies allowed: serde and toml for parsing, plus one syscall crate (rustix or libc). Check current versions with cargo info before pinning. Versions are not verified in the research notes.

Test harness facts the spec must state under Public surface:
- Tests locate the binary only through the XRGINIT_BIN environment variable (absolute path), via the xrginit_bin() helper in tests/common/mod.rs. See CLAUDE.md.
- The gate builds xrginit fresh and sets XRGINIT_BIN itself. In the blind test workspace no implementation exists, so tests that run the binary fail at the helper. That is the expected red result, not a defect.
- M0's observable surface is the CLI (xrginit run <unit-file>), exit codes, and structured log lines on stderr. Specify their exact formats with worked examples, because the test author will assert on them.

Earlier specs in specs/ still apply. Read them all.
</context>

<process>
1. Read the brief and every earlier spec.
2. Write specs/M0.md with these sections: Scope, Non-goals, Public surface (names from xinit-api and their intended behaviour), Behaviour clauses, Error behaviour, Invariants, Open questions.
3. Give each behaviour clause an ID such as B-M0-007, a Given/When/Then statement, and at least one worked example with exact input and exact expected output. Exact values let a blind agent write oracle assertions without guessing.
4. Cover every edge case in the brief. For each clause, say what happens on failure.
5. Where the brief is ambiguous or two rules conflict, write the question under Open questions and mark the clause OPEN. Do not choose silently.
</process>

<rules>
- Specify observable behaviour only: exit codes, files written, unit states, log lines, IPC replies, cgroup contents. Reason: a blind test author can test only what is visible.
- Do not name private functions or describe algorithms. Reason: implementation hints leak into tests, and then the tests describe the implementation.
- Every requirement must be checkable with a yes or no test. Phrases such as "should be robust" are not requirements.
- Do not add features the brief does not list. Reason: each extra clause is more work for the implementer and the reviewer, and it inflates scope.
- Keep the spec under 400 lines.
</rules>

<done_when>
Every clause has an ID and an example. The open-questions list is complete. You have written no code.
</done_when>

<report>
Three lines: the spec path, the number of clauses, the number of OPEN items.
</report>
```

### M0 · 1.1 follow-up: resolve the OPEN items and review fixes

Paste into the same spec-writer session (or a fresh one, telling it to edit `specs/M0.md`). This is based on a review of the draft spec: it settles Q1 to Q4 and fixes problems that would have broken frozen tests later.

```
Apply the following to specs/M0.md. Keep everything not mentioned unchanged. Move Q1 to Q4 to a Resolved section with the decision, remove the OPEN markers, and reply with the three-line report again plus a list of changed clause IDs.

RESOLVED QUESTIONS

Q1 / B-M0-010: [dependencies] allows exactly two keys, requires and after, each an array of unit-name strings matching the service.name pattern, both optional, default empty. [restart] allows exactly two keys: policy, one of "no", "on-failure", "always", default "no"; and maximum_restarts, a non-negative integer, default 0. In M0 both are parsed and validated (wrong type is InvalidType, a bad policy value or a negative count is InvalidValue, an unknown key is UnknownKey) and have no effect on behaviour. Add worked examples for each, and add these fields to the xinit-api surface (Unit gains dependencies: DependenciesSection and restart: RestartSection, with a RestartPolicy enum) so tests can check the parsed values.

Q2 / B-M0-026: Credential handling, decided by comparing the target user's uid and primary gid (from /etc/passwd) with xrginit's effective uid and gid:
- user absent: no credential change (as B-M0-024).
- target uid and gid equal xrginit's: no credential change at all, including no setgroups; supplementary groups stay inherited. The service runs. This is testable unprivileged using the name of the user running the test.
- otherwise: setgroups (groups from /etc/group), setgid, setuid, in that order, in the child before exec. If any call fails, the child exits before exec, the start fails with error=credentials errno=<name> (EPERM when unprivileged), StartError::Credentials, exit 4, and no process is left behind. Never fall back to running as the current user.
- Test unprivileged: a different existing user (for example root or nobody, whichever exists and differs from the test user) gives credentials/EPERM. The privileged success path is specified but marked "requires root"; such a test must fail with a message naming the requirement when not root, never skip.

Q3: all defaults accepted, with the changes below.

Q4: a separate API stub writer step writes the xinit-api stub from Public surface after this spec is committed. State that in Public surface, and make sure every type, field, variant and signature a test needs is listed exactly.

FIXES (these protect frozen tests from breaking in later milestones or on other machines)

F1. Values later milestones will make valid. Frozen M0 tests stay in the suite through M4, so a test must never use a value a later milestone will accept. B-M0-007: replace "forking" with "xinit-m0-bogus" (M2 adds forking; oneshot, notify and target arrive in M1); keep "Simple" as the case-sensitivity example. B-M0-004: unknown-key examples must use names no milestone will add, such as xinit_m0_bogus inside a section and a top-level [xinit_m0_bogus] table; replace [timers] (M4 may add timer tables). B-M0-011: test only "frobnicate" as an unknown subcommand and state that the behaviour of other subcommand names (for example manager, which the Core uses later) is unspecified in M0. Add an Invariants note: a later spec may supersede an M0 clause only by naming its ID; superseded tests are then retired by the orchestrator, never edited.

F2. Double fork test would hang. In B-M0-019 the grandchild inherits xrginit's stderr, so a test that reads stderr to EOF blocks for 30 s. Change the command to ["/bin/sh","-c","/usr/bin/sleep 30 </dev/null >/dev/null 2>&1 & echo $! > \"$1\"; exit 0","sh","<pidfile>"], where the test supplies <pidfile>, reads the grandchild pid from it, checks it is alive, then kills it. Add a general note: tests wait for xrginit to exit and then read its output; they never rely on EOF on a pipe the service may share.

F3. Portable binary checks. B-M0-013: /proc/<P>/exe must resolve to the same file as realpath("/usr/bin/sleep"), not the literal string, because on merged-/usr or busybox systems they differ. State the test prerequisites under Public surface: /bin/sh, /usr/bin/sleep, /usr/bin/false and /proc must exist; a missing one fails the test with a message naming it.

F4. Child detection without CONFIG_PROC_CHILDREN. B-M0-016 and B-M0-032 use /proc/self/task/*/children, which depends on a kernel option. Replace with: no entry in /proc/[0-9]*/stat has field 4 (ppid) equal to the test process's pid, other than processes the test itself started on purpose.

F5. Reaped means gone or replaced. B-M0-029: "no longer refers to that process" must be checkable: /proc/<pid> is absent, or its start time (field 22 of /proc/<pid>/stat) differs from the value read before wait.

F6. Wall-clock timestamps can step backwards (NTP, manual changes), so drop "timestamps do not decrease". Timing assertions (B-M0-017, B-M0-018, B-M0-031) are measured by the test with its own monotonic clock around observable events, not from log timestamps. Keep the 5000 to 6000 ms window for B-M0-018. Widen "within 1000 ms" in B-M0-017 and B-M0-031 to 2000 ms so a loaded machine does not cause flaky failures.

F7. Bytes outside ASCII in log values. In B-M0-028 VALUE rules, add: bytes 0x7F and above, and invalid UTF-8, are written as \xHH inside a quoted value. A path containing such bytes is therefore always quoted.

F8. Service trait derives. Service derives Debug only (no Clone, PartialEq or Eq), because it owns a pidfd. State that Service has private fields so tests cannot construct it; they obtain one only from Service::start.

F9. SIGHUP. Add to Error behaviour: in M0, SIGHUP keeps its default action, so closing xrginit's terminal kills xrginit and leaves the service running in its own session, unsupervised. This is a known M0 limitation, not a bug, and no test asserts it either way.

F10. Error traits. UnitError, StartError, SignalError and WaitError implement std::fmt::Display and std::error::Error. The Display text is unspecified and tests must not assert it (like detail in logs); tests compare the enum values.

F11. Copy derives. Signal, ServiceType, RestartPolicy and Termination also derive Copy, since they are small plain values.

F12. Record in Public surface that the xinit-api stub already exists from a draft of this spec (commit e3b39b4) and will be updated by the stub writer after this revision, so the listed surface is the one that counts.
```

### M0 · 1.1 final touch, then commit

Paste into the same spec-writer session.

```
Three last changes, then commit.

1. B-M0-031: widen "less than 1300 ms" to "less than 2300 ms", consistent with F6. Leave the "at least 300 ms" lower bound.
2. Your F2 reading is correct: tests may read stderr line by line, with a deadline, to wait for the running line before sending a signal; the end of a run is detected only by xrginit exiting. Add one sentence to Test harness facts: once a test starts reading, it keeps draining stderr until xrginit exits, so a full pipe can never block xrginit.
3. features/M0.json: dissolve the M0-open feature. Put B-M0-010 with the unit-loading feature and B-M0-026 with the service-environment feature (or whichever existing features fit best), all passes false. In PROGRESS.md, change the M0 row from "2 OPEN" to "spec final, 32 clauses, 0 OPEN".

Then commit specs/M0.md, features/M0.json and PROGRESS.md together with the message spec(M0): final spec, Q1-Q4 resolved, review fixes F1-F12. Reply with the commit hash.
```

### M0 · 1.1b API stub writer

Run in a fresh session after the revised `specs/M0.md` is committed. It updates the draft stub from e3b39b4 to match.

```
<role>
You are the API stub writer for xInit milestone 0. You turn the Public surface section of the spec into Rust signatures with no behaviour. You write no logic and no tests.
</role>

<context>
specs/M0.md is committed. Its Public surface section lists the types and functions the blind test author may call. The test author will compile against crates/xinit-api and never see the implementation, so the stub must match the spec exactly and contain nothing else.
</context>

<process>
0. Precondition: specs/M0.md is committed (git log shows it) and contains no clause marked OPEN. If either is false, stop and report it; do not write a stub from a draft.
1. Read specs/ARCHITECTURE.md and the Public surface section of every spec up to M0. If crates/xinit-api already has items from an earlier draft, update them to match the spec exactly and remove anything the spec no longer names.
2. In crates/xinit-api, add each public type, enum, error type and function the spec names, with the exact names, fields, variants and signatures it gives. Every function body is unimplemented!(). Each item gets a doc comment citing its clause IDs.
3. Error types are real enums with the variants the spec lists, because tests match on them. Data types are real structs with the fields the spec lists. Only function bodies are stubbed.
4. Run cargo build --locked and cargo clippy --locked -p xinit-api -- -D warnings. Both must pass.
5. In PROGRESS.md, replace the outdated "xinit-api stub not yet written (Q4)" note in the M0 row with "xinit-api stub matches final spec 3ca7927".
6. Following the Git and GitHub section of CLAUDE.md: work on the branch m0/stub, commit with the message stub(M0): public API signatures, push, and open a pull request with your report as its body. Do not merge.
</process>

<rules>
- Add nothing the spec does not name: no helpers, no private modules, no extra variants, no default implementations that do work. Reason: anything extra leaks implementation into the blind tests.
- If the spec's Public surface is missing something a test would need (a type with no fields listed, a function with no signature), stop and list it as a question. Do not invent it. Reason: the spec, not the stub, is the contract.
- Do not touch crates/xinit-core, crates/xinit-manager, tests/ or specs/.
</rules>

<report>
Under 10 lines: items added with their clause IDs, build and clippy results, and any questions for the spec.
</report>
```

### M0 · 1.2 Blind test author

**Before pasting**, in your terminal, from the main repo with PR #2 merged and `git pull` done:

```bash
mkdir -p ~/xinit-holdout/m0
scripts/make-blind-workspace.sh ../xinit-blind
cd ../xinit-blind && cargo test --offline --test m0; cd -   # fails now only because no m0 tests exist yet
```

Then open a **fresh session whose working directory is `../xinit-blind`** (not the main repo), with `XINIT_HOLDOUT_DIR=$HOME/xinit-holdout/m0` exported in that shell, and paste:

```
<role>
You are the test author for xInit milestone 0. You never see the implementation, and that is the point of this session. Your tests express the spec. They must catch a wrong implementation, not agree with one.
</role>

<context>
Your workspace is a separate copy made by scripts/make-blind-workspace.sh. It contains specs/ (every spec up to and including this milestone), crates/xinit-api (public signatures whose bodies are unimplemented!()), the root xinit-tests package and any earlier frozen tests. It does not contain the implementation, and it has no git remote: do not push or open pull requests from it; the orchestrator copies your output back. If you find implementation code anywhere, stop and report it. Do not read it.

Test layout and harness (see CLAUDE.md, Test layout): this milestone's tests go in tests/m0/, reached from the entry file tests/m0.rs; the gate fails on any test file the compiler never reaches. Tests that run the binary get its path only from xrginit_bin() in tests/common/mod.rs, which reads the XRGINIT_BIN environment variable at run time and panics with a message naming it if the variable is unset, relative, or not executable. Never fall back to target/debug or $PATH. If tests/common/mod.rs does not exist yet, writing it is your job. In this workspace no binary exists, so binary tests fail at the helper: that is the expected red result, not a compile problem to fix.

Why blind: tests inferred from code tend to validate the code's bugs. Your independence from the implementation is what gives the suite its value.
</context>

<rules>
1. Each test cites its clause in a comment, for example // spec: B-M0-007, and its name contains the clause ID.
2. Expected values come from the spec's worked examples, or from values you compute by hand, with the arithmetic in a comment. Never compute an expected value by calling the API under test. Reason: an oracle such as assert_eq!(f(x), f(x)) passes for every implementation.
3. Every test must fail against the stub. Run cargo test on the stub and save the output to specs/red/M0.txt. A test that passes against the stub is a defect, unless it is a negative test whose clause says "must not do X" and which still fails on the stub. Fix or remove any other passing test and say so.
4. Forbidden: #[ignore], assert!(true), tests that always return Ok, should_panic without an expected message, and sleeps that wait for a condition. Reason: each one lets an unfinished or wrong implementation pass, or hides a race. For timing (restart backoff, timeouts), use the injectable clock in the API or deterministic channels, as the spec describes.
5. A skip is a failure. If a test needs something the environment lacks (root, cgroup delegation, pidfd support), it must fail with a message naming the requirement. Reason: a skipped test reports green and hides the gap.
6. Do not add dependencies. M0 has no restart or ordering behaviour to model, so property-based and state-machine tests start in M1. Reason: the blind workspace builds offline, and every dependency is unreviewed code.
7. Write a second, black-box suite directly in the directory named by $XINIT_HOLDOUT_DIR (outside this workspace; never inside it). Make it a standalone Cargo package (its own Cargo.toml and tests/, no dependency on this workspace) runnable with cargo test --manifest-path "$XINIT_HOLDOUT_DIR/Cargo.toml". It finds the binary through XRGINIT_BIN with the same no-fallback rule as xrginit_bin(), runs it as a process with its own fixture unit files, and checks exit codes, stderr log lines and /proc state. Use different fixtures and inputs from the main suite. If XINIT_HOLDOUT_DIR is unset, stop and report it. Reason: the implementer never sees it, so the implementation cannot be tuned to it.
8. Self-check: in a scratch directory outside the workspace (for example /tmp/xinit-scratch), write three plausible wrong implementations of the most complex clause. Confirm your tests fail for each one. Delete the scratch directory and record the results in specs/red/M0.txt.
9. Do not test behaviour the spec does not state. List the gap under Untested in specs/trace/M0.md.
10. Frozen tests stay in the suite for every later milestone. For negative cases (unknown keys, unsupported types, unknown subcommands), use values no later milestone could make valid, such as xinit_m0_bogus. Reason: a test asserting that "forking" is rejected would break the day M2 adds forking.
11. Wait for the process under test to exit, then read its output. Never wait for EOF on a pipe a service or its children may still hold. Reason: a background grandchild keeps the pipe open and the test hangs.
</rules>

<process>
1. Read the spec and earlier specs.
2. Write tests into tests/m0/, one file per spec section.
3. Run the red check and save the output.
4. Write specs/trace/M0.md: each clause ID mapped to its test names, plus an Untested list.
5. Do not freeze. The orchestrator freezes after review.
</process>

<done_when>
Every clause is traced or listed as Untested. The red output shows assertion failures, not compile errors (if the suite fails to compile, fix the test, not the stub). The self-check results are recorded.
</done_when>

<report>
Under 15 lines: test count per clause group, red result, self-check result, Untested clause IDs, and any OPEN spec items that block testing.
</report>
```

**After it finishes**, copy the output back to the main repo on a branch, and open a pull request:

```bash
cd ~/Documents/Projects/xInit && git checkout main && git pull && git checkout -b m0/tests
rsync -a --delete ../xinit-blind/tests/ tests/
rsync -a ../xinit-blind/specs/red/ specs/red/
rsync -a ../xinit-blind/specs/trace/ specs/trace/
git add tests specs/red specs/trace && git commit -m "test(M0): blind test suite"
git push -u origin m0/tests
```

Push the branch as a backup, but **do not open a pull request for it**. These tests fail by design until the implementation exists, so CI on main would be red and branch protection would (rightly) block the merge. The tests reach main together with the implementation: you freeze them on this branch, the implementer works on top of it, and one pull request merges both once CI is green.

Skim the tests now (each cites a clause; the red output shows assertion failures, not compile errors), then send me the test author's report.

### M0 orchestrator (does steps 1.2 to the freeze for you)

Paste into a fresh session in your main repo. It runs all the commands itself and stops only when it needs you.

```
You are the orchestrator for xInit milestone M0, working in the main repo. You do the git and file plumbing between steps. You never write specs, tests or implementation code, and you never read files under ../xinit-blind except the output paths named below. Run every command yourself and show its output. If any command fails, stop and report it.

PHASE 1: prepare the blind workspace (do this now)
1. git checkout main && git pull. Delete the stale local branch m0/stub with git branch -D (ignore "not found"), and delete origin/m0/stub if it still exists.
2. git log --oneline -1 must show the stub(M0) commit 8115136 or later. Stop if not.
3. mkdir -p ~/xinit-holdout/m0
4. Check that cargo --version reports the toolchain pinned in rust-toolchain.toml and that which cargo points into ~/.cargo/bin; stop and report if not. Remove ../xinit-blind if it exists from an earlier attempt. Run scripts/make-blind-workspace.sh ../xinit-blind, then (cd ../xinit-blind && cargo test --offline --test m0). The cargo command must fail because no m0 test target exists yet; report its last lines.
5. Write the text between the BEGIN and END markers below, exactly, to ../xinit-blind/PROMPT.txt.
6. Tell me, then stop and wait:
   "Phase 1 done. In a new terminal run:
      cd <absolute path of ../xinit-blind> && XINIT_HOLDOUT_DIR=$HOME/xinit-holdout/m0 claude
    and type: Read PROMPT.txt and follow it exactly.
    It will ask permission to write to ~/xinit-holdout/m0 (the hidden holdout suite) and /tmp/xinit-scratch; allow those. Refuse any request to read the main xInit repo.
    When it reports back, tell me: tests done."
   Fill in the absolute path yourself so I can copy the command as is.

PHASE 2: bring the tests back (only when I say "tests done")
1. git checkout main && git pull && git checkout -b m0/tests
2. rsync -a --delete ../xinit-blind/tests/ tests/ ; rsync -a ../xinit-blind/specs/red/ specs/red/ ; rsync -a ../xinit-blind/specs/trace/ specs/trace/
3. Confirm nothing outside tests/, specs/red/ and specs/trace/ changed (git status). Confirm ~/xinit-holdout/m0 is non-empty and that no holdout file is inside the repo.
4. Run cargo test --locked --test m0 and report: total tests, how many fail, and whether every failure is a runtime failure (assertion or the XRGINIT_BIN helper), with zero compile errors.
5. Check the suite against the rules: every test cites a B-M0 clause; no #[ignore], assert!(true), sleeps or should_panic without a message (scripts/check-hygiene.sh); every clause in specs/M0.md appears in specs/trace/M0.md or under Untested. Report any gaps.
6. Commit as test(M0): blind test suite and git push -u origin m0/tests. Do not open a pull request: the tests fail by design until the implementation exists.
7. Stop and give me a short summary for my review, then wait for "freeze" or for changes.

PHASE 3: freeze (only when I say "freeze")
1. On m0/tests, run scripts/freeze-tests.sh M0, then scripts/check-frozen.sh. Commit as test(M0): freeze suite, and push.
2. Tell me: "Frozen. Next is the implementer, in a fresh session on a branch from m0/tests." Then stop.

BEGIN
<role>
You are the test author for xInit milestone 0. You never see the implementation, and that is the point of this session. Your tests express the spec. They must catch a wrong implementation, not agree with one.
</role>

<context>
Your workspace is a separate copy made by scripts/make-blind-workspace.sh. It contains specs/ (every spec up to and including this milestone), crates/xinit-api (public signatures whose bodies are unimplemented!()), the root xinit-tests package and any earlier frozen tests. It does not contain the implementation, and it has no git remote: do not push or open pull requests from it; the orchestrator copies your output back. If you find implementation code anywhere, stop and report it. Do not read it.

Test layout and harness (see CLAUDE.md, Test layout): this milestone's tests go in tests/m0/, reached from the entry file tests/m0.rs; the gate fails on any test file the compiler never reaches. Tests that run the binary get its path only from xrginit_bin() in tests/common/mod.rs, which reads the XRGINIT_BIN environment variable at run time and panics with a message naming it if the variable is unset, relative, or not executable. Never fall back to target/debug or $PATH. If tests/common/mod.rs does not exist yet, writing it is your job. In this workspace no binary exists, so binary tests fail at the helper: that is the expected red result, not a compile problem to fix.

Why blind: tests inferred from code tend to validate the code's bugs. Your independence from the implementation is what gives the suite its value.
</context>

<rules>
1. Each test cites its clause in a comment, for example // spec: B-M0-007, and its name contains the clause ID.
2. Expected values come from the spec's worked examples, or from values you compute by hand, with the arithmetic in a comment. Never compute an expected value by calling the API under test. Reason: an oracle such as assert_eq!(f(x), f(x)) passes for every implementation.
3. Every test must fail against the stub. Run cargo test on the stub and save the output to specs/red/M0.txt. A test that passes against the stub is a defect, unless it is a negative test whose clause says "must not do X" and which still fails on the stub. Fix or remove any other passing test and say so.
4. Forbidden: #[ignore], assert!(true), tests that always return Ok, should_panic without an expected message, and sleeps that wait for a condition. Reason: each one lets an unfinished or wrong implementation pass, or hides a race. For timing (restart backoff, timeouts), use the injectable clock in the API or deterministic channels, as the spec describes.
5. A skip is a failure. If a test needs something the environment lacks (root, cgroup delegation, pidfd support), it must fail with a message naming the requirement. Reason: a skipped test reports green and hides the gap.
6. Do not add dependencies. M0 has no restart or ordering behaviour to model, so property-based and state-machine tests start in M1. Reason: the blind workspace builds offline, and every dependency is unreviewed code.
7. Write a second, black-box suite directly in the directory named by $XINIT_HOLDOUT_DIR (outside this workspace; never inside it). Make it a standalone Cargo package (its own Cargo.toml and tests/, no dependency on this workspace) runnable with cargo test --manifest-path "$XINIT_HOLDOUT_DIR/Cargo.toml". It finds the binary through XRGINIT_BIN with the same no-fallback rule as xrginit_bin(), runs it as a process with its own fixture unit files, and checks exit codes, stderr log lines and /proc state. Use different fixtures and inputs from the main suite. If XINIT_HOLDOUT_DIR is unset, stop and report it. Reason: the implementer never sees it, so the implementation cannot be tuned to it.
8. Self-check: in a scratch directory outside the workspace (for example /tmp/xinit-scratch), write three plausible wrong implementations of the most complex clause. Confirm your tests fail for each one. Delete the scratch directory and record the results in specs/red/M0.txt.
9. Do not test behaviour the spec does not state. List the gap under Untested in specs/trace/M0.md.
10. Frozen tests stay in the suite for every later milestone. For negative cases (unknown keys, unsupported types, unknown subcommands), use values no later milestone could make valid, such as xinit_m0_bogus. Reason: a test asserting that "forking" is rejected would break the day M2 adds forking.
11. Wait for the process under test to exit, then read its output. Never wait for EOF on a pipe a service or its children may still hold. Reason: a background grandchild keeps the pipe open and the test hangs.
</rules>

<process>
1. Read the spec and earlier specs.
2. Write tests into tests/m0/, one file per spec section.
3. Run the red check and save the output.
4. Write specs/trace/M0.md: each clause ID mapped to its test names, plus an Untested list.
5. Do not freeze. The orchestrator freezes after review.
</process>

<done_when>
Every clause is traced or listed as Untested. The red output shows assertion failures, not compile errors (if the suite fails to compile, fix the test, not the stub). The self-check results are recorded.
</done_when>

<report>
Under 15 lines: test count per clause group, red result, self-check result, Untested clause IDs, and any OPEN spec items that block testing.
</report>
END
```
