use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteConfig {
    pub site: SiteIdentity,
    pub proxmox: ProxmoxSite,
    pub workload_profile: WorkloadProfile,
    pub backup: SiteBackup,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteIdentity {
    pub name: String,
    pub environment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxmoxSite {
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
pub struct SiteBackup {
    pub pbs: SitePbs,
    pub s3: SiteS3,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SitePbs {
    pub deployment: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteS3 {
    pub configured_in_pbs: bool,
    pub endpoint: String,
}
