//! Shared helpers for the frozen xrginit suites. Spec: specs/M0.md, "Test harness facts",
//! and specs/ARCHITECTURE.md, "Finding the xrginit binary: XRGINIT_BIN".
//!
//! Output handling (CLAUDE.md rule 11, spec "Test harness facts"): xrginit's stdout and
//! stderr are pipes whose write ends the test also keeps. Reader threads drain both pipes
//! for the whole run. After xrginit has exited (observed by waiting on its pid), the test
//! writes a unique sentinel line into its own write end. Everything xrginit wrote is in the
//! pipe before that sentinel, so the readers stop at the sentinel and the test never waits
//! for EOF on a pipe a service or grandchild may still hold.
#![allow(dead_code, reason = "each suite uses a subset of the shared helpers")]

use std::ffi::{CString, c_char, c_int, c_long, c_uint, c_void};
use std::fs;
use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Binary and prerequisites
// ---------------------------------------------------------------------------

/// Path to the xrginit binary under test, from XRGINIT_BIN. No fallback, ever.
pub fn xrginit_bin() -> PathBuf {
    let Some(raw) = std::env::var_os("XRGINIT_BIN") else {
        panic!(
            "XRGINIT_BIN is not set: it must hold the absolute path of the xrginit binary under test \
             (there is no fallback path)"
        );
    };
    let path = PathBuf::from(&raw);
    if !path.is_absolute() {
        panic!("XRGINIT_BIN={raw:?} is not an absolute path");
    }
    let meta = fs::metadata(&path)
        .unwrap_or_else(|e| panic!("XRGINIT_BIN={} cannot be accessed: {e}", path.display()));
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        panic!("XRGINIT_BIN={} is not an executable file", path.display());
    }
    path
}

/// Test prerequisites from specs/M0.md. A missing one fails the test, naming it.
pub fn require_prereqs() {
    for p in [
        "/bin/sh",
        "/usr/bin/sleep",
        "/usr/bin/false",
        "/proc/self/stat",
    ] {
        assert!(
            Path::new(p).exists(),
            "test prerequisite {p} is missing (specs/M0.md, Test harness facts); this test cannot run"
        );
    }
}

// ---------------------------------------------------------------------------
// Child-process accounting (B-M0-016, B-M0-032)
// ---------------------------------------------------------------------------

static CHILD_LOCK: RwLock<()> = RwLock::new(());

/// Held by every test that creates child processes.
pub fn child_lock_shared() -> RwLockReadGuard<'static, ()> {
    CHILD_LOCK
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Held by tests that assert the test process has no children, so no concurrent
/// test creates one meanwhile.
pub fn child_lock_exclusive() -> RwLockWriteGuard<'static, ()> {
    CHILD_LOCK
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Pids whose ppid (field 4 of /proc/<pid>/stat) is this test process, running or zombie.
pub fn children_of_self() -> Vec<u32> {
    let me = std::process::id().to_string();
    let mut out = Vec::new();
    for e in fs::read_dir("/proc").expect("read /proc").flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if field(pid, 4).as_deref() == Some(me.as_str()) {
            out.push(pid);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// /proc helpers
// ---------------------------------------------------------------------------

/// Field `n` (1-based, as in proc(5)) of /proc/<pid>/stat.
pub fn field(pid: u32, n: usize) -> Option<String> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    field_in(&s, n)
}

/// Field `n` (1-based, as in proc(5)) of the text of one /proc/<pid>/stat read.
fn field_in(s: &str, n: usize) -> Option<String> {
    if n == 1 {
        return s.split_whitespace().next().map(String::from);
    }
    let close = s.rfind(')')?;
    if n == 2 {
        return Some(s[..=close].split_once(' ')?.1.to_string());
    }
    s[close + 1..]
        .split_whitespace()
        .nth(n - 3)
        .map(String::from)
}

/// Start time (field 22) of a pid, if it exists.
pub fn start_time(pid: u32) -> Option<u64> {
    field(pid, 22).and_then(|s| s.parse().ok())
}

/// The process (pid, start time) still exists, running or zombie.
pub fn same_process_exists(pid: u32, start: u64) -> bool {
    start_time(pid) == Some(start)
}

/// The process (pid, start time) exists and is not a zombie.
pub fn is_live(pid: u32, start: u64) -> bool {
    same_process_exists(pid, start) && field(pid, 3).as_deref() != Some("Z")
}

/// Value of a line of /proc/<pid>/status, whitespace-normalised.
pub fn status_value(pid: u32, key: &str) -> String {
    let s = fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_else(|e| panic!("cannot read /proc/{pid}/status: {e}"));
    s.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}:")))
        .map(|v| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| panic!("no {key}: line in /proc/{pid}/status"))
}

