use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryChecks {
    pub(crate) checks: Vec<RecoveryCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryCheck {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) command: String,
}
