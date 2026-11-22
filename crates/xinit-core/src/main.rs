//! xrginit: the xInit Core. Runs as PID 1 or as an ordinary supervisor.
//!
//! M0: only `xrginit run <unit-file>` exists, as an ordinary unprivileged process
//! (specs/M0.md). The supervision logic lives in `xinit-manager`.

#![cfg_attr(
    not(test),
    forbid(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::exit,
        clippy::todo,
        clippy::unimplemented
    )
)]

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    ExitCode::from(xinit_manager::cli::main(&args))
}
