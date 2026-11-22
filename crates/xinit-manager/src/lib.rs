//! The xInit Manager. Linked into the `xrginit` binary and entered through
//! the `manager` subcommand. See specs/ARCHITECTURE.md.

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

pub mod cli;
pub mod log;
