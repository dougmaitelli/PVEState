use super::{PbsClient, transport::JsonApiClient};
use crate::{
    discovery::{
        ApiObject, ObjectResponse, ObjectsResponse, RawResponse, capture as capture_response,
    },
    settings::{ApiCredential, PbsSettings},
};
use anyhow::Result;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Method;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct Pbs {
    transport: JsonApiClient,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub schema_version: u8,
    pub collected_at: chrono::DateTime<chrono::Utc>,
    pub mode: &'static str,
    pub endpoint: String,
    pub requests: Responses,
    pub datastores: BTreeMap<String, Datastore>,
}

#[derive(Debug, Serialize)]
pub struct Responses {
    pub version: RawResponse,
    pub datastore_usage: ObjectsResponse,
    pub datastores: ObjectsResponse,
    pub s3_endpoints: ObjectsResponse,
    pub remotes: ObjectsResponse,
    pub sync_jobs: ObjectsResponse,
    pub prune_jobs: ObjectsResponse,
    pub verify_jobs: ObjectsResponse,
    pub node_status: ObjectResponse,
}

#[derive(Debug, Serialize)]
pub struct Datastore {
    pub config: ApiObject,
    pub status: ObjectResponse,
    pub groups: ObjectsResponse,
    pub snapshots: ObjectsResponse,
}

impl Pbs {
    pub fn discovery(settings: &PbsSettings) -> Result<Self> {
        Self::new(settings, settings.discovery_credential()?)
    }

    pub fn mutation(settings: &PbsSettings) -> Result<Self> {
        Self::new(settings, settings.mutation_credential()?)
    }

    fn new(settings: &PbsSettings, credential: &ApiCredential) -> Result<Self> {
        Ok(Self {
            transport: JsonApiClient::new(
                &settings.endpoint,
                settings.verify_tls,
                settings.ca_file.as_deref(),
                &format!("PBSAPIToken {}:{}", credential.id, credential.secret),
            )?,
        })
    }

    pub fn endpoint(&self) -> &str {
        self.transport.endpoint()
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        self.transport.get_data(path)
    }

    pub fn put(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.mutate(Method::PUT, path, data)
    }

    pub fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.mutate(Method::POST, path, data)
    }

    pub fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.mutate(Method::DELETE, path, data)
    }

    fn mutate(&self, method: Method, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.transport.form(method, path, data)
    }
}

pub fn capture(client: &dyn PbsClient) -> Snapshot {
    let requests = Responses {
        version: get(client, "/version"),
        datastore_usage: get(client, "/status/datastore-usage"),
        datastores: get(client, "/config/datastore"),
        s3_endpoints: get(client, "/config/s3"),
        remotes: get(client, "/config/remote"),
        sync_jobs: get(client, "/config/sync"),
        prune_jobs: get(client, "/config/prune"),
        verify_jobs: get(client, "/config/verify"),
        node_status: get(client, "/nodes/localhost/status"),
    };

    let mut datastores = BTreeMap::new();
    if let Some(items) = requests.datastores.data.as_ref() {
        for config in items {
            let Some(name) = config
                .get("name")
                .or_else(|| config.get("store"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let encoded = utf8_percent_encode(name, NON_ALPHANUMERIC);
            let root = format!("/admin/datastore/{encoded}");
            datastores.insert(
                name.into(),
                Datastore {
                    config: config.clone(),
                    status: get(client, &format!("{root}/status")),
                    groups: get(client, &format!("{root}/groups")),
                    snapshots: get(client, &format!("{root}/snapshots")),
                },
            );
        }
    }

    Snapshot {
        schema_version: 1,
        collected_at: chrono::Utc::now(),
        mode: "read-only",
        endpoint: client.endpoint().into(),
        requests,
        datastores,
    }
}

fn get<T: serde::de::DeserializeOwned>(
    client: &dyn PbsClient,
    path: &str,
) -> crate::discovery::CapturedResponse<T> {
    capture_response(path, || client.get(path))
}

impl PbsClient for Pbs {
    fn endpoint(&self) -> &str {
        self.endpoint()
    }
    fn get(&self, path: &str) -> Result<Value> {
        self.get(path)
    }
    fn put(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.put(path, data)
    }
    fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.post(path, data)
    }
    fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.delete(path, data)
    }
}

