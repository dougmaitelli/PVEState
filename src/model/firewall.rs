use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallConfig {
    pub schema_version: u16,
    pub cluster: FirewallPolicy,
    pub nodes: BTreeMap<String, NodeFirewall>,
    pub guests: BTreeMap<u32, FirewallPolicy>,
    pub absent_guest_files: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeFirewall {
    pub present: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub log_level_in: Option<String>,
    pub rules: Vec<FirewallRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallPolicy {
    pub enabled: bool,
    pub log_level_in: Option<String>,
    pub rules: Vec<FirewallRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallRule {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub direction: String,
    pub action: String,
    pub interface: Option<String>,
    pub protocol: Option<String>,
    pub destination_port: Option<String>,
    pub log: String,
    pub comment: Option<String>,
}

fn yes() -> bool {
    true
}
