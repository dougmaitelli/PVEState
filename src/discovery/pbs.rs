use super::{ApiObject, ObjectResponse, ObjectsResponse, RawResponse, capture_with_events};
use crate::{client::PbsClient, utility::progress::EventSink};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub(crate) struct PbsSnapshot {
    pub(crate) schema_version: u8,
    pub(crate) collected_at: chrono::DateTime<chrono::Utc>,
    pub(crate) mode: &'static str,
    pub(crate) endpoint: String,
    pub(crate) requests: Responses,
    pub(crate) datastores: BTreeMap<String, Datastore>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Responses {
    pub(crate) version: RawResponse,
    pub(crate) datastore_usage: ObjectsResponse,
    pub(crate) datastores: ObjectsResponse,
    pub(crate) s3_endpoints: ObjectsResponse,
    pub(crate) remotes: ObjectsResponse,
    pub(crate) sync_jobs: ObjectsResponse,
    pub(crate) prune_jobs: ObjectsResponse,
    pub(crate) verify_jobs: ObjectsResponse,
    pub(crate) node_status: ObjectResponse,
}

#[derive(Debug, Serialize)]
pub(crate) struct Datastore {
    pub(crate) config: ApiObject,
    pub(crate) status: ObjectResponse,
    pub(crate) groups: ObjectsResponse,
    pub(crate) snapshots: ObjectsResponse,
}

pub(crate) fn capture(client: &dyn PbsClient, events: &dyn EventSink) -> PbsSnapshot {
    let requests = Responses {
        version: get(client, "/version", events),
        datastore_usage: get(client, "/status/datastore-usage", events),
        datastores: get(client, "/config/datastore", events),
        s3_endpoints: get(client, "/config/s3", events),
        remotes: get(client, "/config/remote", events),
        sync_jobs: get(client, "/config/sync", events),
        prune_jobs: get(client, "/config/prune", events),
        verify_jobs: get(client, "/config/verify", events),
        node_status: get(client, "/nodes/localhost/status", events),
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
                    status: get(client, &format!("{root}/status"), events),
                    groups: get(client, &format!("{root}/groups"), events),
                    snapshots: get(client, &format!("{root}/snapshots"), events),
                },
            );
        }
    }

    PbsSnapshot {
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
    events: &dyn EventSink,
) -> crate::discovery::CapturedResponse<T> {
    capture_with_events(path, || client.get(path), events)
}

impl PbsSnapshot {
    pub(crate) fn failures(&self) -> Vec<String> {
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
    use anyhow::{Result, bail};
    use std::cell::RefCell;

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
        let snapshot = capture(&FakePbs, &crate::utility::progress::NullEventSink);
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
        let snapshot = PbsSnapshot {
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

    struct RecordingPbs {
        paths: RefCell<Vec<String>>,
    }

    impl PbsClient for RecordingPbs {
        fn endpoint(&self) -> &str {
            "https://pbs.test:8007"
        }

        fn get(&self, path: &str) -> Result<Value> {
            self.paths.borrow_mut().push(path.into());
            match path {
                "/version" | "/nodes/localhost/status" => Ok(serde_json::json!({})),
                "/config/datastore" => Ok(serde_json::json!([{"name": "primary store/1"}])),
                "/admin/datastore/primary%20store%2F1/status" => Ok(serde_json::json!({})),
                "/admin/datastore/primary%20store%2F1/snapshots" => Ok(serde_json::json!([])),
                "/admin/datastore/primary%20store%2F1/groups" => bail!("groups denied"),
                "/status/datastore-usage"
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
    fn discovery_enumerates_encoded_datastore_endpoints_and_reports_failure() {
        let client = RecordingPbs {
            paths: RefCell::new(Vec::new()),
        };

        let snapshot = capture(&client, &crate::utility::progress::NullEventSink);

        assert!(
            client
                .paths
                .borrow()
                .iter()
                .any(|path| { path == "/admin/datastore/primary%20store%2F1/groups" })
        );
        assert_eq!(
            snapshot.failures(),
            ["primary store/1/groups: groups denied"]
        );
    }

    #[test]
    fn snapshot_serialization_keeps_the_archival_envelope() {
        let snapshot = capture(&FakePbs, &crate::utility::progress::NullEventSink);
        let value = serde_json::to_value(snapshot).unwrap();

        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["mode"], "read-only");
        assert!(value.get("collected_at").is_some());
        assert!(value.get("endpoint").is_some());
        assert!(value["requests"].get("datastore_usage").is_some());
        assert!(value.get("datastores").is_some());
    }
}
