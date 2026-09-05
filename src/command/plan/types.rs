use crate::model::{DiskInterface, GuestRef};
use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, ops::Deref};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Domain {
    #[serde(rename = "guests")]
    Guest,
    Network,
    Firewall,
    Dns,
    Backup,
    Pbs,
}

impl Domain {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Guest => "guests",
            Self::Network => "network",
            Self::Firewall => "firewall",
            Self::Dns => "dns",
            Self::Backup => "backup",
            Self::Pbs => "pbs",
        }
    }
}

impl fmt::Display for Domain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<&str> for Domain {
    fn from(value: &str) -> Self {
        match value {
            "guests" => Self::Guest,
            "network" => Self::Network,
            "firewall" => Self::Firewall,
            "dns" => Self::Dns,
            "backup" => Self::Backup,
            "pbs" => Self::Pbs,
            _ => panic!("unknown operation domain {value}"),
        }
    }
}
impl std::str::FromStr for Domain {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "guests" => Ok(Self::Guest),
            "network" => Ok(Self::Network),
            "firewall" => Ok(Self::Firewall),
            "dns" => Ok(Self::Dns),
            "backup" => Ok(Self::Backup),
            "pbs" => Ok(Self::Pbs),
            _ => bail!("unknown operation domain `{value}`"),
        }
    }
}
impl From<Domain> for String {
    fn from(value: Domain) -> Self {
        value.to_string()
    }
}
impl PartialEq<str> for Domain {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
impl PartialEq<&str> for Domain {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResourceId {
    Cluster,
    Node(String),
    Guest(GuestRef),
    GuestDisk(GuestRef, String),
    Named(String),
}

impl ResourceId {
    pub(crate) fn parse(value: &str) -> Self {
        if value == "cluster" {
            return Self::Cluster;
        }
        if let Some(node) = value.strip_prefix("node/") {
            return Self::Node(node.into());
        }
        if let Ok(guest) = value.parse::<GuestRef>() {
            return Self::Guest(guest);
        }
        if let Some((guest, field)) = value.rsplit_once('/')
            && let Ok(guest) = guest.parse()
            && disk_id_is_valid(field)
        {
            return Self::GuestDisk(guest, field.into());
        }
        Self::Named(value.into())
    }

    pub(crate) fn guest(&self) -> Option<GuestRef> {
        match self {
            Self::Guest(guest) | Self::GuestDisk(guest, _) => Some(*guest),
            _ => None,
        }
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cluster => formatter.write_str("cluster"),
            Self::Node(node) => write!(formatter, "node/{node}"),
            Self::Guest(guest) => guest.fmt(formatter),
            Self::GuestDisk(guest, field) => write!(formatter, "{guest}/{field}"),
            Self::Named(value) => formatter.write_str(value),
        }
    }
}

impl From<&str> for ResourceId {
    fn from(value: &str) -> Self {
        Self::parse(value)
    }
}
impl From<String> for ResourceId {
    fn from(value: String) -> Self {
        Self::parse(&value)
    }
}
impl Serialize for ResourceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for ResourceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::parse(&String::deserialize(deserializer)?))
    }
}

macro_rules! validated_string {
    ($name:ident, $description:literal, $validate:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub(crate) struct $name(String);
        impl $name {
            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
            pub(crate) fn is_valid(&self) -> bool {
                ($validate)(self.as_str())
            }
        }
        impl Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.into())
            }
        }
        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let value = String::deserialize(d)?;
                if ($validate)(&value) {
                    Ok(Self(value))
                } else {
                    Err(de::Error::custom(format!(
                        "invalid {} `{value}`",
                        $description
                    )))
                }
            }
        }
    };
}

validated_string!(ApiPath, "API path", |value: &str| value.starts_with('/')
    && !value.contains(".."));
validated_string!(
    SecretName,
    "secret environment variable",
    |value: &str| !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
);
fn disk_id_is_valid(value: &str) -> bool {
    value == "rootfs" || value.parse::<DiskInterface>().is_ok()
}

validated_string!(DiskId, "guest disk identifier", disk_id_is_valid);

pub(crate) fn validate_operation(
    domain: Domain,
    target: super::ApiTarget,
    resource: &ResourceId,
) -> Result<()> {
    if (target == super::ApiTarget::Pbs) != (domain == Domain::Pbs) {
        bail!("operation target {target:?} is incompatible with domain {domain}");
    }
    match (domain, resource) {
        (Domain::Guest, ResourceId::Guest(_) | ResourceId::GuestDisk(_, _))
        | (Domain::Network, ResourceId::Named(_))
        | (
            Domain::Firewall,
            ResourceId::Cluster | ResourceId::Node(_) | ResourceId::Guest(_) | ResourceId::Named(_),
        )
        | (Domain::Dns, ResourceId::Named(_) | ResourceId::Node(_))
        | (Domain::Backup | Domain::Pbs, ResourceId::Named(_)) => Ok(()),
        _ => bail!("resource {resource} is incompatible with domain {domain}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::plan::ApiTarget;

    #[test]
    fn rejects_pbs_target_labeled_as_guest_domain() {
        let resource = ResourceId::parse("lxc/101");
        let error = validate_operation(Domain::Guest, ApiTarget::Pbs, &resource).unwrap_err();

        assert!(error.to_string().contains("incompatible"));
    }

    #[test]
    fn resource_ids_recognize_guests_disks_nodes_and_cluster() {
        assert!(matches!(ResourceId::parse("lxc/101"), ResourceId::Guest(_)));
        assert!(matches!(
            ResourceId::parse("qemu/201/scsi0"),
            ResourceId::GuestDisk(_, _)
        ));
        assert!(matches!(ResourceId::parse("node/pve"), ResourceId::Node(_)));
        assert_eq!(ResourceId::parse("cluster"), ResourceId::Cluster);
    }

    #[test]
    fn transport_identifiers_validate_when_deserialized() {
        assert!(serde_json::from_str::<ApiPath>("\"relative\"").is_err());
        assert!(serde_json::from_str::<DiskId>("\"not-a-disk\"").is_err());
        assert!(serde_json::from_str::<SecretName>("\"lower-case\"").is_err());
        assert!("unknown".parse::<Domain>().is_err());
    }
}
