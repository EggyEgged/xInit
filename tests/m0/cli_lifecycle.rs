//! specs/M0.md, "CLI and lifecycle": B-M0-011 to B-M0-019, the CLI part of B-M0-010,
//! and invariants I-2 to I-5 on every run.

use super::common::{
    EXIT_DEADLINE, Finished, LogLine, Run, SIGINT, SIGKILL, SIGTERM, TempDir, await_ready,
    child_lock_shared, fifo, guard_env, is_live, signal_verified, start_time, unit_toml,
    verify_by_env, xrginit_bin,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn state_is(l: &LogLine, s: &str) -> bool {
    l.state().as_deref() == Some(s)
}

/// The non-`detail` shape of one line: level, unit, event.
fn head(l: &LogLine) -> (&str, &str, &str) {
    (l.level.as_str(), l.unit.as_str(), l.event.as_str())
}

/// Events in order, with `state` lines shown as `state:<value>`.
fn event_seq(d: &Finished) -> Vec<String> {
    d.lines
        .iter()
        .map(|l| {
            l.state()
                .map_or_else(|| l.event.clone(), |s| format!("state:{s}"))
        })
        .collect()
}

// --- B-M0-011 -------------------------------------------------------------

fn usage_case(args: &[&str], marker: Option<&Path>) {
    let _g = child_lock_shared();
    let mut c = Command::new(xrginit_bin());
    c.args(args).stdin(Stdio::null());
    let d = Run::spawn(c).finish(EXIT_DEADLINE);
    let t = &d.text;
    assert_eq!(d.code, Some(2), "B-M0-011: exit code for {args:?}\n{t}");
    assert!(d.stdout.is_empty(), "B-M0-011: nothing on stdout\n{t}");
    assert_eq!(d.lines.len(), 1, "B-M0-011: exactly one line\n{t}");
    let l = &d.lines[0];
    assert_eq!(head(l), ("ERROR", "-", "usage"), "B-M0-011\n{t}");
    assert_eq!(
        l.keys(),
        ["detail"],
        "B-M0-011: usage carries only detail\n{t}"
    );
    if let Some(m) = marker {
        assert!(!m.exists(), "B-M0-011: no process may be started");
    }
}

// spec: B-M0-011
#[test]
fn b_m0_011_usage_no_arguments() {
    usage_case(&[], None);
}

// spec: B-M0-011 (spec example)
#[test]
fn b_m0_011_usage_run_without_unit() {
    usage_case(&["run"], None);
}

// spec: B-M0-011 (two arguments after run; the first names a valid unit that would leave a marker)
#[test]
fn b_m0_011_usage_run_with_two_paths() {
    let t = TempDir::new();
    let marker = t.join("started");
    let u = t.write(
        "u.toml",
        &unit_toml(
            "marker",
            &["/bin/sh", "-c", "echo x > \"$1\"", "sh", &t.s("started")],
            "",
        ),
    );
    usage_case(&["run", u.to_str().expect("utf-8"), "b"], Some(&marker));
}

// spec: B-M0-011 (only `frobnicate` is used as an unknown subcommand)
#[test]
fn b_m0_011_usage_unknown_subcommand_frobnicate() {
    usage_case(&["frobnicate"], None);
}

// --- B-M0-012 -------------------------------------------------------------

/// Runs an invalid unit and checks the common B-M0-012 shape; returns the unit-invalid line.
fn invalid_run(path: &Path, marker: &Path) -> LogLine {
    let _g = child_lock_shared();
    let d = Run::unit(path).finish(EXIT_DEADLINE);
    let t = &d.text;
    assert_eq!(d.code, Some(3), "B-M0-012: exit code\n{t}");
    assert!(d.stdout.is_empty(), "I-5\n{t}");
    assert!(d.states().is_empty(), "B-M0-012: no state line\n{t}");
    let inv = d.events("unit-invalid");
    assert_eq!(inv.len(), 1, "B-M0-012: one unit-invalid line\n{t}");
    let l = inv[0].clone();
    assert_eq!(head(&l), ("ERROR", "-", "unit-invalid"), "B-M0-012\n{t}");
    assert_eq!(
        l.keys().first().copied(),
        Some("path"),
        "B-M0-012: path first\n{t}"
    );
    assert_eq!(
        l.keys().last().copied(),
        Some("detail"),
        "B-M0-012: detail last\n{t}"
    );
    assert!(!marker.exists(), "B-M0-012: no process may be started");
    l
}

fn marker_unit(t: &TempDir, extra_exec: &str) -> String {
    unit_toml(
        "victim",
        &["/bin/sh", "-c", "echo x > \"$1\"", "sh", &t.s("started")],
        extra_exec,
    )
}

// spec: B-M0-012 (spec example: comand), B-M0-004
#[test]
fn b_m0_012_cli_unknown_key() {
    let t = TempDir::new();
    let p = t.write(
        "bad.toml",
        &marker_unit(&t, "comand = [\"/usr/bin/sleep\", \"1\"]\n"),
    );
    let l = invalid_run(&p, &t.join("started"));
    assert_eq!(l.keys(), ["path", "error", "key", "detail"], "{}", l.raw);
    assert_eq!(
        l.get_bytes("path"),
        Some(p.as_os_str().as_encoded_bytes()),
        "{}",
        l.raw
    );
    assert_eq!(l.get("error").as_deref(), Some("unknown-key"));
    assert_eq!(l.get("key").as_deref(), Some("execution.comand"));
}

// spec: B-M0-012 (spec example: missing file), B-M0-009
#[test]
fn b_m0_012_cli_missing_file() {
    let t = TempDir::new();
    let p = t.join("xinit-m0-absent.toml");
    let l = invalid_run(&p, &t.join("started"));
    assert_eq!(l.keys(), ["path", "error", "errno", "detail"], "{}", l.raw);
    assert_eq!(l.get("error").as_deref(), Some("io"));
    assert_eq!(l.get("errno").as_deref(), Some("ENOENT"));
    assert_eq!(
        l.get_bytes("path"),
        Some(p.as_os_str().as_encoded_bytes()),
        "{}",
        l.raw
    );
}

// spec: B-M0-012, B-M0-009
#[test]
fn b_m0_012_cli_syntax_error() {
    let t = TempDir::new();
    let p = t.write("bad.toml", "schema_version = 1\n[service\n");
    let l = invalid_run(&p, &t.join("started"));
    assert_eq!(l.keys(), ["path", "error", "detail"], "{}", l.raw);
    assert_eq!(l.get("error").as_deref(), Some("syntax"));
}

// spec: B-M0-012, B-M0-005
#[test]
fn b_m0_012_cli_missing_key() {
    let t = TempDir::new();
    let p = t.write(
        "bad.toml",
        &marker_unit(&t, "").replace("type = \"simple\"\n", ""),
    );
    let l = invalid_run(&p, &t.join("started"));
    assert_eq!(l.keys(), ["path", "error", "key", "detail"], "{}", l.raw);
    assert_eq!(l.get("error").as_deref(), Some("missing-key"));
    assert_eq!(l.get("key").as_deref(), Some("service.type"));
}

// spec: B-M0-012, B-M0-006
#[test]
fn b_m0_012_cli_invalid_type() {
    let t = TempDir::new();
    let p = t.write("bad.toml", &marker_unit(&t, "user = 0\n"));
    let l = invalid_run(&p, &t.join("started"));
    assert_eq!(l.keys(), ["path", "error", "key", "detail"], "{}", l.raw);
    assert_eq!(l.get("error").as_deref(), Some("invalid-type"));
    assert_eq!(l.get("key").as_deref(), Some("execution.user"));
}

// spec: B-M0-012, B-M0-010
#[test]
fn b_m0_012_cli_invalid_value() {
    let t = TempDir::new();
    let p = t.write(
        "bad.toml",
        &format!("{}[restart]\nmaximum_restarts = -1\n", marker_unit(&t, "")),
    );
    let l = invalid_run(&p, &t.join("started"));
    assert_eq!(l.keys(), ["path", "error", "key", "detail"], "{}", l.raw);
    assert_eq!(l.get("error").as_deref(), Some("invalid-value"));
    assert_eq!(l.get("key").as_deref(), Some("restart.maximum_restarts"));
}

// spec: B-M0-012, B-M0-003
#[test]
fn b_m0_012_cli_schema_version() {
    let t = TempDir::new();
    let p = t.write(
        "bad.toml",
        &marker_unit(&t, "").replace("schema_version = 1", "schema_version = 2"),
    );
    let l = invalid_run(&p, &t.join("started"));
    assert_eq!(
        l.get("error").as_deref(),
        Some("schema-version"),
        "{}",
        l.raw
    );
}

// spec: B-M0-012, B-M0-007
#[test]
fn b_m0_012_cli_unsupported_service_type() {
    let t = TempDir::new();
    let p = t.write(
        "bad.toml",
        &marker_unit(&t, "").replace("type = \"simple\"", "type = \"xinit-m0-bogus\""),
    );
    let l = invalid_run(&p, &t.join("started"));
    assert_eq!(
        l.get("error").as_deref(),
        Some("unsupported-service-type"),
        "{}",
        l.raw
    );
}

// --- B-M0-013 -------------------------------------------------------------

// spec: B-M0-013, I-2 to I-5
#[test]
fn b_m0_013_successful_run() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write(
        "sleeper.toml",
        &unit_toml("sleeper", &["/usr/bin/sleep", "1"], ""),
    );
    let mut r = Run::unit(&p);
    let starting = r.wait_line("state=starting", |l| state_is(l, "starting"));
    assert_eq!(head(&starting), ("INFO", "sleeper", "state"));
    assert_eq!(
        starting.keys(),
        ["state"],
        "B-M0-013: starting has no pid\n{}",
        starting.raw
    );
    let running = r.wait_line("state=running", |l| state_is(l, "running"));
    assert_eq!(head(&running), ("INFO", "sleeper", "state"));
    assert_eq!(running.keys(), ["state", "pid"], "{}", running.raw);
    let pid: u32 = running
        .get("pid")
        .and_then(|v| v.parse().ok())
        .expect("numeric pid");
    // The service program has replaced the forked child by the time `running` is logged.
    let exe = fs::read_link(format!("/proc/{pid}/exe"))
        .expect("B-M0-013: /proc/<P>/exe readable while running");
    let want = fs::canonicalize("/usr/bin/sleep").expect("realpath /usr/bin/sleep");
    assert_eq!(exe, want, "B-M0-013: running logged before exec");
    let st = start_time(pid).expect("start time");
    let d = r.finish(EXIT_DEADLINE);
    let t = &d.text;
    assert_eq!(d.code, Some(0), "B-M0-013: exit code\n{t}");
    d.check_run_invariants("sleeper");
    assert_eq!(d.state_names(), ["starting", "running", "exited"], "{t}");
    let last = d.final_state();
    assert_eq!(head(last), ("INFO", "sleeper", "state"));
    assert_eq!(last.keys(), ["state", "pid", "code"], "{t}");
    assert_eq!(last.get("pid"), Some(pid.to_string()));
    assert_eq!(last.get("code").as_deref(), Some("0"));
    assert!(
        !super::common::same_process_exists(pid, st),
        "B-M0-013: /proc/<P> must be gone"
    );
}

