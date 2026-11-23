//! specs/M0.md, "Service process environment": B-M0-020 to B-M0-026, checked through
//! /proc/<P> while the service is running, and the API parts of B-M0-025 / B-M0-026
//! with the no-leftover-child check of B-M0-032.

use super::common::{
    EXIT_DEADLINE, Finished, PasswdEntry, Run, SIGKILL, SIGTERM, TempDir, await_ready,
    child_lock_exclusive, child_lock_shared, children_of_self, field, fifo, passwd_entries,
    self_euid_egid, status_mask, status_value, unit_toml, xrginit_bin,
};
use std::collections::BTreeSet;
use std::fs;
use std::process::{Command, Stdio};
use xinit_api::{ExecutionSection, Service, Signal, StartError, Termination};

/// Ends a run whose service ends on SIGTERM (`/usr/bin/sleep 60`, or the B-M0-023 shell
/// that exits 0 on TERM) with a requested stop.
fn stop_sleeper(mut r: Run, name: &str) -> Finished {
    r.signal_xrginit(SIGTERM);
    let d = r.finish(EXIT_DEADLINE);
    assert_eq!(d.code, Some(0), "requested stop of {name}\n{}", d.text);
    d.check_run_invariants(name);
    d
}

fn fd_entries(pid: u32) -> BTreeSet<String> {
    fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap_or_else(|e| panic!("read /proc/{pid}/fd: {e}"))
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

// --- B-M0-020 -------------------------------------------------------------

// spec: B-M0-020
#[test]
fn b_m0_020_argv_passed_verbatim() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let argv = ["/bin/sh", "-c", "/usr/bin/sleep 60; exit 0", "$HOME", "a b"];
    let p = t.write("argv.toml", &unit_toml("argv", &argv, ""));
    let mut r = Run::unit(&p);
    let pid = r.wait_running();
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).expect("cmdline");
    // End the service and its sleep (same process group, pgid == P per B-M0-022).
    // Both calls panic, naming the pid, unless it was verified as xrginit's child.
    let _ = r.signal_service_group(SIGKILL);
    let _ = r.signal_service(SIGKILL);
    let d = r.finish(EXIT_DEADLINE);
    // Expected bytes written out by hand from the spec.
    let want: &[u8] = b"/bin/sh\0-c\0/usr/bin/sleep 60; exit 0\0$HOME\0a b\0";
    assert_eq!(
        cmdline,
        want,
        "B-M0-020: cmdline {:?}",
        String::from_utf8_lossy(&cmdline)
    );
    d.check_run_invariants("argv");
}

// --- B-M0-021 -------------------------------------------------------------

fn assert_clean_signal_state(pid: u32) {
    for k in ["SigBlk", "SigIgn", "SigCgt"] {
        assert_eq!(
            status_mask(pid, k),
            0,
            "B-M0-021: service {k} must be 0000000000000000"
        );
    }
}

// spec: B-M0-021
#[test]
fn b_m0_021_signal_state_reset_plain_start() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write("s.toml", &unit_toml("sigs", &["/usr/bin/sleep", "60"], ""));
    let mut r = Run::unit(&p);
    let pid = r.wait_running();
    assert_clean_signal_state(pid);
    stop_sleeper(r, "sigs");
}

// spec: B-M0-021 ("whatever mask and dispositions xrginit itself has": xrginit
// inherits ignored HUP, QUIT, USR1, USR2 and TSTP)
#[test]
fn b_m0_021_signal_state_reset_with_inherited_ignores() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write("s.toml", &unit_toml("sigs2", &["/usr/bin/sleep", "60"], ""));
    let mut c = Command::new("/bin/sh");
    c.arg("-c")
        .arg("trap '' HUP QUIT USR1 USR2 TSTP; exec \"$0\" run \"$1\"")
        .arg(xrginit_bin())
        .arg(&p)
        .stdin(Stdio::null());
    let mut r = Run::spawn(c);
    let pid = r.wait_running();
    // Recorded for the failure message only.
    let xr_ign = status_mask(r.pid, "SigIgn");
    let svc = (
        status_mask(pid, "SigBlk"),
        status_mask(pid, "SigIgn"),
        status_mask(pid, "SigCgt"),
    );
    let d = stop_sleeper(r, "sigs2");
    assert_eq!(
        svc,
        (0, 0, 0),
        "B-M0-021: service (SigBlk, SigIgn, SigCgt); xrginit SigIgn was {xr_ign:016x}\n{}",
        d.text
    );
}

