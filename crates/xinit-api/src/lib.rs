//! Public types and function signatures for xInit.
//!
//! The spec writer adds items here. Function bodies stay `unimplemented!()`
//! until the implementer session for the owning milestone fills them in.
//!
//! M0 items follow specs/M0.md, section "Public surface".

use std::fmt;
use std::path::Path;
use std::time::Duration;

/// The only unit file schema version M0 accepts. Spec: B-M0-003.
pub const SCHEMA_VERSION: i64 = 1;

/// Fixed stop timeout of the CLI, 5000 ms. Spec: B-M0-017, B-M0-018, I-6.
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_millis(5000);

/// Parses unit file text (schema version 1). Spec: B-M0-001 to B-M0-010.
///
/// # Errors
/// Returns the [`UnitError`] for the unit's defect (B-M0-003 to B-M0-010).
#[expect(unused_variables, reason = "stub body; remove when implemented")]
pub fn parse_unit(text: &str) -> Result<Unit, UnitError> {
    unimplemented!()
}

/// Reads the file at `path` and parses it with [`parse_unit`]. Spec: B-M0-009, B-M0-012.
///
/// # Errors
/// [`UnitError::Io`] if the file cannot be read, otherwise as [`parse_unit`].
#[expect(unused_variables, reason = "stub body; remove when implemented")]
pub fn load_unit(path: &Path) -> Result<Unit, UnitError> {
    unimplemented!()
}

/// A parsed unit file. Spec: B-M0-001, B-M0-002, B-M0-010.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The `[service]` table.
    pub service: ServiceSection,
    /// The `[execution]` table.
    pub execution: ExecutionSection,
    /// The `[dependencies]` table; defaults when absent. Spec: B-M0-010.
    pub dependencies: DependenciesSection,
    /// The `[restart]` table; defaults when absent. Spec: B-M0-010.
    pub restart: RestartSection,
}

/// The `[service]` table. Spec: B-M0-001, B-M0-002, B-M0-005 to B-M0-008.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSection {
    /// `service.name`, matching `^[a-z0-9][a-z0-9._-]{0,63}$`.
    pub name: String,
    /// `service.description`, optional.
    pub description: Option<String>,
    /// `service.type`.
    pub service_type: ServiceType,
    /// `service.provides`, default empty.
    pub provides: Vec<String>,
}

/// Service type. M0 supports only `simple`. Spec: B-M0-007.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    /// `type = "simple"`.
    Simple,
}

/// The `[execution]` table. Spec: B-M0-001, B-M0-002, B-M0-008, B-M0-020, B-M0-024 to B-M0-026.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSection {
    /// `execution.command`: argv, element 0 an absolute path.
    pub command: Vec<String>,
    /// `execution.user`, optional and non-empty.
    pub user: Option<String>,
}

/// The `[dependencies]` table. Parsed and validated, no effect in M0. Spec: B-M0-010, Q1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependenciesSection {
    /// `dependencies.requires`: unit names, default empty.
    pub requires: Vec<String>,
    /// `dependencies.after`: unit names, default empty.
    pub after: Vec<String>,
}

/// The `[restart]` table. Parsed and validated, no effect in M0. Spec: B-M0-010, Q1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartSection {
    /// `restart.policy`, default `"no"`.
    pub policy: RestartPolicy,
    /// `restart.maximum_restarts`, non-negative, default 0.
    pub maximum_restarts: u64,
}

/// `restart.policy`. Spec: B-M0-010, Q1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// `"no"`.
    No,
    /// `"on-failure"`.
    OnFailure,
    /// `"always"`.
    Always,
}

/// Unit loading error. Log `error=` kinds in variant order: `io`, `syntax`,
/// `schema-version`, `unknown-key`, `missing-key`, `invalid-type`, `invalid-value`,
/// `unsupported-service-type`. Spec: B-M0-003 to B-M0-010, B-M0-012.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitError {
    /// The file could not be read. Spec: B-M0-009.
    Io {
        /// OS errno.
        errno: i32,
    },
    /// TOML syntax error. Spec: B-M0-009.
    Syntax,
    /// `schema_version` is an integer other than 1. Spec: B-M0-003.
    SchemaVersion {
        /// The version found.
        found: i64,
    },
    /// A key or table not in the schema. Spec: B-M0-004, B-M0-010.
    UnknownKey {
        /// Key path.
        key: String,
    },
    /// A required key or table is absent. Spec: B-M0-003, B-M0-005.
    MissingKey {
        /// Key path.
        key: String,
    },
    /// A key has the wrong TOML type. Spec: B-M0-003, B-M0-006, B-M0-010.
    InvalidType {
        /// Key path.
        key: String,
    },
    /// A key has a value its rule rejects. Spec: B-M0-008, B-M0-010.
    InvalidValue {
        /// Key path.
        key: String,
    },
    /// `service.type` is a string other than `"simple"`. Spec: B-M0-007.
    UnsupportedServiceType {
        /// The value found.
        value: String,
    },
}

