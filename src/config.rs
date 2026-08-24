use anyhow::{Context, Result};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Guests {
    pub node: String,
    #[serde(default)]
    pub lxcs: BTreeMap<u32, Lxc>,
    #[serde(default)]
    pub vms: BTreeMap<u32, Vm>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Lxc {
    pub hostname: String,
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
pub struct Vm {
    pub name: String,
    pub machine: String,
    pub bios: String,
    pub cpu: Cpu,
    pub memory_mb: u32,
    pub disk: VmDisk,
    pub networks: Vec<VmNic>,
    #[serde(default)]
    pub usb_passthrough: Vec<Usb>,
    pub qemu_guest_agent: bool,
    pub start: Start,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Cpu {
    pub r#type: String,
    pub sockets: u16,
    pub cores: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Disk {
    pub storage: String,
    pub size_gb: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VmDisk {
    pub storage: String,
    pub interface: String,
    pub size_gb: u64,
    #[serde(default)]
    pub discard: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
pub struct VmNic {
    pub model: String,
    pub mac: String,
    pub bridge: String,
    #[serde(default)]
    pub firewall: bool,
    pub vlan: Option<u16>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Usb {
    pub slot: String,
    pub host: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Start {
    pub onboot: bool,
    pub order: u16,
    pub delay_seconds: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BindMount {
    pub source: String,
    pub target: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Network {
    pub host: String,
    pub management_address: String,
    pub management_gateway: String,
    pub dns: Dns,
    pub interfaces: Vec<Interface>,
    pub bridges: Vec<Bridge>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Dns {
    pub search: String,
    #[serde(default)]
    pub servers: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Interface {
    pub name: String,
    pub method: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    pub firewall: serde_yaml::Value,
    pub recovery_checks: RecoveryChecks,
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
        let text = serde_json::to_string_pretty(&schema_for!(Guests))?;
        if let Some(p) = output {
            fs::write(p, text + "\n")?
        } else {
            println!("{text}")
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecoveryChecks {
    pub checks: Vec<RecoveryCheck>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecoveryCheck {
    pub id: String,
    pub description: String,
    pub command: String,
}
fn read<T: for<'a> Deserialize<'a>>(root: &Path, name: &str) -> Result<T> {
    let p = root.join("config").join(name);
    serde_yaml::from_str(&fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?)
        .with_context(|| format!("parse {}", p.display()))
}