// --- B-M0-022 -------------------------------------------------------------

// spec: B-M0-022
#[test]
fn b_m0_022_new_session_and_process_group() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write(
        "s.toml",
        &unit_toml("session", &["/usr/bin/sleep", "60"], ""),
    );
    let mut r = Run::unit(&p);
    let pid = r.wait_running();
    let pgrp = field(pid, 5);
    let sid = field(pid, 6);
    stop_sleeper(r, "session");
    assert_eq!(
        pgrp,
        Some(pid.to_string()),
        "B-M0-022: field 5 (pgrp) equals P"
    );
    assert_eq!(
        sid,
        Some(pid.to_string()),
        "B-M0-022: field 6 (session) equals P"
    );
}

// --- B-M0-023 -------------------------------------------------------------

// spec: B-M0-023 (xrginit inherits a non-close-on-exec fd 3, spec example)
//
// The service is a shell that has finished its own startup before it reports
// readiness: the readiness write happens in a forked subshell, so the service's fd
// table is never touched by it, and the service then only waits. Reading the fds right
// after the running line would race the dynamic loader, which briefly holds fd 3 while
// it loads shared libraries.
#[test]
fn b_m0_023_only_fds_0_1_2_open() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let (f, ready) = fifo(&t.path, "ready");
    let script =
        "trap 'kill $!; exit 0' TERM; (echo ready > \"$1\"; exec /usr/bin/sleep 60) & wait";
    let p = t.write(
        "s.toml",
        &unit_toml(
            "fds",
            &["/bin/sh", "-c", script, "sh", f.to_str().expect("utf-8")],
            "",
        ),
    );
    let mut c = Command::new("/bin/sh");
    c.arg("-c")
        .arg("exec 3</dev/null; exec \"$0\" run \"$1\"")
        .arg(xrginit_bin())
        .arg(&p)
        .stdin(Stdio::null());
    let mut r = Run::spawn(c);
    let pid = r.wait_running();
    await_ready(&ready, "B-M0-023");
    let fds = fd_entries(pid);
    let links: Vec<_> = (0..3)
        .map(|n| {
            (
                fs::read_link(format!("/proc/{pid}/fd/{n}")).ok(),
                fs::read_link(format!("/proc/{}/fd/{n}", r.pid)).ok(),
            )
        })
        .collect();
    stop_sleeper(r, "fds");
    let want: BTreeSet<String> = ["0", "1", "2"].iter().map(|s| s.to_string()).collect();
    assert_eq!(fds, want, "B-M0-023: open fds of the service");
    for (n, (svc, xr)) in links.iter().enumerate() {
        assert!(svc.is_some(), "B-M0-023: service fd {n} unreadable");
        assert_eq!(
            svc, xr,
            "B-M0-023: fd {n} links to the same target as xrginit's fd {n}"
        );
    }
}

// --- B-M0-024 -------------------------------------------------------------

// spec: B-M0-024
#[test]
fn b_m0_024_identity_cwd_and_env_inherited() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write("s.toml", &unit_toml("ident", &["/usr/bin/sleep", "60"], ""));
    let cwd = t.join("cwd");
    fs::create_dir(&cwd).expect("mkdir cwd");
    let mut c = Command::new(xrginit_bin());
    c.arg("run")
        .arg(&p)
        .current_dir(&cwd)
        .env("XINIT_M0_PROBE", "1")
        .stdin(Stdio::null());
    let mut r = Run::spawn(c);
    let pid = r.wait_running();
    let x = r.pid;
    let ids: Vec<(String, String)> = ["Uid", "Gid", "Groups"]
        .iter()
        .map(|k| (status_value(pid, k), status_value(x, k)))
        .collect();
    let svc_cwd = fs::read_link(format!("/proc/{pid}/cwd")).ok();
    let xr_cwd = fs::read_link(format!("/proc/{x}/cwd")).ok();
    let env = fs::read(format!("/proc/{pid}/environ")).expect("environ");
    stop_sleeper(r, "ident");
    for (k, (svc, xr)) in ["Uid", "Gid", "Groups"].iter().zip(&ids) {
        assert_eq!(svc, xr, "B-M0-024: {k} of service equals xrginit's");
    }
    assert_eq!(svc_cwd, xr_cwd, "B-M0-024: cwd");
    assert_eq!(
        svc_cwd,
        Some(cwd),
        "B-M0-024: cwd is the one xrginit was started in"
    );
    assert!(
        env.split(|&b| b == 0).any(|e| e == b"XINIT_M0_PROBE=1"),
        "B-M0-024: XINIT_M0_PROBE=1 in environ"
    );
}