/// A hex mask line (SigBlk, SigIgn, SigCgt) of /proc/<pid>/status.
pub fn status_mask(pid: u32, key: &str) -> u64 {
    let v = status_value(pid, key);
    u64::from_str_radix(&v, 16).unwrap_or_else(|e| panic!("{key}: {v:?} is not hex: {e}"))
}

/// (effective uid, effective gid) of the test process.
pub fn self_euid_egid() -> (u32, u32) {
    let me = std::process::id();
    let uid = status_value(me, "Uid");
    let gid = status_value(me, "Gid");
    let second = |v: &str| {
        v.split(' ')
            .nth(1)
            .and_then(|x| x.parse().ok())
            .expect("Uid/Gid line")
    };
    (second(&uid), second(&gid))
}

/// One /etc/passwd entry.
#[derive(Debug, Clone)]
pub struct PasswdEntry {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
}

/// All parseable /etc/passwd entries.
pub fn passwd_entries() -> Vec<PasswdEntry> {
    let text = fs::read_to_string("/etc/passwd")
        .unwrap_or_else(|e| panic!("test requirement /etc/passwd is unreadable: {e}"));
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            (f.len() >= 4).then_some(())?;
            Some(PasswdEntry {
                name: f[0].to_string(),
                uid: f[2].parse().ok()?,
                gid: f[3].parse().ok()?,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Signals and FIFOs (FFI)
// ---------------------------------------------------------------------------

pub const SIGINT: i32 = 2;
pub const SIGKILL: i32 = 9;
pub const SIGTERM: i32 = 15;

unsafe extern "C" {
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn syscall(num: c_long, ...) -> c_long;
    fn mkfifo(path: *const c_char, mode: u32) -> c_int;
}

// Syscall numbers from the unified table (identical on x86_64 and aarch64).
const SYS_PIDFD_SEND_SIGNAL: c_long = 424;
const SYS_PIDFD_OPEN: c_long = 434;
/// include/uapi/linux/pidfd.h (Linux 6.9): signal the process group of the pidfd's process.
const PIDFD_SIGNAL_PROCESS_GROUP: c_uint = 1 << 2;
const EINVAL: i32 = 22;

// Signal safety. Every pid a test signals is either its own spawned xrginit (signalled
// through a pidfd opened before the child can be reaped) or a process the test verified
// (`verify_child`, `verify_by_env`) and identifies by (pid, start time). Nothing ever
// signals a pid <= 1 or a group whose id is <= 1, so a lying xrginit that reports
// `pid=1` or `pid=0` cannot make the test signal init, every process of the user
// (kill(-1)) or the test's own process group (kill(0)).

/// pidfd_open(2) for `pid`, which must be > 1.
fn pidfd_open(pid: u32) -> std::io::Result<OwnedFd> {
    if pid <= 1 {
        return Err(std::io::Error::from_raw_os_error(EINVAL));
    }
    let raw = c_int::try_from(pid).map_err(|_| std::io::Error::from_raw_os_error(EINVAL))?;
    // SAFETY: pidfd_open takes a pid and a flags word (0) and touches no memory of ours.
    let fd = unsafe { syscall(SYS_PIDFD_OPEN, raw, 0 as c_uint) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let fd = RawFd::try_from(fd).map_err(|_| std::io::Error::from_raw_os_error(EINVAL))?;
    // SAFETY: `fd` is a descriptor the kernel has just created for us; nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// pidfd_send_signal(2) with no siginfo.
fn pidfd_send(fd: &OwnedFd, sig: i32, flags: c_uint) -> std::io::Result<()> {
    let info: *const c_void = std::ptr::null();
    // SAFETY: the pidfd is open for the duration of the call and the siginfo pointer is
    // NULL, so the kernel reads no memory of ours.
    let rc = unsafe { syscall(SYS_PIDFD_SEND_SIGNAL, fd.as_raw_fd(), sig, info, flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Opens a pidfd for the verified process `(pid, start)` and checks, after opening,
/// that the pid still names that process. Returns the pidfd and the stat text.
fn open_verified(v: (u32, u64)) -> Result<(OwnedFd, String), String> {
    let (pid, start) = v;
    if pid <= 1 {
        return Err(format!("refusing to signal pid {pid} (<= 1)"));
    }
    let fd = pidfd_open(pid).map_err(|e| format!("pidfd_open({pid}): {e}"))?;
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|e| format!("pid {pid} is gone: {e}"))?;
    let now = field_in(&stat, 22).and_then(|s| s.parse::<u64>().ok());
    if now != Some(start) {
        return Err(format!(
            "pid {pid} no longer names the verified process (start time {now:?}, verified {start})"
        ));
    }
    Ok((fd, stat))
}

/// Verifies that `pid` is a direct child of `parent` (field 4 of /proc/<pid>/stat) and
/// returns (pid, start time) from the same read. Fails for pid <= 1.
pub fn verify_child(pid: u32, parent: u32) -> Result<(u32, u64), String> {
    if pid <= 1 {
        return Err(format!("pid {pid} is <= 1"));
    }
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|e| format!("pid {pid} does not exist: {e}"))?;
    let ppid = field_in(&stat, 4);
    if ppid.as_deref() != Some(parent.to_string().as_str()) {
        return Err(format!(
            "pid {pid} is not a direct child of xrginit {parent} (its parent is {ppid:?})"
        ));
    }
    let start = field_in(&stat, 22)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("pid {pid}: no start time"))?;
    Ok((pid, start))
}

/// A unique `(key, value)` environment entry. A process whose environ carries it was
/// started (directly or indirectly) by a command the test gave it to.
pub fn guard_env() -> (String, String) {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    (
        "XINIT_M0_GUARD".to_string(),
        format!(
            "{}-{}-{nanos}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ),
    )
}

/// Verifies that `pid` (> 1) carries the environment entry `key=value` from
/// [`guard_env`], and returns (pid, start time) for a later [`signal_verified`].
pub fn verify_by_env(pid: u32, key: &str, value: &str) -> Result<(u32, u64), String> {
    if pid <= 1 {
        return Err(format!("pid {pid} is <= 1"));
    }
    let start = start_time(pid).ok_or_else(|| format!("pid {pid} does not exist"))?;
    let env = fs::read(format!("/proc/{pid}/environ"))
        .map_err(|e| format!("/proc/{pid}/environ: {e}"))?;
    let want = format!("{key}={value}");
    if !env.split(|&b| b == 0).any(|e| e == want.as_bytes()) {
        return Err(format!("pid {pid} does not carry {want}"));
    }
    if start_time(pid) != Some(start) {
        return Err(format!("pid {pid} changed while it was verified"));
    }
    Ok((pid, start))
}

/// Signals the verified process `(pid, start)` through a pidfd, so a recycled pid is
/// never reached.
pub fn signal_verified(v: (u32, u64), sig: i32) -> Result<(), String> {
    let (fd, _) = open_verified(v)?;
    pidfd_send(&fd, sig, 0).map_err(|e| format!("pidfd_send_signal({}, {sig}): {e}", v.0))
}

/// Signals the process group led by the verified process `(pid, start)`. Only when that
/// process is still the leader of its own group (field 5 == pid, so pgid == pid > 1).
pub fn signal_verified_group(v: (u32, u64), sig: i32) -> Result<(), String> {
    let (fd, stat) = open_verified(v)?;
    let pid = v.0;
    if field_in(&stat, 5).as_deref() != Some(pid.to_string().as_str()) {
        return Err(format!("pid {pid} does not lead its own process group"));
    }
    match pidfd_send(&fd, sig, PIDFD_SIGNAL_PROCESS_GROUP) {
        Ok(()) => Ok(()),
        // Kernels before 6.9 do not know the flag: fall back to kill(-pgid), still
        // guarded by the checks above (pgid == verified leader pid > 1).
        Err(e) if e.raw_os_error() == Some(EINVAL) => kill_group(pid, sig),
        Err(e) => Err(format!("pidfd_send_signal(group {pid}, {sig}): {e}")),
    }
}

/// kill(-pgid, sig) for pgid > 1. The only raw kill(2) call in the suite.
fn kill_group(pgid: u32, sig: i32) -> Result<(), String> {
    let g = c_int::try_from(pgid).map_err(|_| format!("pgid {pgid} out of range"))?;
    if g <= 1 {
        return Err(format!("refusing to signal process group {pgid} (<= 1)"));
    }
    // SAFETY: kill takes two plain integers and touches no memory of ours; -g < -1.
    if unsafe { kill(-g, sig) } == 0 {
        Ok(())
    } else {
        Err(format!(
            "kill(-{g}, {sig}): {}",
            std::io::Error::last_os_error()
        ))
    }
}

/// Creates a FIFO and a thread that reads it to the end. The receiver yields the
/// text once a writer has opened, written and closed it. Used as a readiness signal
/// from a service script, so no test ever polls or sleeps.
pub fn fifo(dir: &Path, name: &str) -> (PathBuf, Receiver<String>) {
    let p = dir.join(name);
    let c = CString::new(p.as_os_str().as_bytes()).expect("path has no NUL");
    // SAFETY: `c` is a valid NUL-terminated path that outlives the call.
    let rc = unsafe { mkfifo(c.as_ptr(), 0o600) };
    assert_eq!(rc, 0, "mkfifo {} failed", p.display());
    let (tx, rx) = mpsc::channel();
    let reader_path = p.clone();
    thread::spawn(move || {
        let mut s = String::new();
        if let Ok(mut f) = fs::File::open(&reader_path) {
            let _ = f.read_to_string(&mut s);
        }
        let _ = tx.send(s);
    });
    (p, rx)
}

/// Waits (with a deadline) for the readiness text written into a [`fifo`].
pub fn await_ready(rx: &Receiver<String>, what: &str) {
    match rx.recv_timeout(LINE_DEADLINE) {
        Ok(s) if s.trim() == "ready" => {}
        Ok(s) => panic!("{what}: unexpected readiness text {s:?}"),
        Err(_) => panic!("{what}: service did not report readiness within {LINE_DEADLINE:?}"),
    }
}

// ---------------------------------------------------------------------------
// Temp dirs and unit text
// ---------------------------------------------------------------------------

/// A fresh directory under the system temp dir, removed on drop. Its path is canonical.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new() -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "xinit-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap_or_else(|e| panic!("create {}: {e}", p.display()));
        let path = fs::canonicalize(&p).expect("canonicalize temp dir");
        TempDir { path }
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.path.join(rel)
    }

    /// The joined path as a UTF-8 string, for use inside unit files.
    pub fn s(&self, rel: &str) -> String {
        self.join(rel)
            .to_str()
            .expect("UTF-8 temp path")
            .to_string()
    }

    pub fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let p = self.join(rel);
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).expect("create parent dir");
        }
        fs::write(&p, contents).unwrap_or_else(|e| panic!("write {}: {e}", p.display()));
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A TOML basic string.
pub fn toml_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => o.push_str(&format!("\\u{:04X}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// A TOML array of basic strings.
pub fn toml_array(items: &[&str]) -> String {
    let v: Vec<String> = items.iter().map(|s| toml_str(s)).collect();
    format!("[{}]", v.join(", "))
}

/// A unit in the shape of the B-M0-001 example. `exec_extra` is appended inside
/// `[execution]` (for example `user = "x"\n`), and may be followed by more tables.
pub fn unit_toml(name: &str, argv: &[&str], exec_extra: &str) -> String {
    format!(
        "schema_version = 1\n[service]\nname = {}\ntype = \"simple\"\n[execution]\ncommand = {}\n{exec_extra}",
        toml_str(name),
        toml_array(argv)
    )
}

// ---------------------------------------------------------------------------
// Log line format (B-M0-028)
// ---------------------------------------------------------------------------

/// One parsed stderr line of xrginit.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub raw: String,
    pub level: String,
    pub unit: String,
    pub event: String,
    /// Keys in order, with decoded value bytes.
    pub kv: Vec<(String, Vec<u8>)>,
}

impl LogLine {
    pub fn get(&self, key: &str) -> Option<String> {
        self.kv
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
    }
    pub fn get_bytes(&self, key: &str) -> Option<&[u8]> {
        self.kv
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_slice())
    }
    pub fn keys(&self) -> Vec<&str> {
        self.kv.iter().map(|(k, _)| k.as_str()).collect()
    }
    /// The `state=` value of an `event=state` line.
    pub fn state(&self) -> Option<String> {
        if self.event == "state" {
            self.get("state")
        } else {
            None
        }
    }
}

fn is_bare_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"._/:+@%,-".contains(&b)
}

fn is_unit_value(u: &str) -> bool {
    let b = u.as_bytes();
    u == "-"
        || (!b.is_empty()
            && b.len() <= 64
            && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
            && b.iter()
                .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c)))
}