// spec: B-M0-013 (unit path relative to the working directory, per CLI section)
#[test]
fn b_m0_013_relative_unit_path() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    t.write(
        "sub/rel.toml",
        &unit_toml("rel", &["/bin/sh", "-c", "exit 0"], ""),
    );
    let mut c = Command::new(xrginit_bin());
    c.args(["run", "sub/rel.toml"])
        .current_dir(&t.path)
        .stdin(Stdio::null());
    let d = Run::spawn(c).finish(EXIT_DEADLINE);
    assert_eq!(d.code, Some(0), "{}", d.text);
    d.check_run_invariants("rel");
    assert_eq!(
        d.final_state().get("code").as_deref(),
        Some("0"),
        "{}",
        d.text
    );
}

// --- B-M0-014 -------------------------------------------------------------

// spec: B-M0-014
#[test]
fn b_m0_014_nonzero_exit_is_failed() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write("f.toml", &unit_toml("falsy", &["/usr/bin/false"], ""));
    let d = Run::unit(&p).finish(EXIT_DEADLINE);
    let t = &d.text;
    assert_eq!(d.code, Some(1), "B-M0-014: exit code\n{t}");
    d.check_run_invariants("falsy");
    assert_eq!(
        d.state_names(),
        ["starting", "running", "failed"],
        "exit 1 means failed after running\n{t}"
    );
    let last = d.final_state();
    assert_eq!(head(last), ("ERROR", "falsy", "state"));
    assert_eq!(last.keys(), ["state", "pid", "code"], "{t}");
    assert_eq!(last.get("code").as_deref(), Some("1"));
}

