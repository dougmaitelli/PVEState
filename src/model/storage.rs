use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    pub host_mounts: Vec<HostMount>,
    pub pools: Vec<Pool>,
    pub storages: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostMount {
    pub path: String,
    pub required: bool,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pool {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub zpool: String,
    pub mountpoint: String,
    pub content: Vec<String>,
    pub sparse: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub path: Option<String>,
    pub vgname: Option<String>,
    pub thinpool: Option<String>,
    pub pool: Option<String>,
    pub mountpoint: Option<String>,
    pub sparse: Option<bool>,
    pub server: Option<String>,
    pub datastore: Option<String>,
    pub username: Option<String>,
    pub content: Vec<String>,
}
