//! `xrginit run <unit-file>`: load one unit, start it, supervise it until it ends.
//! Spec: specs/M0.md, "CLI" and B-M0-011 to B-M0-019, B-M0-027.

use crate::log::{Level, Value, emit, errno_name, signal_name, v};
use std::ffi::OsString;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Instant;
use xinit_api::{
    DEFAULT_STOP_TIMEOUT, Service, Signal, SignalError, StartError, Termination, UnitError,
    load_unit,
};

/// Exit codes (spec, "CLI").
const EXIT_EXITED: u8 = 0;
const EXIT_FAILED: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_UNIT_INVALID: u8 = 3;
const EXIT_START_FAILED: u8 = 4;
const EXIT_INTERNAL: u8 = 5;

fn errno() -> i32 {
    // SAFETY: __errno_location returns a valid pointer to this thread's errno.
    unsafe { *libc::__errno_location() }
}

fn internal_error(unit: &str, detail: &str) -> u8 {
    emit(
        Level::Error,
        unit,
        "internal-error",
        &[("detail", v(detail))],
    );
    EXIT_INTERNAL
}

/// The `error=` kind and extra keys of a unit error (B-M0-012).
fn unit_error_fields(e: &UnitError) -> (&'static str, Vec<(&'static str, Value)>) {
    match e {
        UnitError::Io { errno } => ("io", vec![("errno", v(errno_name(*errno)))]),
        UnitError::Syntax => ("syntax", Vec::new()),
        UnitError::SchemaVersion { .. } => ("schema-version", Vec::new()),
        UnitError::UnknownKey { key } => ("unknown-key", vec![("key", v(key))]),
        UnitError::MissingKey { key } => ("missing-key", vec![("key", v(key))]),
        UnitError::InvalidType { key } => ("invalid-type", vec![("key", v(key))]),
        UnitError::InvalidValue { key } => ("invalid-value", vec![("key", v(key))]),
        UnitError::UnsupportedServiceType { .. } => ("unsupported-service-type", Vec::new()),
    }
}

/// The `start-failed` keys of a start error (B-M0-016, B-M0-025, B-M0-026).
fn start_error_fields(e: &StartError) -> Vec<(&'static str, Value)> {
    match e {
        StartError::Exec { errno } => vec![("error", v("exec")), ("errno", v(errno_name(*errno)))],
        StartError::UserUnknown { user } => vec![("error", v("user-unknown")), ("user", v(user))],
        StartError::Credentials { errno } => vec![
            ("error", v("credentials")),
            ("errno", v(errno_name(*errno))),
        ],
        StartError::Spawn { errno } => {
            vec![("error", v("spawn")), ("errno", v(errno_name(*errno)))]
        }
    }
}

/// Removes the SIGSEGV and SIGBUS handlers the Rust runtime installs, so xrginit
/// catches only the signals it acts on (B-M0-027).
fn reset_runtime_handlers() -> Result<(), i32> {
    for sig in [libc::SIGSEGV, libc::SIGBUS] {
        // SAFETY: sigaction is plain data; all-zero means SIG_DFL, no flags, empty mask.
        let dfl: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: dfl is a valid sigaction; the old-action pointer is null.
        if unsafe { libc::sigaction(sig, &raw const dfl, std::ptr::null_mut()) } == -1 {
            return Err(errno());
        }
    }
    Ok(())
}

/// Blocks SIGINT and SIGTERM and returns a signalfd that receives them (B-M0-017, B-M0-027).
fn stop_signalfd() -> Result<OwnedFd, i32> {
    // SAFETY: sigset_t is plain data; sigemptyset initialises it.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: set is a valid sigset_t; the signal numbers are valid.
    let ok = unsafe {
        libc::sigemptyset(&raw mut set) == 0
            && libc::sigaddset(&raw mut set, libc::SIGINT) == 0
            && libc::sigaddset(&raw mut set, libc::SIGTERM) == 0
    };
    if !ok {
        return Err(errno());
    }
    // SAFETY: set is initialised; the old-set pointer is null.
    if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &raw const set, std::ptr::null_mut()) } == -1 {
        return Err(errno());
    }
    // SAFETY: set is initialised; -1 asks for a new fd.
    let fd = unsafe { libc::signalfd(-1, &raw const set, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) };
    if fd == -1 {
        return Err(errno());
    }
    // SAFETY: signalfd returned a new fd that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Reads every pending signal from the signalfd. Returns the first one, if any.
