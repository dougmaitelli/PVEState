use crate::{
    client::{PbsClient, PveClient},
    command::plan::{ApiMethod, ApiTarget, Operation},
    config::LocalState,
    model::{PruneJob, SyncJob, VerifyJob},
};
use anyhow::{Context, Result};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn plan(
    repo: &LocalState,
    pve: &dyn PveClient,
    pbs: &dyn PbsClient,
    operations: &mut Vec<Operation>,
    blockers: &mut Vec<String>,
) -> Result<()> {
    pve_jobs(repo, &pve.get("/cluster/backup")?, operations)?;
    let datastores = pbs.get("/config/datastore")?;
    let s3 = pbs.get("/config/s3")?;
    let prune = pbs.get("/config/prune")?;
    let verify = pbs.get("/config/verify")?;
    let sync = pbs.get("/config/sync")?;

    datastore(repo, &datastores, operations, blockers)?;
    s3_endpoint(repo, &s3, operations)?;
    prune_jobs(repo, &prune, operations)?;
    verify_jobs(repo, &verify, operations)?;
    sync_jobs(repo, &sync, operations)?;
    Ok(())
}

fn pve_jobs(repo: &LocalState, actual: &Value, operations: &mut Vec<Operation>) -> Result<()> {
    for (id, desired) in &repo.backup.pve_backup_jobs {
        let current = find(actual, "id", id);
        let mut changes: BTreeMap<String, String> = BTreeMap::from([
            ("storage".into(), desired.storage.clone()),
            ("schedule".into(), desired.schedule.clone()),
            ("mode".into(), desired.mode.to_string()),
            (
                "vmid".into(),
                desired
                    .guest_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            (
                "prune-backups".into(),
                format!("keep-last={}", desired.retention.keep_last),
            ),
        ]);
        let method = if let Some(current) = current {
            changes.retain(|key, wanted| !same_pve_job_value(current, key, wanted));
            ApiMethod::Put
        } else {
            changes.insert("id".into(), id.clone());
            changes.insert("node".into(), repo.guests.node.clone());
            ApiMethod::Post
        };
        if !changes.is_empty() {
            push(
                operations,
                ApiTarget::Pve,
                method,
                crate::command::plan::Domain::Backup,
                &format!("pve/{id}"),
                if method == ApiMethod::Post {
                    "/cluster/backup".into()
                } else {
                    format!("/cluster/backup/{}", encoded(id))
                },
                changes,
            );
        }
    }
    for id in &repo.backup.absent_pve_backup_jobs {
        if find(actual, "id", id).is_some() {
            push(
                operations,
                ApiTarget::Pve,
                ApiMethod::Delete,
                crate::command::plan::Domain::Backup,
                &format!("pve/{id}"),
                format!("/cluster/backup/{}", encoded(id)),
                BTreeMap::new(),
            );
        }
    }
    Ok(())
}

fn datastore(
    repo: &LocalState,
    actual: &Value,
    operations: &mut Vec<Operation>,
    blockers: &mut Vec<String>,
) -> Result<()> {
    let desired = &repo.backup.pbs.datastore;
    let wanted_backend = format!(
        "type={},client={},bucket={}",
        desired.backend, desired.s3_endpoint_id, desired.bucket
    );
    let Some(current) = find(actual, "name", &desired.name) else {
        push(
            operations,
            ApiTarget::Pbs,
            ApiMethod::Post,
            crate::command::plan::Domain::Pbs,
            &format!("datastore/{}", desired.name),
            "/config/datastore".into(),
            BTreeMap::from([
                ("name".into(), desired.name.clone()),
                ("path".into(), desired.local_cache_path.clone()),
                ("backend".into(), wanted_backend),
                (
                    "gc-schedule".into(),
                    desired.garbage_collection_schedule.clone(),
                ),
            ]),
        );
        return Ok(());
    };
    if normalize_options(text(current, "backend")?) != normalize_options(&wanted_backend) {
        blockers.push(format!(
            "pbs datastore {} backend changes require datastore migration",
            desired.name
        ));
    }
    if text(current, "path")? != desired.local_cache_path {
        blockers.push(format!(
            "pbs datastore {} cache path changes require datastore migration",
            desired.name
        ));
    }
    let mut changes = BTreeMap::new();
    compare_text(
        &mut changes,
        current,
        "gc-schedule",
        &desired.garbage_collection_schedule,
    );
    if !changes.is_empty() {
        push(
            operations,
            ApiTarget::Pbs,
            ApiMethod::Put,
            crate::command::plan::Domain::Pbs,
            &format!("datastore/{}", desired.name),
            format!("/config/datastore/{}", encoded(&desired.name)),
            changes,
        );
    }
    Ok(())
}

fn s3_endpoint(repo: &LocalState, actual: &Value, operations: &mut Vec<Operation>) -> Result<()> {
    let desired = &repo.backup.pbs.s3_endpoint;
    let Some(current) = find(actual, "id", &desired.id) else {
        operations.push(Operation::ApiMutation {
            target: ApiTarget::Pbs,
            method: ApiMethod::Post,
            domain: crate::command::plan::Domain::Pbs,
            resource: format!("s3/{}", desired.id).into(),
            endpoint: "/config/s3".into(),
            changes: BTreeMap::from([
                ("id".into(), desired.id.clone()),
                ("endpoint".into(), desired.endpoint_template.clone()),
                ("region".into(), desired.region.clone()),
            ]),
            environment_changes: BTreeMap::from([
                (
                    "access-key".into(),
                    crate::settings::env::PBS_APPLY_S3_ACCESS_KEY.into(),
                ),
                (
                    "secret-key".into(),
                    crate::settings::env::PBS_APPLY_S3_SECRET_KEY.into(),
                ),
            ]),
            digest: None,
        });
        return Ok(());
    };
    let mut changes = BTreeMap::new();
    compare_text(
        &mut changes,
        current,
        "endpoint",
        &desired.endpoint_template,
    );
    compare_text(&mut changes, current, "region", &desired.region);
    if !changes.is_empty() {
        push(
            operations,
            ApiTarget::Pbs,
            ApiMethod::Put,
            crate::command::plan::Domain::Pbs,
            &format!("s3/{}", desired.id),
            format!("/config/s3/{}", encoded(&desired.id)),
            changes,
        );
    }
    Ok(())
}

fn prune_jobs(repo: &LocalState, actual: &Value, operations: &mut Vec<Operation>) -> Result<()> {
    for (id, desired) in &repo.backup.pbs.jobs.prune {
        reconcile_job(actual, id, "/config/prune", prune_data(desired), operations);
    }
    absent_jobs(
        actual,
        &repo.backup.pbs.jobs.absent_prune,
        "/config/prune",
        operations,
    );
    Ok(())
}

fn verify_jobs(repo: &LocalState, actual: &Value, operations: &mut Vec<Operation>) -> Result<()> {
    for (id, desired) in &repo.backup.pbs.jobs.verify {
        reconcile_job(
            actual,
            id,
            "/config/verify",
            verify_data(desired),
            operations,
        );
    }
    absent_jobs(
        actual,
        &repo.backup.pbs.jobs.absent_verify,
        "/config/verify",
        operations,
    );
    Ok(())
}

fn sync_jobs(repo: &LocalState, actual: &Value, operations: &mut Vec<Operation>) -> Result<()> {
    for (id, desired) in &repo.backup.pbs.jobs.sync {
        reconcile_job(actual, id, "/config/sync", sync_data(desired), operations);
    }
    absent_jobs(
        actual,
        &repo.backup.pbs.jobs.absent_sync,
        "/config/sync",
        operations,
    );
    Ok(())
}

fn reconcile_job(
    actual: &Value,
    id: &str,
    root: &str,
    mut wanted: BTreeMap<String, String>,
    operations: &mut Vec<Operation>,
) {
    let method = if let Some(current) = find(actual, "id", id) {
        wanted.retain(|key, value| current.get(key).map(value_string).as_deref() != Some(value));
        ApiMethod::Put
    } else {
        wanted.insert("id".into(), id.into());
        ApiMethod::Post
    };
    if !wanted.is_empty() {
        push(
            operations,
            ApiTarget::Pbs,
            method,
            crate::command::plan::Domain::Pbs,
            &format!("{}/{id}", root.trim_start_matches("/config/")),
            if method == ApiMethod::Post {
                root.into()
            } else {
                format!("{root}/{}", encoded(id))
            },
            wanted,
        );
    }
}

fn absent_jobs(actual: &Value, absent: &[String], root: &str, operations: &mut Vec<Operation>) {
    for id in absent {
        if find(actual, "id", id).is_some() {
            push(
                operations,
                ApiTarget::Pbs,
                ApiMethod::Delete,
                crate::command::plan::Domain::Pbs,
                &format!("{}/{id}", root.trim_start_matches("/config/")),
                format!("{root}/{}", encoded(id)),
                BTreeMap::new(),
            );
        }
    }
}

fn prune_data(job: &PruneJob) -> BTreeMap<String, String> {
    let mut data = BTreeMap::from([
        ("store".into(), job.store.clone()),
        ("schedule".into(), job.schedule.clone()),
    ]);
    if let Some(value) = job.keep_last {
        data.insert("keep-last".into(), value.to_string());
    }
    data
}

fn verify_data(job: &VerifyJob) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("store".into(), job.store.clone()),
        ("schedule".into(), job.schedule.clone()),
        ("ignore-verified".into(), job.ignore_verified.to_string()),
        ("outdated-after".into(), job.outdated_after_days.to_string()),
    ])
}

