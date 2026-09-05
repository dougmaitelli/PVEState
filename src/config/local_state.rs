use super::RepositoryLayout;
use crate::model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct Repository {
    pub layout: RepositoryLayout,
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
    pub fn root(&self) -> &Path {
        self.layout.root()
    }

    pub fn runtime(&self) -> PathBuf {
        self.layout.runtime()
    }

    pub fn observed(&self) -> PathBuf {
        self.layout.observed()
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
