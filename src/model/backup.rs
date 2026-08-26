use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupConfig {
    pub pbs: PbsBackup,
    #[serde(default)]
    pub pve_backup_jobs: BTreeMap<String, PveBackupJob>,
    #[serde(default)]
    pub absent_pve_backup_jobs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PbsBackup {
    pub endpoint: String,
    pub version: String,
    pub guest: BackupGuest,
    pub datastore: Datastore,
    pub s3_endpoint: S3Endpoint,
    pub jobs: BackupJobs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupGuest {
    #[serde(rename = "type")]
    pub kind: String,
    pub vmid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Datastore {
    pub name: String,
    pub backend: String,
    pub local_cache_path: String,
    pub bucket: String,
    pub s3_endpoint_id: String,
    pub garbage_collection_schedule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct S3Endpoint {
    pub id: String,
    pub endpoint_template: String,
    pub region: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupJobs {
    #[serde(default)]
    pub prune: BTreeMap<String, PruneJob>,
    #[serde(default)]
    pub verify: BTreeMap<String, VerifyJob>,
    #[serde(default)]
    pub sync: BTreeMap<String, SyncJob>,
    #[serde(default)]
    pub absent_prune: Vec<String>,
    #[serde(default)]
    pub absent_verify: Vec<String>,
    #[serde(default)]
    pub absent_sync: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PruneJob {
    pub store: String,
    pub schedule: String,
    #[serde(default)]
    pub keep_last: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyJob {
    pub store: String,
    pub schedule: String,
    pub ignore_verified: bool,
    pub outdated_after_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncJob {
    pub store: String,
    pub remote_store: String,
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub remove_vanished: bool,
    #[serde(default = "pull")]
    pub direction: String,
}

fn pull() -> String {
    "pull".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PveBackupJob {
    pub storage: String,
    pub schedule: String,
    pub mode: String,
    pub guest_ids: Vec<u32>,
    pub retention: Retention,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Retention {
    pub keep_last: u32,
}
