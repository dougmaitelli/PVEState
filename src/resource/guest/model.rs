use crate::model::{DiskInterface, FirewallPolicy, UsbSlot};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(transparent)]
pub(crate) struct MacAddress(String);

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
pub(crate) struct Guests {
    pub(crate) node: String,
    #[serde(default)]
    pub(crate) lxcs: BTreeMap<u32, Lxc>,
    #[serde(default)]
    pub(crate) vms: BTreeMap<u32, Vm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Lxc {
    pub(crate) hostname: String,
    pub(crate) os: String,
    pub(crate) unprivileged: bool,
    pub(crate) cores: u16,
    pub(crate) memory_mb: u32,
    pub(crate) swap_mb: u32,
    pub(crate) rootfs: Disk,
    #[serde(default)]
    pub(crate) networks: Vec<Nic>,
    pub(crate) start: Start,
    #[serde(default)]
    pub(crate) bind_mounts: Vec<BindMount>,
    #[serde(default)]
    pub(crate) firewall: Option<FirewallPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Vm {
    pub(crate) name: String,
    pub(crate) machine: String,
    pub(crate) bios: String,
    pub(crate) cpu: Cpu,
    pub(crate) memory_mb: u32,
    pub(crate) disk: VmDisk,
    pub(crate) efi: Efi,
    pub(crate) networks: Vec<VmNic>,
    #[serde(default)]
    pub(crate) usb_passthrough: Vec<Usb>,
    pub(crate) qemu_guest_agent: bool,
    pub(crate) start: Start,
    #[serde(default)]
    pub(crate) firewall: Option<FirewallPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cpu {
    pub(crate) r#type: String,
    pub(crate) sockets: u16,
    pub(crate) cores: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Disk {
    pub(crate) storage: String,
    pub(crate) size_gb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct VmDisk {
    pub(crate) storage: String,
    #[schemars(with = "String")]
    pub(crate) interface: DiskInterface,
    pub(crate) size_gb: u64,
    #[serde(default)]
    pub(crate) discard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Efi {
    pub(crate) storage: String,
    pub(crate) pre_enrolled_keys: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Nic {
    pub(crate) name: String,
    pub(crate) mac: MacAddress,
    pub(crate) bridge: String,
    #[serde(default)]
    pub(crate) firewall: bool,
    pub(crate) ipv4: String,
    pub(crate) gateway4: Option<String>,
    pub(crate) ipv6: Option<String>,
    pub(crate) gateway6: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct VmNic {
    pub(crate) model: String,
    pub(crate) mac: MacAddress,
    pub(crate) bridge: String,
    #[serde(default)]
    pub(crate) firewall: bool,
    pub(crate) vlan: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Usb {
    #[schemars(with = "String")]
    pub(crate) slot: UsbSlot,
    pub(crate) host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    pub(crate) onboot: bool,
    pub(crate) order: u16,
    pub(crate) delay_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindMount {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) backed_up_by_pve: bool,
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
