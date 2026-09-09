use crate::model::{GuestKind, GuestRef};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub(crate) struct CapturedGuest {
    pub(crate) reference: GuestRef,
    pub(crate) name: String,
    pub(crate) cores: u16,
    pub(crate) memory_mb: u32,
    config: Value,
}

impl CapturedGuest {
    pub(crate) fn decode(reference: GuestRef, config: &Value, path: &str) -> Result<Self> {
        let object = require_object(config, path)?;
        let name_field = match reference.kind {
            GuestKind::Lxc => "hostname",
            GuestKind::Qemu => "name",
        };
        let cores = required_u32(object, "cores", path)?;
        Ok(Self {
            reference,
            name: required_string(object, name_field, path)?,
            cores: u16::try_from(cores).with_context(|| {
                format!("decode managed response {path}: field cores exceeds u16")
            })?,
            memory_mb: required_u32(object, "memory", path)?,
            config: config.clone(),
        })
    }

    pub(crate) fn config(&self) -> &Value {
        &self.config
    }
}

#[derive(Debug)]
pub(crate) struct CapturedDns {
    pub(crate) search: Option<String>,
    pub(crate) servers: BTreeMap<u8, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BackupCollection {
    PveJobs,
    Datastores,
    S3Endpoints,
    PruneJobs,
    VerifyJobs,
    SyncJobs,
}

#[derive(Debug)]
pub(crate) struct CapturedBackup {
    collections: BTreeMap<BackupCollection, Vec<CapturedBackupResource>>,
}

#[derive(Debug)]
pub(crate) struct CapturedBackupResource {
    pub(crate) id: String,
    raw: Value,
}

impl CapturedBackupResource {
    pub(crate) fn raw(&self) -> &Value {
        &self.raw
    }
}

impl CapturedBackup {
    pub(crate) fn collection(&self, kind: BackupCollection) -> &[CapturedBackupResource] {
        self.collections
            .get(&kind)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[derive(Debug)]
pub(crate) struct ManagedViews {
    pub(crate) guests: BTreeMap<GuestRef, CapturedGuest>,
    pub(crate) dns: BTreeMap<String, CapturedDns>,
    pub(crate) backup: CapturedBackup,
}

pub(crate) fn decode(
    pve: &BTreeMap<String, Value>,
    pbs: &BTreeMap<String, Value>,
) -> Result<ManagedViews> {
    let mut guests = BTreeMap::new();
    let mut dns = BTreeMap::new();
    for (path, value) in pve {
        if let Some(reference) = guest_config_reference(path) {
            guests.insert(reference, CapturedGuest::decode(reference, value, path)?);
        } else if let Some(node) = node_dns(path) {
            dns.insert(node.into(), decode_dns(value, path)?);
        }
    }

    let specs = [
        (BackupCollection::PveJobs, pve, "/cluster/backup", "id"),
        (
            BackupCollection::Datastores,
            pbs,
            "/config/datastore",
            "name",
        ),
        (BackupCollection::S3Endpoints, pbs, "/config/s3", "id"),
        (BackupCollection::PruneJobs, pbs, "/config/prune", "id"),
        (BackupCollection::VerifyJobs, pbs, "/config/verify", "id"),
        (BackupCollection::SyncJobs, pbs, "/config/sync", "id"),
    ];
    let mut collections = BTreeMap::new();
    for (kind, source, path, identity) in specs {
        let value = source
            .get(path)
            .with_context(|| format!("decode managed response {path}: response is missing"))?;
        collections.insert(kind, decode_collection(value, path, identity)?);
    }
    Ok(ManagedViews {
        guests,
        dns,
        backup: CapturedBackup { collections },
    })
}

fn decode_dns(value: &Value, path: &str) -> Result<CapturedDns> {
    let object = require_object(value, path)?;
    let search = optional_string(object, "search", path)?;
    let mut servers = BTreeMap::new();
    for (key, value) in object {
        let Some(index) = key.strip_prefix("dns").and_then(|value| value.parse().ok()) else {
            continue;
        };
        let server = value.as_str().with_context(|| {
            format!("decode managed response {path}: field {key} is not a string")
        })?;
        servers.insert(index, server.into());
    }
    Ok(CapturedDns { search, servers })
}

fn decode_collection(
    value: &Value,
    path: &str,
    identity: &str,
) -> Result<Vec<CapturedBackupResource>> {
    let mut identities = BTreeSet::new();
    value
        .as_array()
        .with_context(|| format!("decode managed response {path}: expected an array"))?
        .iter()
        .map(|item| {
            let object = require_object(item, path)?;
            let id = object
                .get(identity)
                .and_then(Value::as_str)
                .with_context(|| {
                    format!("decode managed response {path}: missing string field {identity}")
                })?;
            if !identities.insert(id) {
                bail!("decode managed response {path}: duplicate identity `{id}`")
            }
            Ok(CapturedBackupResource {
                id: id.into(),
                raw: item.clone(),
            })
        })
        .collect()
}

fn require_object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .with_context(|| format!("decode managed response {path}: expected an object"))
}

fn optional_string(object: &Map<String, Value>, key: &str, path: &str) -> Result<Option<String>> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!("decode managed response {path}: field {key} is not a string"),
    }
}

