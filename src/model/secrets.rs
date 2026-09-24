use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RequiredSecretsConfig {
    pub(crate) required_for_disaster_recovery: SecretGroups,
    pub(crate) never_commit: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SecretGroups {
    pub(crate) pbs_s3: Vec<String>,
    pub(crate) pbs_identity: Vec<String>,
    pub(crate) pve_to_pbs: Vec<String>,
}
