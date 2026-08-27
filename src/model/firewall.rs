use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
