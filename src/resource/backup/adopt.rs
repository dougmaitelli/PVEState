use crate::{
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::{BackupCollection, CapturedState},
    model::{PruneJob, SyncJob, VerifyJob},
    reconcile::{ApiMethod, Operation},
    resource::backup::{BackupMode, Datastore, PveBackupJob, Retention, S3Endpoint},
    utility::yaml_patch::Segment,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;

pub(crate) fn candidates(
    _local: &LocalState,
    captured: &CapturedState,
    operation: &Operation,
) -> Result<Vec<AdoptionCandidate>> {
    let Operation::ApiMutation {
        method,
        resource,
        changes,
        ..
    } = operation
    else {
        return Ok(Vec::new());
    };
    let name = resource.to_string();
    let patches = if let Some(id) = name.strip_prefix("pve/") {
        pve_job(captured, id)
    } else if let Some(id) = name.strip_prefix("prune/") {
        pbs_job(captured, "prune", id)
    } else if let Some(id) = name.strip_prefix("verify/") {
        pbs_job(captured, "verify", id)
    } else if let Some(id) = name.strip_prefix("sync/") {
        pbs_job(captured, "sync", id)
    } else if let Some(id) = name.strip_prefix("datastore/") {
        fixed_pbs_resource(captured, "datastore", id)
    } else if let Some(id) = name.strip_prefix("s3/") {
        fixed_pbs_resource(captured, "s3", id)
    } else {
        Ok(None)
    };
    let field = changes.keys().cloned().collect::<Vec<_>>().join(",");
    Ok(vec![match patches {
        Ok(Some(patches)) => AdoptionCandidate::adoptable(
            resource,
            if field.is_empty() {
                method_name(*method)
            } else {
                &field
            },
            "local backup state",
            "captured backup state",
            patches,
        ),
        Ok(None) => AdoptionCandidate::blocked(
            resource,
            if field.is_empty() {
                method_name(*method)
            } else {
                &field
            },
            "local backup state",
            "absent from captured state",
            "captured backup resource cannot be represented safely",
        ),
        Err(error) => AdoptionCandidate::blocked(
            resource,
            if field.is_empty() {
                method_name(*method)
            } else {
                &field
            },
            "local backup state",
            "invalid captured backup state",
            format!("captured backup resource cannot be represented safely: {error:#}"),
        ),
    }])
}

fn pve_job(captured: &CapturedState, id: &str) -> Result<Option<Vec<LocalPatch>>> {
    let actual = captured.backup.collection(BackupCollection::PveJobs);
    let item = find(actual, id)?;
    let resource_path = ["pve_backup_jobs", id];
    let resource = if let Some(item) = item {
        replace(&resource_path, parse_pve_job(item)?)?
    } else {
        remove(&resource_path)
    };
    Ok(Some(vec![
        resource,
        remove_absent(&["absent_pve_backup_jobs"], id),
    ]))
}

fn pbs_job(captured: &CapturedState, kind: &str, id: &str) -> Result<Option<Vec<LocalPatch>>> {
    let collection = match kind {
        "prune" => BackupCollection::PruneJobs,
        "verify" => BackupCollection::VerifyJobs,
        "sync" => BackupCollection::SyncJobs,
        _ => bail!("unsupported PBS job kind {kind}"),
    };
    let actual = captured.backup.collection(collection);
    let item = find(actual, id)?;
    let (resource, absent) = match kind {
        "prune" => (
            item.map(parse_prune)
                .transpose()?
                .map(|value| replace(&["pbs", "jobs", "prune", id], value))
                .transpose()?
                .unwrap_or_else(|| remove(&["pbs", "jobs", "prune", id])),
            "absent_prune",
        ),
        "verify" => (
            item.map(parse_verify)
                .transpose()?
                .map(|value| replace(&["pbs", "jobs", "verify", id], value))
                .transpose()?
                .unwrap_or_else(|| remove(&["pbs", "jobs", "verify", id])),
            "absent_verify",
        ),
        "sync" => (
            item.map(parse_sync)
                .transpose()?
                .map(|value| replace(&["pbs", "jobs", "sync", id], value))
                .transpose()?
                .unwrap_or_else(|| remove(&["pbs", "jobs", "sync", id])),
            "absent_sync",
        ),
        _ => bail!("unsupported PBS job kind {kind}"),
    };
    Ok(Some(vec![
        resource,
        remove_absent(&["pbs", "jobs", absent], id),
    ]))
}

fn fixed_pbs_resource(
    captured: &CapturedState,
    kind: &str,
    id: &str,
) -> Result<Option<Vec<LocalPatch>>> {
    let collection = match kind {
        "datastore" => BackupCollection::Datastores,
        "s3" => BackupCollection::S3Endpoints,
        _ => bail!("unsupported PBS resource kind {kind}"),
    };
    let actual = captured.backup.collection(collection);
    let item = find(actual, id)?;
    let patch = match (kind, item) {
        ("datastore", Some(item)) => replace(&["pbs", "datastore"], parse_datastore(item)?)?,
        ("s3", Some(item)) => replace(&["pbs", "s3_endpoint"], parse_s3(item)?)?,
        ("datastore", None) => remove(&["pbs", "datastore"]),
        ("s3", None) => remove(&["pbs", "s3_endpoint"]),
        _ => unreachable!(),
    };
    Ok(Some(vec![patch]))
}

fn parse_pve_job(value: &Value) -> Result<PveBackupJob> {
    let mode: BackupMode = serde_yaml::from_str(text(value, "mode")?)?;
    let keep_last = match value.get("prune-backups") {
        None | Some(Value::Null) => None,
        Some(Value::Object(options)) => options
            .get("keep-last")
            .map(|value| required_number_value(value, "prune-backups.keep-last"))
            .transpose()?,
        Some(Value::String(options)) => option(options, "keep-last")
            .map(|value| parse_u32(value, "prune-backups.keep-last"))
            .transpose()?,
        Some(_) => bail!("captured PVE field prune-backups has an invalid type"),
    };
    let guest_ids = text(value, "vmid")?
        .split(',')
        .map(|id| parse_u32(id, "vmid"))
        .collect::<Result<Vec<_>>>()?;
    if guest_ids.is_empty() {
        bail!("captured PVE field vmid is empty")
    }
    Ok(PveBackupJob {
        storage: text(value, "storage")?.into(),
        schedule: text(value, "schedule")?.into(),
        mode,
        guest_ids,
        retention: Retention { keep_last },
    })
}

fn parse_datastore(value: &Value) -> Result<Datastore> {
    let backend = text(value, "backend")?;
    let options = options(backend)?;
    let backend = options
        .get("type")
        .context("captured PBS backend field type")?;
    let backend = serde_yaml::from_str(backend)?;
    let (bucket, s3_endpoint_id) = match backend {
        crate::resource::backup::DatastoreBackend::Local => (None, None),
        crate::resource::backup::DatastoreBackend::S3 => (
            Some(required_option(&options, "bucket")?.into()),
            Some(required_option(&options, "client")?.into()),
        ),
    };
    Ok(Datastore {
        name: text(value, "name")?.into(),
        backend,
        local_cache_path: text(value, "path")?.into(),
        bucket,
        s3_endpoint_id,
        garbage_collection_schedule: text(value, "gc-schedule")?.into(),
    })
}

fn parse_s3(value: &Value) -> Result<S3Endpoint> {
    Ok(S3Endpoint {
        id: text(value, "id")?.into(),
        endpoint_template: text(value, "endpoint")?.into(),
        region: text(value, "region")?.into(),
    })
}

fn parse_prune(value: &Value) -> Result<PruneJob> {
    Ok(PruneJob {
        store: text(value, "store")?.into(),
        schedule: text(value, "schedule")?.into(),
        keep_last: number(value, "keep-last")?,
    })
}

fn parse_verify(value: &Value) -> Result<VerifyJob> {
    Ok(VerifyJob {
        store: text(value, "store")?.into(),
        schedule: text(value, "schedule")?.into(),
        ignore_verified: boolean(value, "ignore-verified")?,
        outdated_after_days: number(value, "outdated-after")?,
    })
}

fn parse_sync(value: &Value) -> Result<SyncJob> {
    let direction = string(value, "sync-direction")?
        .map(|value| serde_yaml::from_str(&value))
        .transpose()?;
    Ok(SyncJob {
        store: text(value, "store")?.into(),
        remote_store: text(value, "remote-store")?.into(),
        remote: string(value, "remote")?,
        schedule: string(value, "schedule")?,
        remove_vanished: boolean(value, "remove-vanished")?,
        direction,
    })
}

fn replace<T: serde::Serialize>(path: &[&str], value: T) -> Result<LocalPatch> {
    Ok(LocalPatch::ReplaceResource {
        document: ConfigDocument::Backup,
        path: path.iter().map(|key| Segment::Key((*key).into())).collect(),
        value: serde_yaml::to_value(value)?,
    })
}

fn remove(path: &[&str]) -> LocalPatch {
    LocalPatch::RemoveResource {
        document: ConfigDocument::Backup,
        path: path.iter().map(|key| Segment::Key((*key).into())).collect(),
    }
}

fn remove_absent(path: &[&str], id: &str) -> LocalPatch {
    LocalPatch::RemoveSequenceValue {
        document: ConfigDocument::Backup,
        path: path.iter().map(|key| Segment::Key((*key).into())).collect(),
        value: serde_yaml::Value::String(id.into()),
    }
}

fn find<'a>(
    items: &'a [crate::discovery::CapturedBackupResource],
    wanted: &str,
) -> Result<Option<&'a Value>> {
    let mut found = None;
    for item in items {
        if item.id == wanted && found.replace(item.raw()).is_some() {
            bail!("captured backup collection has duplicate identity `{wanted}`")
        }
    }
    Ok(found)
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("captured PBS field {key}"))
}