/// Strict check of the B-M0-028 format:
/// `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z (INFO|WARN|ERROR) unit=(-|NAME) event=[a-z-]+( [a-z_]+=(VALUE))*$`
/// with VALUE bare when it matches `[A-Za-z0-9._/:+@%,-]+`, otherwise quoted with the
/// escapes `\\ \" \n \t \xHH` (`\xHH` only for bytes below 0x20 other than newline and
/// tab, and bytes 0x7F and above).
pub fn parse_log_line(line: &str) -> Result<LogLine, String> {
    let b = line.as_bytes();
    if !line.is_ascii() {
        return Err("line contains non-ASCII bytes (they must be written as \\xHH)".into());
    }
    if b.len() < 25 {
        return Err("line too short for timestamp and level".into());
    }
    for (i, &c) in b[..24].iter().enumerate() {
        let ok = match i {
            4 | 7 => c == b'-',
            10 => c == b'T',
            13 | 16 => c == b':',
            19 => c == b'.',
            23 => c == b'Z',
            _ => c.is_ascii_digit(),
        };
        if !ok {
            return Err(format!("timestamp: unexpected byte at offset {i}"));
        }
    }
    if b[24] != b' ' {
        return Err("no space after timestamp".into());
    }
    let mut parts = line[25..].splitn(3, ' ');
    let level = parts.next().unwrap_or_default().to_string();
    if !matches!(level.as_str(), "INFO" | "WARN" | "ERROR") {
        return Err(format!("level {level:?}"));
    }
    let unit = parts
        .next()
        .and_then(|u| u.strip_prefix("unit="))
        .ok_or("no unit= field")?
        .to_string();
    if !is_unit_value(&unit) {
        return Err(format!("unit value {unit:?}"));
    }
    let rest = parts
        .next()
        .and_then(|r| r.strip_prefix("event="))
        .ok_or("no event= field")?;
    let end = rest.find(' ').unwrap_or(rest.len());
    let event = rest[..end].to_string();
    if event.is_empty() || !event.bytes().all(|c| c.is_ascii_lowercase() || c == b'-') {
        return Err(format!("event value {event:?}"));
    }
    let mut r = &rest.as_bytes()[end..];
    let mut kv = Vec::new();
    while !r.is_empty() {
        if r[0] != b' ' {
            return Err("fields must be separated by one space".into());
        }
        r = &r[1..];
        let eq = r
            .iter()
            .position(|&c| c == b'=')
            .ok_or("field without '='")?;
        let key = String::from_utf8_lossy(&r[..eq]).into_owned();
        if key.is_empty() || !key.bytes().all(|c| c.is_ascii_lowercase() || c == b'_') {
            return Err(format!("key {key:?}"));
        }
        r = &r[eq + 1..];
        let mut v = Vec::new();
        if r.first() == Some(&b'"') {
            let mut i = 1;
            loop {
                let c = *r.get(i).ok_or("unterminated quoted value")?;
                i += 1;
                match c {
                    b'"' => break,
                    b'\\' => {
                        let e = *r.get(i).ok_or("dangling backslash")?;
                        i += 1;
                        match e {
                            b'\\' | b'"' => v.push(e),
                            b'n' => v.push(b'\n'),
                            b't' => v.push(b'\t'),
                            b'x' => {
                                let h = r.get(i..i + 2).ok_or("short \\x escape")?;
                                let h = std::str::from_utf8(h).map_err(|_| "bad \\x escape")?;
                                let x = u8::from_str_radix(h, 16).map_err(|_| "bad \\x escape")?;
                                let allowed = (x < 0x20 && x != b'\n' && x != b'\t') || x >= 0x7f;
                                if !allowed {
                                    return Err(format!(
                                        "\\x{h} used for a byte that has another spelling"
                                    ));
                                }
                                v.push(x);
                                i += 2;
                            }
                            _ => return Err(format!("unknown escape \\{}", e as char)),
                        }
                    }
                    0x20..=0x7e => v.push(c),
                    _ => return Err("raw control byte inside quoted value".into()),
                }
            }
            if !v.is_empty() && v.iter().all(|&c| is_bare_byte(c)) {
                return Err(format!("value for {key} is quoted but must be bare"));
            }
            r = &r[i..];
        } else {
            let n = r.iter().take_while(|&&c| is_bare_byte(c)).count();
            if n == 0 {
                return Err(format!("empty or invalid bare value for {key}"));
            }
            v.extend_from_slice(&r[..n]);
            r = &r[n..];
        }
        if !r.is_empty() && r[0] != b' ' {
            return Err(format!("unexpected byte after value of {key}"));
        }
        kv.push((key, v));
    }
    Ok(LogLine {
        raw: line.to_string(),
        level,
        unit,
        event,
        kv,
    })
}

