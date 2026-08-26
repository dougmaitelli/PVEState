use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaptureStatus {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureManifest {
    pub schema_version: u8,
    pub capture_id: String,
    pub exported_at: DateTime<Utc>,
    pub status: CaptureStatus,
    pub sources: BTreeMap<String, SourceEvidence>,
    pub artifacts: BTreeMap<String, ArtifactEvidence>,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEvidence {
    pub endpoint: String,
    pub required: bool,
    pub complete: bool,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEvidence {
    pub sha256: String,
    pub size: u64,
}

impl CaptureManifest {
    pub fn new(
        exported_at: DateTime<Utc>,
        sources: BTreeMap<String, SourceEvidence>,
        artifacts: BTreeMap<String, ArtifactEvidence>,
    ) -> Self {
        let failures = sources
            .iter()
            .flat_map(|(source, evidence)| {
                evidence
                    .failures
                    .iter()
                    .map(move |failure| format!("{source}: {failure}"))
            })
            .collect::<Vec<_>>();
        let complete = failures.is_empty()
            && sources
                .values()
                .filter(|source| source.required)
                .all(|source| source.complete);
        Self {
            schema_version: 2,
            capture_id: exported_at.format("%Y%m%dT%H%M%S%.fZ").to_string(),
            exported_at,
            status: if complete {
                CaptureStatus::Complete
            } else {
                CaptureStatus::Partial
            },
            sources,
            artifacts,
            failures,
        }
    }

    pub fn verify(&self, observed: &Path, max_age: Duration) -> Result<()> {
        if self.schema_version != 2 {
            bail!(
                "unsupported capture manifest schema {}; run capture",
                self.schema_version
            )
        }
        if self.status != CaptureStatus::Complete {
            bail!("capture is partial: {}", self.failures.join("; "))
        }
        let incomplete = self
            .sources
            .iter()
            .filter(|(_, source)| source.required && !source.complete)
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        if !incomplete.is_empty() {
            bail!(
                "required capture sources are incomplete: {}",
                incomplete.join(", ")
            )
        }
        if Utc::now() - self.exported_at > max_age {
            bail!("observed exports are stale; run capture before plan")
        }

        let actual = collect_artifacts(observed)?;
        let expected_paths: BTreeSet<_> = self.artifacts.keys().collect();
        let actual_paths: BTreeSet<_> = actual.keys().collect();
        if expected_paths != actual_paths {
            let missing = expected_paths
                .difference(&actual_paths)
                .copied()
                .collect::<Vec<_>>();
            let unexpected = actual_paths
                .difference(&expected_paths)
                .copied()
                .collect::<Vec<_>>();
            bail!("capture artifact set changed; missing={missing:?}, unexpected={unexpected:?}")
        }
        for (path, expected) in &self.artifacts {
            let current = &actual[path];
            if current.sha256 != expected.sha256 || current.size != expected.size {
                bail!("capture artifact changed after capture: {path}")
            }
        }
        Ok(())
    }
}

pub fn collect_artifacts(observed: &Path) -> Result<BTreeMap<String, ArtifactEvidence>> {
    let mut files = Vec::new();
    visit(observed, observed, &mut files)?;
    let mut artifacts = BTreeMap::new();
    for (relative, path) in files {
        if relative == "manifest.json" {
            continue;
        }
        let content = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        artifacts.insert(
            relative,
            ArtifactEvidence {
                sha256: hex::encode(Sha256::digest(&content)),
                size: content.len() as u64,
            },
        );
    }
    Ok(artifacts)
}

fn visit(root: &Path, directory: &Path, files: &mut Vec<(String, PathBuf)>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            visit(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path));
        } else {
            bail!("unsupported capture artifact type: {}", path.display())
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(complete: bool) -> BTreeMap<String, SourceEvidence> {
        BTreeMap::from([(
            "pve-api".into(),
            SourceEvidence {
                endpoint: "https://pve.test:8006".into(),
                required: true,
                complete,
                failures: if complete {
                    Vec::new()
                } else {
                    vec!["denied".into()]
                },
            },
        )])
    }

    #[test]
    fn partial_capture_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let manifest = CaptureManifest::new(Utc::now(), source(false), BTreeMap::new());
        assert!(manifest.verify(temp.path(), Duration::minutes(30)).is_err());
    }

    #[test]
    fn changed_artifact_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("pve.json"), "before").unwrap();
        let artifacts = collect_artifacts(temp.path()).unwrap();
        let manifest = CaptureManifest::new(Utc::now(), source(true), artifacts);
        fs::write(temp.path().join("pve.json"), "after").unwrap();
        assert!(manifest.verify(temp.path(), Duration::minutes(30)).is_err());
    }
}
