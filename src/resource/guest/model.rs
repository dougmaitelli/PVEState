use crate::model::{DiskInterface, EfiSlot, FirewallPolicy, NetworkSlot, UsbSlot};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) options: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub(crate) firewall: Option<FirewallPolicy>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Vm {
    pub(crate) name: String,
    pub(crate) machine: String,
    pub(crate) bios: String,
    pub(crate) cpu: Cpu,
    pub(crate) memory_mb: u32,
    #[schemars(with = "BTreeMap<String, VmDisk>")]
    pub(crate) disks: BTreeMap<DiskInterface, VmDisk>,
    #[schemars(with = "BTreeMap<String, Efi>")]
    pub(crate) efi_disks: BTreeMap<EfiSlot, Efi>,
    #[schemars(with = "BTreeMap<String, VmNic>")]
    pub(crate) networks: BTreeMap<NetworkSlot, VmNic>,
    #[schemars(with = "BTreeMap<String, Usb>")]
    pub(crate) usb_devices: BTreeMap<UsbSlot, Usb>,
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
    pub(crate) host: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VmWire {
    name: String,
    machine: String,
    bios: String,
    cpu: Cpu,
    memory_mb: u32,
    disk: Option<LegacyVmDisk>,
    #[serde(default)]
    disks: BTreeMap<DiskInterface, VmDisk>,
    efi: Option<Efi>,
    #[serde(default)]
    efi_disks: BTreeMap<EfiSlot, Efi>,
    #[serde(default)]
    networks: NetworkCollection,
    #[serde(default)]
    usb_passthrough: Vec<LegacyUsb>,
    #[serde(default)]
    usb_devices: BTreeMap<UsbSlot, Usb>,
    qemu_guest_agent: bool,
    start: Start,
    #[serde(default)]
    firewall: Option<FirewallPolicy>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyVmDisk {
    storage: String,
    interface: DiskInterface,
    size_gb: u64,
    #[serde(default)]
    discard: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyUsb {
    slot: UsbSlot,
    host: String,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum NetworkCollection {
    #[default]
    Empty,
    Legacy(Vec<VmNic>),
    Slots(BTreeMap<NetworkSlot, VmNic>),
}

impl<'de> Deserialize<'de> for Vm {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = VmWire::deserialize(deserializer)?;
        let mut disks = wire.disks;
        if let Some(disk) = wire.disk
            && disks
                .insert(
                    disk.interface,
                    VmDisk {
                        storage: disk.storage,
                        size_gb: disk.size_gb,
                        discard: disk.discard,
                    },
                )
                .is_some()
        {
            return Err(serde::de::Error::custom("duplicate legacy VM disk slot"));
        }
        if disks.is_empty() {
            return Err(serde::de::Error::custom("VM requires at least one disk"));
        }
        let mut efi_disks = wire.efi_disks;
        if let Some(efi) = wire.efi
            && efi_disks.insert(EfiSlot(0), efi).is_some()
        {
            return Err(serde::de::Error::custom("duplicate legacy EFI disk slot"));
        }
        let networks = match wire.networks {
            NetworkCollection::Empty => BTreeMap::new(),
            NetworkCollection::Legacy(values) => values
                .into_iter()
                .enumerate()
                .map(|(index, value)| (NetworkSlot(index as u8), value))
                .collect(),
            NetworkCollection::Slots(values) => values,
        };
        let mut usb_devices = wire.usb_devices;
        for usb in wire.usb_passthrough {
            if usb_devices
                .insert(usb.slot, Usb { host: usb.host })
                .is_some()
            {
                return Err(serde::de::Error::custom("duplicate legacy USB slot"));
            }
        }
        Ok(Self {
            name: wire.name,
            machine: wire.machine,
            bios: wire.bios,
            cpu: wire.cpu,
            memory_mb: wire.memory_mb,
            disks,
            efi_disks,
            networks,
            usb_devices,
            qemu_guest_agent: wire.qemu_guest_agent,
            start: wire.start,
            firewall: wire.firewall,
        })
    }
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

    #[test]
    fn legacy_vm_devices_are_normalized_into_slot_maps() {
        let yaml = r#"
name: legacy
machine: q35
bios: ovmf
cpu: {type: host, sockets: 1, cores: 2}
memory_mb: 2048
disk: {storage: local-lvm, interface: scsi0, size_gb: 32, discard: true}
efi: {storage: local-lvm, pre_enrolled_keys: true}
networks:
  - {model: virtio, mac: "02:00:00:00:02:01", bridge: vmbr0, firewall: true, vlan: null}
usb_passthrough:
  - {slot: usb0, host: "1a86:7523"}
qemu_guest_agent: true
start: {onboot: true, order: 30, delay_seconds: null}
"#;
        let vm: Vm = serde_yaml::from_str(yaml).unwrap();

        assert!(vm.disks.contains_key(&DiskInterface::Scsi(0)));
        assert!(vm.efi_disks.contains_key(&EfiSlot(0)));
        assert!(vm.networks.contains_key(&NetworkSlot(0)));
        assert!(vm.usb_devices.contains_key(&UsbSlot(0)));

        let canonical = serde_yaml::to_string(&vm).unwrap();
        assert!(canonical.contains("disks:"));
        assert!(canonical.contains("efi_disks:"));
        assert!(canonical.contains("usb_devices:"));
        assert!(!canonical.contains("usb_passthrough:"));
    }

    #[test]
    fn rejects_conflicting_legacy_and_slot_keyed_disks() {
        let yaml = r#"
name: conflict
machine: q35
bios: ovmf
cpu: {type: host, sockets: 1, cores: 2}
memory_mb: 2048
disk: {storage: old, interface: scsi0, size_gb: 8}
disks:
  scsi0: {storage: new, size_gb: 16}
qemu_guest_agent: false
start: {onboot: false, order: 1, delay_seconds: null}
"#;
        assert!(serde_yaml::from_str::<Vm>(yaml).is_err());
    }
}
