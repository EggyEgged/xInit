//! specs/M0.md, "Unit loading": B-M0-001 to B-M0-010 through `parse_unit` / `load_unit`.
//! Every error fixture has exactly one defect (spec: "a unit with exactly one defect
//! yields exactly that error").

use super::common::TempDir;
use xinit_api::{
    DependenciesSection, ExecutionSection, RestartPolicy, RestartSection, ServiceSection, ServiceType, Unit,
    UnitError, load_unit, parse_unit,
};

/// The B-M0-001 example text, verbatim.
const MINIMAL: &str = "schema_version = 1\n[service]\nname = \"sleeper\"\ntype = \"simple\"\n[execution]\ncommand = [\"/usr/bin/sleep\", \"1\"]\n";

/// The B-M0-001 expected value, written out by hand from the spec.
fn minimal_unit() -> Unit {
    Unit {
        service: ServiceSection {
            name: "sleeper".to_string(),
            description: None,
            service_type: ServiceType::Simple,
            provides: vec![],
        },
        execution: ExecutionSection { command: vec!["/usr/bin/sleep".to_string(), "1".to_string()], user: None },
        dependencies: DependenciesSection { requires: vec![], after: vec![] },
        restart: RestartSection { policy: RestartPolicy::No, maximum_restarts: 0 },
    }
}

/// MINIMAL with one exact substring replaced (the substring must occur).
fn edit(from: &str, to: &str) -> String {
    assert!(MINIMAL.contains(from), "fixture bug: {from:?} not in MINIMAL");
    MINIMAL.replacen(from, to, 1)
}

fn ok(text: &str) -> Unit {
    parse_unit(text).unwrap_or_else(|e| panic!("expected Ok, got Err({e:?}) for:\n{text}"))
}

fn err(text: &str) -> UnitError {
    match parse_unit(text) {
        Ok(u) => panic!("expected an error, got Ok({u:?}) for:\n{text}"),
        Err(e) => e,
    }
}

fn s(v: &str) -> String {
    v.to_string()
}

fn unknown(k: &str) -> UnitError {
    UnitError::UnknownKey { key: s(k) }
}
fn missing(k: &str) -> UnitError {
    UnitError::MissingKey { key: s(k) }
}
fn invalid_type(k: &str) -> UnitError {
    UnitError::InvalidType { key: s(k) }
}
fn invalid_value(k: &str) -> UnitError {
    UnitError::InvalidValue { key: s(k) }
}

// --- B-M0-001 -------------------------------------------------------------

// spec: B-M0-001
#[test]
fn b_m0_001_minimal_unit_spec_example() {
    assert_eq!(ok(MINIMAL), minimal_unit());
}

// spec: B-M0-001 (load_unit = read + parse_unit)
#[test]
fn b_m0_001_minimal_unit_via_load_unit() {
    let t = TempDir::new();
    let p = t.write("sleeper.toml", MINIMAL);
    assert_eq!(load_unit(&p), Ok(minimal_unit()));
}

// spec: B-M0-001 (TOML table order carries no meaning)
#[test]
fn b_m0_001_tables_in_other_order() {
    let text = "schema_version = 1\n[execution]\ncommand = [\"/usr/bin/sleep\", \"1\"]\n[service]\ntype = \"simple\"\nname = \"sleeper\"\n";
    assert_eq!(ok(text), minimal_unit());
}

// --- B-M0-002 -------------------------------------------------------------

// spec: B-M0-002
#[test]
fn b_m0_002_all_optional_fields() {
    let text = "schema_version = 1\n[service]\nname = \"sleeper\"\ndescription = \"Sleeps\"\ntype = \"simple\"\nprovides = [\"nap\"]\n[execution]\ncommand = [\"/usr/bin/sleep\", \"1\"]\nuser = \"nobody\"\n";
    let mut want = minimal_unit();
    want.service.description = Some(s("Sleeps"));
    want.service.provides = vec![s("nap")];
    want.execution.user = Some(s("nobody"));
    assert_eq!(ok(text), want);
}