// --- B-M0-015 -------------------------------------------------------------

// spec: B-M0-015
#[test]
fn b_m0_015_killed_by_foreign_sigkill() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write(
        "s.toml",
        &unit_toml("sleeper", &["/usr/bin/sleep", "60"], ""),
    );
    let mut r = Run::unit(&p);
    let pid = r.wait_running();
    // Panics, naming the pid, unless it was verified as xrginit's direct child.
    if let Err(e) = r.signal_service(SIGKILL) {
        panic!("kill service {pid}: {e}");
    }
    let d = r.finish(EXIT_DEADLINE);
    let t = &d.text;
    assert_eq!(d.code, Some(1), "B-M0-015: exit code\n{t}");
    d.check_run_invariants("sleeper");
    let last = d.final_state();
    assert_eq!(head(last), ("ERROR", "sleeper", "state"));
    assert_eq!(last.keys(), ["state", "pid", "signal"], "{t}");
    assert_eq!(last.get("state").as_deref(), Some("failed"));
    assert_eq!(last.get("pid"), Some(pid.to_string()));
    assert_eq!(last.get("signal").as_deref(), Some("SIGKILL"));
    assert!(
        d.events("shutdown").is_empty() && d.events("stop").is_empty(),
        "no stop was requested\n{t}"
    );
}

