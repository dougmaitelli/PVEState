use super::FirewallPolicy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    pub node: NodeIdentity,
    pub storage_topology: BTreeMap<String, StorageTopology>,
    #[serde(default)]
    pub firewall: Option<FirewallPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeIdentity {
    pub name: String,
    pub standalone: bool,
    pub pve_version: String,
    pub kernel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageTopology {
    pub layout: Option<String>,
    pub redundancy: String,
    pub device_class: String,
    pub provides: Option<Vec<String>>,
    pub devices: Option<u16>,
    pub mount: Option<String>,
    pub filesystem: Option<String>,
    pub size_tb: Option<u64>,
    pub consumers: Option<Vec<String>>,
}
