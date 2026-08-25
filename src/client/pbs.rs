use crate::config::env;
use anyhow::{Context, Result};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Certificate, blocking::Client};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, fs, time::Duration};

const ENDPOINTS: [(&str, &str); 9] = [
    ("version", "/version"),
    ("datastore_usage", "/status/datastore-usage"),
    ("datastores", "/config/datastore"),
    ("s3_endpoints", "/config/s3"),
    ("remotes", "/config/remote"),
    ("sync_jobs", "/config/sync"),
    ("prune_jobs", "/config/prune"),
    ("verify_jobs", "/config/verify"),
    ("node_status", "/nodes/localhost/status"),
];

pub struct Pbs {
    base: String,
    authorization: String,
    http: Client,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub schema_version: u8,
    pub collected_at: chrono::DateTime<chrono::Utc>,
    pub mode: &'static str,
    pub endpoint: String,
    pub requests: BTreeMap<String, Response>,
    pub datastores: BTreeMap<String, Datastore>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub ok: bool,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Datastore {
    pub config: Value,
    pub status: Response,
    pub groups: Response,
    pub snapshots: Response,
}

impl Pbs {
    pub fn discovery() -> Result<Self> {
        let endpoint = env("PBS_ENDPOINT", None)?;
        let token_id = env("PBS_API_TOKEN_ID", None)?;
        let token_secret = env("PBS_API_TOKEN_SECRET", None)?;
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(std::env::var("PBS_VERIFY_TLS").as_deref() == Ok("false"));
        if let Ok(path) = std::env::var("PBS_CA_FILE") {
            let pem = fs::read(&path).with_context(|| format!("read PBS CA file {path}"))?;
            builder = builder.add_root_certificate(Certificate::from_pem(&pem)?);
        }
        Ok(Self {
            base: format!("{}/api2/json", endpoint.trim_end_matches('/')),
            authorization: format!("PBSAPIToken {token_id}:{token_secret}"),
            http: builder.build()?,
        })
    }

    pub fn endpoint(&self) -> &str {
        self.base.trim_end_matches("/api2/json")
    }

    pub fn discover(&self) -> Snapshot {
        let mut requests = BTreeMap::new();
        for (name, path) in ENDPOINTS {
            requests.insert(name.into(), self.safe_get(path));
        }

        let mut datastores = BTreeMap::new();
        if let Some(items) = requests
            .get("datastores")
            .and_then(|response| response.data.as_ref())
            .and_then(Value::as_array)
        {
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
                        status: self.safe_get(&format!("{root}/status")),
                        groups: self.safe_get(&format!("{root}/groups")),
                        snapshots: self.safe_get(&format!("{root}/snapshots")),
                    },
                );
            }
        }

        Snapshot {
            schema_version: 1,
            collected_at: chrono::Utc::now(),
            mode: "read-only",
            endpoint: self.endpoint().into(),
            requests,
            datastores,
        }
    }

    fn get(&self, path: &str) -> Result<Value> {
        let payload: Value = self
            .http
            .get(format!("{}{}", self.base, path))
            .header("Authorization", &self.authorization)
            .header("Accept", "application/json")
            .header(
                "User-Agent",
                concat!("pvestate/", env!("CARGO_PKG_VERSION")),
            )
            .send()?
            .error_for_status()?
            .json()?;
        Ok(payload.get("data").cloned().unwrap_or(Value::Null))
    }

    fn safe_get(&self, path: &str) -> Response {
        match self.get(path) {
            Ok(data) => Response {
                ok: true,
                path: path.into(),
                data: Some(data),
                error: None,
            },
            Err(error) => Response {
                ok: false,
                path: path.into(),
                data: None,
                error: Some(format!("{error:#}")),
            },
        }
    }
}

impl Snapshot {
    pub fn failures(&self) -> Vec<String> {
        let mut failures = Vec::new();
        for (name, response) in &self.requests {
            if let Some(error) = &response.error {
                failures.push(format!("{name}: {error}"));
            }
        }
        for (datastore, details) in &self.datastores {
            for (name, response) in [
                ("status", &details.status),
                ("groups", &details.groups),
                ("snapshots", &details.snapshots),
            ] {
                if let Some(error) = &response.error {
                    failures.push(format!("{datastore}/{name}: {error}"));
                }
            }
        }
        failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datastore_names_are_safe_in_api_paths() {
        assert_eq!(
            utf8_percent_encode("primary store/1", NON_ALPHANUMERIC).to_string(),
            "primary%20store%2F1"
        );
    }

    #[test]
    fn snapshot_reports_static_and_datastore_failures() {
        let failed = |path: &str| Response {
            ok: false,
            path: path.into(),
            data: None,
            error: Some("denied".into()),
        };
        let snapshot = Snapshot {
            schema_version: 1,
            collected_at: chrono::Utc::now(),
            mode: "read-only",
            endpoint: "https://pbs.example.test:8007".into(),
            requests: BTreeMap::from([("version".into(), failed("/version"))]),
            datastores: BTreeMap::from([(
                "backup".into(),
                Datastore {
                    config: Value::Null,
                    status: failed("/status"),
                    groups: failed("/groups"),
                    snapshots: failed("/snapshots"),
                },
            )]),
        };
        assert_eq!(snapshot.failures().len(), 4);
    }
}