// ---------------------------------------------------------------------------
// Running xrginit
// ---------------------------------------------------------------------------

/// Deadline for a line a test waits for.
pub const LINE_DEADLINE: Duration = Duration::from_secs(10);
/// Deadline for xrginit to exit in tests that do not measure time.
pub const EXIT_DEADLINE: Duration = Duration::from_secs(20);

enum Msg {
    Err(Vec<u8>),
    ErrDone,
    Out(Vec<u8>),
    OutDone,
    Exit(ExitStatus, Instant),
}

fn pump(r: PipeReader, sentinel: Vec<u8>, tx: Sender<Msg>, is_err: bool) {
    let mut br = BufReader::new(r);
    loop {
        let mut buf = Vec::new();
        match br.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let done = buf.ends_with(&sentinel);
                if done {
                    buf.truncate(buf.len() - sentinel.len());
                }
                if is_err {
                    if buf.last() == Some(&b'\n') {
                        buf.pop();
                    }
                    if !buf.is_empty() || !done {
                        let _ = tx.send(Msg::Err(buf));
                    }
                } else if !buf.is_empty() {
                    let _ = tx.send(Msg::Out(buf));
                }
                if done {
                    break;
                }
            }
        }
    }
    let _ = tx.send(if is_err { Msg::ErrDone } else { Msg::OutDone });
}

