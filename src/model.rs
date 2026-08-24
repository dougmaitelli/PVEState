use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryManifest {
    pub schema_version: u16,
    pub tool: Option<ToolRequirement>,
    pub environment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolRequirement {
    pub minimum_version: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    pub host: HostIdentity,
    pub storage_topology: BTreeMap<String, StorageTopology>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostIdentity {
    pub name: String,
    pub standalone: bool,
    pub pve_version: String,
    pub kernel: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageTopology {
    pub layout: Option<String>,
    pub redundancy: String,
    pub device_class: String,
    pub provides: Option<Vec<String>>,
    pub devices: Option<u16>,
    pub mount: Option<String>,
    pub filesystem: Option<String>,
    pub size_tb: Option<u64>,
    pub consumers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallConfig {
    pub schema_version: u16,
    pub cluster: FirewallPolicy,
    pub nodes: BTreeMap<String, NodeFirewall>,
    pub guests: BTreeMap<u32, FirewallPolicy>,
    pub absent_guest_files: Vec<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeFirewall {
    pub present: bool,
    pub rules: Vec<FirewallRule>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallPolicy {
    pub enabled: bool,
    pub log_level_in: Option<String>,
    pub rules: Vec<FirewallRule>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallRule {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub direction: String,
    pub action: String,
    pub interface: Option<String>,
    pub protocol: Option<String>,
    pub destination_port: Option<String>,
    pub log: String,
    pub comment: Option<String>,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    pub host_mounts: Vec<HostMount>,
    pub pools: Vec<Pool>,
    pub storages: Vec<Storage>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostMount {
    pub path: String,
    pub required: bool,
    pub purpose: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pool {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub zpool: String,
    pub mountpoint: String,
    pub content: Vec<String>,
    pub sparse: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub path: Option<String>,
    pub vgname: Option<String>,
    pub thinpool: Option<String>,
    pub pool: Option<String>,
    pub mountpoint: Option<String>,
    pub sparse: Option<bool>,
    pub server: Option<String>,
    pub datastore: Option<String>,
    pub username: Option<String>,
    pub content: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupConfig {
    pub pbs: PbsBackup,
    pub pve_backup_job: PveBackupJob,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PbsBackup {
    pub endpoint: String,
    pub version: String,
    pub guest: BackupGuest,
    pub datastore: Datastore,
    pub s3_endpoint: S3Endpoint,
    pub jobs: BackupJobs,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupGuest {
    #[serde(rename = "type")]
    pub kind: String,
    pub vmid: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Datastore {
    pub name: String,
    pub backend: String,
    pub local_cache_path: String,
    pub bucket: String,
    pub s3_endpoint_id: String,
    pub garbage_collection_schedule: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct S3Endpoint {
    pub id: String,
    pub endpoint_template: String,
    pub region: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupJobs {
    pub prune: Schedule,
    pub verify: VerifySchedule,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub schedule: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifySchedule {
    pub schedule: String,
    pub ignore_verified: bool,
    pub outdated_after_days: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PveBackupJob {
    pub datastore: String,
    pub schedule: String,
    pub mode: String,
    pub guest_ids: Vec<u32>,
    pub retention: Retention,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Retention {
    pub keep_last: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreConfig {
    pub schema_version: u16,
    pub target: RestoreTarget,
    pub pbs_bootstrap: PbsBootstrap,
    pub archives: BTreeMap<u32, Option<String>>,
    pub restore_order: Vec<u32>,
    pub protected_vmids: Vec<u32>,
    pub reattach_mounts: Vec<RestoreMount>,
    pub application: RestoreApplication,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreTarget {
    pub expected_hostname: String,
    pub production_address: String,
    pub plan_max_age_minutes: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PbsBootstrap {
    pub vmid: u32,
    pub datastore: String,
    pub cache_path: String,
    pub s3_endpoint_id: String,
    pub bucket: String,
    pub region: String,
    pub lxc_template: Option<String>,
    pub storage_attached_to_pve: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreMount {
    pub vmid: u32,
    pub index: u16,
    pub source: String,
    pub target: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreApplication {
    pub repository: String,
    pub docker_guest_vmid: u32,
    pub configure_playbook: Option<String>,
    pub configure_command: Option<String>,
}

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
