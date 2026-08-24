use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupConfig {
    pub pbs: PbsBackup,
    pub pve_backup_job: PveBackupJob,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupJobs {
    pub prune: Schedule,
    pub verify: VerifySchedule,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub schedule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifySchedule {
    pub schedule: String,
    pub ignore_verified: bool,
    pub outdated_after_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PveBackupJob {
    pub datastore: String,
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
