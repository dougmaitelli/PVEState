use super::FirewallPolicy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeConfig {
    pub(crate) node: NodeIdentity,
    pub(crate) storage_topology: BTreeMap<String, StorageTopology>,
    #[serde(default)]
    pub(crate) firewall: Option<FirewallPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeIdentity {
    pub(crate) name: String,
    pub(crate) standalone: bool,
    pub(crate) pve_version: String,
    pub(crate) kernel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageTopology {
    pub(crate) layout: Option<String>,
    pub(crate) redundancy: String,
    pub(crate) device_class: String,
    pub(crate) provides: Option<Vec<String>>,
    pub(crate) devices: Option<u16>,
    pub(crate) mount: Option<String>,
    pub(crate) filesystem: Option<String>,
    pub(crate) size_tb: Option<u64>,
    pub(crate) consumers: Option<Vec<String>>,
}
