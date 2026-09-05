use crate::utility::runtime_security;
use anyhow::Result;
use std::{fs, path::Path};

pub fn initialize(path: &Path) -> Result<()> {
    fs::create_dir_all(path.join("config"))?;
    fs::create_dir_all(path.join("observed/production"))?;
    runtime_security::prepare(&path.join(".runtime"))?;
    fs::write(path.join("pves.yml"), "schema_version: 1\n")?;
    fs::write(path.join(".gitignore"), ".pves.env\n.secrets/\n.runtime/\n")?;

    for (name, content) in [
        (
            "guests.yml",
            include_str!("../../examples/basic/config/guests.yml"),
        ),
        (
            "network.yml",
            include_str!("../../examples/basic/config/network.yml"),
        ),
        (
            "recovery-checks.yml",
            include_str!("../../examples/basic/config/recovery-checks.yml"),
        ),
        (
            "restore.yml",
            include_str!("../../examples/basic/config/restore.yml"),
        ),
        (
            "cluster.yml",
            include_str!("../../examples/basic/config/cluster.yml"),
        ),
        (
            "node.yml",
            include_str!("../../examples/basic/config/node.yml"),
        ),
        (
            "storage.yml",
            include_str!("../../examples/basic/config/storage.yml"),
        ),
        (
            "backup.yml",
            include_str!("../../examples/basic/config/backup.yml"),
        ),
        (
            "services.yml",
            include_str!("../../examples/basic/config/services.yml"),
        ),
        (
            "required-secrets.yml",
            include_str!("../../examples/basic/config/required-secrets.yml"),
        ),
    ] {
        fs::write(path.join("config").join(name), content)?;
    }

    fs::write(
        path.join(".pves.env.example"),
        include_str!("../../examples/basic/.pves.env.example"),
    )?;

    Ok(())
}
