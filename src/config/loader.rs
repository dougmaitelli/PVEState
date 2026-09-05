use super::{LocalState, RepositoryLayout};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{fs, path::Path};

pub(crate) fn open(root: &Path) -> Result<LocalState> {
    let layout = RepositoryLayout::new(root);

    let state = LocalState {
        guests: read(&layout, "guests.yml")?,
        network: read(&layout, "network.yml")?,
        recovery_checks: read(&layout, "recovery-checks.yml")?,
        manifest: serde_yaml::from_str(&fs::read_to_string(
            root.join(crate::config::artifacts::REPOSITORY_MANIFEST),
        )?)?,
        cluster: read(&layout, "cluster.yml")?,
        node: read(&layout, "node.yml")?,
        storage: read(&layout, "storage.yml")?,
        backup: read(&layout, "backup.yml")?,
        restore: read(&layout, "restore.yml")?,
        services: read(&layout, "services.yml")?,
        required_secrets: read(&layout, "required-secrets.yml")?,
        layout,
    };
    state.confirm_all_documents_loaded();
    Ok(state)
}

fn read<T: for<'de> Deserialize<'de>>(layout: &RepositoryLayout, name: &str) -> Result<T> {
    let path = layout.document(name);
    serde_yaml::from_str(
        &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))
}
