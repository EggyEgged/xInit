//! One service process, created with clone3(CLONE_PIDFD) and tracked only by its pidfd.
//! Spec: B-M0-016, B-M0-020 to B-M0-026, B-M0-029 to B-M0-032, I-1.
//!
//! Start protocol: everything the child needs (argv, envp, credentials) is allocated
//! before clone3. The child then runs only raw syscalls, which are async-signal-safe,
//! and reports a failure as 8 bytes (stage, errno) on a close-on-exec pipe. EOF on that
//! pipe without data means exec succeeded, so `start` returns only after the service
//! program has replaced the child (B-M0-013).

use crate::{
    ExecutionSection, Signal, SignalError, StartError, StopOutcome, Termination, WaitError,
};
use libc::{c_char, c_int, c_long};
use std::ffi::{CString, OsString};
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::time::{Duration, Instant};

/// One started process, tracked by pidfd. Spec: B-M0-016, B-M0-029 to B-M0-032, I-1.
#[derive(Debug)]
pub struct Service {
    pid: u32,
    /// Stays open until drop. After the reap it is never used to signal (I-1).
    pidfd: OwnedFd,
    /// Set once the process has been reaped.
    reaped: Option<Termination>,
}

const STAGE_SPAWN: i32 = 1;
const STAGE_CREDENTIALS: i32 = 2;
const STAGE_EXEC: i32 = 3;

/// Credential change applied in the child (B-M0-026).
struct Credentials {
    uid: libc::uid_t,
    gid: libc::gid_t,
    groups: Vec<libc::gid_t>,
}

/// Everything the child uses, allocated before clone3.
struct Prepared {
    path: CString,
    _argv: Vec<CString>,
    argv_ptrs: Vec<*const c_char>,
    _envp: Vec<CString>,
    envp_ptrs: Vec<*const c_char>,
    credentials: Option<Credentials>,
}

/// clone3 arguments, version 0 layout (64 bytes). See clone(2).
#[repr(C)]
struct CloneArgs {
    flags: u64,
    pidfd: u64,
    child_tid: u64,
    parent_tid: u64,
    exit_signal: u64,
    stack: u64,
    stack_size: u64,
    tls: u64,
}

/// The kernel's `struct sigaction` for rt_sigaction(2) on 64-bit Linux.
#[repr(C)]
struct KernelSigaction {
    handler: usize,
    flags: u64,
    restorer: usize,
    mask: u64,
}

/// The errno of an I/O error. Errors from std's file calls always carry one; EIO
/// stands in if one ever does not.
fn os_errno(e: &std::io::Error) -> i32 {
    match e.raw_os_error() {
        Some(n) => n,
        None => libc::EIO,
    }
}

fn errno() -> i32 {
    // SAFETY: __errno_location returns a valid pointer to this thread's errno.
    unsafe { *libc::__errno_location() }
}

fn cstring(s: &str) -> Result<CString, StartError> {
    // A string with an interior NUL cannot be passed to execve.
    CString::new(s).map_err(|_| StartError::Exec {
        errno: libc::EINVAL,
    })
}

fn null_terminated(v: &[CString]) -> Vec<*const c_char> {
    v.iter()
        .map(|c| c.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}

/// `name:password:uid:gid:...` from /etc/passwd.
fn lookup_user(name: &str) -> Result<(libc::uid_t, libc::gid_t), StartError> {
    let unknown = || StartError::UserUnknown {
        user: name.to_string(),
    };
    let text = match std::fs::read_to_string("/etc/passwd") {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(unknown()),
        Err(e) => {
            return Err(StartError::Credentials {
                errno: os_errno(&e),
            });
        }
    };
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() >= 4 && f[0] == name {
            match (f[2].parse(), f[3].parse()) {
                (Ok(uid), Ok(gid)) => return Ok((uid, gid)),
                _ => return Err(unknown()),
            }
        }
    }
    Err(unknown())
}

/// Groups in /etc/group whose member list names `user`.
fn supplementary_groups(user: &str) -> Result<Vec<libc::gid_t>, StartError> {
    let text = match std::fs::read_to_string("/etc/group") {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(StartError::Credentials {
                errno: os_errno(&e),
            });
        }
    };
    let mut groups = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        // A line whose gid does not parse is malformed and skipped, as getgrent(3) does.
        if f.len() >= 4
            && f[3].split(',').any(|m| m == user)
            && let Ok(gid) = f[2].parse()
            && !groups.contains(&gid)
        {
            groups.push(gid);
        }
    }
    Ok(groups)
}