impl Snapshot {
    pub fn failures(&self) -> Vec<String> {
        let mut failures = Vec::new();
        add_failure("version", &self.requests.version, &mut failures);
        add_failure(
            "datastore_usage",
            &self.requests.datastore_usage,
            &mut failures,
        );
        add_failure("datastores", &self.requests.datastores, &mut failures);
        add_failure("s3_endpoints", &self.requests.s3_endpoints, &mut failures);
        add_failure("remotes", &self.requests.remotes, &mut failures);
        add_failure("sync_jobs", &self.requests.sync_jobs, &mut failures);
        add_failure("prune_jobs", &self.requests.prune_jobs, &mut failures);
        add_failure("verify_jobs", &self.requests.verify_jobs, &mut failures);
        add_failure("node_status", &self.requests.node_status, &mut failures);
        for (datastore, details) in &self.datastores {
            add_failure(
                &format!("{datastore}/status"),
                &details.status,
                &mut failures,
            );
            add_failure(
                &format!("{datastore}/groups"),
                &details.groups,
                &mut failures,
            );
            add_failure(
                &format!("{datastore}/snapshots"),
                &details.snapshots,
                &mut failures,
            );
        }
        failures
    }
}

fn add_failure<T>(
    name: &str,
    response: &crate::discovery::CapturedResponse<T>,
    failures: &mut Vec<String>,
) {
    if let Some(error) = response.failure() {
        failures.push(format!("{name}: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;

    struct FakePbs;

    impl PbsClient for FakePbs {
        fn endpoint(&self) -> &str {
            "https://pbs.test:8007"
        }

        fn get(&self, path: &str) -> Result<Value> {
            match path {
                "/version" | "/nodes/localhost/status" => Ok(serde_json::json!({})),
                "/status/datastore-usage"
                | "/config/datastore"
                | "/config/s3"
                | "/config/remote"
                | "/config/sync"
                | "/config/prune"
                | "/config/verify" => Ok(serde_json::json!([])),
                _ => bail!("unexpected fake endpoint {path}"),
            }
        }

        fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }

        fn post(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }

        fn delete(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }
    }

    #[test]
    fn datastore_names_are_safe_in_api_paths() {
        assert_eq!(
            utf8_percent_encode("primary store/1", NON_ALPHANUMERIC).to_string(),
            "primary%20store%2F1"
        );
    }

    #[test]
    fn discovery_accepts_an_injected_pbs_client() {
        let snapshot = capture(&FakePbs);
        assert_eq!(snapshot.endpoint, "https://pbs.test:8007");
        assert_eq!(snapshot.requests.datastores.path, "/config/datastore");
        assert!(snapshot.failures().is_empty());
    }

    #[test]
    fn snapshot_reports_static_and_datastore_failures() {
        fn failed<T>(path: &str) -> crate::discovery::CapturedResponse<T> {
            crate::discovery::CapturedResponse {
                ok: false,
                path: path.into(),
                data: None,
                error: Some("denied".into()),
            }
        }
        let snapshot = Snapshot {
            schema_version: 1,
            collected_at: chrono::Utc::now(),
            mode: "read-only",
            endpoint: "https://pbs.example.test:8007".into(),
            requests: Responses {
                version: failed("/version"),
                datastore_usage: failed("/status/datastore-usage"),
                datastores: failed("/config/datastore"),
                s3_endpoints: failed("/config/s3"),
                remotes: failed("/config/remote"),
                sync_jobs: failed("/config/sync"),
                prune_jobs: failed("/config/prune"),
                verify_jobs: failed("/config/verify"),
                node_status: failed("/nodes/localhost/status"),
            },
            datastores: BTreeMap::from([(
                "backup".into(),
                Datastore {
                    config: BTreeMap::new(),
                    status: failed("/status"),
                    groups: failed("/groups"),
                    snapshots: failed("/snapshots"),
                },
            )]),
        };
        assert_eq!(snapshot.failures().len(), 12);
    }
}
