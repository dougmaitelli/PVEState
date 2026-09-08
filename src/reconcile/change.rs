use crate::{
    model::{DiskInterface, GuestRef},
    resource::native_paths,
};
use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, ops::Deref};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum ManagedFile {
    NetworkInterfaces,
    ClusterFirewall,
    NodeFirewall { node: String },
    GuestFirewall { vmid: u32 },
}

impl ManagedFile {
    pub(crate) fn path(&self) -> String {
        match self {
            Self::NetworkInterfaces => native_paths::NETWORK_REMOTE.into(),
            Self::ClusterFirewall => native_paths::CLUSTER_FIREWALL_REMOTE.into(),
            Self::NodeFirewall { node } => native_paths::node_firewall_remote(node),
            Self::GuestFirewall { vmid } => native_paths::guest_firewall_remote(vmid),
        }
    }

    pub(crate) const fn domain(&self) -> Domain {
        match self {
            Self::NetworkInterfaces => Domain::Network,
            Self::ClusterFirewall | Self::NodeFirewall { .. } | Self::GuestFirewall { .. } => {
                Domain::Firewall
            },
        }
    }

    pub(crate) const fn requires_activation(&self) -> bool {
        matches!(self, Self::NetworkInterfaces)
    }

    pub(crate) const fn mode(&self) -> &'static str {
        match self {
            Self::NetworkInterfaces => "0644",
            Self::ClusterFirewall | Self::NodeFirewall { .. } | Self::GuestFirewall { .. } => {
                "0640"
            },
        }
    }

    pub(crate) fn matches(&self, domain: Domain, resource: &ResourceId) -> bool {
        if !self.is_valid() || domain != self.domain() {
            return false;
        }
        match (self, resource) {
            (Self::NetworkInterfaces, ResourceId::Named(_))
            | (Self::ClusterFirewall, ResourceId::Cluster) => true,
            (Self::NodeFirewall { node }, ResourceId::Node(resource_node)) => node == resource_node,
            (Self::GuestFirewall { vmid }, ResourceId::Guest(resource_guest)) => {
                *vmid == resource_guest.vmid
            },
            (Self::GuestFirewall { vmid }, ResourceId::Named(resource_vmid)) => {
                resource_vmid.parse::<u32>() == Ok(*vmid)
            },
            _ => false,
        }
    }

    fn is_valid(&self) -> bool {
        match self {
            Self::NodeFirewall { node } => {
                !node.is_empty()
                    && node.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
                    })
            },
            Self::NetworkInterfaces | Self::ClusterFirewall | Self::GuestFirewall { .. } => true,
        }
    }
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

impl TryFrom<&str> for Domain {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        value.parse()
    }
}

impl<'de> Deserialize<'de> for Domain {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub(crate) struct MutationEndpoint(String);

impl MutationEndpoint {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn family(&self) -> Option<MutationEndpointFamily<'_>> {
        mutation_endpoint_family(&self.0)
    }
}

