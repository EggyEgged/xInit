//! Unit file schema version 1. Spec: specs/M0.md, "Unit file schema" and B-M0-001 to B-M0-010.

use crate::{
    DependenciesSection, ExecutionSection, RestartPolicy, RestartSection, SCHEMA_VERSION,
    ServiceSection, ServiceType, Unit, UnitError,
};
use toml::{Table, Value};

const TOP_KEYS: &[&str] = &[
    "schema_version",
    "service",
    "execution",
    "dependencies",
    "restart",
];
const SERVICE_KEYS: &[&str] = &["name", "description", "type", "provides"];
const EXECUTION_KEYS: &[&str] = &["command", "user"];
const DEPENDENCIES_KEYS: &[&str] = &["requires", "after"];
const RESTART_KEYS: &[&str] = &["policy", "maximum_restarts"];

/// `^[a-z0-9][a-z0-9._-]{0,63}$`, the pattern of `service.name` and of unit names in
/// `[dependencies]`.
fn is_unit_name(s: &str) -> bool {
    let b = s.as_bytes();
    let Some((&first, rest)) = b.split_first() else {
        return false;
    };
    b.len() <= 64
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && rest.iter().all(|&c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
        })
}

/// `section.key`, or the bare key at top level.
fn path(section: &str, key: &str) -> String {
    if section.is_empty() {
        key.to_string()
    } else {
        format!("{section}.{key}")
    }
}

fn check_keys(table: &Table, section: &str, allowed: &[&str]) -> Result<(), UnitError> {
    match table.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(UnitError::UnknownKey {
            key: path(section, k),
        }),
        None => Ok(()),
    }
}

/// A sub-table of the top level. `Ok(None)` when absent.
fn table<'a>(root: &'a Table, name: &str) -> Result<Option<&'a Table>, UnitError> {
    match root.get(name) {
        None => Ok(None),
        Some(Value::Table(t)) => Ok(Some(t)),
        Some(_) => Err(UnitError::InvalidType {
            key: name.to_string(),
        }),
    }
}

fn required_table<'a>(root: &'a Table, name: &str) -> Result<&'a Table, UnitError> {
    table(root, name)?.ok_or_else(|| UnitError::MissingKey {
        key: name.to_string(),
    })
}

fn string(t: &Table, section: &str, key: &str) -> Result<Option<String>, UnitError> {
    match t.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(UnitError::InvalidType {
            key: path(section, key),
        }),
    }
}

fn required_string(t: &Table, section: &str, key: &str) -> Result<String, UnitError> {
    string(t, section, key)?.ok_or_else(|| UnitError::MissingKey {
        key: path(section, key),
    })
}

fn string_array(t: &Table, section: &str, key: &str) -> Result<Option<Vec<String>>, UnitError> {
    let invalid_type = || UnitError::InvalidType {
        key: path(section, key),
    };
    match t.get(key) {
        None => Ok(None),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::String(s) => Ok(s.clone()),
                _ => Err(invalid_type()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(invalid_type()),
    }
}

/// An optional array of unit names (B-M0-010); default empty.
fn unit_names(t: &Table, section: &str, key: &str) -> Result<Vec<String>, UnitError> {
    let names = string_array(t, section, key)?.unwrap_or_default();
    if names.iter().all(|n| is_unit_name(n)) {
        Ok(names)
    } else {
        Err(UnitError::InvalidValue {
            key: path(section, key),
        })
    }
}

fn parse_schema_version(root: &Table) -> Result<(), UnitError> {
    const KEY: &str = "schema_version";
    match root.get(KEY) {
        None => Err(UnitError::MissingKey {
            key: KEY.to_string(),
        }),
        Some(Value::Integer(n)) if *n == SCHEMA_VERSION => Ok(()),
        Some(Value::Integer(n)) => Err(UnitError::SchemaVersion { found: *n }),
        Some(_) => Err(UnitError::InvalidType {
            key: KEY.to_string(),
        }),
    }
}

fn parse_service(root: &Table) -> Result<ServiceSection, UnitError> {
    const S: &str = "service";
    let t = required_table(root, S)?;
    check_keys(t, S, SERVICE_KEYS)?;
    let name = required_string(t, S, "name")?;
    if !is_unit_name(&name) {
        return Err(UnitError::InvalidValue {
            key: path(S, "name"),
        });
    }
    let description = string(t, S, "description")?;
    let service_type = match required_string(t, S, "type")?.as_str() {
        "simple" => ServiceType::Simple,
        other => {
            return Err(UnitError::UnsupportedServiceType {
                value: other.to_string(),
            });
        }
    };
    let provides = string_array(t, S, "provides")?.unwrap_or_default();
    Ok(ServiceSection {
        name,
        description,
        service_type,
        provides,
    })
}

fn parse_execution(root: &Table) -> Result<ExecutionSection, UnitError> {
    const S: &str = "execution";
    let t = required_table(root, S)?;
    check_keys(t, S, EXECUTION_KEYS)?;
    let command = string_array(t, S, "command")?.ok_or_else(|| UnitError::MissingKey {
        key: path(S, "command"),
    })?;
    if !command.first().is_some_and(|c| c.starts_with('/')) {
        return Err(UnitError::InvalidValue {
            key: path(S, "command"),
        });
    }
    let user = string(t, S, "user")?;
    if user.as_deref() == Some("") {
        return Err(UnitError::InvalidValue {
            key: path(S, "user"),
        });
    }
    Ok(ExecutionSection { command, user })
}

fn parse_dependencies(root: &Table) -> Result<DependenciesSection, UnitError> {
    const S: &str = "dependencies";
    let Some(t) = table(root, S)? else {
        return Ok(DependenciesSection {
            requires: Vec::new(),
            after: Vec::new(),
        });
    };
    check_keys(t, S, DEPENDENCIES_KEYS)?;
    Ok(DependenciesSection {
        requires: unit_names(t, S, "requires")?,
        after: unit_names(t, S, "after")?,
    })
}

fn parse_restart(root: &Table) -> Result<RestartSection, UnitError> {
    const S: &str = "restart";
    let mut out = RestartSection {
        policy: RestartPolicy::No,
        maximum_restarts: 0,
    };
    let Some(t) = table(root, S)? else {
        return Ok(out);
    };
    check_keys(t, S, RESTART_KEYS)?;
    if let Some(p) = string(t, S, "policy")? {
        out.policy = match p.as_str() {
            "no" => RestartPolicy::No,
            "on-failure" => RestartPolicy::OnFailure,
            "always" => RestartPolicy::Always,
            _ => {
                return Err(UnitError::InvalidValue {
                    key: path(S, "policy"),
                });
            }
        };
    }
    match t.get("maximum_restarts") {
        None => {}
        Some(Value::Integer(n)) => {
            out.maximum_restarts = u64::try_from(*n).map_err(|_| UnitError::InvalidValue {
                key: path(S, "maximum_restarts"),
            })?;
        }
        Some(_) => {
            return Err(UnitError::InvalidType {
                key: path(S, "maximum_restarts"),
            });
        }
    }
    Ok(out)
}

pub(crate) fn parse(text: &str) -> Result<Unit, UnitError> {
    let root: Table = text.parse().map_err(|_| UnitError::Syntax)?;
    check_keys(&root, "", TOP_KEYS)?;
    parse_schema_version(&root)?;
    Ok(Unit {
        service: parse_service(&root)?,
        execution: parse_execution(&root)?,
        dependencies: parse_dependencies(&root)?,
        restart: parse_restart(&root)?,
    })
}
