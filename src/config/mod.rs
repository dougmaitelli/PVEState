use crate::model::*;
use anyhow::{Context, Result};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct Repository {
    pub root: PathBuf,
    pub guests: Guests,
    pub network: Network,
    pub firewall: FirewallConfig,
    pub recovery_checks: RecoveryChecks,
    pub manifest: RepositoryManifest,
    pub site: SiteConfig,
    pub host: HostConfig,
    pub storage: StorageConfig,
    pub backup: BackupConfig,
    pub restore: RestoreConfig,
    pub services: ServicesConfig,
    pub required_secrets: RequiredSecretsConfig,
}

impl Repository {
    pub fn open(root: &Path) -> Result<Self> {
        dotenvy::from_path(root.join(".env")).ok();
        Ok(Self {
            root: root.to_path_buf(),
            guests: read(root, "guests.yml")?,
            network: read(root, "network.yml")?,
            firewall: read(root, "firewall.yml")?,
            recovery_checks: read(root, "recovery-checks.yml")?,
            manifest: serde_yaml::from_str(&fs::read_to_string(root.join("iac.yml"))?)?,
            site: read(root, "site.yml")?,
            host: read(root, "host.yml")?,
            storage: read(root, "storage.yml")?,
            backup: read(root, "backup.yml")?,
            restore: read(root, "restore.yml")?,
            services: read(root, "services.yml")?,
            required_secrets: read(root, "required-secrets.yml")?,
        })
    }

    pub fn runtime(&self) -> PathBuf {
        self.root.join(".runtime")
    }

    pub fn observed(&self) -> PathBuf {
        self.root.join("observed/production")
    }

    pub fn initialize(path: &Path) -> Result<()> {
        fs::create_dir_all(path.join("config"))?;
        fs::create_dir_all(path.join("observed/production"))?;
        fs::write(path.join("iac.yml"), "schema_version: 1\n")?;
        fs::write(path.join(".gitignore"), ".env\n.secrets/\n.runtime/\n")?;
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
                "firewall.yml",
                include_str!("../../examples/basic/config/firewall.yml"),
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
                "site.yml",
                include_str!("../../examples/basic/config/site.yml"),
            ),
            (
                "host.yml",
                include_str!("../../examples/basic/config/host.yml"),
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
            path.join(".env.example"),
            include_str!("../../examples/basic/.env.example"),
        )?;
        println!("initialized configuration repository: {}", path.display());
        Ok(())
    }

    pub fn write_schema(output: Option<&Path>) -> Result<()> {
        let dir = output.unwrap_or_else(|| Path::new("schemas"));
        fs::create_dir_all(dir)?;
        macro_rules! write {
            ($name:literal,$ty:ty) => {
                fs::write(
                    dir.join($name),
                    serde_json::to_string_pretty(&schema_for!($ty))? + "\n",
                )?
            };
        }
        write!("repository.schema.json", RepositoryDocument);
        write!("site.schema.json", SiteConfig);
        write!("host.schema.json", HostConfig);
        write!("guests.schema.json", Guests);
        write!("network.schema.json", Network);
        write!("firewall.schema.json", FirewallConfig);
        write!("storage.schema.json", StorageConfig);
        write!("backup.schema.json", BackupConfig);
        write!("restore.schema.json", RestoreConfig);
        write!("recovery-checks.schema.json", RecoveryChecks);
        write!("services.schema.json", ServicesConfig);
        write!("required-secrets.schema.json", RequiredSecretsConfig);
        println!("wrote configuration schemas to {}", dir.display());
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryDocument {
    pub iac: RepositoryManifest,
    pub site: SiteConfig,
    pub host: HostConfig,
    pub guests: Guests,
    pub network: Network,
    pub firewall: FirewallConfig,
    pub storage: StorageConfig,
    pub backup: BackupConfig,
    pub restore: RestoreConfig,
    pub recovery_checks: RecoveryChecks,
    pub services: ServicesConfig,
    pub required_secrets: RequiredSecretsConfig,
}

fn read<T: for<'a> Deserialize<'a>>(root: &Path, name: &str) -> Result<T> {
    let path = root.join("config").join(name);
    serde_yaml::from_str(
        &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))
}

pub(crate) fn env(name: &str, default: Option<&str>) -> Result<String> {
    std::env::var(name)
        .or_else(|_| {
            default
                .map(str::to_owned)
                .ok_or(std::env::VarError::NotPresent)
        })
        .with_context(|| format!("missing {name}"))
}
