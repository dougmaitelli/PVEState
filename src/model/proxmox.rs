use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GuestKind {
    Lxc,
    Qemu,
}

impl GuestKind {
    pub(crate) const fn api_name(self) -> &'static str {
        match self {
            Self::Lxc => "lxc",
            Self::Qemu => "qemu",
        }
    }

    pub(crate) const fn collection_name(self) -> &'static str {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GuestRef {
    pub(crate) kind: GuestKind,
    pub(crate) vmid: u32,
}

impl GuestRef {
    pub(crate) const fn new(kind: GuestKind, vmid: u32) -> Self {
        Self { kind, vmid }
    }
    pub(crate) fn config_endpoint(self, node: &str) -> String {
        format!("/nodes/{node}/{}/{}/config", self.kind, self.vmid)
    }
    pub(crate) fn resize_endpoint(self, node: &str) -> String {
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
pub(crate) enum GuestField {
    Hostname,
    OsType,
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
    BindMount(u8),
    Network(u8),
}

impl GuestField {
    pub(crate) fn api_name(self) -> String {
        match self {
            Self::Hostname => "hostname".into(),
            Self::OsType => "ostype".into(),
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
            Self::BindMount(index) => format!("mp{index}"),
            Self::Network(index) => format!("net{index}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum DiskInterface {
    Scsi(u8),
    Sata(u8),
    Virtio(u8),
    Ide(u8),
}

macro_rules! numbered_slot {
    ($name:ident, $prefix:literal, $description:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) struct $name(pub u8);

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}{}", $prefix, self.0)
            }
        }

        impl FromStr for $name {
            type Err = anyhow::Error;
            fn from_str(value: &str) -> Result<Self> {
                Ok(Self(
                    value
                        .strip_prefix($prefix)
                        .context(concat!($description, " must be ", $prefix, "N"))?
                        .parse()?,
                ))
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> std::result::Result<Self, D::Error> {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

numbered_slot!(EfiSlot, "efidisk", "EFI slot");
numbered_slot!(NetworkSlot, "net", "network slot");

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
pub(crate) struct UsbSlot(pub u8);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_references_generate_api_paths() {
        let guest: GuestRef = "qemu/201".parse().unwrap();
        assert_eq!(guest.config_endpoint("pve"), "/nodes/pve/qemu/201/config");
    }
}
