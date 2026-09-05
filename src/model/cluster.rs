use super::FirewallPolicy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterConfig {
    pub(crate) cluster: ClusterIdentity,
    pub(crate) proxmox: ProxmoxCluster,
    pub(crate) workload_profile: WorkloadProfile,
    pub(crate) backup: ClusterBackup,
    #[serde(default)]
    pub(crate) firewall: Option<FirewallPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterIdentity {
    pub(crate) name: String,
    pub(crate) environment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProxmoxCluster {
    pub(crate) endpoint: String,
    pub(crate) existing_environment: bool,
    pub(crate) change_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkloadProfile {
    pub(crate) virtual_machines: Count,
    pub(crate) containers: Count,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum Count {
    Number(u32),
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterBackup {
    pub(crate) pbs: ClusterPbs,
    pub(crate) s3: ClusterS3,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterPbs {
    pub(crate) deployment: String,
    pub(crate) endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterS3 {
    pub(crate) configured_in_pbs: bool,
    pub(crate) endpoint: String,
}
