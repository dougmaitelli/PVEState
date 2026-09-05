use crate::{
    client::{PbsClient, PveClient},
    config::LocalState,
};
use anyhow::{Context, Result, bail};
use chrono::Duration;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use super::CaptureManifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureId(String);

impl CaptureId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) struct VerifiedCaptureManifest(CaptureManifest);

pub(crate) struct CapturedState {
    id: CaptureId,
    pub(crate) manifest: VerifiedCaptureManifest,
    pub(crate) pve: CapturedPve,
    pub(crate) pbs: CapturedPbs,
    pub(crate) native: CapturedNative,
}

struct CapturedApi {
    endpoint: String,
    responses: BTreeMap<String, Value>,
}

pub(crate) struct CapturedNative {
    root: PathBuf,
}

pub(crate) struct CapturedPve(CapturedApi);

pub(crate) struct CapturedPbs(CapturedApi);

impl CapturedState {
    pub(crate) fn load(local: &LocalState, max_age: Duration) -> Result<Self> {
        let observed = local.observed();
        let manifest: CaptureManifest = serde_json::from_slice(
            &fs::read(observed.join(crate::config::artifacts::CAPTURE_MANIFEST))
                .context("run capture first")?,
        )?;
        manifest.verify(&observed, max_age)?;
        let id = CaptureId(manifest.capture_id.clone());
        let pve = CapturedPve(CapturedApi::load(
            &observed.join(crate::config::artifacts::PVE_SNAPSHOT),
            source_endpoint(&manifest, "pve-api")?,
        )?);
        let pbs = CapturedPbs(CapturedApi::load(
            &observed.join(crate::config::artifacts::PBS_SNAPSHOT),
            source_endpoint(&manifest, "pbs-api")?,
        )?);

        Ok(Self {
            id,
            manifest: VerifiedCaptureManifest(manifest),
            pve,
            pbs,
            native: CapturedNative { root: observed },
        })
    }

    pub(crate) fn id(&self) -> &CaptureId {
        debug_assert_eq!(self.id.as_str(), self.manifest.0.capture_id);
        &self.id
    }

    #[cfg(test)]
    pub(crate) fn fixture(
        id: &str,
        pve_endpoint: &str,
        pve: BTreeMap<String, Value>,
        pbs_endpoint: &str,
        pbs: BTreeMap<String, Value>,
        native_root: PathBuf,
    ) -> Self {
        let manifest = CaptureManifest::new(chrono::Utc::now(), BTreeMap::new(), BTreeMap::new());
        Self {
            id: CaptureId(id.into()),
            manifest: VerifiedCaptureManifest(manifest),
            pve: CapturedPve(CapturedApi {
                endpoint: pve_endpoint.into(),
                responses: pve,
            }),
            pbs: CapturedPbs(CapturedApi {
                endpoint: pbs_endpoint.into(),
                responses: pbs,
            }),
            native: CapturedNative { root: native_root },
        }
    }
}

impl CapturedApi {
    fn load(path: &Path, expected_endpoint: &str) -> Result<Self> {
        let snapshot: Value = serde_json::from_slice(
            &fs::read(path)
                .with_context(|| format!("read captured API snapshot {}", path.display()))?,
        )?;
        let endpoint = snapshot
            .get("endpoint")
            .and_then(Value::as_str)
            .context("captured API snapshot endpoint")?;
        if endpoint.trim_end_matches('/') != expected_endpoint.trim_end_matches('/') {
            bail!(
                "captured API endpoint {endpoint} does not match manifest endpoint {expected_endpoint}"
            )
        }
        let mut responses = BTreeMap::new();
        collect_responses(&snapshot, &mut responses);
        Ok(Self {
            endpoint: endpoint.trim_end_matches('/').into(),
            responses,
        })
    }

    pub(crate) fn response(&self, path: &str) -> Result<Value> {
        self.responses
            .get(path)
            .cloned()
            .with_context(|| format!("complete capture has no successful response for {path}"))
    }
}

impl CapturedPve {
    pub(crate) fn response(&self, path: &str) -> Result<Value> {
        self.0.response(path)
    }
}

impl CapturedPbs {
    pub(crate) fn response(&self, path: &str) -> Result<Value> {
        self.0.response(path)
    }
}

impl CapturedNative {
    pub(crate) fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    pub(crate) fn read_to_string(&self, relative: impl AsRef<Path>) -> Result<String> {
        let path = self.path(relative);
        fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
    }
}

fn source_endpoint<'a>(manifest: &'a CaptureManifest, source: &str) -> Result<&'a str> {
    manifest
        .sources
        .get(source)
        .map(|evidence| evidence.endpoint.as_str())
        .with_context(|| format!("capture manifest has no {source} source"))
}

fn collect_responses(value: &Value, responses: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(object) => {
            if object.get("ok").and_then(Value::as_bool) == Some(true)
                && let (Some(path), Some(data)) = (
                    object.get("path").and_then(Value::as_str),
                    object.get("data"),
                )
            {
                responses.insert(path.into(), data.clone());
            }
            object
                .values()
                .for_each(|value| collect_responses(value, responses));
        },
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_responses(value, responses)),
        _ => {},
    }
}

macro_rules! captured_client {
    ($type:ident, $trait:ident) => {
        impl $trait for $type {
            fn endpoint(&self) -> &str {
                &self.0.endpoint
            }
            fn get(&self, path: &str) -> Result<Value> {
                self.0.response(path)
            }
            fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
                bail!("planner attempted to mutate through a captured-state client")
            }
            fn post(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
                bail!("planner attempted to mutate through a captured-state client")
            }
            fn delete(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
                bail!("planner attempted to mutate through a captured-state client")
            }
        }
    };
}

captured_client!(CapturedPve, PveClient);
captured_client!(CapturedPbs, PbsClient);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_successful_nested_responses_by_api_path() {
        let snapshot = serde_json::json!({
            "nested": {
                "ok": true,
                "path": "/nodes/pve/dns",
                "data": {"search": "example.test"}
            },
            "failed": {"ok": false, "path": "/failed", "error": "denied"}
        });
        let mut responses = BTreeMap::new();

        collect_responses(&snapshot, &mut responses);

        assert_eq!(responses["/nodes/pve/dns"]["search"], "example.test");
        assert!(!responses.contains_key("/failed"));
    }

    #[test]
    fn rejects_snapshot_from_a_different_endpoint() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("pve.json");
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "endpoint": "https://other.test:8006",
                "requests": {}
            }))
            .unwrap(),
        )
        .unwrap();

        let error = CapturedApi::load(&path, "https://pve.test:8006")
            .err()
            .unwrap();

        assert!(
            error
                .to_string()
                .contains("does not match manifest endpoint")
        );
    }
}
