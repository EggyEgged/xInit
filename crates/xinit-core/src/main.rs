//! xrginit: the xInit Core. Runs as PID 1 or as an ordinary supervisor.
//!
//! Scaffolding only. No init behaviour is implemented yet, so the binary
//! reports that and exits with a failure status instead of pretending to work.

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

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("xrginit: not yet implemented");
    ExitCode::FAILURE
}