impl Deref for MutationEndpoint {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for MutationEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<&str> for MutationEndpoint {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<String> for MutationEndpoint {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl<'de> Deserialize<'de> for MutationEndpoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        mutation_endpoint_family(&value).ok_or_else(|| {
            de::Error::custom(format!("API mutation endpoint is not managed `{value}`"))
        })?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MutationEndpointFamily<'a> {
    GuestConfig {
        node: &'a str,
        kind: &'a str,
        vmid: &'a str,
    },
    NodeDns {
        node: &'a str,
    },
    PveBackupCollection,
    PveBackupJob {
        id: &'a str,
    },
    PbsCollection {
        kind: &'a str,
    },
    PbsResource {
        kind: &'a str,
        id: &'a str,
    },
}

fn mutation_endpoint_family(value: &str) -> Option<MutationEndpointFamily<'_>> {
    let segments = value.strip_prefix('/')?.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| {
        segment.is_empty() || *segment == "." || *segment == ".." || segment.contains('\\')
    }) {
        return None;
    }
    match segments.as_slice() {
        ["nodes", node, kind @ ("lxc" | "qemu"), vmid, "config"] => {
            Some(MutationEndpointFamily::GuestConfig { node, kind, vmid })
        },
        ["nodes", node, "dns"] => Some(MutationEndpointFamily::NodeDns { node }),
        ["cluster", "backup"] => Some(MutationEndpointFamily::PveBackupCollection),
        ["cluster", "backup", id] => Some(MutationEndpointFamily::PveBackupJob { id }),
        [
            "config",
            kind @ ("datastore" | "s3" | "prune" | "verify" | "sync"),
        ] => Some(MutationEndpointFamily::PbsCollection { kind }),
        [
            "config",
            kind @ ("datastore" | "s3" | "prune" | "verify" | "sync"),
            id,
        ] => Some(MutationEndpointFamily::PbsResource { kind, id }),
        _ => None,
    }
}
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
    use crate::reconcile::ApiTarget;

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
    fn managed_files_have_canonical_paths_domains_and_activation() {
        let network = ManagedFile::NetworkInterfaces;
        assert_eq!(network.path(), "/etc/network/interfaces");
        assert_eq!(network.domain(), Domain::Network);
        assert!(network.requires_activation());

        let cluster = ManagedFile::ClusterFirewall;
        assert_eq!(cluster.path(), "/etc/pve/firewall/cluster.fw");
        assert_eq!(cluster.domain(), Domain::Firewall);
        assert!(!cluster.requires_activation());

        let node = ManagedFile::NodeFirewall { node: "pve".into() };
        assert_eq!(node.path(), "/etc/pve/nodes/pve/host.fw");

        let guest = ManagedFile::GuestFirewall { vmid: 101 };
        assert_eq!(guest.path(), "/etc/pve/firewall/101.fw");
    }

    #[test]
    fn managed_file_rejects_traversal_and_identity_mismatches() {
        let traversal = ManagedFile::NodeFirewall {
            node: "../../shadow".into(),
        };
        assert!(!traversal.matches(Domain::Firewall, &ResourceId::Node("../../shadow".into())));

        let guest = ManagedFile::GuestFirewall { vmid: 101 };
        assert!(!guest.matches(
            Domain::Firewall,
            &ResourceId::Guest("lxc/102".parse().unwrap())
        ));
        assert!(!guest.matches(
            Domain::Network,
            &ResourceId::Guest("lxc/101".parse().unwrap())
        ));
    }

    #[test]
    fn transport_identifiers_validate_when_deserialized() {
        assert!(serde_json::from_str::<ApiPath>("\"relative\"").is_err());
        assert!(serde_json::from_str::<MutationEndpoint>("\"/access/users\"").is_err());
        assert!(serde_json::from_str::<DiskId>("\"not-a-disk\"").is_err());
        assert!(serde_json::from_str::<SecretName>("\"lower-case\"").is_err());
        assert!("unknown".parse::<Domain>().is_err());
    }

    #[test]
    fn domain_parses_every_serialized_spelling() {
        for (spelling, expected) in [
            ("guests", Domain::Guest),
            ("network", Domain::Network),
            ("firewall", Domain::Firewall),
            ("dns", Domain::Dns),
            ("backup", Domain::Backup),
            ("pbs", Domain::Pbs),
        ] {
            assert_eq!(spelling.parse::<Domain>().unwrap(), expected);
            assert_eq!(Domain::try_from(spelling).unwrap(), expected);
            assert_eq!(
                serde_json::to_string(&expected).unwrap(),
                format!("\"{spelling}\"")
            );
            assert_eq!(
                serde_json::from_str::<Domain>(&format!("\"{spelling}\"")).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn unknown_and_case_mismatched_domains_are_errors() {
        for spelling in ["unknown", "Guest", "GUESTS", "Network"] {
            assert!(spelling.parse::<Domain>().is_err());
            assert!(serde_json::from_str::<Domain>(&format!("\"{spelling}\"")).is_err());
        }
    }
}