fn prepare(exec: &ExecutionSection) -> Result<Prepared, StartError> {
    let Some(program) = exec.command.first() else {
        return Err(StartError::Exec {
            errno: libc::ENOENT,
        });
    };
    let path = cstring(program)?;
    let argv = exec
        .command
        .iter()
        .map(|a| cstring(a))
        .collect::<Result<Vec<_>, _>>()?;
    let envp: Vec<CString> = std::env::vars_os()
        .map(|(k, v)| {
            let mut kv: OsString = k;
            kv.push("=");
            kv.push(v);
            // The environment cannot hold an interior NUL; if it somehow does, fail.
            CString::new(kv.into_vec())
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StartError::Spawn {
            errno: libc::EINVAL,
        })?;
    let credentials = match &exec.user {
        None => None,
        Some(name) => {
            let (uid, gid) = lookup_user(name)?;
            // SAFETY: geteuid and getegid have no preconditions.
            let (euid, egid) = unsafe { (libc::geteuid(), libc::getegid()) };
            if uid == euid && gid == egid {
                None
            } else {
                Some(Credentials {
                    uid,
                    gid,
                    groups: supplementary_groups(name)?,
                })
            }
        }
    };
    Ok(Prepared {
        argv_ptrs: null_terminated(&argv),
        envp_ptrs: null_terminated(&envp),
        path,
        _argv: argv,
        _envp: envp,
        credentials,
    })
}

/// Runs in the child after clone3. Raw syscalls only: no allocation, no locks, no
/// libc wrappers that consult process-wide state. Never returns.
fn child(p: &Prepared, report: c_int) -> ! {
    let fail = |stage: i32| -> ! {
        let e = errno();
        let mut buf = [0u8; 8];
        let (a, b) = buf.split_at_mut(4);
        a.copy_from_slice(&stage.to_ne_bytes());
        b.copy_from_slice(&e.to_ne_bytes());
        // SAFETY: buf is valid for 8 bytes; report is the pipe's write end. An 8-byte
        // write to an empty pipe is atomic, and with every disposition reset to
        // default no handler can interrupt it.
        unsafe {
            libc::write(report, buf.as_ptr().cast(), buf.len());
            libc::_exit(127)
        }
    };
    let dfl = KernelSigaction {
        handler: libc::SIG_DFL,
        flags: 0,
        restorer: 0,
        mask: 0,
    };
    for sig in 1..=64 {
        if sig == libc::SIGKILL || sig == libc::SIGSTOP {
            continue;
        }
        // SAFETY: dfl is a valid kernel sigaction; the old-action pointer is null.
        let r = unsafe {
            libc::syscall(
                libc::SYS_rt_sigaction,
                sig as c_long,
                &raw const dfl,
                std::ptr::null::<u8>(),
                8 as c_long,
            )
        };
        if r == -1 {
            fail(STAGE_SPAWN);
        }
    }
    let empty: u64 = 0;
    // SAFETY: empty is a valid 8-byte signal set; the old-set pointer is null.
    let r = unsafe {
        libc::syscall(
            libc::SYS_rt_sigprocmask,
            libc::SIG_SETMASK as c_long,
            &raw const empty,
            std::ptr::null::<u8>(),
            8 as c_long,
        )
    };
    if r == -1 {
        fail(STAGE_SPAWN);
    }
    // SAFETY: setsid takes no arguments.
    if unsafe { libc::syscall(libc::SYS_setsid) } == -1 {
        fail(STAGE_SPAWN);
    }
    if let Some(c) = &p.credentials {
        // SAFETY: groups is a valid array of c.groups.len() gids allocated before clone3.
        if unsafe {
            libc::syscall(
                libc::SYS_setgroups,
                c.groups.len() as c_long,
                c.groups.as_ptr(),
            )
        } == -1
        {
            fail(STAGE_CREDENTIALS);
        }
        // SAFETY: plain integer argument.
        if unsafe { libc::syscall(libc::SYS_setgid, c.gid as c_long) } == -1 {
            fail(STAGE_CREDENTIALS);
        }
        // SAFETY: plain integer argument.
        if unsafe { libc::syscall(libc::SYS_setuid, c.uid as c_long) } == -1 {
            fail(STAGE_CREDENTIALS);
        }
    }
    // Every fd from 3 up becomes close-on-exec, including inherited ones (B-M0-023).
    // SAFETY: plain integer arguments.
    let r = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3 as c_long,
            c_long::from(u32::MAX),
            libc::CLOSE_RANGE_CLOEXEC as c_long,
        )
    };
    if r == -1 {
        fail(STAGE_SPAWN);
    }
    // SAFETY: path, argv and envp are NUL-terminated strings and null-terminated
    // pointer arrays that live in this (copied) address space.
    unsafe {
        libc::syscall(
            libc::SYS_execve,
            p.path.as_ptr(),
            p.argv_ptrs.as_ptr(),
            p.envp_ptrs.as_ptr(),
        );
    }
    fail(STAGE_EXEC)
}

