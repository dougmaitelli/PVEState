use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryManifest {
    pub(crate) schema_version: u16,
    pub(crate) tool: Option<ToolRequirement>,
    pub(crate) environment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolRequirement {
    pub(crate) minimum_version: String,
}
