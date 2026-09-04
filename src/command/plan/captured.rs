use crate::{
    client::{PbsClient, PveClient},
    config::Repository,
    discovery::CaptureManifest,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

pub(super) struct CapturedClients {
    pub pve: CapturedApi,
    pub pbs: CapturedApi,
}

pub(super) struct CapturedApi {
    endpoint: String,
    responses: BTreeMap<String, Value>,
}

impl CapturedClients {
    pub fn load(repo: &Repository, manifest: &CaptureManifest) -> Result<Self> {
        Ok(Self {
            pve: CapturedApi::load(
                &repo.observed().join("api/pve.json"),
                source_endpoint(manifest, "pve-api")?,
            )?,
            pbs: CapturedApi::load(
                &repo.observed().join("api/pbs.json"),
                source_endpoint(manifest, "pbs-api")?,
            )?,
        })
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

    fn get(&self, path: &str) -> Result<Value> {
        self.responses
            .get(path)
            .cloned()
            .with_context(|| format!("complete capture has no successful response for {path}"))
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
    ($trait:ident) => {
        impl $trait for CapturedApi {
            fn endpoint(&self) -> &str {
                &self.endpoint
            }

            fn get(&self, path: &str) -> Result<Value> {
                self.get(path)
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

captured_client!(PveClient);
captured_client!(PbsClient);

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
            "failed": {
                "ok": false,
                "path": "/failed",
                "error": "denied"
            }
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
