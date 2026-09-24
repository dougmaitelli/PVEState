use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreConfig {
    pub(crate) schema_version: u16,
    pub(crate) target: RestoreTarget,
    pub(crate) pbs_bootstrap: PbsBootstrap,
    pub(crate) archives: BTreeMap<u32, Option<String>>,
    pub(crate) restore_order: Vec<u32>,
    pub(crate) protected_vmids: Vec<u32>,
    pub(crate) reattach_mounts: Vec<RestoreMount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreTarget {
    pub(crate) expected_hostname: String,
    /// OpenSSH SHA-256 fingerprint, for example `SHA256:...`.
    #[serde(default)]
    pub(crate) expected_host_key_sha256: Option<String>,
    pub(crate) production_address: String,
    pub(crate) plan_max_age_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PbsBootstrap {
    pub(crate) vmid: u32,
    pub(crate) datastore: String,
    pub(crate) cache_path: String,
    pub(crate) s3_endpoint_id: String,
    pub(crate) bucket: String,
    pub(crate) region: String,
    pub(crate) lxc_template: Option<String>,
    pub(crate) storage_attached_to_pve: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreMount {
    pub(crate) vmid: u32,
    pub(crate) index: u16,
    pub(crate) source: String,
    pub(crate) target: String,
}
