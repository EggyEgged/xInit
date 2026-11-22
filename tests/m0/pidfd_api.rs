//! specs/M0.md, "pidfd-tracked processes (API)": B-M0-029 to B-M0-032, plus the API
//! part of B-M0-016. Durations are measured with the test's monotonic clock.

use super::common::{
    TempDir, await_ready, child_lock_exclusive, child_lock_shared, children_of_self, fifo, is_live,
    require_prereqs, same_process_exists, start_time,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use xinit_api::{
    DEFAULT_STOP_TIMEOUT, ExecutionSection, Service, Signal, SignalError, StartError, StopOutcome,
    Termination,
};

fn exec(argv: &[&str]) -> ExecutionSection {
    ExecutionSection {
        command: argv.iter().map(|s| s.to_string()).collect(),
        user: None,
    }
}

fn start(argv: &[&str]) -> Service {
    require_prereqs();
    Service::start(&exec(argv)).unwrap_or_else(|e| panic!("Service::start({argv:?}) failed: {e:?}"))
}

/// Kills and reaps the service if a test assertion fails half-way.
struct Reaper(Option<Service>);
impl Reaper {
    fn get(&mut self) -> &mut Service {
        self.0.as_mut().expect("service present")
    }
}
impl Drop for Reaper {
    fn drop(&mut self) {
        if let Some(s) = self.0.as_mut() {
            let _ = s.signal(Signal::Kill);
            let _ = s.wait();
        }
    }
}

// --- B-M0-016 / B-M0-032 (API) ----------------------------------------------

fn failed_start_leaves_no_child(argv: &[&str], want: StartError) {
    require_prereqs();
    let _g = child_lock_exclusive();
    let before = children_of_self();
    assert!(
        before.is_empty(),
        "precondition: test process has children {before:?}"
    );
    let res = Service::start(&exec(argv));
    let left = children_of_self();
    match res {
        Ok(mut s) => {
            let _ = s.signal(Signal::Kill);
            let _ = s.wait();
            panic!("expected Err({want:?}), got Ok");
        }
        Err(e) => assert_eq!(e, want),
    }
    assert!(
        left.is_empty(),
        "B-M0-032: children (running or zombie) left after a failed start: {left:?}"
    );
}

// spec: B-M0-016 (ENOENT = 2), B-M0-032
#[test]
fn b_m0_016_b_m0_032_api_exec_enoent_no_child() {
    failed_start_leaves_no_child(
        &["/nonexistent/xinit-m0-missing"],
        StartError::Exec { errno: 2 },
    );
}

// spec: B-M0-016 (EACCES = 13), B-M0-032
#[test]
fn b_m0_016_b_m0_032_api_exec_eacces_no_child() {
    let t = TempDir::new();
    let prog = t.write("noexec", "#!/bin/sh\nexit 0\n");
    fs::set_permissions(&prog, fs::Permissions::from_mode(0o644)).expect("chmod 644");
    failed_start_leaves_no_child(
        &[prog.to_str().expect("utf-8")],
        StartError::Exec { errno: 13 },
    );
}

// --- B-M0-029 -------------------------------------------------------------

// spec: B-M0-029
#[test]
fn b_m0_029_start_and_wait_exit_7() {
    let _g = child_lock_shared();
    let mut s = start(&["/bin/sh", "-c", "exit 7"]);
    let pid = s.pid();
    let st = start_time(pid);
    assert_eq!(s.wait(), Ok(Termination::Exited(7)), "B-M0-029: first wait");
    if let Some(st) = st {
        assert!(
            !same_process_exists(pid, st),
            "B-M0-029: /proc/{pid} still refers to the process after wait"
        );
    }
    assert_eq!(s.pid(), pid, "pid() is stable");
    let t0 = Instant::now();
    assert_eq!(
        s.wait(),
        Ok(Termination::Exited(7)),
        "B-M0-029: second wait returns the same result"
    );
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "B-M0-029: second wait must not block"
    );
}

// spec: B-M0-029 (pid() is the process exec'd from command: its cmdline is the argv)
#[test]
fn b_m0_029_pid_names_the_started_process() {
    let _g = child_lock_shared();
    let mut r = Reaper(Some(start(&["/usr/bin/sleep", "60"])));
    let pid = r.get().pid();
    let cmdline = fs::read(format!("/proc/{pid}/cmdline"))
        .unwrap_or_else(|e| panic!("/proc/{pid}/cmdline: {e}"));
    assert_eq!(
        cmdline, b"/usr/bin/sleep\x0060\x00",
        "B-M0-029: pid() must be the service process"
    );
    assert_eq!(r.get().signal(Signal::Kill), Ok(()));
    assert_eq!(r.get().wait(), Ok(Termination::Signaled(9)));
    r.0 = None;
}

// --- B-M0-030 -------------------------------------------------------------

