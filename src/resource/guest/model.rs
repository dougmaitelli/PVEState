use crate::model::{DiskInterface, FirewallPolicy, UsbSlot};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(transparent)]
pub struct MacAddress(String);

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl FromStr for MacAddress {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid = value.split(':').count() == 6
            && value
                .split(':')
                .all(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if valid {
            Ok(Self(value.to_ascii_uppercase()))
        } else {
            anyhow::bail!("invalid MAC address `{value}`")
        }
    }
}
impl<'de> Deserialize<'de> for MacAddress {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

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
    #[serde(default)]
    pub networks: Vec<Nic>,
    pub start: Start,
    #[serde(default)]
    pub bind_mounts: Vec<BindMount>,
    #[serde(default)]
    pub firewall: Option<FirewallPolicy>,
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
    #[serde(default)]
    pub firewall: Option<FirewallPolicy>,
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
    #[schemars(with = "String")]
    pub interface: DiskInterface,
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
    pub mac: MacAddress,
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
    pub mac: MacAddress,
    pub bridge: String,
    #[serde(default)]
    pub firewall: bool,
    pub vlan: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Usb {
    #[schemars(with = "String")]
    pub slot: UsbSlot,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_addresses_are_validated_and_normalized() {
        let mac: MacAddress = serde_yaml::from_str("02:aa:00:bb:01:cc").unwrap();
        assert_eq!(mac.to_string(), "02:AA:00:BB:01:CC");
        assert!(serde_yaml::from_str::<MacAddress>("not-a-mac").is_err());
    }
}
