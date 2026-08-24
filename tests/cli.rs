use std::process::Command;

#[test]
fn help_lists_compact_interface() {
    let output = Command::new(env!("CARGO_BIN_EXE_pve-iac"))
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
    let status = Command::new(env!("CARGO_BIN_EXE_pve-iac"))
        .args(["init", root.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(root.join("iac.yml").is_file());
    assert!(root.join("config").is_dir());
    assert!(root.join("observed/production").is_dir());
}
