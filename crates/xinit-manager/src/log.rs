//! Structured log lines on stderr. Spec: B-M0-028.
//!
//! `<UTC timestamp, ms> <LEVEL> unit=<unit> event=<event>( key=value)*`, one write per line.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// Log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// `INFO`
    Info,
    /// `WARN`
    Warn,
    /// `ERROR`
    Error,
}

/// The value of one `key=value` pair, as bytes (paths may be any bytes).
pub type Value = Vec<u8>;

/// Builds a value from text.
pub fn v(s: impl AsRef<[u8]>) -> Value {
    s.as_ref().to_vec()
}

fn is_bare(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"._/:+@%,-".contains(&b)
}

/// Appends `value`, bare when every byte is in `[A-Za-z0-9._/:+@%,-]`, otherwise quoted.
fn push_value(out: &mut Vec<u8>, value: &[u8]) {
    if !value.is_empty() && value.iter().all(|&b| is_bare(b)) {
        out.extend_from_slice(value);
        return;
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(b'"');
    for &b in value {
        match b {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x20..=0x7e => out.push(b),
            _ => out.extend_from_slice(&[
                b'\\',
                b'x',
                HEX[usize::from(b >> 4)],
                HEX[usize::from(b & 0xf)],
            ]),
        }
    }
    out.push(b'"');
}

/// Proleptic Gregorian date from days since 1970-01-01 (H. Hinnant, "civil_from_days").
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` for the current wall-clock time.
fn timestamp() -> String {
    // A clock set before 1970 is shown as the epoch: the format has no sign.
    let since = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => std::time::Duration::ZERO,
    };
    let secs = since.as_secs() as i64;
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let sod = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        sod / 3600,
        (sod / 60) % 60,
        sod % 60,
        since.subsec_millis()
    )
}

/// Formats one line, without the timestamp prefix.
fn format_body(level: Level, unit: &str, event: &str, kv: &[(&str, Value)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(match level {
        Level::Info => b"INFO",
        Level::Warn => b"WARN",
        Level::Error => b"ERROR",
    });
    out.extend_from_slice(b" unit=");
    out.extend_from_slice(unit.as_bytes());
    out.extend_from_slice(b" event=");
    out.extend_from_slice(event.as_bytes());
    for (k, val) in kv {
        out.push(b' ');
        out.extend_from_slice(k.as_bytes());
        out.push(b'=');
        push_value(&mut out, val);
    }
    out.push(b'\n');
    out
}

/// Writes one line to stderr at once. `unit` is a unit name or `-`.
pub fn emit(level: Level, unit: &str, event: &str, kv: &[(&str, Value)]) {
    let mut line = timestamp().into_bytes();
    line.push(b' ');
    line.extend(format_body(level, unit, event, kv));
    // Logging is the only channel xrginit has; if stderr is gone there is nowhere
    // to report that, and supervision must go on. See PROGRESS.md, Decisions.
    if std::io::stderr().lock().write_all(&line).is_err() {}
}

/// Signal name per signal(7), x86-64 numbering, or the decimal number.
pub fn signal_name(sig: i32) -> String {
    const NAMES: [&str; 31] = [
        "SIGHUP",
        "SIGINT",
        "SIGQUIT",
        "SIGILL",
        "SIGTRAP",
        "SIGABRT",
        "SIGBUS",
        "SIGFPE",
        "SIGKILL",
        "SIGUSR1",
        "SIGSEGV",
        "SIGUSR2",
        "SIGPIPE",
        "SIGALRM",
        "SIGTERM",
        "SIGSTKFLT",
        "SIGCHLD",
        "SIGCONT",
        "SIGSTOP",
        "SIGTSTP",
        "SIGTTIN",
        "SIGTTOU",
        "SIGURG",
        "SIGXCPU",
        "SIGXFSZ",
        "SIGVTALRM",
        "SIGPROF",
        "SIGWINCH",
        "SIGIO",
        "SIGPWR",
        "SIGSYS",
    ];
    match usize::try_from(sig)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| NAMES.get(i))
    {
        Some(name) => (*name).to_string(),
        None => sig.to_string(),
    }
}

/// Errno name per errno(3), Linux numbering, or the decimal number.
pub fn errno_name(errno: i32) -> String {
    const NAMES: [&str; 40] = [
        "EPERM",
        "ENOENT",
        "ESRCH",
        "EINTR",
        "EIO",
        "ENXIO",
        "E2BIG",
        "ENOEXEC",
        "EBADF",
        "ECHILD",
        "EAGAIN",
        "ENOMEM",
        "EACCES",
        "EFAULT",
        "ENOTBLK",
        "EBUSY",
        "EEXIST",
        "EXDEV",
        "ENODEV",
        "ENOTDIR",
        "EISDIR",
        "EINVAL",
        "ENFILE",
        "EMFILE",
        "ENOTTY",
        "ETXTBSY",
        "EFBIG",
        "ENOSPC",
        "ESPIPE",
        "EROFS",
        "EMLINK",
        "EPIPE",
        "EDOM",
        "ERANGE",
        "EDEADLK",
        "ENAMETOOLONG",
        "ENOLCK",
        "ENOSYS",
        "ENOTEMPTY",
        "ELOOP",
    ];
    match usize::try_from(errno)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| NAMES.get(i))
    {
        Some(name) => (*name).to_string(),
        None => errno.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(kv: &[(&str, Value)]) -> String {
        String::from_utf8(format_body(Level::Info, "-", "x", kv)).expect("ascii")
    }

    #[test]
    fn bare_and_quoted_values() {
        assert_eq!(
            body(&[("a", v("/tmp/x.toml"))]),
            "INFO unit=- event=x a=/tmp/x.toml\n"
        );
        assert_eq!(body(&[("a", v("a b"))]), "INFO unit=- event=x a=\"a b\"\n");
        assert_eq!(body(&[("a", v(""))]), "INFO unit=- event=x a=\"\"\n");
        assert_eq!(
            body(&[("a", v(b"q\"\\\t\n\x01\x7f\xff"))]),
            "INFO unit=- event=x a=\"q\\\"\\\\\\t\\n\\x01\\x7F\\xFF\"\n"
        );
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(civil_from_days(20_414), (2025, 11, 22));
    }

    #[test]
    fn names() {
        assert_eq!(signal_name(9), "SIGKILL");
        assert_eq!(signal_name(15), "SIGTERM");
        assert_eq!(signal_name(40), "40");
        assert_eq!(errno_name(2), "ENOENT");
        assert_eq!(errno_name(13), "EACCES");
        assert_eq!(errno_name(1), "EPERM");
        assert_eq!(errno_name(200), "200");
    }
}