// spec: B-M0-002 (empty [dependencies] and [restart] tables accepted), B-M0-010 (defaults)
#[test]
fn b_m0_002_empty_dependencies_and_restart_tables() {
    let text = format!("{MINIMAL}[dependencies]\n[restart]\n");
    assert_eq!(ok(&text), minimal_unit());
}

// --- B-M0-003 -------------------------------------------------------------

// spec: B-M0-003
#[test]
fn b_m0_003_schema_version_absent() {
    assert_eq!(err(&edit("schema_version = 1\n", "")), missing("schema_version"));
}

// spec: B-M0-003 (spec example)
#[test]
fn b_m0_003_schema_version_string() {
    assert_eq!(err(&edit("schema_version = 1", "schema_version = \"1\"")), invalid_type("schema_version"));
}

// spec: B-M0-003 (a float is not an integer)
#[test]
fn b_m0_003_schema_version_float() {
    assert_eq!(err(&edit("schema_version = 1", "schema_version = 1.0")), invalid_type("schema_version"));
}

// spec: B-M0-003 (spec example)
#[test]
fn b_m0_003_schema_version_two() {
    assert_eq!(err(&edit("schema_version = 1", "schema_version = 2")), UnitError::SchemaVersion { found: 2 });
}

// spec: B-M0-003
#[test]
fn b_m0_003_schema_version_zero() {
    assert_eq!(err(&edit("schema_version = 1", "schema_version = 0")), UnitError::SchemaVersion { found: 0 });
}

// spec: B-M0-003
#[test]
fn b_m0_003_schema_version_negative() {
    assert_eq!(err(&edit("schema_version = 1", "schema_version = -1")), UnitError::SchemaVersion { found: -1 });
}

// --- B-M0-004 -------------------------------------------------------------

// spec: B-M0-004 (spec example; `command` stays, so the typo is the only defect)
#[test]
fn b_m0_004_unknown_key_execution_comand() {
    let text = format!("{MINIMAL}comand = [\"/usr/bin/sleep\", \"1\"]\n");
    assert_eq!(err(&text), unknown("execution.comand"));
}

// spec: B-M0-004 (spec example)
#[test]
fn b_m0_004_unknown_key_in_service() {
    assert_eq!(err(&edit("type = \"simple\"\n", "type = \"simple\"\nxinit_m0_bogus = 1\n")), unknown("service.xinit_m0_bogus"));
}

// spec: B-M0-004
#[test]
fn b_m0_004_unknown_key_in_execution() {
    let text = format!("{MINIMAL}xinit_m0_bogus = \"x\"\n");
    assert_eq!(err(&text), unknown("execution.xinit_m0_bogus"));
}

// spec: B-M0-004 (spec example: top-level table)
#[test]
fn b_m0_004_unknown_top_level_table() {
    let text = format!("{MINIMAL}[xinit_m0_bogus]\n");
    assert_eq!(err(&text), unknown("xinit_m0_bogus"));
}

// spec: B-M0-004 (top-level keys use the bare name)
#[test]
fn b_m0_004_unknown_top_level_key() {
    assert_eq!(err(&edit("schema_version = 1\n", "schema_version = 1\nxinit_m0_bogus = 1\n")), unknown("xinit_m0_bogus"));
}

// spec: B-M0-004 (a sub-table is a key of its parent table)
#[test]
fn b_m0_004_unknown_sub_table_of_service() {
    let text = format!("{MINIMAL}[service.xinit_m0_bogus]\n");
    assert_eq!(err(&text), unknown("service.xinit_m0_bogus"));
}

// --- B-M0-005 -------------------------------------------------------------

// spec: B-M0-005 (spec example)
#[test]
fn b_m0_005_missing_service_type() {
    assert_eq!(err(&edit("type = \"simple\"\n", "")), missing("service.type"));
}

// spec: B-M0-005 (spec example)
#[test]
fn b_m0_005_missing_execution_table() {
    assert_eq!(err(&edit("[execution]\ncommand = [\"/usr/bin/sleep\", \"1\"]\n", "")), missing("execution"));
}

// spec: B-M0-005 (service.name is required per the schema table)
#[test]
fn b_m0_005_missing_service_name() {
    assert_eq!(err(&edit("name = \"sleeper\"\n", "")), missing("service.name"));
}