/// A running xrginit process.
pub struct Run {
    pub pid: u32,
    rx: Receiver<Msg>,
    err_w: Option<PipeWriter>,
    out_w: Option<PipeWriter>,
    sentinel: Vec<u8>,
    raw: Vec<Vec<u8>>,
    cursor: usize,
    stdout: Vec<u8>,
    exit: Option<(ExitStatus, Instant)>,
    err_done: bool,
    out_done: bool,
    /// pidfd of xrginit, opened before the child can be reaped.
    xr_fd: OwnedFd,
    /// The pid of the first `running` line, as xrginit reported it (unverified).
    pub reported: Option<u32>,
    /// (pid, start time) of the service, recorded when its `running` line is seen and
    /// only if that pid was then a direct child of this xrginit (`verify_child`).
    pub service: Option<(u32, u64)>,
}

impl Run {
    /// `xrginit run <unit>` with stdin from /dev/null.
    pub fn unit(path: &Path) -> Run {
        let mut c = Command::new(xrginit_bin());
        c.arg("run").arg(path).stdin(Stdio::null());
        Run::spawn(c)
    }

    /// Spawns `cmd` (which must be xrginit, or a shell that execs it) with piped
    /// stdout and stderr.
    pub fn spawn(mut cmd: Command) -> Run {
        require_prereqs();
        static N: AtomicU64 = AtomicU64::new(0);
        let sentinel = format!(
            "\u{1}xinit-test-sentinel-{}-{}\u{1}\n",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        )
        .into_bytes();
        let (err_r, err_w) = std::io::pipe().expect("pipe");
        let (out_r, out_w) = std::io::pipe().expect("pipe");
        cmd.stderr(err_w.try_clone().expect("dup"))
            .stdout(out_w.try_clone().expect("dup"));
        let mut child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("cannot spawn {:?}: {e}", cmd.get_program()));
        drop(cmd);
        let pid = child.id();
        // The child is not reaped before the waiter thread below starts, so this pidfd
        // refers to it and never to a recycled pid.
        let xr_fd = pidfd_open(pid).unwrap_or_else(|e| {
            let _ = child.kill();
            let _ = child.wait();
            panic!("test requirement: pidfd_open({pid}) failed: {e} (pidfd support, Linux 5.3+)")
        });
        let (tx, rx) = mpsc::channel();
        let (s1, t1) = (sentinel.clone(), tx.clone());
        thread::spawn(move || pump(err_r, s1, t1, true));
        let (s2, t2) = (sentinel.clone(), tx.clone());
        thread::spawn(move || pump(out_r, s2, t2, false));
        thread::spawn(move || {
            if let Ok(st) = child.wait() {
                let _ = tx.send(Msg::Exit(st, Instant::now()));
            }
        });
        Run {
            pid,
            rx,
            err_w: Some(err_w),
            out_w: Some(out_w),
            sentinel,
            raw: Vec::new(),
            cursor: 0,
            stdout: Vec::new(),
            exit: None,
            err_done: false,
            out_done: false,
            xr_fd,
            reported: None,
            service: None,
        }
    }

    /// The verified service (pid, start time). Panics, naming the pid, when the pid of
    /// the running line was not verified as a direct child of this xrginit: the test
    /// then refuses to signal it.
    pub fn service_verified(&self) -> (u32, u64) {
        self.service.unwrap_or_else(|| {
            panic!(
                "refusing to signal the service: pid {:?} from the running line was not verified \
                 as a direct child of xrginit {} (B-M0-013, B-M0-019)\n{}",
                self.reported,
                self.pid,
                self.describe()
            )
        })
    }

    /// Signals the verified service process (see [`Run::service_verified`]).
    pub fn signal_service(&self, sig: i32) -> Result<(), String> {
        signal_verified(self.service_verified(), sig)
    }

    /// Signals the process group led by the verified service (B-M0-022: pgid == P).
    pub fn signal_service_group(&self, sig: i32) -> Result<(), String> {
        signal_verified_group(self.service_verified(), sig)
    }

    fn handle(&mut self, m: Msg) {
        match m {
            Msg::Err(l) => self.raw.push(l),
            Msg::ErrDone => self.err_done = true,
            Msg::Out(b) => self.stdout.extend(b),
            Msg::OutDone => self.out_done = true,
            Msg::Exit(s, at) => self.exit = Some((s, at)),
        }
    }

    fn write_sentinels(&mut self) {
        for w in [self.err_w.take(), self.out_w.take()].into_iter().flatten() {
            let mut w = w;
            let _ = w.write_all(&self.sentinel);
        }
    }

    fn drained(&self) -> bool {
        self.err_done && self.out_done
    }

    fn describe(&self) -> String {
        let lines: Vec<String> = self
            .raw
            .iter()
            .map(|l| String::from_utf8_lossy(l).into_owned())
            .collect();
        format!(
            "exit: {:?}\nstderr:\n{}",
            self.exit.map(|e| e.0),
            lines.join("\n")
        )
    }

    /// Sends a signal to xrginit and returns the instant just before sending it.
    pub fn signal_xrginit(&mut self, sig: i32) -> Instant {
        while let Ok(m) = self.rx.try_recv() {
            self.handle(m);
        }
        assert!(
            self.exit.is_none(),
            "xrginit exited before the test could signal it\n{}",
            self.describe()
        );
        let t = Instant::now();
        if let Err(e) = pidfd_send(&self.xr_fd, sig, 0) {
            panic!("signal {sig} to xrginit {} failed: {e}", self.pid);
        }
        t
    }

    /// Reads stderr lines (with a deadline) until one satisfies `pred`. Every line read
    /// must match the B-M0-028 format.
    pub fn wait_line(&mut self, what: &str, pred: impl Fn(&LogLine) -> bool) -> LogLine {
        let deadline = Instant::now() + LINE_DEADLINE;
        loop {
            while self.cursor < self.raw.len() {
                let text = String::from_utf8_lossy(&self.raw[self.cursor]).into_owned();
                self.cursor += 1;
                let line = parse_log_line(&text).unwrap_or_else(|e| {
                    panic!(
                        "B-M0-028: bad log line ({e}): {text:?}\n{}",
                        self.describe()
                    )
                });
                if line.state().as_deref() == Some("running")
                    && self.reported.is_none()
                    && let Some(pid) = line.get("pid").and_then(|p| p.parse::<u32>().ok())
                {
                    self.reported = Some(pid);
                    self.service = verify_child(pid, self.pid).ok();
                }
                if pred(&line) {
                    return line;
                }
            }
            if self.exit.is_some() {
                if self.drained() {
                    panic!(
                        "xrginit exited without writing a {what} line\n{}",
                        self.describe()
                    );
                }
                self.write_sentinels();
                let m = self
                    .rx
                    .recv()
                    .expect("reader threads end only after the sentinel");
                self.handle(m);
                continue;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(m) => self.handle(m),
                Err(_) => panic!(
                    "no {what} line within {LINE_DEADLINE:?}\n{}",
                    self.describe()
                ),
            }
        }
    }

    /// Waits for the `state=running` line and returns the service pid.
    pub fn wait_running(&mut self) -> u32 {
        let l = self.wait_line("state=running", |l| l.state().as_deref() == Some("running"));
        l.get("pid")
            .and_then(|p| p.parse().ok())
            .unwrap_or_else(|| panic!("running line without a numeric pid: {}", l.raw))
    }

    /// Waits for xrginit to exit (deadline `limit`), then collects the output that is
    /// already in the pipes.
    pub fn finish(mut self, limit: Duration) -> Finished {
        let deadline = Instant::now() + limit;
        while self.exit.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(m) => self.handle(m),
                Err(_) => panic!("xrginit did not exit within {limit:?}\n{}", self.describe()),
            }
        }
        self.write_sentinels();
        while !self.drained() {
            let m = self
                .rx
                .recv()
                .expect("reader threads end only after the sentinel");
            self.handle(m);
        }
        let text = self.describe();
        let raw = std::mem::take(&mut self.raw);
        let lines: Vec<LogLine> = raw
            .iter()
            .map(|l| {
                let s = String::from_utf8_lossy(l).into_owned();
                parse_log_line(&s)
                    .unwrap_or_else(|e| panic!("B-M0-028: bad log line ({e}): {s:?}\n{text}"))
            })
            .collect();
        let (status, at) = self.exit.expect("exit recorded");
        Finished {
            status,
            code: status.code(),
            at,
            lines,
            stdout: std::mem::take(&mut self.stdout),
            text,
            service: self.service,
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            self.handle(m);
        }
        if self.exit.is_none() {
            let _ = pidfd_send(&self.xr_fd, SIGKILL, 0);
        }
        // Only a verified service is signalled; both calls re-check its identity.
        if let Some(v) = self.service
            && is_live(v.0, v.1)
        {
            let _ = signal_verified_group(v, SIGKILL);
            let _ = signal_verified(v, SIGKILL);
        }
        self.write_sentinels();
    }
}

