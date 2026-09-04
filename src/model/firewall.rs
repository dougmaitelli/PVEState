use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallPolicy {
    pub enabled: bool,
    #[serde(default)]
    pub log_level_in: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<FirewallAlias>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ip_sets: Vec<FirewallIpSet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub security_groups: Vec<FirewallSecurityGroup>,
    #[serde(default)]
    pub rules: Vec<FirewallRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallAlias {
    pub name: String,
    pub network: String,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallIpSet {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<FirewallIpSetEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallIpSetEntry {
    pub network: String,
    #[serde(default)]
    pub nomatch: bool,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallSecurityGroup {
    pub name: String,
    #[serde(default)]
    pub rules: Vec<FirewallRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallRule {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub direction: String,
    pub action: String,
    #[serde(default)]
    pub macro_name: Option<String>,
    #[serde(default)]
    pub interface: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub source_port: Option<String>,
    #[serde(default)]
    pub destination_port: Option<String>,
    #[serde(default = "no_log")]
    pub log: String,
    #[serde(default)]
    pub comment: Option<String>,
}

fn yes() -> bool {
    true
}

fn no_log() -> String {
    "nolog".into()
}
use std::collections::BTreeMap;