/// One started process, tracked by pidfd. Fields are private; obtain one only
/// from [`Service::start`]. Spec: B-M0-016, B-M0-029 to B-M0-032, I-1.
#[derive(Debug)]
pub struct Service {
    _private: (),
}

impl Service {
    /// Starts the command in `exec`. Spec: B-M0-016, B-M0-020 to B-M0-026, B-M0-029, B-M0-032.
    ///
    /// # Errors
    /// A [`StartError`]; the caller then has no new child (B-M0-032).
    #[expect(unused_variables, reason = "stub body; remove when implemented")]
    pub fn start(exec: &ExecutionSection) -> Result<Service, StartError> {
        unimplemented!()
    }

    /// The service's pid. Spec: B-M0-029, B-M0-030.
    pub fn pid(&self) -> u32 {
        unimplemented!()
    }

    /// Sends `sig` through the pidfd. Spec: B-M0-030, I-1.
    ///
    /// # Errors
    /// [`SignalError::Exited`] after the process was reaped, otherwise [`SignalError::Os`].
    #[expect(unused_variables, reason = "stub body; remove when implemented")]
    pub fn signal(&self, sig: Signal) -> Result<(), SignalError> {
        unimplemented!()
    }

    /// Blocks until the process is reaped; later calls return the same result. Spec: B-M0-029, B-M0-030.
    ///
    /// # Errors
    /// [`WaitError::Os`] on an OS failure.
    pub fn wait(&mut self) -> Result<Termination, WaitError> {
        unimplemented!()
    }

    /// Sends SIGTERM, then SIGKILL after `timeout`. Spec: B-M0-031.
    ///
    /// # Errors
    /// [`WaitError::Os`] on an OS failure.
    #[expect(unused_variables, reason = "stub body; remove when implemented")]
    pub fn stop(&mut self, timeout: Duration) -> Result<StopOutcome, WaitError> {
        unimplemented!()
    }
}

/// A signal the API can send. Spec: B-M0-030.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGTERM.
    Term,
    /// SIGKILL.
    Kill,
    /// SIGINT.
    Int,
}

/// How a reaped process ended. Spec: B-M0-029 to B-M0-031.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    /// Exited with this exit code.
    Exited(i32),
    /// Killed by this signal number.
    Signaled(i32),
}

/// Result of [`Service::stop`]. Spec: B-M0-031.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopOutcome {
    /// How the process ended.
    pub termination: Termination,
    /// Whether SIGKILL was sent after the timeout.
    pub escalated: bool,
}

/// Start error. Log `error=` kinds in variant order: `exec`, `user-unknown`,
/// `credentials`, `spawn`. Spec: B-M0-016, B-M0-025, B-M0-026, B-M0-032.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// exec of `command[0]` failed. Spec: B-M0-016.
    Exec {
        /// OS errno.
        errno: i32,
    },
    /// `user` not found in `/etc/passwd`. Spec: B-M0-025.
    UserUnknown {
        /// The user name.
        user: String,
    },
    /// A credential change failed. Spec: B-M0-026.
    Credentials {
        /// OS errno.
        errno: i32,
    },
    /// The process could not be created.
    Spawn {
        /// OS errno.
        errno: i32,
    },
}

/// Error from [`Service::signal`]. Spec: B-M0-030.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalError {
    /// The process was already reaped; no signal was sent.
    Exited,
    /// The OS rejected the signal.
    Os {
        /// OS errno.
        errno: i32,
    },
}

/// Error from [`Service::wait`] and [`Service::stop`]. Spec: B-M0-029, B-M0-031.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitError {
    /// An OS failure.
    Os {
        /// OS errno.
        errno: i32,
    },
}

/// Human-readable message. Tests must not assert this text. Spec: B-M0-003 to B-M0-010, B-M0-012.
impl fmt::Display for UnitError {
    #[expect(unused_variables, reason = "stub body; remove when implemented")]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!()
    }
}

/// Spec: B-M0-003 to B-M0-010, B-M0-012.
impl std::error::Error for UnitError {}

/// Human-readable message. Tests must not assert this text. Spec: B-M0-016, B-M0-025, B-M0-026.
impl fmt::Display for StartError {
    #[expect(unused_variables, reason = "stub body; remove when implemented")]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!()
    }
}

/// Spec: B-M0-016, B-M0-025, B-M0-026.
impl std::error::Error for StartError {}

/// Human-readable message. Tests must not assert this text. Spec: B-M0-030.
impl fmt::Display for SignalError {
    #[expect(unused_variables, reason = "stub body; remove when implemented")]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!()
    }
}

/// Spec: B-M0-030.
impl std::error::Error for SignalError {}

/// Human-readable message. Tests must not assert this text. Spec: B-M0-029, B-M0-031.
impl fmt::Display for WaitError {
    #[expect(unused_variables, reason = "stub body; remove when implemented")]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!()
    }
}

/// Spec: B-M0-029, B-M0-031.
impl std::error::Error for WaitError {}
