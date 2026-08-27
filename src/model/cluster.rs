use super::FirewallPolicy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClusterConfig {
    pub cluster: ClusterIdentity,
    pub proxmox: ProxmoxCluster,
    pub workload_profile: WorkloadProfile,
    pub backup: ClusterBackup,
    #[serde(default)]
    pub firewall: Option<FirewallPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClusterIdentity {
    pub name: String,
    pub environment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxmoxCluster {
    pub endpoint: String,
    pub existing_environment: bool,
    pub change_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkloadProfile {
    pub virtual_machines: Count,
    pub containers: Count,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Count {
    Number(u32),
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClusterBackup {
    pub pbs: ClusterPbs,
    pub s3: ClusterS3,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClusterPbs {
    pub deployment: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClusterS3 {
    pub configured_in_pbs: bool,
    pub endpoint: String,
}
