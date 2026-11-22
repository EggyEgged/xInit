//! specs/M0.md, "xrginit process": B-M0-027 (own signal state) and B-M0-028 (log line
//! format). Every run in this suite also parses each stderr line strictly against
//! B-M0-028 (tests/common/mod.rs, parse_log_line).

use super::common::{EXIT_DEADLINE, Run, SIGTERM, TempDir, child_lock_shared, status_mask, unit_toml};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;

// Signal bits in /proc/<pid>/status masks: signal n is bit n-1.
// SIGINT = 2 -> 1 << 1 = 0x2; SIGTERM = 15 -> 1 << 14 = 0x4000; SIGCHLD = 17 -> 1 << 16 = 0x10000.
// 0x2 + 0x4000 + 0x10000 = 0x14002 (the spec's example SigBlk value).
const INT_TERM_CHLD: u64 = 0x14002;
// SIGPIPE = 13 -> 1 << 12 = 0x1000 (the spec's example SigIgn value).
const PIPE: u64 = 0x1000;

// spec: B-M0-027
#[test]
fn b_m0_027_xrginit_signal_dispositions() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.write("s.toml", &unit_toml("sigown", &["/usr/bin/sleep", "60"], ""));
    let mut r = Run::unit(&p);
    r.wait_running();
    let x = r.pid;
    let (cgt, blk, ign) = (status_mask(x, "SigCgt"), status_mask(x, "SigBlk"), status_mask(x, "SigIgn"));
    r.signal_xrginit(SIGTERM);
    let d = r.finish(EXIT_DEADLINE);
    assert_eq!(cgt & !INT_TERM_CHLD, 0, "B-M0-027: SigCgt {cgt:016x} has bits outside INT, TERM, CHLD");
    assert_eq!(blk & !INT_TERM_CHLD, 0, "B-M0-027: SigBlk {blk:016x} has bits outside INT, TERM, CHLD");
    assert!(ign == 0 || ign == PIPE, "B-M0-027: SigIgn {ign:016x} must be empty or exactly SIGPIPE");
    assert_eq!(d.code, Some(0), "{}", d.text);
    d.check_run_invariants("sigown");
}

// spec: B-M0-028 (quoting: '"' as \", tab as \t, bytes >= 0x80 as \xHH, space forces quotes)
#[test]
fn b_m0_028_quoted_path_escapes() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let dir = t.join("q \"x\"\tz \u{e9}");
    fs::create_dir(&dir).expect("mkdir special dir");
    let p = dir.join("bad.toml");
    fs::write(&p, "schema_version = 1\n[service]\nname = \"q\"\ntype = \"simple\"\n[execution]\ncommand = [\"/usr/bin/sleep\", \"1\"]\nxinit_m0_bogus = 1\n")
        .expect("write unit");
    let d = Run::unit(&p).finish(EXIT_DEADLINE);
    assert_eq!(d.code, Some(3), "{}", d.text);
    let l = d.events("unit-invalid");
    assert_eq!(l.len(), 1, "{}", d.text);
    let l = l[0];
    assert_eq!(l.get_bytes("path"), Some(p.as_os_str().as_bytes()), "B-M0-028: decoded path value\n{}", l.raw);
    // U+00E9 is UTF-8 0xC3 0xA9, so it must appear as \xC3\xA9 (hex case unspecified).
    let lower = l.raw.to_ascii_lowercase();
    assert!(lower.contains("\\xc3\\xa9"), "B-M0-028: non-ASCII bytes as \\xHH: {}", l.raw);
    assert!(l.raw.contains("q \\\"x\\\"\\tz "), "B-M0-028: '\"' as \\\" and tab as \\t: {}", l.raw);
    assert_eq!(l.get("key").as_deref(), Some("execution.xinit_m0_bogus"), "{}", l.raw);
}

// spec: B-M0-028 (invalid UTF-8 in a value is written as \xHH inside quotes)
#[test]
fn b_m0_028_invalid_utf8_path() {
    let _g = child_lock_shared();
    let t = TempDir::new();
    let p = t.path.join(OsStr::from_bytes(b"xinit-m0-\xff-absent.toml"));
    let d = Run::unit(&p).finish(EXIT_DEADLINE);
    assert_eq!(d.code, Some(3), "{}", d.text);
    let l = d.events("unit-invalid");
    assert_eq!(l.len(), 1, "{}", d.text);
    let l = l[0];
    assert_eq!(l.get("errno").as_deref(), Some("ENOENT"), "{}", l.raw);
    assert_eq!(l.get_bytes("path"), Some(p.as_os_str().as_bytes()), "B-M0-028: decoded path value\n{}", l.raw);
    assert!(l.raw.to_ascii_lowercase().contains("-\\xff-"), "B-M0-028: byte 0xFF as \\xFF: {}", l.raw);
}