fn sync_data(job: &SyncJob) -> BTreeMap<String, String> {
    let mut data = BTreeMap::from([
        ("store".into(), job.store.clone()),
        ("remote-store".into(), job.remote_store.clone()),
        ("remove-vanished".into(), job.remove_vanished.to_string()),
        ("sync-direction".into(), job.direction.to_string()),
    ]);
    if let Some(value) = &job.remote {
        data.insert("remote".into(), value.clone());
    }
    if let Some(value) = &job.schedule {
        data.insert("schedule".into(), value.clone());
    }
    data
}

fn push(
    operations: &mut Vec<Operation>,
    target: ApiTarget,
    method: ApiMethod,
    domain: crate::command::plan::Domain,
    resource: &str,
    endpoint: String,
    changes: BTreeMap<String, String>,
) {
    operations.push(Operation::ApiMutation {
        target,
        method,
        domain,
        resource: resource.into(),
        endpoint: endpoint.into(),
        changes,
        environment_changes: BTreeMap::new(),
        digest: None,
    });
}

fn find<'a>(items: &'a Value, key: &str, value: &str) -> Option<&'a Value> {
    items
        .as_array()?
        .iter()
        .find(|item| item.get(key).and_then(Value::as_str) == Some(value))
}

fn compare_text(changes: &mut BTreeMap<String, String>, actual: &Value, key: &str, wanted: &str) {
    if actual.get(key).map(value_string).as_deref() != Some(wanted) {
        changes.insert(key.into(), wanted.into());
    }
}

