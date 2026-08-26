use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuestKind {
    Lxc,
    Qemu,
}

impl GuestKind {
    pub const fn api_name(self) -> &'static str {
        match self {
            Self::Lxc => "lxc",
            Self::Qemu => "qemu",
        }
    }

    pub const fn collection_name(self) -> &'static str {
        match self {
            Self::Lxc => "lxcs",
            Self::Qemu => "vms",
        }
    }
}

impl fmt::Display for GuestKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.api_name())
    }
}

impl FromStr for GuestKind {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "lxc" => Ok(Self::Lxc),
            "qemu" => Ok(Self::Qemu),
            _ => bail!("unknown guest kind {value}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestRef {
    pub kind: GuestKind,
    pub vmid: u32,
}

impl GuestRef {
    pub const fn new(kind: GuestKind, vmid: u32) -> Self {
        Self { kind, vmid }
    }
    pub fn config_endpoint(self, node: &str) -> String {
        format!("/nodes/{node}/{}/{}/config", self.kind, self.vmid)
    }
    pub fn resize_endpoint(self, node: &str) -> String {
        format!("/nodes/{node}/{}/{}/resize", self.kind, self.vmid)
    }
}

impl fmt::Display for GuestRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.kind, self.vmid)
    }
}

impl FromStr for GuestRef {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        let (kind, vmid) = value
            .split_once('/')
            .context("guest reference must be KIND/VMID")?;
        Ok(Self::new(kind.parse()?, vmid.parse()?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestField {
    Hostname,
    OsType,
    Unprivileged,
    Name,
    Machine,
    Bios,
    Cores,
    Sockets,
    Memory,
    Swap,
    OnBoot,
    Cpu,
    Agent,
    Startup,
    RootFs,
    EfiDisk,
    DiskSize,
    BindMountBackup(u8),
    BindMount(u8),
    Network(u8),
    Usb(u8),
    Delete,
}

impl GuestField {
    pub fn api_name(self) -> String {
        match self {
            Self::Hostname => "hostname".into(),
            Self::OsType => "ostype".into(),
            Self::Unprivileged => "unprivileged".into(),
            Self::Name => "name".into(),
            Self::Machine => "machine".into(),
            Self::Bios => "bios".into(),
            Self::Cores => "cores".into(),
            Self::Sockets => "sockets".into(),
            Self::Memory => "memory".into(),
            Self::Swap => "swap".into(),
            Self::OnBoot => "onboot".into(),
            Self::Cpu => "cpu".into(),
            Self::Agent => "agent".into(),
            Self::Startup => "startup".into(),
            Self::RootFs => "rootfs".into(),
            Self::EfiDisk => "efidisk0".into(),
            Self::DiskSize => "size_gb".into(),
            Self::BindMountBackup(index) => format!("mp{index}.backed_up_by_pve"),
            Self::BindMount(index) => format!("mp{index}"),
            Self::Network(index) => format!("net{index}"),
            Self::Usb(index) => format!("usb{index}"),
            Self::Delete => "delete".into(),
        }
    }

    pub fn from_api(value: &str) -> Option<Self> {
        let scalar = match value {
            "hostname" => Self::Hostname,
            "ostype" => Self::OsType,
            "unprivileged" => Self::Unprivileged,
            "name" => Self::Name,
            "machine" => Self::Machine,
            "bios" => Self::Bios,
            "cores" => Self::Cores,
            "sockets" => Self::Sockets,
            "memory" => Self::Memory,
            "swap" => Self::Swap,
            "onboot" => Self::OnBoot,
            "cpu" => Self::Cpu,
            "agent" => Self::Agent,
            "startup" => Self::Startup,
            "rootfs" => Self::RootFs,
            "efidisk0" => Self::EfiDisk,
            "size_gb" => Self::DiskSize,
            "delete" => Self::Delete,
            _ => return numbered(value),
        };
        Some(scalar)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskInterface {
    Scsi(u8),
    Sata(u8),
    Virtio(u8),
    Ide(u8),
}

impl fmt::Display for DiskInterface {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (prefix, index) = match self {
            Self::Scsi(i) => ("scsi", i),
            Self::Sata(i) => ("sata", i),
            Self::Virtio(i) => ("virtio", i),
            Self::Ide(i) => ("ide", i),
        };
        write!(formatter, "{prefix}{index}")
    }
}

impl FromStr for DiskInterface {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        for (prefix, constructor) in [
            ("scsi", Self::Scsi as fn(u8) -> Self),
            ("sata", Self::Sata),
            ("virtio", Self::Virtio),
            ("ide", Self::Ide),
        ] {
            if let Some(index) = value
                .strip_prefix(prefix)
                .and_then(|value| value.parse().ok())
            {
                return Ok(constructor(index));
            }
        }
        bail!("invalid disk interface {value}")
    }
}

impl Serialize for DiskInterface {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DiskInterface {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UsbSlot(pub u8);

impl fmt::Display for UsbSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "usb{}", self.0)
    }
}

impl FromStr for UsbSlot {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        Ok(Self(
            value
                .strip_prefix("usb")
                .context("USB slot must be usbN")?
                .parse()?,
        ))
    }
}

impl Serialize for UsbSlot {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for UsbSlot {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

fn numbered(value: &str) -> Option<GuestField> {
    for (prefix, constructor) in [
        ("mp", GuestField::BindMount as fn(u8) -> GuestField),
        ("net", GuestField::Network),
        ("usb", GuestField::Usb),
    ] {
        if let Some(index) = value
            .strip_prefix(prefix)
            .and_then(|value| value.parse().ok())
        {
            return Some(constructor(index));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_references_generate_api_paths() {
        let guest: GuestRef = "qemu/201".parse().unwrap();
        assert_eq!(guest.config_endpoint("pve"), "/nodes/pve/qemu/201/config");
    }

    #[test]
    fn numbered_fields_are_typed() {
        assert_eq!(GuestField::from_api("mp3"), Some(GuestField::BindMount(3)));
        assert_eq!(
            GuestField::BindMountBackup(3).api_name(),
            "mp3.backed_up_by_pve"
        );
    }
}