// spec: B-M0-005 (execution.command is required per the schema table)
#[test]
fn b_m0_005_missing_execution_command() {
    let text = edit("command = [\"/usr/bin/sleep\", \"1\"]\n", "");
    assert_eq!(err(&text), missing("execution.command"));
}

// spec: B-M0-005 (whole table uses its name)
#[test]
fn b_m0_005_missing_service_table() {
    assert_eq!(err(&edit("[service]\nname = \"sleeper\"\ntype = \"simple\"\n", "")), missing("service"));
}

// --- B-M0-006 -------------------------------------------------------------

// spec: B-M0-006 (spec example)
#[test]
fn b_m0_006_command_is_string() {
    let text = edit("command = [\"/usr/bin/sleep\", \"1\"]", "command = \"/usr/bin/sleep 1\"");
    assert_eq!(err(&text), invalid_type("execution.command"));
}

// spec: B-M0-006 (spec example)
#[test]
fn b_m0_006_provides_is_string() {
    assert_eq!(err(&edit("type = \"simple\"\n", "type = \"simple\"\nprovides = \"nap\"\n")), invalid_type("service.provides"));
}

// spec: B-M0-006
#[test]
fn b_m0_006_name_is_integer() {
    assert_eq!(err(&edit("name = \"sleeper\"", "name = 1")), invalid_type("service.name"));
}

// spec: B-M0-006 (a non-string type is a type error, not an unsupported type)
#[test]
fn b_m0_006_type_is_integer() {
    assert_eq!(err(&edit("type = \"simple\"", "type = 1")), invalid_type("service.type"));
}

// spec: B-M0-006
#[test]
fn b_m0_006_description_is_bool() {
    assert_eq!(err(&edit("type = \"simple\"\n", "type = \"simple\"\ndescription = true\n")), invalid_type("service.description"));
}

// spec: B-M0-006
#[test]
fn b_m0_006_user_is_integer() {
    let text = format!("{MINIMAL}user = 0\n");
    assert_eq!(err(&text), invalid_type("execution.user"));
}

// spec: B-M0-006 (command must be an array of strings)
#[test]
fn b_m0_006_command_element_not_string() {
    let text = edit("command = [\"/usr/bin/sleep\", \"1\"]", "command = [\"/usr/bin/sleep\", 1]");
    assert_eq!(err(&text), invalid_type("execution.command"));
}

// --- B-M0-007 -------------------------------------------------------------

// spec: B-M0-007 (spec example)
#[test]
fn b_m0_007_bogus_type_unsupported() {
    let text = edit("type = \"simple\"", "type = \"xinit-m0-bogus\"");
    assert_eq!(err(&text), UnitError::UnsupportedServiceType { value: s("xinit-m0-bogus") });
}

// spec: B-M0-007 (case-sensitive, spec example)
#[test]
fn b_m0_007_capitalised_simple_unsupported() {
    let text = edit("type = \"simple\"", "type = \"Simple\"");
    assert_eq!(err(&text), UnitError::UnsupportedServiceType { value: s("Simple") });
}

// --- B-M0-008 -------------------------------------------------------------

// spec: B-M0-008
#[test]
fn b_m0_008_command_empty() {
    let text = edit("command = [\"/usr/bin/sleep\", \"1\"]", "command = []");
    assert_eq!(err(&text), invalid_value("execution.command"));
}

// spec: B-M0-008
#[test]
fn b_m0_008_command_relative() {
    let text = edit("command = [\"/usr/bin/sleep\", \"1\"]", "command = [\"sleep\", \"1\"]");
    assert_eq!(err(&text), invalid_value("execution.command"));
}

// spec: B-M0-008 (element 0 must start with '/'; "./x" does not)
#[test]
fn b_m0_008_command_dot_relative() {
    let text = edit("command = [\"/usr/bin/sleep\", \"1\"]", "command = [\"./sleep\", \"1\"]");
    assert_eq!(err(&text), invalid_value("execution.command"));
}

// spec: B-M0-008 (only element 0 must be absolute; a single element is enough)
#[test]
fn b_m0_008_command_single_absolute_element_ok() {
    let text = edit("command = [\"/usr/bin/sleep\", \"1\"]", "command = [\"/usr/bin/false\"]");
    let mut want = minimal_unit();
    want.execution.command = vec![s("/usr/bin/false")];
    assert_eq!(ok(&text), want);
}