fn string(value: &Value, key: &str) -> Result<Option<String>> {
    value
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .with_context(|| format!("captured PBS field {key} is not a string"))
        })
        .transpose()
}

fn number(value: &Value, key: &str) -> Result<Option<u32>> {
    value
        .get(key)
        .map(|value| required_number_value(value, key))
        .transpose()
}

fn boolean(value: &Value, key: &str) -> Result<Option<bool>> {
    value
        .get(key)
        .map(|value| match value {
            Value::Bool(value) => Ok(*value),
            Value::Number(value) if value.as_u64() == Some(0) => Ok(false),
            Value::Number(value) if value.as_u64() == Some(1) => Ok(true),
            Value::String(value) if matches!(value.as_str(), "0" | "false") => Ok(false),
            Value::String(value) if matches!(value.as_str(), "1" | "true") => Ok(true),
            _ => bail!("captured PBS field {key} is not a boolean"),
        })
        .transpose()
}

fn option<'a>(value: &'a str, key: &str) -> Option<&'a str> {
    value
        .split(',')
        .filter_map(|part| part.split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn options(value: &str) -> Result<std::collections::BTreeMap<&str, &str>> {
    let mut result = std::collections::BTreeMap::new();
    for part in value.split(',') {
        let (key, value) = part
            .split_once('=')
            .with_context(|| format!("invalid PBS backend option `{part}`"))?;
        if key.is_empty() || value.is_empty() {
            bail!("invalid PBS backend option `{part}`")
        }
        if result.insert(key, value).is_some() {
            bail!("duplicate PBS backend option `{key}`")
        }
    }
    Ok(result)
}

fn required_option<'a>(
    options: &'a std::collections::BTreeMap<&str, &str>,
    key: &str,
) -> Result<&'a str> {
    options
        .get(key)
        .copied()
        .with_context(|| format!("captured PBS backend field {key}"))
}

