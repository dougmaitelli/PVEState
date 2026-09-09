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
    Unprivileged,
    RootFs,
    BindMount(u8),
    Network(u8),
}

impl GuestField {
    const NAMED: [Self; 15] = [
        Self::Hostname,
        Self::OsType,
        Self::Name,
        Self::Machine,
        Self::Bios,
        Self::Cores,
        Self::Sockets,
        Self::Memory,
        Self::Swap,
        Self::OnBoot,
        Self::Cpu,
        Self::Agent,
        Self::Startup,
        Self::Unprivileged,
        Self::RootFs,
    ];

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
            Self::Unprivileged => "unprivileged".into(),
            Self::RootFs => "rootfs".into(),
            Self::BindMount(index) => format!("mp{index}"),
            Self::Network(index) => format!("net{index}"),
        }
    }

    pub(crate) fn from_api_name(value: &str) -> Option<Self> {
        Self::NAMED
            .into_iter()
            .find(|field| field.api_name() == value)
            .or_else(|| parse_numbered_field(value, "mp").map(Self::BindMount))
            .or_else(|| parse_numbered_field(value, "net").map(Self::Network))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LxcConfigField {
    Managed(GuestField),
    Device(LxcDeviceField),
    Additional(LxcOptionName),
    ReadOnly(LxcMetadataField),
    Sensitive(LxcSensitiveField),
    Invalid,
}

impl LxcConfigField {
    pub(crate) fn classify(value: &str) -> Self {
        if let Some(field) = GuestField::from_api_name(value) {
            return match field {
                GuestField::BindMount(index) => Self::Device(LxcDeviceField::BindMount(index)),
                GuestField::Network(index) => Self::Device(LxcDeviceField::Network(index)),
                GuestField::Hostname
                | GuestField::OsType
                | GuestField::Cores
                | GuestField::Memory
                | GuestField::Swap
                | GuestField::OnBoot
                | GuestField::Startup
                | GuestField::Unprivileged
                | GuestField::RootFs => Self::Managed(field),
                GuestField::Name
                | GuestField::Machine
                | GuestField::Bios
                | GuestField::Sockets
                | GuestField::Cpu
                | GuestField::Agent => Self::Additional(LxcOptionName(value.into())),
            };
        }
        if let Some(field) = LxcMetadataField::from_api_name(value) {
            return Self::ReadOnly(field);
        }
        if let Some(field) = LxcSensitiveField::from_api_name(value) {
            return Self::Sensitive(field);
        }
        if valid_lxc_option_name(value) {
            return Self::Additional(LxcOptionName(value.into()));
        }
        Self::Invalid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LxcDeviceField {
    BindMount(u8),
    Network(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LxcMetadataField {
    Digest,
    RawConfig,
}

impl LxcMetadataField {
    pub(crate) const fn api_name(self) -> &'static str {
        match self {
            Self::Digest => "digest",
            Self::RawConfig => "lxc",
        }
    }

    fn from_api_name(value: &str) -> Option<Self> {
        [Self::Digest, Self::RawConfig]
            .into_iter()
            .find(|field| field.api_name() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LxcSensitiveField {
    Password,
}

impl LxcSensitiveField {
    pub(crate) const fn api_name(self) -> &'static str {
        match self {
            Self::Password => "password",
        }
    }

    fn from_api_name(value: &str) -> Option<Self> {
        [Self::Password]
            .into_iter()
            .find(|field| field.api_name() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LxcOptionName(String);

impl LxcOptionName {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

fn parse_numbered_field(value: &str, prefix: &str) -> Option<u8> {
    value.strip_prefix(prefix)?.parse().ok()
}

fn valid_lxc_option_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
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

    #[test]
    fn lxc_fields_have_one_authoritative_classification() {
        for field in [
            GuestField::Hostname,
            GuestField::RootFs,
            GuestField::Unprivileged,
        ] {
            assert_eq!(
                LxcConfigField::classify(&field.api_name()),
                LxcConfigField::Managed(field)
            );
        }
        assert_eq!(
            LxcConfigField::classify(&GuestField::Network(0).api_name()),
            LxcConfigField::Device(LxcDeviceField::Network(0))
        );
        assert_eq!(
            LxcConfigField::classify(&GuestField::BindMount(12).api_name()),
            LxcConfigField::Device(LxcDeviceField::BindMount(12))
        );
        for field in ["arch", "features", "searchdomain", "future-option"] {
            let LxcConfigField::Additional(name) = LxcConfigField::classify(field) else {
                panic!("expected additional LXC option")
            };
            assert_eq!(name.as_str(), field);
        }
        for field in ["digest", "lxc"] {
            assert!(matches!(
                LxcConfigField::classify(field),
                LxcConfigField::ReadOnly(_)
            ));
        }
        assert!(matches!(
            LxcConfigField::classify("password"),
            LxcConfigField::Sensitive(LxcSensitiveField::Password)
        ));
        for field in ["", "UPPERCASE", "path/name"] {
            assert_eq!(LxcConfigField::classify(field), LxcConfigField::Invalid);
        }
    }
}