/// waitid(P_PIDFD, WEXITED), retried on EINTR.
fn reap(pidfd: BorrowedFd<'_>) -> Result<Termination, WaitError> {
    loop {
        // SAFETY: siginfo_t is plain data; all-zero is a valid value.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: info is a valid siginfo_t; the pidfd is open.
        let r = unsafe {
            libc::waitid(
                libc::P_PIDFD,
                pidfd.as_raw_fd() as libc::id_t,
                &raw mut info,
                libc::WEXITED,
            )
        };
        if r == -1 {
            match errno() {
                libc::EINTR => continue,
                e => return Err(WaitError::Os { errno: e }),
            }
        }
        // SAFETY: waitid filled info for a child that changed state (SIGCHLD layout).
        let status = unsafe { info.si_status() };
        return match info.si_code {
            libc::CLD_EXITED => Ok(Termination::Exited(status)),
            libc::CLD_KILLED | libc::CLD_DUMPED => Ok(Termination::Signaled(status)),
            _ => Err(WaitError::Os {
                errno: libc::EINVAL,
            }),
        };
    }
}

fn send(pidfd: BorrowedFd<'_>, sig: c_int) -> Result<(), SignalError> {
    // SAFETY: the pidfd is open; info is null, flags 0.
    let r = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd() as c_long,
            sig as c_long,
            std::ptr::null::<u8>(),
            0 as c_long,
        )
    };
    if r == -1 {
        Err(SignalError::Os { errno: errno() })
    } else {
        Ok(())
    }
}