fn required_number_value(value: &Value, key: &str) -> Result<u32> {
    if let Some(value) = value.as_u64() {
        return u32::try_from(value).with_context(|| format!("captured field {key} exceeds u32"));
    }
    parse_u32(
        value
            .as_str()
            .with_context(|| format!("captured field {key} is not a number"))?,
        key,
    )
}

fn parse_u32(value: &str, key: &str) -> Result<u32> {
    if value.is_empty() {
        bail!("captured field {key} is empty")
    }
    value
        .parse()
        .with_context(|| format!("captured field {key} has invalid number `{value}`"))
}

const fn method_name(method: ApiMethod) -> &'static str {
    match method {
        ApiMethod::Post => "create",
        ApiMethod::Put => "update",
        ApiMethod::Delete => "delete",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_captured_backup_jobs_into_local_types() {
        let job = parse_pve_job(&serde_json::json!({
            "storage": "pbs",
            "schedule": "daily",
            "mode": "snapshot",
            "vmid": "201,101",
            "prune-backups": {"keep-last": 3}
        }))
        .unwrap();
        assert_eq!(job.guest_ids, [201, 101]);
        assert_eq!(job.retention.keep_last, Some(3));

        let sync = parse_sync(&serde_json::json!({
            "store": "primary",
            "remote-store": "archive",
            "sync-direction": "push",
            "remove-vanished": true
        }))
        .unwrap();
        assert_eq!(
            sync.direction,
            Some(crate::resource::backup::SyncDirection::Push)
        );
        assert_eq!(sync.remove_vanished, Some(true));
    }

    #[test]
    fn rejects_invalid_vmids_and_malformed_booleans() {
        let invalid_job = serde_json::json!({
            "storage": "pbs",
            "schedule": "daily",
            "mode": "snapshot",
            "vmid": "201,not-a-vmid"
        });
        assert!(parse_pve_job(&invalid_job).is_err());

        let invalid_verify = serde_json::json!({
            "store": "primary",
            "schedule": "daily",
            "ignore-verified": "sometimes"
        });
        assert!(parse_verify(&invalid_verify).is_err());
    }

    #[test]
    fn preserves_missing_values_distinct_from_zero_and_false() {
        let missing = parse_pve_job(&serde_json::json!({
            "storage": "pbs",
            "schedule": "daily",
            "mode": "snapshot",
            "vmid": "201"
        }))
        .unwrap();
        let zero = parse_pve_job(&serde_json::json!({
            "storage": "pbs",
            "schedule": "daily",
            "mode": "snapshot",
            "vmid": "201",
            "prune-backups": {"keep-last": 0}
        }))
        .unwrap();
        assert_eq!(missing.retention.keep_last, None);
        assert_eq!(zero.retention.keep_last, Some(0));

        let verify = parse_verify(&serde_json::json!({
            "store": "primary",
            "schedule": "daily"
        }))
        .unwrap();
        assert_eq!(verify.ignore_verified, None);
        assert_eq!(verify.outdated_after_days, None);
    }

    #[test]
    fn requires_complete_s3_backend_but_accepts_unknown_upstream_fields() {
        for backend in [
            "type=s3",
            "type=s3,bucket=archive",
            "bucket=archive,client=s3",
        ] {
            let value = serde_json::json!({
                "name": "primary",
                "path": "/mnt/cache",
                "backend": backend,
                "gc-schedule": "daily"
            });
            assert!(parse_datastore(&value).is_err(), "{backend}");
        }

        let parsed = parse_datastore(&serde_json::json!({
            "name": "primary",
            "path": "/mnt/cache",
            "backend": "type=s3,bucket=archive,client=s3,future-option=value",
            "gc-schedule": "daily",
            "future-field": "accepted"
        }))
        .unwrap();
        assert_eq!(parsed.bucket.as_deref(), Some("archive"));
        assert_eq!(parsed.s3_endpoint_id.as_deref(), Some("s3"));
    }
}
