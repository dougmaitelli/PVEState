use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServicesConfig {
    pub(crate) services: BTreeMap<u32, Service>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Service {
    pub(crate) guest: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) os: String,
    pub(crate) primary_services: Option<Vec<String>>,
    pub(crate) compose_roots: Option<Vec<String>>,
    pub(crate) containers: Option<Vec<String>>,
    pub(crate) named_volumes: Option<u32>,
    pub(crate) anonymous_volumes: Option<u32>,
    pub(crate) persistence: Persistence,
    pub(crate) qemu_guest_agent: Option<bool>,
    pub(crate) risks: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum Persistence {
    One(String),
    Many(Vec<String>),
}
