use crate::{
    command::plan::{ApiMethod, Operation},
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedState,
    model::{PruneJob, SyncJob, VerifyJob},
    resource::backup::{BackupMode, Datastore, PveBackupJob, Retention, S3Endpoint, SyncDirection},
    utility::yaml_patch::Segment,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;

pub(crate) fn candidates(
    local: &LocalState,
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
        pve_job(captured, id)?
    } else if let Some(id) = name.strip_prefix("prune/") {
        pbs_job(captured, "prune", id)?
    } else if let Some(id) = name.strip_prefix("verify/") {
        pbs_job(captured, "verify", id)?
    } else if let Some(id) = name.strip_prefix("sync/") {
        pbs_job(captured, "sync", id)?
    } else if name.starts_with("datastore/") {
        fixed_pbs_resource(local, captured, "datastore")?
    } else if name.starts_with("s3/") {
        fixed_pbs_resource(local, captured, "s3")?
    } else {
        None
    };
    let field = changes.keys().cloned().collect::<Vec<_>>().join(",");
    Ok(vec![match patches {
        Some(patches) => AdoptionCandidate::adoptable(
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
        None => AdoptionCandidate::blocked(
            resource,
            if field.is_empty() {
                method_name(*method)
            } else {
                &field
            },
            "local backup state",
            "absent from captured state",
            "this required singleton cannot be removed from the local schema",
        ),
    }])
}

fn pve_job(captured: &CapturedState, id: &str) -> Result<Option<Vec<LocalPatch>>> {
    let actual = captured.pve.response("/cluster/backup")?;
    let item = find(&actual, "id", id);
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
    let actual = captured.pbs.response(&format!("/config/{kind}"))?;
    let item = find(&actual, "id", id);
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
    local: &LocalState,
    captured: &CapturedState,
    kind: &str,
) -> Result<Option<Vec<LocalPatch>>> {
    let (path, key, id) = match kind {
        "datastore" => (
            "/config/datastore",
            "name",
            local.backup.pbs.datastore.name.as_str(),
        ),
        "s3" => ("/config/s3", "id", local.backup.pbs.s3_endpoint.id.as_str()),
        _ => bail!("unsupported PBS resource kind {kind}"),
    };
    let actual = captured.pbs.response(path)?;
    let Some(item) = find(&actual, key, id) else {
        return Ok(None);
    };
    let patch = match kind {
        "datastore" => replace(&["pbs", "datastore"], parse_datastore(item, local)?)?,
        "s3" => replace(&["pbs", "s3_endpoint"], parse_s3(item)?)?,
        _ => unreachable!(),
    };
    Ok(Some(vec![patch]))
}

fn parse_pve_job(value: &Value) -> Result<PveBackupJob> {
    let mode: BackupMode = serde_yaml::from_str(text(value, "mode")?)?;
    let keep_last = value
        .get("prune-backups")
        .and_then(|v| v.get("keep-last"))
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get("prune-backups")
                .and_then(Value::as_str)
                .and_then(|v| option(v, "keep-last"))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0) as u32;
    Ok(PveBackupJob {
        storage: text(value, "storage")?.into(),
        schedule: text(value, "schedule")?.into(),
        mode,
        guest_ids: text(value, "vmid")?
            .split(',')
            .filter_map(|id| id.parse().ok())
            .collect(),
        retention: Retention { keep_last },
    })
}

fn parse_datastore(value: &Value, local: &LocalState) -> Result<Datastore> {
    let backend = text(value, "backend")?;
    let options = options(backend);
    Ok(Datastore {
        name: text(value, "name")?.into(),
        backend: serde_yaml::from_str(options.get("type").copied().unwrap_or("local"))?,
        local_cache_path: text(value, "path")?.into(),
        bucket: options
            .get("bucket")
            .copied()
            .unwrap_or(&local.backup.pbs.datastore.bucket)
            .into(),
        s3_endpoint_id: options
            .get("client")
            .copied()
            .unwrap_or(&local.backup.pbs.datastore.s3_endpoint_id)
            .into(),
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
        keep_last: number(value, "keep-last"),
    })
}

fn parse_verify(value: &Value) -> Result<VerifyJob> {
    Ok(VerifyJob {
        store: text(value, "store")?.into(),
        schedule: text(value, "schedule")?.into(),
        ignore_verified: boolean(value, "ignore-verified"),
        outdated_after_days: number(value, "outdated-after").unwrap_or(0),
    })
}

fn parse_sync(value: &Value) -> Result<SyncJob> {
    let direction: SyncDirection = serde_yaml::from_str(
        value
            .get("sync-direction")
            .and_then(Value::as_str)
            .unwrap_or("pull"),
    )?;
    Ok(SyncJob {
        store: text(value, "store")?.into(),
        remote_store: text(value, "remote-store")?.into(),
        remote: string(value, "remote"),
        schedule: string(value, "schedule"),
        remove_vanished: boolean(value, "remove-vanished"),
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

fn find<'a>(items: &'a Value, key: &str, wanted: &str) -> Option<&'a Value> {
    items
        .as_array()?
        .iter()
        .find(|item| item.get(key).and_then(Value::as_str) == Some(wanted))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("captured PBS field {key}"))
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn number(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .map(|v| v as u32)
}

fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(|v| {
        v.as_bool()
            .unwrap_or_else(|| matches!(v.as_str(), Some("1" | "true")))
    })
}

fn option<'a>(value: &'a str, key: &str) -> Option<&'a str> {
    value
        .split(',')
        .filter_map(|part| part.split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn options(value: &str) -> std::collections::BTreeMap<&str, &str> {
    value
        .split(',')
        .filter_map(|part| part.split_once('='))
        .collect()
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
        assert_eq!(job.retention.keep_last, 3);

        let sync = parse_sync(&serde_json::json!({
            "store": "primary",
            "remote-store": "archive",
            "sync-direction": "push",
            "remove-vanished": true
        }))
        .unwrap();
        assert_eq!(sync.direction, SyncDirection::Push);
        assert!(sync.remove_vanished);
    }
}