fn same_pve_job_value(actual: &Value, key: &str, wanted: &str) -> bool {
    if key == "prune-backups" {
        return actual
            .get(key)
            .and_then(Value::as_object)
            .and_then(|value| value.get("keep-last"))
            .map(value_string)
            .as_deref()
            == wanted.strip_prefix("keep-last=");
    }
    if key == "vmid" {
        let mut have = actual[key]
            .as_str()
            .unwrap_or("")
            .split(',')
            .collect::<Vec<_>>();
        let mut want = wanted.split(',').collect::<Vec<_>>();
        have.sort_unstable();
        want.sort_unstable();
        return have == want;
    }
    actual.get(key).map(value_string).as_deref() == Some(wanted)
}

fn text<'a>(actual: &'a Value, key: &str) -> Result<&'a str> {
    actual
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("PBS field {key}"))
}

fn value_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn normalize_options(value: &str) -> BTreeMap<&str, &str> {
    value
        .split(',')
        .filter_map(|item| item.split_once('='))
        .collect()
}

fn encoded(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pve_job_guest_order_is_semantic() {
        let actual = serde_json::json!({"vmid":"102,101"});
        assert!(same_pve_job_value(&actual, "vmid", "101,102"));
    }

    #[test]
    fn backend_option_order_is_semantic() {
        assert_eq!(
            normalize_options("bucket=b,client=c,type=s3"),
            normalize_options("type=s3,client=c,bucket=b")
        );
    }
}