// --- B-M0-025 -------------------------------------------------------------

const NO_SUCH_USER: &str = "xinit-m0-no-such-user";

fn require_absent_user(name: &str) {
    assert!(
        passwd_entries().iter().all(|e| e.name != name),
        "test requirement: user {name} must not exist in /etc/passwd"
    );
}

fn start_failure_lines(d: &Finished, name: &str) {
    let t = &d.text;
    assert_eq!(d.code, Some(4), "exit code 4 for a start failure\n{t}");
    d.check_run_invariants(name);
    let seq: Vec<String> = d
        .lines
        .iter()
        .map(|l| {
            l.state()
                .map_or_else(|| l.event.clone(), |s| format!("state:{s}"))
        })
        .collect();
    assert_eq!(
        seq,
        ["state:starting", "start-failed", "state:failed"],
        "{t}"
    );
    assert_eq!(d.lines[1].level, "ERROR", "{t}");
}

// spec: B-M0-025 (CLI)
#[test]
fn b_m0_025_cli_unknown_user() {
    require_absent_user(NO_SUCH_USER);
    let _g = child_lock_shared();
    let t = TempDir::new();
    let marker = t.join("started");
    let p = t.write(
        "u.toml",
        &unit_toml(
            "who",
            &["/bin/sh", "-c", "echo x > \"$1\"", "sh", &t.s("started")],
            &format!("user = \"{NO_SUCH_USER}\"\n"),
        ),
    );
    let d = Run::unit(&p).finish(EXIT_DEADLINE);
    start_failure_lines(&d, "who");
    let sf = &d.lines[1];
    assert_eq!(sf.keys(), ["error", "user"], "{}", d.text);
    assert_eq!(sf.get("error").as_deref(), Some("user-unknown"));
    assert_eq!(sf.get("user").as_deref(), Some(NO_SUCH_USER));
    assert!(!marker.exists(), "B-M0-025: the service must not run");
}

// spec: B-M0-025 (API), B-M0-032
#[test]
fn b_m0_025_api_unknown_user_no_child() {
    require_absent_user(NO_SUCH_USER);
    let _g = child_lock_exclusive();
    let before = children_of_self();
    assert!(
        before.is_empty(),
        "precondition: test process has children {before:?}"
    );
    let exec = ExecutionSection {
        command: vec!["/usr/bin/sleep".into(), "60".into()],
        user: Some(NO_SUCH_USER.into()),
    };
    let res = Service::start(&exec);
    let left = children_of_self();
    assert_eq!(
        res.as_ref().err(),
        Some(&StartError::UserUnknown {
            user: NO_SUCH_USER.into()
        }),
        "B-M0-025: {res:?}"
    );
    assert!(
        left.is_empty(),
        "B-M0-032: children left after a failed start: {left:?}"
    );
}

// --- B-M0-026 -------------------------------------------------------------

/// The passwd entry whose uid and primary gid equal this process's effective ids.
fn same_user() -> PasswdEntry {
    let (uid, gid) = self_euid_egid();
    passwd_entries()
        .into_iter()
        .find(|e| e.uid == uid && e.gid == gid)
        .unwrap_or_else(|| panic!("test requirement: /etc/passwd needs an entry with uid {uid} and gid {gid} (the test user)"))
}

/// root or nobody, whichever exists and differs from the test user (spec example).
fn other_user() -> PasswdEntry {
    let (uid, gid) = self_euid_egid();
    assert_ne!(
        uid, 0,
        "test requirement: this unprivileged test must not run as root (B-M0-026)"
    );
    passwd_entries()
        .into_iter()
        .find(|e| (e.name == "root" || e.name == "nobody") && (e.uid != uid || e.gid != gid))
        .unwrap_or_else(|| {
            panic!(
                "test requirement: /etc/passwd needs root or nobody with ids other than {uid}:{gid}"
            )
        })
}