fn drain_signals(fd: &OwnedFd) -> Result<Option<i32>, i32> {
    let mut first = None;
    loop {
        // SAFETY: signalfd_siginfo is plain data.
        let mut info: libc::signalfd_siginfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::signalfd_siginfo>();
        // SAFETY: info is valid for `size` bytes.
        let n = unsafe { libc::read(fd.as_raw_fd(), (&raw mut info).cast(), size) };
        if n == -1 {
            match errno() {
                libc::EAGAIN => return Ok(first),
                libc::EINTR => continue,
                e => return Err(e),
            }
        }
        if first.is_none() {
            first = Some(info.ssi_signo as i32);
        }
    }
}

/// Entry point of the CLI. `args` is the full argv. Returns the exit code.
pub fn main(args: &[OsString]) -> u8 {
    let [_, cmd, unit_path] = args else {
        return usage();
    };
    if cmd.as_bytes() != b"run" {
        return usage();
    }
    if let Err(e) = reset_runtime_handlers() {
        return internal_error("-", &format!("sigaction: {}", errno_name(e)));
    }
    let sigfd = match stop_signalfd() {
        Ok(fd) => fd,
        Err(e) => return internal_error("-", &format!("signalfd setup: {}", errno_name(e))),
    };
    run(Path::new(unit_path), &sigfd)
}

fn usage() -> u8 {
    emit(
        Level::Error,
        "-",
        "usage",
        &[("detail", v("expected: xrginit run <unit-file>"))],
    );
    EXIT_USAGE
}