// spec: B-M0-030, I-1
#[test]
fn b_m0_030_signal_after_reap_fails_and_reaches_nobody() {
    let _g = child_lock_shared();
    let mut s = start(&["/usr/bin/sleep", "60"]);
    assert_eq!(
        s.signal(Signal::Kill),
        Ok(()),
        "B-M0-030: signal while running"
    );
    assert_eq!(
        s.wait(),
        Ok(Termination::Signaled(9)),
        "B-M0-030: wait after SIGKILL"
    );
    // Canary processes started after the reap; one may reuse the pid.
    let mut canaries: Vec<_> = (0..4)
        .map(|_| {
            Command::new("/usr/bin/sleep")
                .arg("60")
                .stdin(Stdio::null())
                .spawn()
                .expect("spawn canary")
        })
        .collect();
    let r_term = s.signal(Signal::Term);
    let r_int = s.signal(Signal::Int);
    let r_kill = s.signal(Signal::Kill);
    let alive: Vec<bool> = canaries
        .iter()
        .map(|c| start_time(c.id()).is_some_and(|st| is_live(c.id(), st)))
        .collect();
    for c in &mut canaries {
        let _ = c.kill();
        let _ = c.wait();
    }
    assert_eq!(
        r_term,
        Err(SignalError::Exited),
        "B-M0-030: Term after reap"
    );
    assert_eq!(r_int, Err(SignalError::Exited), "B-M0-030: Int after reap");
    assert_eq!(
        r_kill,
        Err(SignalError::Exited),
        "B-M0-030: Kill after reap"
    );
    assert!(
        alive.iter().all(|a| *a),
        "B-M0-030: a canary was signalled: {alive:?}"
    );
}

// spec: B-M0-030 (Signal::Term is SIGTERM = 15, Signal::Int is SIGINT = 2)
#[test]
fn b_m0_030_signal_numbers_term_and_int() {
    let _g = child_lock_shared();
    for (sig, num) in [(Signal::Term, 15), (Signal::Int, 2)] {
        let mut s = start(&["/usr/bin/sleep", "60"]);
        assert_eq!(s.signal(sig), Ok(()), "{sig:?}");
        assert_eq!(
            s.wait(),
            Ok(Termination::Signaled(num)),
            "{sig:?} -> signal {num}"
        );
    }
}

// --- B-M0-031 -------------------------------------------------------------

// spec: B-M0-031 (B-M0-018 command; a readiness echo is added so the trap is in place)
#[test]
fn b_m0_031_stop_escalates_after_timeout() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let (f, ready) = fifo(&t.path, "ready");
    let script = "trap '' TERM; echo ready > \"$1\"; exec /usr/bin/sleep 60";
    let mut r = Reaper(Some(start(&[
        "/bin/sh",
        "-c",
        script,
        "sh",
        f.to_str().expect("utf-8"),
    ])));
    await_ready(&ready, "B-M0-031");
    let t0 = Instant::now();
    let out = r.get().stop(Duration::from_millis(300));
    let took = t0.elapsed();
    assert_eq!(
        out,
        Ok(StopOutcome {
            termination: Termination::Signaled(9),
            escalated: true
        }),
        "B-M0-031"
    );
    assert!(
        took >= Duration::from_millis(300),
        "B-M0-031: returned after {took:?}, before the 300 ms timeout"
    );
    assert!(
        took < Duration::from_millis(2300),
        "B-M0-031: returned after {took:?} (limit 2300 ms)"
    );
    r.0 = None;
}

// spec: B-M0-031
#[test]
fn b_m0_031_stop_without_escalation() {
    let _g = child_lock_shared();
    let mut r = Reaper(Some(start(&["/usr/bin/sleep", "60"])));
    let pid = r.get().pid();
    let st = start_time(pid).expect("service exists before stop");
    let t0 = Instant::now();
    let out = r.get().stop(Duration::from_secs(5));
    let took = t0.elapsed();
    assert_eq!(
        out,
        Ok(StopOutcome {
            termination: Termination::Signaled(15),
            escalated: false
        }),
        "B-M0-031"
    );
    assert!(
        took < Duration::from_millis(2000),
        "B-M0-031: took {took:?} (limit 2000 ms)"
    );
    assert!(
        !same_process_exists(pid, st),
        "B-M0-031: stop returns after the process is reaped"
    );
    assert_eq!(
        r.get().signal(Signal::Term),
        Err(SignalError::Exited),
        "I-1: no signal after the reap in stop"
    );
    r.0 = None;
}

// spec: B-M0-031, I-6 (DEFAULT_STOP_TIMEOUT is 5000 ms)
#[test]
fn b_m0_031_default_stop_timeout_is_5000_ms_and_used_by_stop() {
    let _g = child_lock_shared();
    assert_eq!(DEFAULT_STOP_TIMEOUT, Duration::from_millis(5000), "I-6");
    let mut r = Reaper(Some(start(&["/usr/bin/sleep", "60"])));
    let out = r.get().stop(DEFAULT_STOP_TIMEOUT);
    assert_eq!(
        out,
        Ok(StopOutcome {
            termination: Termination::Signaled(15),
            escalated: false
        })
    );
    r.0 = None;
}
