use super::{ApiPath, DiskId, Domain, ManagedFile, ResourceId, SecretName};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub(crate) enum Operation {
    ApiMutation {
        target: ApiTarget,
        method: ApiMethod,
        domain: Domain,
        resource: ResourceId,
        endpoint: ApiPath,
        changes: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        environment_changes: BTreeMap<String, SecretName>,
        digest: Option<String>,
    },
    GrowDisk {
        domain: Domain,
        resource: ResourceId,
        endpoint: ApiPath,
        disk: DiskId,
        size_gb: u64,
    },
    WriteFile {
        domain: Domain,
        resource: ResourceId,
        target: ManagedFile,
        content: String,
        #[serde(skip)]
        before_content: Option<String>,
        before_sha256: Option<String>,
    },
    DeleteFile {
        domain: Domain,
        resource: ResourceId,
        target: ManagedFile,
        #[serde(skip)]
        before_content: String,
        before_sha256: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ApiTarget {
    Pve,
    Pbs,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ApiMethod {
    Post,
    Put,
    Delete,
}

impl Operation {
    pub(crate) const fn resource(&self) -> &ResourceId {
        match self {
            Self::ApiMutation { resource, .. }
            | Self::GrowDisk { resource, .. }
            | Self::WriteFile { resource, .. }
            | Self::DeleteFile { resource, .. } => resource,
        }
    }

    pub(crate) fn domain(&self) -> Domain {
        match self {
            Self::ApiMutation { domain, .. }
            | Self::GrowDisk { domain, .. }
            | Self::WriteFile { domain, .. }
            | Self::DeleteFile { domain, .. } => *domain,
        }
    }

    pub(crate) fn description(&self) -> String {
        match self {
            Self::ApiMutation {
                target,
                method,
                resource,
                changes,
                ..
            } => format!(
                "{method:?} {target:?} {resource} ({} change(s))",
                changes.len()
            ),
            Self::GrowDisk {
                resource, size_gb, ..
            } => format!("grow {resource} to {size_gb} GiB"),
            Self::WriteFile { target, .. } => format!("write {}", target.path()),
            Self::DeleteFile { target, .. } => format!("delete {}", target.path()),
        }
    }
}
