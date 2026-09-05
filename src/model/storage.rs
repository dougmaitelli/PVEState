use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageConfig {
    pub(crate) host_mounts: Vec<HostMount>,
    pub(crate) pools: Vec<Pool>,
    pub(crate) storages: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostMount {
    pub(crate) path: String,
    pub(crate) required: bool,
    pub(crate) purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pool {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) zpool: String,
    pub(crate) mountpoint: String,
    pub(crate) content: Vec<String>,
    pub(crate) sparse: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Storage {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) path: Option<String>,
    pub(crate) vgname: Option<String>,
    pub(crate) thinpool: Option<String>,
    pub(crate) pool: Option<String>,
    pub(crate) mountpoint: Option<String>,
    pub(crate) sparse: Option<bool>,
    pub(crate) server: Option<String>,
    pub(crate) datastore: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) content: Vec<String>,
}
