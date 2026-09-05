use std::fs;
use std::process::Command;

use pvestate::{initialize_local_state, validate_local_state, write_schemas};

#[test]
fn help_lists_compact_interface() {
    let output = Command::new(env!("CARGO_BIN_EXE_pves"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in [
        "init", "capture", "plan", "adopt", "apply", "validate", "recover",
    ] {
        assert!(stdout.contains(command));
    }
}

#[test]
fn plan_help_offers_machine_readable_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_pves"))
        .args(["plan", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--json"), "{stdout}");
    assert!(stdout.contains("machine-readable plan"), "{stdout}");
}

#[test]
fn adopt_requires_exactly_one_mode() {
    let missing = Command::new(env!("CARGO_BIN_EXE_pves"))
        .arg("adopt")
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let stderr = String::from_utf8(missing.stderr).unwrap();
    assert!(
        stderr.lines().any(|line| {
            line.starts_with("Usage: ") && line.split_whitespace().nth(2) == Some("adopt")
        }),
        "{stderr}"
    );
    for mode in ["--preview", "--all", "[ID]..."] {
        assert!(stderr.contains(mode), "{stderr}");
    }

    let conflicting = Command::new(env!("CARGO_BIN_EXE_pves"))
        .args(["adopt", "--preview", "lxc/106:mp0.backed_up_by_pve"])
        .output()
        .unwrap();
    assert!(!conflicting.status.success());
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
    assert!(root.join("pves.yml").is_file());
    assert!(root.join(".pves.env.example").is_file());
    assert!(root.join("config").is_dir());
    assert!(root.join("config/cluster.yml").is_file());
    assert!(root.join("config/node.yml").is_file());
    assert!(!root.join("config/site.yml").exists());
    assert!(!root.join("config/host.yml").exists());
    assert!(!root.join("config/firewall.yml").exists());
    assert!(root.join("observed/production").is_dir());
    validate_local_state(&root).expect("generated repository must satisfy every typed contract");
}

#[test]
fn configuration_rejects_unknown_fields() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("environment");
    initialize_local_state(&root).unwrap();
    let network = root.join("config/network.yml");
    let mut yaml = fs::read_to_string(&network).unwrap();
    yaml.push_str("unexpected_setting: true\n");
    fs::write(network, yaml).unwrap();
    let error = match validate_local_state(&root) {
        Ok(_) => panic!("unknown field was accepted"),
        Err(error) => format!("{error:#}"),
    };
    assert!(
        error.contains("unknown field `unexpected_setting`"),
        "{error}"
    );
}

#[test]
fn normal_configuration_allows_recovery_host_key_to_be_omitted() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("environment");
    initialize_local_state(&root).unwrap();
    let restore = root.join("config/restore.yml");
    let yaml = fs::read_to_string(&restore)
        .unwrap()
        .lines()
        .filter(|line| !line.trim_start().starts_with("expected_host_key_sha256:"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(restore, yaml).unwrap();

    validate_local_state(&root).unwrap();
}

#[test]
fn schema_command_writes_every_document_schema() {
    let temp = tempfile::tempdir().unwrap();
    write_schemas(Some(temp.path())).unwrap();
    for name in [
        "repository",
        "cluster",
        "node",
        "guests",
        "network",
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
    assert!(temp.path().join("management-scope.json").is_file());
}
