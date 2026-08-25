use crate::model::*;
use anyhow::{Context, Result};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Guests {
    pub node: String,
    #[serde(default)]
    pub lxcs: BTreeMap<u32, Lxc>,
    #[serde(default)]
    pub vms: BTreeMap<u32, Vm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lxc {
    pub hostname: String,
    pub os: String,
    pub unprivileged: bool,
    pub cores: u16,
    pub memory_mb: u32,
    pub swap_mb: u32,
    pub rootfs: Disk,
    pub network: Nic,
    #[serde(default)]
    pub additional_networks: Vec<Nic>,
    pub start: Start,
    #[serde(default)]
    pub bind_mounts: Vec<BindMount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Vm {
    pub name: String,
    pub machine: String,
    pub bios: String,
    pub cpu: Cpu,
    pub memory_mb: u32,
    pub disk: VmDisk,
    pub efi: Efi,
    pub networks: Vec<VmNic>,
    #[serde(default)]
    pub usb_passthrough: Vec<Usb>,
    pub qemu_guest_agent: bool,
    pub start: Start,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cpu {
    pub r#type: String,
    pub sockets: u16,
    pub cores: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Disk {
    pub storage: String,
    pub size_gb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VmDisk {
    pub storage: String,
    pub interface: String,
    pub size_gb: u64,
    #[serde(default)]
    pub discard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Efi {
    pub storage: String,
    pub pre_enrolled_keys: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Nic {
    pub name: String,
    pub mac: String,
    pub bridge: String,
    #[serde(default)]
    pub firewall: bool,
    pub ipv4: String,
    pub gateway4: Option<String>,
    pub ipv6: Option<String>,
    pub gateway6: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VmNic {
    pub model: String,
    pub mac: String,
    pub bridge: String,
    #[serde(default)]
    pub firewall: bool,
    pub vlan: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Usb {
    pub slot: String,
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub onboot: bool,
    pub order: u16,
    pub delay_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BindMount {
    pub source: String,
    pub target: String,
    pub backed_up_by_pve: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub host: String,
    pub management_address: String,
    pub management_gateway: String,
    pub dns: Dns,
    pub interfaces: Vec<Interface>,
    pub bridges: Vec<Bridge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Dns {
    pub search: String,
    #[serde(default)]
    pub servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub name: String,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bridge {
    pub name: String,
    pub method: String,
    pub address: Option<String>,
    pub gateway: Option<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    pub stp: bool,
    pub forward_delay: u16,
}

pub struct Repository {
    pub root: PathBuf,
    pub guests: Guests,
    pub network: Network,
    pub firewall: FirewallConfig,
    pub recovery_checks: RecoveryChecks,
    pub manifest: RepositoryManifest,
    pub site: SiteConfig,
    pub host: HostConfig,
    pub storage: StorageConfig,
    pub backup: BackupConfig,
    pub restore: RestoreConfig,
    pub services: ServicesConfig,
    pub required_secrets: RequiredSecretsConfig,
}

impl Repository {
    pub fn open(root: &Path) -> Result<Self> {
        dotenvy::from_path(root.join(".env")).ok();
        Ok(Self {
            root: root.to_path_buf(),
            guests: read(root, "guests.yml")?,
            network: read(root, "network.yml")?,
            firewall: read(root, "firewall.yml")?,
            recovery_checks: read(root, "recovery-checks.yml")?,
            manifest: serde_yaml::from_str(&fs::read_to_string(root.join("iac.yml"))?)?,
            site: read(root, "site.yml")?,
            host: read(root, "host.yml")?,
            storage: read(root, "storage.yml")?,
            backup: read(root, "backup.yml")?,
            restore: read(root, "restore.yml")?,
            services: read(root, "services.yml")?,
            required_secrets: read(root, "required-secrets.yml")?,
        })
    }
    pub fn runtime(&self) -> PathBuf {
        self.root.join(".runtime")
    }
    pub fn observed(&self) -> PathBuf {
        self.root.join("observed/production")
    }
    pub fn initialize(path: &Path) -> Result<()> {
        fs::create_dir_all(path.join("config"))?;
        fs::create_dir_all(path.join("observed/production"))?;
        fs::write(path.join("iac.yml"), "schema_version: 1\n")?;
        fs::write(path.join(".gitignore"), ".env\n.secrets/\n.runtime/\n")?;
        for (name, content) in [
            (
                "guests.yml",
                include_str!("../examples/basic/config/guests.yml"),
            ),
            (
                "network.yml",
                include_str!("../examples/basic/config/network.yml"),
            ),
            (
                "firewall.yml",
                include_str!("../examples/basic/config/firewall.yml"),
            ),
            (
                "recovery-checks.yml",
                include_str!("../examples/basic/config/recovery-checks.yml"),
            ),
            (
                "restore.yml",
                include_str!("../examples/basic/config/restore.yml"),
            ),
            (
                "site.yml",
                include_str!("../examples/basic/config/site.yml"),
            ),
            (
                "host.yml",
                include_str!("../examples/basic/config/host.yml"),
            ),
            (
                "storage.yml",
                include_str!("../examples/basic/config/storage.yml"),
            ),
            (
                "backup.yml",
                include_str!("../examples/basic/config/backup.yml"),
            ),
            (
                "services.yml",
                include_str!("../examples/basic/config/services.yml"),
            ),
            (
                "required-secrets.yml",
                include_str!("../examples/basic/config/required-secrets.yml"),
            ),
        ] {
            fs::write(path.join("config").join(name), content)?;
        }
        fs::write(
            path.join(".env.example"),
            include_str!("../examples/basic/.env.example"),
        )?;
        println!("initialized configuration repository: {}", path.display());
        Ok(())
    }
    pub fn write_schema(output: Option<&Path>) -> Result<()> {
        let dir = output.unwrap_or_else(|| Path::new("schemas"));
        fs::create_dir_all(dir)?;
        macro_rules! write {
            ($name:literal,$ty:ty) => {
                fs::write(
                    dir.join($name),
                    serde_json::to_string_pretty(&schema_for!($ty))? + "\n",
                )?
            };
        }
        write!("repository.schema.json", RepositoryDocument);
        write!("site.schema.json", SiteConfig);
        write!("host.schema.json", HostConfig);
        write!("guests.schema.json", Guests);
        write!("network.schema.json", Network);
        write!("firewall.schema.json", FirewallConfig);
        write!("storage.schema.json", StorageConfig);
        write!("backup.schema.json", BackupConfig);
        write!("restore.schema.json", RestoreConfig);
        write!("recovery-checks.schema.json", RecoveryChecks);
        write!("services.schema.json", ServicesConfig);
        write!("required-secrets.schema.json", RequiredSecretsConfig);
        println!("wrote configuration schemas to {}", dir.display());
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoveryChecks {
    pub checks: Vec<RecoveryCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCheck {
    pub id: String,
    pub description: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryDocument {
    pub iac: RepositoryManifest,
    pub site: SiteConfig,
    pub host: HostConfig,
    pub guests: Guests,
    pub network: Network,
    pub firewall: FirewallConfig,
    pub storage: StorageConfig,
    pub backup: BackupConfig,
    pub restore: RestoreConfig,
    pub recovery_checks: RecoveryChecks,
    pub services: ServicesConfig,
    pub required_secrets: RequiredSecretsConfig,
}

fn read<T: for<'a> Deserialize<'a>>(root: &Path, name: &str) -> Result<T> {
    let p = root.join("config").join(name);
    serde_yaml::from_str(&fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?)
        .with_context(|| format!("parse {}", p.display()))
}

pub(crate) fn env(name: &str, default: Option<&str>) -> Result<String> {
    std::env::var(name)
        .or_else(|_| {
            default
                .map(str::to_owned)
                .ok_or(std::env::VarError::NotPresent)
        })
        .with_context(|| format!("missing {name}"))
}