// --- B-M0-016 -------------------------------------------------------------

fn start_failed_run(p: &Path, name: &str) -> Finished {
    let _g = child_lock_shared();
    let d = Run::unit(p).finish(EXIT_DEADLINE);
    let t = &d.text;
    assert_eq!(d.code, Some(4), "exit code 4 for a start failure\n{t}");
    d.check_run_invariants(name);
    assert_eq!(
        event_seq(&d),
        ["state:starting", "start-failed", "state:failed"],
        "{t}"
    );
    assert_eq!(head(&d.lines[1]), ("ERROR", name, "start-failed"), "{t}");
    assert_eq!(head(&d.lines[2]), ("ERROR", name, "state"), "{t}");
    assert!(
        d.lines[2].get("code").is_none() && d.lines[2].get("signal").is_none(),
        "{t}"
    );
    d
}

// spec: B-M0-016 (CLI)
#[test]
fn b_m0_016_cli_command_not_found() {
    let t = TempDir::new();
    let p = t.write(
        "m.toml",
        &unit_toml("missing", &["/nonexistent/xinit-m0-missing"], ""),
    );
    let d = start_failed_run(&p, "missing");
    assert_eq!(d.lines[1].keys(), ["error", "errno"], "{}", d.text);
    assert_eq!(d.lines[1].get("error").as_deref(), Some("exec"));
    assert_eq!(d.lines[1].get("errno").as_deref(), Some("ENOENT"));
}

// spec: B-M0-016 (non-executable regular file gives EACCES)
#[test]
fn b_m0_016_cli_not_executable_eacces() {
    let t = TempDir::new();
    let prog = t.write("noexec", "#!/bin/sh\nexit 0\n");
    fs::set_permissions(&prog, fs::Permissions::from_mode(0o644)).expect("chmod 644");
    let p = t.write(
        "m.toml",
        &unit_toml("noexec", &[prog.to_str().expect("utf-8")], ""),
    );
    let d = start_failed_run(&p, "noexec");
    assert_eq!(
        d.lines[1].get("error").as_deref(),
        Some("exec"),
        "{}",
        d.text
    );
    assert_eq!(
        d.lines[1].get("errno").as_deref(),
        Some("EACCES"),
        "{}",
        d.text
    );
}

