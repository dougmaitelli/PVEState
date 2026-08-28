use crate::{model::*, scope, utility::runtime_security};
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
    pub recovery_checks: RecoveryChecks,
    pub manifest: RepositoryManifest,
    pub cluster: ClusterConfig,
    pub node: NodeConfig,
    pub storage: StorageConfig,
    pub backup: BackupConfig,
    pub restore: RestoreConfig,
    pub services: ServicesConfig,
    pub required_secrets: RequiredSecretsConfig,
}

impl Repository {
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            root: root.to_path_buf(),
            guests: read(root, "guests.yml")?,
            network: read(root, "network.yml")?,
            recovery_checks: read(root, "recovery-checks.yml")?,
            manifest: serde_yaml::from_str(&fs::read_to_string(root.join("pves.yml"))?)?,
            cluster: read(root, "cluster.yml")?,
            node: read(root, "node.yml")?,
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

    pub fn secure_runtime(&self) -> Result<PathBuf> {
        let runtime = self.runtime();
        runtime_security::prepare(&runtime)?;
        Ok(runtime)
    }

    pub fn observed(&self) -> PathBuf {
        self.root.join("observed/production")
    }

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
        write!("cluster.schema.json", ClusterConfig);
        write!("node.schema.json", NodeConfig);
        write!("guests.schema.json", Guests);
        write!("network.schema.json", Network);
        write!("storage.schema.json", StorageConfig);
        write!("backup.schema.json", BackupConfig);
        write!("restore.schema.json", RestoreConfig);
        write!("recovery-checks.schema.json", RecoveryChecks);
        write!("services.schema.json", ServicesConfig);
        write!("required-secrets.schema.json", RequiredSecretsConfig);
        fs::write(
            dir.join("management-scope.json"),
            serde_json::to_string_pretty(scope::entries())? + "\n",
        )?;
        println!("wrote configuration schemas to {}", dir.display());
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryDocument {
    pub pves: RepositoryManifest,
    pub cluster: ClusterConfig,
    pub node: NodeConfig,
    pub guests: Guests,
    pub network: Network,
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