fn run(path: &Path, sigfd: &OwnedFd) -> u8 {
    let unit = match load_unit(path) {
        Ok(u) => u,
        Err(e) => {
            let (kind, extra) = unit_error_fields(&e);
            let mut kv = vec![("path", v(path.as_os_str().as_bytes())), ("error", v(kind))];
            kv.extend(extra);
            kv.push(("detail", v(e.to_string())));
            emit(Level::Error, "-", "unit-invalid", &kv);
            return EXIT_UNIT_INVALID;
        }
    };
    let name = unit.service.name.as_str();
    emit(Level::Info, name, "state", &[("state", v("starting"))]);
    let mut svc = match Service::start(&unit.execution) {
        Ok(s) => s,
        Err(e) => {
            emit(Level::Error, name, "start-failed", &start_error_fields(&e));
            emit(Level::Error, name, "state", &[("state", v("failed"))]);
            return EXIT_START_FAILED;
        }
    };
    let pid = v(svc.pid().to_string());
    emit(
        Level::Info,
        name,
        "state",
        &[("state", v("running")), ("pid", pid.clone())],
    );
    let (termination, stop) = match supervise(name, &mut svc, sigfd) {
        Ok(Ended {
            termination,
            stop,
            internal,
        }) => {
            if let Some(detail) = internal {
                emit(
                    Level::Error,
                    name,
                    "internal-error",
                    &[("detail", v(detail))],
                );
            }
            (termination, stop)
        }
        // The service could not be reaped; there is no final state to report.
        Err(detail) => return internal_error(name, &detail),
    };
    // B-M0-017: during a requested stop any end before the timeout is `exited`;
    // one that needed SIGKILL is `failed`. Outside a stop only exit code 0 is `exited`.
    let exited = match stop {
        Stop::Requested { escalated } => !escalated,
        Stop::None => termination == Termination::Exited(0),
        Stop::Internal => false,
    };
    let how = match termination {
        Termination::Exited(c) => ("code", v(c.to_string())),
        Termination::Signaled(s) => ("signal", v(signal_name(s))),
    };
    let (level, state, code) = if exited {
        (Level::Info, "exited", EXIT_EXITED)
    } else {
        (Level::Error, "failed", EXIT_FAILED)
    };
    emit(
        level,
        name,
        "state",
        &[("state", v(state)), ("pid", pid), how],
    );
    if matches!(stop, Stop::Internal) {
        EXIT_INTERNAL
    } else {
        code
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    None,
    Requested {
        escalated: bool,
    },
    /// An internal error forced a kill (exit 5).
    Internal,
}

/// How a supervised service ended.
struct Ended {
    termination: Termination,
    stop: Stop,
    /// Detail of an internal error that forced the end.
    internal: Option<String>,
}

/// Waits for the service to end, handling SIGTERM/SIGINT as a requested stop.
/// Returns only after the service is reaped; `Err` only if reaping itself failed.
fn supervise(name: &str, svc: &mut Service, sigfd: &OwnedFd) -> Result<Ended, String> {
    let pid = svc.pid();
    let mut stop = Stop::None;
    let mut deadline: Option<Instant> = None;
    loop {
        let timeout_ms = match (stop, deadline) {
            (Stop::Requested { escalated: false }, Some(d)) => {
                let left = d.saturating_duration_since(Instant::now());
                left.as_nanos()
                    .div_ceil(1_000_000)
                    .min(libc::c_int::MAX as u128) as libc::c_int
            }
            _ => -1,
        };
        let mut fds = [
            libc::pollfd {
                fd: svc.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: sigfd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: fds is a valid array of two pollfds.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };
        if r == -1 {
            let e = errno();
            if e == libc::EINTR {
                continue;
            }
            return kill_and_reap(svc, format!("poll: {}", errno_name(e)));
        }
        if fds[1].revents != 0 {
            match drain_signals(sigfd) {
                Ok(Some(sig)) if stop == Stop::None => {
                    emit(
                        Level::Info,
                        name,
                        "shutdown",
                        &[("signal", v(signal_name(sig)))],
                    );
                    emit(
                        Level::Info,
                        name,
                        "stop",
                        &[
                            ("pid", v(pid.to_string())),
                            ("signal", v("SIGTERM")),
                            (
                                "timeout_ms",
                                v(DEFAULT_STOP_TIMEOUT.as_millis().to_string()),
                            ),
                        ],
                    );
                    if let Err(e) = signal_alive(svc, Signal::Term) {
                        return kill_and_reap(
                            svc,
                            format!("pidfd_send_signal SIGTERM: {}", errno_name(e)),
                        );
                    }
                    stop = Stop::Requested { escalated: false };
                    deadline = Some(Instant::now() + DEFAULT_STOP_TIMEOUT);
                }
                // B-M0-017: a further SIGTERM or SIGINT during the stop changes nothing.
                Ok(_) => {}
                Err(e) => return kill_and_reap(svc, format!("read signalfd: {}", errno_name(e))),
            }
        }
        if fds[0].revents != 0 {
            return match svc.wait() {
                Ok(termination) => Ok(Ended {
                    termination,
                    stop,
                    internal: None,
                }),
                Err(e) => Err(format!("waitid: {e}")),
            };
        }
        if let (Stop::Requested { escalated: false }, Some(d)) = (stop, deadline)
            && Instant::now() >= d
        {
            emit(
                Level::Warn,
                name,
                "stop-timeout",
                &[("pid", v(pid.to_string())), ("signal", v("SIGKILL"))],
            );
            if let Err(e) = signal_alive(svc, Signal::Kill) {
                return kill_and_reap(svc, format!("pidfd_send_signal SIGKILL: {}", errno_name(e)));
            }
            stop = Stop::Requested { escalated: true };
        }
    }
}

/// Sends `sig` to a service that has not been reaped. ESRCH means it has already
/// exited and waits to be reaped, which the pidfd poll reports next.
fn signal_alive(svc: &Service, sig: Signal) -> Result<(), i32> {
    match svc.signal(sig) {
        Ok(()) | Err(SignalError::Os { errno: libc::ESRCH }) => Ok(()),
        Err(SignalError::Os { errno }) => Err(errno),
        Err(SignalError::Exited) => Err(libc::ECHILD),
    }
}

/// After an internal error: kill the service through its pidfd and reap it, so
/// xrginit never exits with its service unreaped.
fn kill_and_reap(svc: &mut Service, detail: String) -> Result<Ended, String> {
    let detail = match signal_alive(svc, Signal::Kill) {
        Ok(()) => detail,
        Err(e) => format!("{detail}; SIGKILL: {}", errno_name(e)),
    };
    match svc.wait() {
        Ok(termination) => Ok(Ended {
            termination,
            stop: Stop::Internal,
            internal: Some(detail),
        }),
        Err(e) => Err(format!("{detail}; waitid: {e}")),
    }
}