// --- B-M0-017 / B-M0-018 ------------------------------------------------------

fn requested_stop(sig: i32, sig_name: &str) {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write(
        "s.toml",
        &unit_toml("sleeper", &["/usr/bin/sleep", "60"], ""),
    );
    let mut r = Run::unit(&p);
    let pid = r.wait_running();
    let t0 = r.signal_xrginit(sig);
    let d = r.finish(EXIT_DEADLINE);
    let t = &d.text;
    let took = d.at.duration_since(t0);
    assert!(
        took < Duration::from_millis(2000),
        "B-M0-017: exit took {took:?} (limit 2000 ms)\n{t}"
    );
    assert_eq!(d.code, Some(0), "B-M0-017: exit code\n{t}");
    d.check_run_invariants("sleeper");
    let sd = d
        .index_of(|l| l.event == "shutdown")
        .unwrap_or_else(|| panic!("no shutdown line\n{t}"));
    let st = d
        .index_of(|l| l.event == "stop")
        .unwrap_or_else(|| panic!("no stop line\n{t}"));
    let ex = d
        .index_of(|l| state_is(l, "exited"))
        .unwrap_or_else(|| panic!("no exited line\n{t}"));
    let run = d.index_of(|l| state_is(l, "running")).expect("running");
    assert!(
        run < sd && sd < st && st < ex,
        "B-M0-017: order running < shutdown < stop < exited\n{t}"
    );
    let (sl, tl, el) = (&d.lines[sd], &d.lines[st], &d.lines[ex]);
    assert_eq!(head(sl), ("INFO", "sleeper", "shutdown"), "{t}");
    assert_eq!(sl.keys(), ["signal"], "{t}");
    assert_eq!(sl.get("signal").as_deref(), Some(sig_name), "{t}");
    assert_eq!(head(tl), ("INFO", "sleeper", "stop"), "{t}");
    assert_eq!(tl.keys(), ["pid", "signal", "timeout_ms"], "{t}");
    assert_eq!(tl.get("pid"), Some(pid.to_string()));
    assert_eq!(tl.get("signal").as_deref(), Some("SIGTERM"));
    assert_eq!(tl.get("timeout_ms").as_deref(), Some("5000"));
    assert_eq!(el.keys(), ["state", "pid", "signal"], "{t}");
    assert_eq!(el.get("pid"), Some(pid.to_string()));
    assert_eq!(el.get("signal").as_deref(), Some("SIGTERM"), "{t}");
    assert!(d.events("stop-timeout").is_empty(), "{t}");
}

// spec: B-M0-017 (SIGTERM, spec example)
#[test]
fn b_m0_017_sigterm_requested_stop() {
    requested_stop(SIGTERM, "SIGTERM");
}

// spec: B-M0-017 (SIGINT stops like SIGTERM; the service still gets SIGTERM)
#[test]
fn b_m0_017_sigint_requested_stop() {
    requested_stop(SIGINT, "SIGINT");
}

// spec: B-M0-017 ("any termination before the timeout is exited ... recorded with code=")
#[test]
fn b_m0_017_stop_records_exit_code() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let (f, ready) = fifo(&t.path, "ready");
    let script = "trap 'kill $!; exit 3' TERM; /usr/bin/sleep 60 & echo ready > \"$1\"; wait";
    let p = t.write(
        "s.toml",
        &unit_toml(
            "trapper",
            &["/bin/sh", "-c", script, "sh", f.to_str().expect("utf-8")],
            "",
        ),
    );
    let mut r = Run::unit(&p);
    r.wait_running();
    await_ready(&ready, "B-M0-017");
    let t0 = r.signal_xrginit(SIGTERM);
    let d = r.finish(EXIT_DEADLINE);
    let t = &d.text;
    assert!(
        d.at.duration_since(t0) < Duration::from_millis(2000),
        "B-M0-017: too slow\n{t}"
    );
    assert_eq!(
        d.code,
        Some(0),
        "B-M0-017: final state exited gives exit 0\n{t}"
    );
    d.check_run_invariants("trapper");
    let last = d.final_state();
    assert_eq!(head(last), ("INFO", "trapper", "state"), "{t}");
    assert_eq!(last.get("state").as_deref(), Some("exited"), "{t}");
    assert_eq!(last.keys(), ["state", "pid", "code"], "{t}");
    assert_eq!(last.get("code").as_deref(), Some("3"), "{t}");
}