/// The outcome of a finished xrginit run.
pub struct Finished {
    pub status: ExitStatus,
    pub code: Option<i32>,
    pub at: Instant,
    pub lines: Vec<LogLine>,
    pub stdout: Vec<u8>,
    pub text: String,
    pub service: Option<(u32, u64)>,
}

impl Finished {
    pub fn states(&self) -> Vec<&LogLine> {
        self.lines.iter().filter(|l| l.event == "state").collect()
    }
    pub fn state_names(&self) -> Vec<String> {
        self.states().iter().filter_map(|l| l.state()).collect()
    }
    pub fn final_state(&self) -> &LogLine {
        self.states()
            .last()
            .copied()
            .unwrap_or_else(|| panic!("no state line\n{}", self.text))
    }
    pub fn events(&self, event: &str) -> Vec<&LogLine> {
        self.lines.iter().filter(|l| l.event == event).collect()
    }
    /// Position of the first line with this event (and state, for `state`).
    pub fn index_of(&self, pred: impl Fn(&LogLine) -> bool) -> Option<usize> {
        self.lines.iter().position(pred)
    }

    /// I-2, I-3, I-4, I-5 for a run that loaded the unit `unit`.
    pub fn check_run_invariants(&self, unit: &str) {
        let t = &self.text;
        assert!(
            self.stdout.is_empty(),
            "I-5: xrginit wrote to stdout: {:?}\n{t}",
            String::from_utf8_lossy(&self.stdout)
        );
        let s = self.state_names();
        assert_eq!(
            s.first().map(String::as_str),
            Some("starting"),
            "I-2: first state must be starting\n{t}"
        );
        assert_eq!(
            s.iter().filter(|x| *x == "starting").count(),
            1,
            "I-2: exactly one starting\n{t}"
        );
        assert!(
            s.iter().filter(|x| *x == "running").count() <= 1,
            "I-2: at most one running\n{t}"
        );
        let finals = s
            .iter()
            .filter(|x| *x == "exited" || *x == "failed")
            .count();
        assert_eq!(finals, 1, "I-2: exactly one final state line\n{t}");
        assert!(
            matches!(s.last().map(String::as_str), Some("exited" | "failed")),
            "I-2: final state last\n{t}"
        );
        if let Some(i) = s.iter().position(|x| x == "running") {
            assert_eq!(i, 1, "I-2: running must directly follow starting\n{t}");
        }
        for l in self.states() {
            assert_eq!(l.unit, unit, "state lines carry unit={unit}\n{t}");
            let want = if l.state().as_deref() == Some("failed") {
                "ERROR"
            } else {
                "INFO"
            };
            assert_eq!(
                l.level,
                want,
                "B-M0-028: level of state={:?}\n{t}",
                l.state()
            );
        }
        if let Some(r) = self
            .states()
            .into_iter()
            .find(|l| l.state().as_deref() == Some("running"))
        {
            assert_eq!(
                self.final_state().get("pid"),
                r.get("pid"),
                "I-3: final pid equals running pid\n{t}"
            );
        }
        if let Some((p, st)) = self.service {
            assert!(
                !same_process_exists(p, st),
                "I-4: service {p} still exists after xrginit exited\n{t}"
            );
        }
    }
}
