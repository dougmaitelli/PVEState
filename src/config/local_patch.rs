use crate::utility::yaml_patch::{Patch, Segment};
use serde_yaml::Value;

type ConfigPath = Vec<Segment>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConfigDocument {
    Guests,
    Network,
    Cluster,
    Node,
}

impl ConfigDocument {
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::Guests => "config/guests.yml",
            Self::Network => "config/network.yml",
            Self::Cluster => "config/cluster.yml",
            Self::Node => "config/node.yml",
        }
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
}

impl LocalPatch {
    pub(crate) const fn document(&self) -> ConfigDocument {
        match self {
            Self::SetScalar { document, .. }
            | Self::ReplaceResource { document, .. }
            | Self::RemoveResource { document, .. } => *document,
        }
    }

    pub(crate) fn into_yaml_patch(self) -> Patch {
        match self {
            Self::SetScalar { path, value, .. } | Self::ReplaceResource { path, value, .. } => {
                Patch::Set(path, value)
            },
            Self::RemoveResource { path, .. } => Patch::Remove(path),
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