/// Starts the B-M0-018 service (TERM ignored) and waits until its trap is in place.
/// The readiness echo is added to the spec's command so the test cannot race the trap.
fn deaf_service(t: &TempDir, name: &str) -> Run {
    let (f, ready) = fifo(&t.path, "ready");
    let script = "trap '' TERM; echo ready > \"$1\"; exec /usr/bin/sleep 60";
    let p = t.write(
        "deaf.toml",
        &unit_toml(
            name,
            &["/bin/sh", "-c", script, "sh", f.to_str().expect("utf-8")],
            "",
        ),
    );
    let mut r = Run::unit(&p);
    r.wait_running();
    await_ready(&ready, "B-M0-018");
    r
}

fn check_escalated(d: &Finished, name: &str, pid: u32, t0: Instant) {
    let t = &d.text;
    let took = d.at.duration_since(t0);
    assert!(
        took >= Duration::from_millis(5000),
        "B-M0-018: exited after {took:?}, before the 5000 ms timeout\n{t}"
    );
    assert!(
        took < Duration::from_millis(6000),
        "B-M0-018: exited after {took:?} (limit 6000 ms)\n{t}"
    );
    assert_eq!(d.code, Some(1), "B-M0-018: exit code\n{t}");
    d.check_run_invariants(name);
    assert_eq!(d.events("shutdown").len(), 1, "{t}");
    assert_eq!(d.events("stop").len(), 1, "{t}");
    assert_eq!(d.events("stop-timeout").len(), 1, "{t}");
    let st = d.index_of(|l| l.event == "stop").expect("stop");
    let to = d
        .index_of(|l| l.event == "stop-timeout")
        .expect("stop-timeout");
    let fa = d
        .index_of(|l| state_is(l, "failed"))
        .unwrap_or_else(|| panic!("no failed line\n{t}"));
    assert!(
        st < to && to < fa,
        "B-M0-018: order stop < stop-timeout < failed\n{t}"
    );
    let tl = &d.lines[to];
    assert_eq!(head(tl), ("WARN", name, "stop-timeout"), "{t}");
    assert_eq!(tl.keys(), ["pid", "signal"], "{t}");
    assert_eq!(tl.get("pid"), Some(pid.to_string()));
    assert_eq!(tl.get("signal").as_deref(), Some("SIGKILL"));
    let fl = &d.lines[fa];
    assert_eq!(head(fl), ("ERROR", name, "state"), "{t}");
    assert_eq!(fl.keys(), ["state", "pid", "signal"], "{t}");
    assert_eq!(fl.get("pid"), Some(pid.to_string()));
    assert_eq!(
        fl.get("signal").as_deref(),
        Some("SIGKILL"),
        "B-M0-018: the exit signal SIGKILL is recorded\n{t}"
    );
}

// spec: B-M0-018
#[test]
fn b_m0_018_service_ignores_sigterm_escalates() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let mut r = deaf_service(&t, "deaf");
    let pid = r.service.map(|s| s.0).expect("service pid recorded");
    let t0 = r.signal_xrginit(SIGTERM);
    let d = r.finish(EXIT_DEADLINE);
    check_escalated(&d, "deaf", pid, t0);
}

// spec: B-M0-017 ("a further SIGTERM or SIGINT during the stop changes nothing"), B-M0-018
#[test]
fn b_m0_017_further_signals_during_stop_change_nothing() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let mut r = deaf_service(&t, "deaf2");
    let pid = r.service.map(|s| s.0).expect("service pid recorded");
    let t0 = r.signal_xrginit(SIGTERM);
    r.wait_line("event=stop", |l| l.event == "stop");
    r.signal_xrginit(SIGINT);
    r.signal_xrginit(SIGTERM);
    let d = r.finish(EXIT_DEADLINE);
    check_escalated(&d, "deaf2", pid, t0);
    assert_eq!(
        d.events("shutdown")[0].get("signal").as_deref(),
        Some("SIGTERM"),
        "{}",
        d.text
    );
}

