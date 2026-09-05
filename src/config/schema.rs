use super::RepositoryDocument;
use crate::{model::*, scope};
use anyhow::Result;
use schemars::schema_for;
use std::{fs, path::Path};

pub(crate) fn write(output: Option<&Path>) -> Result<()> {
    let dir = output.unwrap_or_else(|| Path::new("schemas"));
    fs::create_dir_all(dir)?;

    macro_rules! write {
        ($name:literal, $ty:ty) => {
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

    Ok(())
}