fn required_string(object: &Map<String, Value>, key: &str, path: &str) -> Result<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .with_context(|| format!("decode managed response {path}: missing string field {key}"))
}

fn required_u32(object: &Map<String, Value>, key: &str, path: &str) -> Result<u32> {
    let value = object
        .get(key)
        .with_context(|| format!("decode managed response {path}: missing field {key}"))?;
    let value = value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .with_context(|| {
            format!("decode managed response {path}: field {key} is not an integer")
        })?;
    u32::try_from(value)
        .with_context(|| format!("decode managed response {path}: field {key} exceeds u32"))
}

fn guest_config_reference(path: &str) -> Option<GuestRef> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    (parts.len() == 5 && parts[0] == "nodes" && parts[4] == "config")
        .then(|| format!("{}/{}", parts[2], parts[3]).parse().ok())
        .flatten()
}

fn node_dns(path: &str) -> Option<&str> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["nodes", node, "dns"] => Some(node),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_payloads_decode_into_typed_managed_views() {
        let pve = BTreeMap::from([
            (
                "/nodes/pve/qemu/201/config".into(),
                serde_json::json!({"name":"home", "cores":2, "memory":4096, "unknown-future-field": true}),
            ),
            (
                "/nodes/pve/dns".into(),
                serde_json::json!({"search":"example.test", "dns1":"1.1.1.1", "extra": 1}),
            ),
            (
                "/cluster/backup".into(),
                serde_json::json!([{"id":"daily"}]),
            ),
        ]);
        let pbs = BTreeMap::from([
            (
                "/config/datastore".into(),
                serde_json::json!([{"name":"backup"}]),
            ),
            ("/config/s3".into(), serde_json::json!([])),
            ("/config/prune".into(), serde_json::json!([])),
            ("/config/verify".into(), serde_json::json!([])),
            ("/config/sync".into(), serde_json::json!([])),
        ]);

        let managed = decode(&pve, &pbs).unwrap();

        assert_eq!(managed.guests.len(), 1);
        assert_eq!(managed.dns["pve"].search.as_deref(), Some("example.test"));
        assert_eq!(managed.dns["pve"].servers[&1], "1.1.1.1");
        assert_eq!(
            managed.backup.collection(BackupCollection::PveJobs)[0].id,
            "daily"
        );
    }

    #[test]
    fn mandatory_fields_fail_with_endpoint_context() {
        let pve = BTreeMap::from([("/cluster/backup".into(), serde_json::json!([{}]))]);
        let error = decode(&pve, &BTreeMap::new()).unwrap_err();

        assert!(error.to_string().contains("/cluster/backup"));
        assert!(error.to_string().contains("missing string field id"));
    }

    #[test]
    fn missing_guest_fields_fail_with_endpoint_context() {
        let path = "/nodes/pve/lxc/101/config";
        let error = CapturedGuest::decode(
            GuestRef::new(GuestKind::Lxc, 101),
            &serde_json::json!({"hostname":"apps", "memory":2048}),
            path,
        )
        .unwrap_err();

        assert!(error.to_string().contains(path));
        assert!(error.to_string().contains("cores"));
    }

    #[test]
    fn unrelated_short_paths_do_not_panic_during_managed_decoding() {
        for path in ["/", "/version", "/nodes", "/nodes/pve"] {
            assert_eq!(node_dns(path), None);
        }
    }

    #[test]
    fn building_managed_views_does_not_change_raw_evidence() {
        let (pve, pbs) = fixture_payloads();
        let original_pve = pve.clone();
        let original_pbs = pbs.clone();

        decode(&pve, &pbs).unwrap();

        assert_eq!(pve, original_pve);
        assert_eq!(pbs, original_pbs);
    }

    fn fixture_payloads() -> (BTreeMap<String, Value>, BTreeMap<String, Value>) {
        (
            BTreeMap::from([("/cluster/backup".into(), serde_json::json!([]))]),
            BTreeMap::from([
                ("/config/datastore".into(), serde_json::json!([])),
                ("/config/s3".into(), serde_json::json!([])),
                ("/config/prune".into(), serde_json::json!([])),
                ("/config/verify".into(), serde_json::json!([])),
                ("/config/sync".into(), serde_json::json!([])),
            ]),
        )
    }
}