// --- B-M0-019 -------------------------------------------------------------

/// Kills a process verified by `verify_by_env` (never an unverified pid).
struct KillOnDrop(Option<(u32, u64)>);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some((p, st)) = self.0
            && is_live(p, st)
        {
            let _ = signal_verified((p, st), SIGKILL);
        }
    }
}

// spec: B-M0-019
#[test]
fn b_m0_019_double_fork_grandchild_untracked() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let pidfile = t.s("grandchild.pid");
    let script = "/usr/bin/sleep 30 </dev/null >/dev/null 2>&1 & echo $! > \"$1\"; exit 0";
    let p = t.write(
        "df.toml",
        &unit_toml("forker", &["/bin/sh", "-c", script, "sh", &pidfile], ""),
    );
    // A unique environment entry (inherited per B-M0-024) lets the test verify that the
    // pid in the pidfile is its own grandchild before it ever signals it.
    let (gk, gv) = guard_env();
    let mut c = Command::new(xrginit_bin());
    c.arg("run").arg(&p).env(&gk, &gv).stdin(Stdio::null());
    let d = Run::spawn(c).finish(EXIT_DEADLINE);
    let g: u32 = fs::read_to_string(&pidfile)
        .unwrap_or_else(|e| panic!("pidfile not written: {e}\n{}", d.text))
        .trim()
        .parse()
        .expect("grandchild pid");
    let gst = start_time(g);
    let _guard = KillOnDrop(verify_by_env(g, &gk, &gv).ok());
    let alive = gst.is_some_and(|s| is_live(g, s));
    let t = &d.text;
    assert_eq!(d.code, Some(0), "B-M0-019: exit code\n{t}");
    d.check_run_invariants("forker");
    let last = d.final_state();
    assert_eq!(last.get("state").as_deref(), Some("exited"), "{t}");
    assert_eq!(last.keys(), ["state", "pid", "code"], "{t}");
    assert_eq!(last.get("code").as_deref(), Some("0"), "{t}");
    assert_ne!(
        last.get("pid"),
        Some(g.to_string()),
        "B-M0-019: the reported pid is the direct child\n{t}"
    );
    assert!(
        alive,
        "B-M0-019: grandchild {g} must still be alive after xrginit exits\n{t}"
    );
    assert!(
        d.lines.iter().all(|l| l.get("pid") != Some(g.to_string())),
        "B-M0-019: xrginit must not report the grandchild\n{t}"
    );
}

// --- B-M0-010 through the CLI ----------------------------------------------

// spec: B-M0-010 ("behaves exactly as without them: one run, and no restart")
#[test]
fn b_m0_010_cli_restart_always_runs_once() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let counter = t.s("runs");
    let body = unit_toml(
        "once",
        &[
            "/bin/sh",
            "-c",
            "echo run >> \"$1\"; exit 1",
            "sh",
            &counter,
        ],
        "",
    );
    let text = format!(
        "{body}[dependencies]\nrequires = [\"xinit-m0-absent\"]\nafter = [\"xinit-m0-absent\"]\n[restart]\npolicy = \"always\"\nmaximum_restarts = 5\n"
    );
    let p = t.write("once.toml", &text);
    let d = Run::unit(&p).finish(EXIT_DEADLINE);
    let t = &d.text;
    assert_eq!(
        d.code,
        Some(1),
        "B-M0-010: the single run fails with code 1\n{t}"
    );
    d.check_run_invariants("once");
    assert_eq!(d.state_names(), ["starting", "running", "failed"], "{t}");
    let runs = fs::read_to_string(&counter).expect("counter file");
    assert_eq!(
        runs.lines().count(),
        1,
        "B-M0-010: the service ran {} times",
        runs.lines().count()
    );
}
