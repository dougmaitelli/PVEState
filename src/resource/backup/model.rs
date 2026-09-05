use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DatastoreBackend {
    Local,
    S3,
}
impl std::fmt::Display for DatastoreBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Local => "local",
            Self::S3 => "s3",
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SyncDirection {
    Pull,
    Push,
}
impl std::fmt::Display for SyncDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pull => "pull",
            Self::Push => "push",
        })
    }
}
fn pull() -> SyncDirection {
    SyncDirection::Pull
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum BackupMode {
    Snapshot,
    Suspend,
    Stop,
}
impl std::fmt::Display for BackupMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Snapshot => "snapshot",
            Self::Suspend => "suspend",
            Self::Stop => "stop",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupConfig {
    pub(crate) pbs: PbsBackup,
    #[serde(default)]
    pub(crate) pve_backup_jobs: BTreeMap<String, PveBackupJob>,
    #[serde(default)]
    pub(crate) absent_pve_backup_jobs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PbsBackup {
    pub(crate) endpoint: String,
    pub(crate) version: String,
    pub(crate) guest: BackupGuest,
    pub(crate) datastore: Datastore,
    pub(crate) s3_endpoint: S3Endpoint,
    pub(crate) jobs: BackupJobs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupGuest {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) vmid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Datastore {
    pub(crate) name: String,
    pub(crate) backend: DatastoreBackend,
    pub(crate) local_cache_path: String,
    pub(crate) bucket: String,
    pub(crate) s3_endpoint_id: String,
    pub(crate) garbage_collection_schedule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct S3Endpoint {
    pub(crate) id: String,
    pub(crate) endpoint_template: String,
    pub(crate) region: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupJobs {
    #[serde(default)]
    pub(crate) prune: BTreeMap<String, PruneJob>,
    #[serde(default)]
    pub(crate) verify: BTreeMap<String, VerifyJob>,
    #[serde(default)]
    pub(crate) sync: BTreeMap<String, SyncJob>,
    #[serde(default)]
    pub(crate) absent_prune: Vec<String>,
    #[serde(default)]
    pub(crate) absent_verify: Vec<String>,
    #[serde(default)]
    pub(crate) absent_sync: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PruneJob {
    pub(crate) store: String,
    pub(crate) schedule: String,
    #[serde(default)]
    pub(crate) keep_last: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerifyJob {
    pub(crate) store: String,
    pub(crate) schedule: String,
    pub(crate) ignore_verified: bool,
    pub(crate) outdated_after_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyncJob {
    pub(crate) store: String,
    pub(crate) remote_store: String,
    #[serde(default)]
    pub(crate) remote: Option<String>,
    #[serde(default)]
    pub(crate) schedule: Option<String>,
    #[serde(default)]
    pub(crate) remove_vanished: bool,
    #[serde(default = "pull")]
    pub(crate) direction: SyncDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PveBackupJob {
    pub(crate) storage: String,
    pub(crate) schedule: String,
    pub(crate) mode: BackupMode,
    pub(crate) guest_ids: Vec<u32>,
    pub(crate) retention: Retention,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Retention {
    pub(crate) keep_last: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_backup_modes_backends_and_directions() {
        assert!(serde_yaml::from_str::<BackupMode>("live").is_err());
        assert!(serde_yaml::from_str::<DatastoreBackend>("tape").is_err());
        assert!(serde_yaml::from_str::<SyncDirection>("sideways").is_err());
    }
}