// spec: B-M0-008
#[test]
fn b_m0_008_name_with_space_and_capital() {
    assert_eq!(err(&edit("name = \"sleeper\"", "name = \"My Unit\"")), invalid_value("service.name"));
}

// spec: B-M0-008
#[test]
fn b_m0_008_name_empty() {
    assert_eq!(err(&edit("name = \"sleeper\"", "name = \"\"")), invalid_value("service.name"));
}

// spec: B-M0-008
#[test]
fn b_m0_008_user_empty() {
    let text = format!("{MINIMAL}user = \"\"\n");
    assert_eq!(err(&text), invalid_value("execution.user"));
}

// spec: B-M0-008 (pattern ^[a-z0-9][a-z0-9._-]{0,63}$: 1 + 63 = 64 chars is the maximum)
#[test]
fn b_m0_008_name_64_chars_ok() {
    let name = format!("a{}", "b".repeat(63)); // 1 + 63 = 64 characters
    let text = edit("name = \"sleeper\"", &format!("name = \"{name}\""));
    let mut want = minimal_unit();
    want.service.name = name;
    assert_eq!(ok(&text), want);
}

// spec: B-M0-008 (1 + 64 = 65 chars exceeds {0,63})
#[test]
fn b_m0_008_name_65_chars_invalid() {
    let name = format!("a{}", "b".repeat(64)); // 1 + 64 = 65 characters
    assert_eq!(err(&edit("name = \"sleeper\"", &format!("name = \"{name}\""))), invalid_value("service.name"));
}

// spec: B-M0-008 (first character must be [a-z0-9])
#[test]
fn b_m0_008_name_leading_hyphen_invalid() {
    assert_eq!(err(&edit("name = \"sleeper\"", "name = \"-sleeper\"")), invalid_value("service.name"));
}

// spec: B-M0-008 (first character must be [a-z0-9])
#[test]
fn b_m0_008_name_leading_dot_invalid() {
    assert_eq!(err(&edit("name = \"sleeper\"", "name = \".sleeper\"")), invalid_value("service.name"));
}

// spec: B-M0-008 (digits first and '.', '_', '-' later are allowed by the pattern)
#[test]
fn b_m0_008_name_with_all_allowed_classes_ok() {
    let text = edit("name = \"sleeper\"", "name = \"9a.b_c-d\"");
    let mut want = minimal_unit();
    want.service.name = s("9a.b_c-d");
    assert_eq!(ok(&text), want);
}

// --- B-M0-009 -------------------------------------------------------------

// spec: B-M0-009 (ENOENT = 2)
#[test]
fn b_m0_009_load_missing_file_enoent() {
    let t = TempDir::new();
    assert_eq!(load_unit(&t.join("xinit-m0-absent.toml")), Err(UnitError::Io { errno: 2 }));
}

// spec: B-M0-009 (spec example)
#[test]
fn b_m0_009_syntax_error() {
    assert_eq!(err("schema_version = 1\n[service\n"), UnitError::Syntax);
}

// spec: B-M0-009 (load_unit reports the parse error of a readable file)
#[test]
fn b_m0_009_load_file_with_syntax_error() {
    let t = TempDir::new();
    let p = t.write("bad.toml", "schema_version = 1\n[service\n");
    assert_eq!(load_unit(&p), Err(UnitError::Syntax));
}

// --- B-M0-010 -------------------------------------------------------------

// spec: B-M0-010 (worked example 1)
#[test]
fn b_m0_010_full_dependencies_and_restart() {
    let text = format!(
        "{MINIMAL}[dependencies]\nrequires = [\"network\", \"db.socket\"]\nafter = [\"network\"]\n[restart]\npolicy = \"on-failure\"\nmaximum_restarts = 3\n"
    );
    let mut want = minimal_unit();
    want.dependencies = DependenciesSection { requires: vec![s("network"), s("db.socket")], after: vec![s("network")] };
    want.restart = RestartSection { policy: RestartPolicy::OnFailure, maximum_restarts: 3 };
    assert_eq!(ok(&text), want);
}