impl Service {
    /// Starts the command in `exec`. Spec: B-M0-016, B-M0-020 to B-M0-026, B-M0-029, B-M0-032.
    ///
    /// # Errors
    /// A [`StartError`]; the caller then has no new child (B-M0-032).
    pub fn start(exec: &ExecutionSection) -> Result<Service, StartError> {
        let prepared = prepare(exec)?;
        let mut fds = [-1 as c_int; 2];
        // SAFETY: fds is a valid array of two ints.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == -1 {
            return Err(StartError::Spawn { errno: errno() });
        }
        // SAFETY: pipe2 returned two new fds that nothing else owns.
        let (report_r, report_w) =
            unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        let mut pidfd: c_int = -1;
        let args = CloneArgs {
            flags: libc::CLONE_PIDFD as u64,
            pidfd: (&raw mut pidfd) as u64,
            child_tid: 0,
            parent_tid: 0,
            exit_signal: libc::SIGCHLD as u64,
            stack: 0,
            stack_size: 0,
            tls: 0,
        };
        // SAFETY: args is a valid clone_args of the size passed. Without CLONE_VM the
        // child gets a copy of this address space and runs only `child`, which never
        // returns.
        let ret = unsafe {
            libc::syscall(
                libc::SYS_clone3,
                &raw const args,
                std::mem::size_of::<CloneArgs>(),
            )
        };
        if ret == -1 {
            return Err(StartError::Spawn { errno: errno() });
        }
        if ret == 0 {
            child(&prepared, report_w.as_raw_fd());
        }
        drop(report_w);
        let pid = u32::try_from(ret).map_err(|_| StartError::Spawn {
            errno: libc::EINVAL,
        })?;
        // SAFETY: clone3 stored a new pidfd that nothing else owns.
        let pidfd = unsafe { OwnedFd::from_raw_fd(pidfd) };
        let mut buf = Vec::with_capacity(8);
        let read = File::from(report_r).read_to_end(&mut buf);
        let failure = match (read, buf.as_slice()) {
            (Ok(0), _) => None,
            (Ok(_), [s0, s1, s2, s3, e0, e1, e2, e3]) => {
                let stage = i32::from_ne_bytes([*s0, *s1, *s2, *s3]);
                let errno = i32::from_ne_bytes([*e0, *e1, *e2, *e3]);
                Some(match stage {
                    STAGE_CREDENTIALS => StartError::Credentials { errno },
                    STAGE_EXEC => StartError::Exec { errno },
                    _ => StartError::Spawn { errno },
                })
            }
            (Ok(_), _) => Some(StartError::Spawn { errno: libc::EIO }),
            (Err(e), _) => {
                // The outcome of exec is unknown: kill the child through its pidfd
                // (it is not reaped yet), then reap it below.
                match send(pidfd.as_fd(), libc::SIGKILL) {
                    Ok(()) | Err(SignalError::Os { errno: libc::ESRCH }) => {}
                    Err(SignalError::Os { errno }) => return Err(StartError::Spawn { errno }),
                    Err(SignalError::Exited) => {}
                }
                Some(StartError::Spawn {
                    errno: os_errno(&e),
                })
            }
        };
        if let Some(err) = failure {
            // Reap the failed child so none is left behind (B-M0-032).
            return match reap(pidfd.as_fd()) {
                Ok(_) => Err(err),
                Err(WaitError::Os { errno }) => Err(StartError::Spawn { errno }),
            };
        }
        Ok(Service {
            pid,
            pidfd,
            reaped: None,
        })
    }

    /// The service's pid. Spec: B-M0-029, B-M0-030.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Sends `sig` through the pidfd. Spec: B-M0-030, I-1.
    ///
    /// # Errors
    /// [`SignalError::Exited`] after the process was reaped, otherwise [`SignalError::Os`].
    pub fn signal(&self, sig: Signal) -> Result<(), SignalError> {
        if self.reaped.is_some() {
            return Err(SignalError::Exited);
        }
        let n = match sig {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Int => libc::SIGINT,
        };
        send(self.pidfd.as_fd(), n)
    }

    /// Blocks until the process is reaped; later calls return the same result. Spec: B-M0-029, B-M0-030.
    ///
    /// # Errors
    /// [`WaitError::Os`] on an OS failure.
    pub fn wait(&mut self) -> Result<Termination, WaitError> {
        if let Some(t) = self.reaped {
            return Ok(t);
        }
        let t = reap(self.pidfd.as_fd())?;
        self.reaped = Some(t);
        Ok(t)
    }

    /// Sends SIGTERM, then SIGKILL after `timeout`. Spec: B-M0-031.
    ///
    /// # Errors
    /// [`WaitError::Os`] on an OS failure.
    pub fn stop(&mut self, timeout: Duration) -> Result<StopOutcome, WaitError> {
        if let Some(termination) = self.reaped {
            return Ok(StopOutcome {
                termination,
                escalated: false,
            });
        }
        signal_unless_gone(self, Signal::Term)?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            if wait_readable(self.pidfd.as_fd(), left)? {
                return Ok(StopOutcome {
                    termination: self.wait()?,
                    escalated: false,
                });
            }
        }
        signal_unless_gone(self, Signal::Kill)?;
        Ok(StopOutcome {
            termination: self.wait()?,
            escalated: true,
        })
    }
}

/// Signals a process that has not been reaped. ESRCH means it already exited and
/// waits to be reaped, which is not an error for a stop.
fn signal_unless_gone(s: &Service, sig: Signal) -> Result<(), WaitError> {
    match s.signal(sig) {
        Ok(()) | Err(SignalError::Os { errno: libc::ESRCH }) | Err(SignalError::Exited) => Ok(()),
        Err(SignalError::Os { errno }) => Err(WaitError::Os { errno }),
    }
}

/// Polls `fd` for readability for at most `left`. Returns whether it became readable.
fn wait_readable(fd: BorrowedFd<'_>, left: Duration) -> Result<bool, WaitError> {
    // Round up so the poll never returns before the deadline.
    let ms = left.as_nanos().div_ceil(1_000_000).min(c_int::MAX as u128) as c_int;
    let mut pfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: pfd is one valid pollfd.
    let r = unsafe { libc::poll(&raw mut pfd, 1, ms) };
    match r {
        -1 => match errno() {
            libc::EINTR => Ok(false),
            e => Err(WaitError::Os { errno: e }),
        },
        0 => Ok(false),
        _ => Ok(true),
    }
}
