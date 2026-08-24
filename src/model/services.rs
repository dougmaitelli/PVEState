use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServicesConfig {
    pub services: BTreeMap<u32, Service>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub guest: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub os: String,
    pub primary_services: Option<Vec<String>>,
    pub compose_roots: Option<Vec<String>>,
    pub containers: Option<Vec<String>>,
    pub named_volumes: Option<u32>,
    pub anonymous_volumes: Option<u32>,
    pub persistence: Persistence,
    pub qemu_guest_agent: Option<bool>,
    pub risks: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Persistence {
    One(String),
    Many(Vec<String>),
}
