use crate::utility::yaml_patch::{Patch, Segment};
use serde_yaml::Value;

type ConfigPath = Vec<Segment>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConfigDocument {
    Guests,
    Network,
    Cluster,
    Node,
    Backup,
}

impl ConfigDocument {
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::Guests => "config/guests.yml",
            Self::Network => "config/network.yml",
            Self::Cluster => "config/cluster.yml",
            Self::Node => "config/node.yml",
            Self::Backup => "config/backup.yml",
        }
    }

    pub(crate) fn from_path(path: &str) -> Option<Self> {
        [
            Self::Guests,
            Self::Network,
            Self::Cluster,
            Self::Node,
            Self::Backup,
        ]
        .into_iter()
        .find(|document| document.path() == path)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum LocalPatch {
    SetScalar {
        document: ConfigDocument,
        path: ConfigPath,
        value: Value,
    },
    ReplaceResource {
        document: ConfigDocument,
        path: ConfigPath,
        value: Value,
    },
    RemoveResource {
        document: ConfigDocument,
        path: ConfigPath,
    },
    RemoveSequenceValue {
        document: ConfigDocument,
        path: ConfigPath,
        value: Value,
    },
}

impl LocalPatch {
    pub(crate) const fn document(&self) -> ConfigDocument {
        match self {
            Self::SetScalar { document, .. }
            | Self::ReplaceResource { document, .. }
            | Self::RemoveResource { document, .. }
            | Self::RemoveSequenceValue { document, .. } => *document,
        }
    }

    pub(crate) fn into_yaml_patch(self) -> Patch {
        match self {
            Self::SetScalar { path, value, .. } | Self::ReplaceResource { path, value, .. } => {
                Patch::Set(path, value)
            },
            Self::RemoveResource { path, .. } => Patch::Remove(path),
            Self::RemoveSequenceValue { path, value, .. } => {
                Patch::RemoveSequenceValue(path, value)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_patch_preserves_document_ownership() {
        let patch = LocalPatch::RemoveResource {
            document: ConfigDocument::Cluster,
            path: vec![Segment::Key("jobs".into())],
        };

        assert_eq!(patch.document(), ConfigDocument::Cluster);
        assert!(matches!(patch.into_yaml_patch(), Patch::Remove(_)));
    }
}
