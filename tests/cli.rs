use std::fs;
use std::process::Command;

use pvestate::config::Repository;

#[test]
fn help_lists_compact_interface() {
    let output = Command::new(env!("CARGO_BIN_EXE_pves"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in ["init", "capture", "plan", "apply", "validate", "recover"] {
        assert!(stdout.contains(command));
    }
}

#[test]
fn init_creates_a_configuration_repository() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("environment");
    let status = Command::new(env!("CARGO_BIN_EXE_pves"))
        .args(["init", root.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(root.join("iac.yml").is_file());
    assert!(root.join("config").is_dir());
    assert!(root.join("observed/production").is_dir());
    Repository::open(&root).expect("generated repository must satisfy every typed contract");
}

#[test]
fn configuration_rejects_unknown_fields() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("environment");
    Repository::initialize(&root).unwrap();
    let network = root.join("config/network.yml");
    let mut yaml = fs::read_to_string(&network).unwrap();
    yaml.push_str("unexpected_setting: true\n");
    fs::write(network, yaml).unwrap();
    let error = match Repository::open(&root) {
        Ok(_) => panic!("unknown field was accepted"),
        Err(error) => format!("{error:#}"),
    };
    assert!(
        error.contains("unknown field `unexpected_setting`"),
        "{error}"
    );
}

#[test]
fn schema_command_writes_every_document_schema() {
    let temp = tempfile::tempdir().unwrap();
    Repository::write_schema(Some(temp.path())).unwrap();
    for name in [
        "repository",
        "site",
        "host",
        "guests",
        "network",
        "firewall",
        "storage",
        "backup",
        "restore",
        "recovery-checks",
        "services",
        "required-secrets",
    ] {
        assert!(
            temp.path().join(format!("{name}.schema.json")).is_file(),
            "{name}"
        );
    }
}
