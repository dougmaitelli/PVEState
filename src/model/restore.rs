use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreConfig {
    pub schema_version: u16,
    pub target: RestoreTarget,
    pub pbs_bootstrap: PbsBootstrap,
    pub archives: BTreeMap<u32, Option<String>>,
    pub restore_order: Vec<u32>,
    pub protected_vmids: Vec<u32>,
    pub reattach_mounts: Vec<RestoreMount>,
    pub application: RestoreApplication,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreTarget {
    pub expected_hostname: String,
    /// OpenSSH SHA-256 fingerprint, for example `SHA256:...`.
    pub expected_host_key_sha256: String,
    pub production_address: String,
    pub plan_max_age_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PbsBootstrap {
    pub vmid: u32,
    pub datastore: String,
    pub cache_path: String,
    pub s3_endpoint_id: String,
    pub bucket: String,
    pub region: String,
    pub lxc_template: Option<String>,
    pub storage_attached_to_pve: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreMount {
    pub vmid: u32,
    pub index: u16,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreApplication {
    pub repository: String,
    pub docker_guest_vmid: u32,
    pub configure_playbook: Option<String>,
    pub configure_command: Option<String>,
}