// spec: B-M0-026 (target ids equal xrginit's: no credential change, groups inherited)
#[test]
fn b_m0_026_cli_same_user_runs_unchanged() {
    let me = same_user();
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write(
        "s.toml",
        &unit_toml(
            "sameuser",
            &["/usr/bin/sleep", "60"],
            &format!("user = \"{}\"\n", me.name),
        ),
    );
    let mut r = Run::unit(&p);
    let pid = r.wait_running();
    let x = r.pid;
    let ids: Vec<(String, String)> = ["Uid", "Gid", "Groups"]
        .iter()
        .map(|k| (status_value(pid, k), status_value(x, k)))
        .collect();
    stop_sleeper(r, "sameuser");
    for (k, (svc, xr)) in ["Uid", "Gid", "Groups"].iter().zip(&ids) {
        assert_eq!(
            svc, xr,
            "B-M0-026: {k} must be unchanged (no setgroups/setgid/setuid)"
        );
    }
}

// spec: B-M0-026 (unprivileged, different user: credentials EPERM, exit 4, no fallback)
#[test]
fn b_m0_026_cli_other_user_credentials_eperm() {
    let other = other_user();
    let _g = child_lock_shared();
    let t = TempDir::new();
    let marker = t.join("started");
    let p = t.write(
        "u.toml",
        &unit_toml(
            "creds",
            &["/bin/sh", "-c", "echo x > \"$1\"", "sh", &t.s("started")],
            &format!("user = \"{}\"\n", other.name),
        ),
    );
    let d = Run::unit(&p).finish(EXIT_DEADLINE);
    start_failure_lines(&d, "creds");
    let sf = &d.lines[1];
    assert_eq!(sf.keys(), ["error", "errno"], "{}", d.text);
    assert_eq!(sf.get("error").as_deref(), Some("credentials"));
    assert_eq!(sf.get("errno").as_deref(), Some("EPERM"));
    assert!(
        !marker.exists(),
        "B-M0-026: never falls back to running as the current user"
    );
}

// spec: B-M0-026 (API, EPERM = 1), B-M0-032
#[test]
fn b_m0_026_api_other_user_credentials_no_child() {
    let other = other_user();
    let _g = child_lock_exclusive();
    let before = children_of_self();
    assert!(
        before.is_empty(),
        "precondition: test process has children {before:?}"
    );
    let exec = ExecutionSection {
        command: vec!["/usr/bin/sleep".into(), "60".into()],
        user: Some(other.name.clone()),
    };
    let res = Service::start(&exec);
    let left = children_of_self();
    match res {
        Ok(mut s) => {
            let _ = s.signal(Signal::Kill);
            let _ = s.wait();
            panic!(
                "B-M0-026: start as {} must fail with Credentials{{errno:1}}, got Ok",
                other.name
            );
        }
        Err(e) => assert_eq!(e, StartError::Credentials { errno: 1 }, "B-M0-026"),
    }
    assert!(
        left.is_empty(),
        "B-M0-032: children left after a failed start: {left:?}"
    );
}

// spec: B-M0-026 (API, same user: start succeeds)
#[test]
fn b_m0_026_api_same_user_starts() {
    let me = same_user();
    let _g = child_lock_shared();
    let exec = ExecutionSection {
        command: vec!["/usr/bin/sleep".into(), "60".into()],
        user: Some(me.name.clone()),
    };
    let mut s = Service::start(&exec)
        .unwrap_or_else(|e| panic!("B-M0-026: start as {} failed: {e:?}", me.name));
    let uid = status_value(s.pid(), "Uid");
    let groups = status_value(s.pid(), "Groups");
    assert_eq!(s.signal(Signal::Kill), Ok(()));
    assert_eq!(s.wait(), Ok(Termination::Signaled(9)));
    let me_pid = std::process::id();
    assert_eq!(uid, status_value(me_pid, "Uid"), "B-M0-026: Uid unchanged");
    assert_eq!(
        groups,
        status_value(me_pid, "Groups"),
        "B-M0-026: Groups unchanged (no setgroups)"
    );
}
