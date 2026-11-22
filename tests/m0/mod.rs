//! M0 frozen suite (specs/M0.md). One file per spec section, plus the
//! state-machine model of the pidfd Service API.

#[path = "../common/mod.rs"]
mod common;

mod api_model;
mod cli_lifecycle;
mod pidfd_api;
mod service_env;
mod unit_loading;
mod xrginit_process;
