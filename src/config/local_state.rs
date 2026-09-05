use super::RepositoryLayout;
use crate::model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(crate) struct LocalState {
    pub(crate) layout: RepositoryLayout,
    pub(crate) guests: Guests,
    pub(crate) network: Network,
    pub(crate) recovery_checks: RecoveryChecks,
    pub(crate) manifest: RepositoryManifest,
    pub(crate) cluster: ClusterConfig,
    pub(crate) node: NodeConfig,
    pub(crate) storage: StorageConfig,
    pub(crate) backup: BackupConfig,
    pub(crate) restore: RestoreConfig,
    pub(crate) services: ServicesConfig,
    pub(crate) required_secrets: RequiredSecretsConfig,
}

impl LocalState {
    pub(super) fn confirm_all_documents_loaded(&self) {
        let _ = (&self.manifest, &self.services, &self.required_secrets);
    }

    pub(crate) fn root(&self) -> &Path {
        self.layout.root()
    }

    pub(crate) fn runtime(&self) -> PathBuf {
        self.layout.runtime()
    }

    pub(crate) fn observed(&self) -> PathBuf {
        self.layout.observed()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryDocument {
    pub(crate) pves: RepositoryManifest,
    pub(crate) cluster: ClusterConfig,
    pub(crate) node: NodeConfig,
    pub(crate) guests: Guests,
    pub(crate) network: Network,
    pub(crate) storage: StorageConfig,
    pub(crate) backup: BackupConfig,
    pub(crate) restore: RestoreConfig,
    pub(crate) recovery_checks: RecoveryChecks,
    pub(crate) services: ServicesConfig,
    pub(crate) required_secrets: RequiredSecretsConfig,
}
