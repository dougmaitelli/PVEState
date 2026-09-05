use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RepositoryLayout {
    root: PathBuf,
}

impl RepositoryLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    pub fn document(&self, name: &str) -> PathBuf {
        self.config().join(name)
    }

    pub fn runtime(&self) -> PathBuf {
        self.root.join(".runtime")
    }

    pub fn observed_root(&self) -> PathBuf {
        self.root.join("observed")
    }

    pub fn observed(&self) -> PathBuf {
        self.observed_root().join("production")
    }
}
