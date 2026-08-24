use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequiredSecretsConfig {
    pub required_for_disaster_recovery: SecretGroups,
    pub never_commit: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretGroups {
    pub pbs_s3: Vec<String>,
    pub pbs_identity: Vec<String>,
    pub pve_to_pbs: Vec<String>,
    pub application: Vec<String>,
}