// spec: B-M0-010 (worked example 2)
#[test]
fn b_m0_010_policy_always_default_count() {
    let mut want = minimal_unit();
    want.restart = RestartSection { policy: RestartPolicy::Always, maximum_restarts: 0 };
    assert_eq!(ok(&format!("{MINIMAL}[restart]\npolicy = \"always\"\n")), want);
}

// spec: B-M0-010 (worked example 2: "no" gives No)
#[test]
fn b_m0_010_policy_no_explicit() {
    let text = format!("{MINIMAL}[restart]\npolicy = \"no\"\nmaximum_restarts = 7\n");
    let mut want = minimal_unit();
    want.restart = RestartSection { policy: RestartPolicy::No, maximum_restarts: 7 };
    assert_eq!(ok(&text), want);
}

// spec: B-M0-010 (worked example 3)
#[test]
fn b_m0_010_after_only() {
    let mut want = minimal_unit();
    want.dependencies = DependenciesSection { requires: vec![], after: vec![s("network")] };
    assert_eq!(ok(&format!("{MINIMAL}[dependencies]\nafter = [\"network\"]\n")), want);
}

// spec: B-M0-010
#[test]
fn b_m0_010_requires_is_string() {
    let text = format!("{MINIMAL}[dependencies]\nrequires = \"network\"\n");
    assert_eq!(err(&text), invalid_type("dependencies.requires"));
}

// spec: B-M0-010 (array element not a string)
#[test]
fn b_m0_010_after_element_integer() {
    let text = format!("{MINIMAL}[dependencies]\nafter = [1]\n");
    assert_eq!(err(&text), invalid_type("dependencies.after"));
}

// spec: B-M0-010 (key path is the key's, even for one bad element)
#[test]
fn b_m0_010_requires_capitalised_name() {
    let text = format!("{MINIMAL}[dependencies]\nrequires = [\"network\", \"Network\"]\n");
    assert_eq!(err(&text), invalid_value("dependencies.requires"));
}

// spec: B-M0-010
#[test]
fn b_m0_010_after_empty_name() {
    let text = format!("{MINIMAL}[dependencies]\nafter = [\"\"]\n");
    assert_eq!(err(&text), invalid_value("dependencies.after"));
}

// spec: B-M0-010
#[test]
fn b_m0_010_policy_integer() {
    assert_eq!(err(&format!("{MINIMAL}[restart]\npolicy = 1\n")), invalid_type("restart.policy"));
}

// spec: B-M0-010
#[test]
fn b_m0_010_policy_bogus() {
    let text = format!("{MINIMAL}[restart]\npolicy = \"xinit-m0-bogus\"\n");
    assert_eq!(err(&text), invalid_value("restart.policy"));
}

// spec: B-M0-010 (policy values are case-sensitive strings from a fixed set)
#[test]
fn b_m0_010_policy_capitalised() {
    let text = format!("{MINIMAL}[restart]\npolicy = \"Always\"\n");
    assert_eq!(err(&text), invalid_value("restart.policy"));
}

// spec: B-M0-010
#[test]
fn b_m0_010_maximum_restarts_string() {
    let text = format!("{MINIMAL}[restart]\nmaximum_restarts = \"3\"\n");
    assert_eq!(err(&text), invalid_type("restart.maximum_restarts"));
}

// spec: B-M0-010
#[test]
fn b_m0_010_maximum_restarts_negative() {
    let text = format!("{MINIMAL}[restart]\nmaximum_restarts = -1\n");
    assert_eq!(err(&text), invalid_value("restart.maximum_restarts"));
}

// spec: B-M0-010
#[test]
fn b_m0_010_unknown_key_in_dependencies() {
    let text = format!("{MINIMAL}[dependencies]\nxinit_m0_bogus = 1\n");
    assert_eq!(err(&text), unknown("dependencies.xinit_m0_bogus"));
}

// spec: B-M0-010
#[test]
fn b_m0_010_unknown_key_in_restart() {
    let text = format!("{MINIMAL}[restart]\nxinit_m0_bogus = 1\n");
    assert_eq!(err(&text), unknown("restart.xinit_m0_bogus"));
}
