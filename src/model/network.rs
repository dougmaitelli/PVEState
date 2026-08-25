use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub host: String,
    pub management_address: String,
    pub management_gateway: String,
    pub dns: Dns,
    pub interfaces: Vec<Interface>,
    pub bridges: Vec<Bridge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Dns {
    pub search: String,
    #[serde(default)]
    pub servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub name: String,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bridge {
    pub name: String,
    pub method: String,
    pub address: Option<String>,
    pub gateway: Option<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    pub stp: bool,
    pub forward_delay: u16,
}
