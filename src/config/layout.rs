use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct RepositoryLayout {
    root: PathBuf,
}

impl RepositoryLayout {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    pub(crate) fn document(&self, name: &str) -> PathBuf {
        self.config().join(name)
    }

    pub(crate) fn runtime(&self) -> PathBuf {
        self.root.join(".runtime")
    }

    pub(crate) fn observed_root(&self) -> PathBuf {
        self.root.join("observed")
    }

    pub(crate) fn observed(&self) -> PathBuf {
        self.observed_root().join("production")
    }
}
