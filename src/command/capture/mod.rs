mod host;
mod native;

use crate::{
    client::{PbsClient, PveClient, RemoteHost, capture_pbs},
    config::Repository,
    discovery::{
        CaptureManifest, CaptureStatus, SourceEvidence, capture_pve, collect_artifacts,
        write_snapshot,
    },
};
use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use serde_json::json;
use std::{collections::BTreeMap, fs};

pub fn run(
    repo: &Repository,
    pve: &dyn PveClient,
    pbs: &dyn PbsClient,
    ssh: &dyn RemoteHost,
) -> Result<()> {
    fs::create_dir_all(repo.runtime())?;
    fs::create_dir_all(repo.observed().join("api"))?;

    let started = Utc::now();
    let initial = CaptureManifest::new(
        started,
        BTreeMap::from([(
            "capture".into(),
            source("local", vec!["capture did not complete".into()]),
        )]),
        BTreeMap::new(),
    );
    write_manifest(repo, &initial)?;

    let sources = match perform(repo, pve, pbs, ssh) {
        Ok(sources) => sources,
        Err(error) => {
            let failed = CaptureManifest::new(
                Utc::now(),
                BTreeMap::from([(
                    "capture".into(),
                    source("local", vec![format!("{error:#}")]),
                )]),
                collect_artifacts(&repo.observed())?,
            );
            write_manifest(repo, &failed)?;
            return Err(error);
        },
    };

    let manifest = CaptureManifest::new(Utc::now(), sources, collect_artifacts(&repo.observed())?);
    write_manifest(repo, &manifest)?;
    if manifest.status == CaptureStatus::Partial {
        bail!("capture is partial: {}", manifest.failures.join("; "))
    }
    println!("captured production into {}", repo.root.display());
    Ok(())
}

fn perform(
    repo: &Repository,
    pve: &dyn PveClient,
    pbs: &dyn PbsClient,
    ssh: &dyn RemoteHost,
) -> Result<BTreeMap<String, SourceEvidence>> {
    let mut sources = BTreeMap::new();

    let pve_snapshot = capture_pve(pve);
    write_snapshot(
        "pve",
        pve_snapshot.collected_at,
        &pve_snapshot,
        &repo.runtime(),
        &repo.observed().join("api"),
    )?;
    sources.insert(
        "pve-api".into(),
        source(pve.endpoint(), pve_snapshot.failures()),
    );

    let pbs_snapshot = capture_pbs(pbs);
    write_snapshot(
        "pbs",
        pbs_snapshot.collected_at,
        &pbs_snapshot,
        &repo.runtime(),
        &repo.observed().join("api"),
    )?;
    sources.insert(
        "pbs-api".into(),
        source(pbs.endpoint(), pbs_snapshot.failures()),
    );

    let native_failures = native::export(repo, ssh, &pve_snapshot)
        .err()
        .map(|error| vec![format!("{error:#}")])
        .unwrap_or_default();
    sources.insert("native-ssh".into(), source(pve.endpoint(), native_failures));

    let host_failures =
        host::capture(repo, ssh).map_err(|error| anyhow!("host evidence capture: {error:#}"))?;
    sources.insert("host-ssh".into(), source(pve.endpoint(), host_failures));
    Ok(sources)
}

fn source(endpoint: &str, failures: Vec<String>) -> SourceEvidence {
    SourceEvidence {
        endpoint: endpoint.into(),
        required: true,
        complete: failures.is_empty(),
        failures,
    }
}

fn write_manifest(repo: &Repository, manifest: &CaptureManifest) -> Result<()> {
    let content = serde_json::to_vec_pretty(manifest)?;
    fs::write(repo.observed().join("manifest.json"), &content)?;
    fs::write(
        repo.runtime()
            .join(format!("capture-manifest-{}.json", manifest.capture_id)),
        content,
    )?;
    Ok(())
}

pub fn validate(repo: &Repository, ssh: &dyn RemoteHost) -> Result<()> {
    let mut failures = Vec::new();
    let mut report = Vec::new();
    for check in &repo.recovery_checks.checks {
        match ssh.run(&check.command) {
            Ok(_) => report
                .push(json!({"id":check.id,"description":check.description,"status":"passed"})),
            Err(error) => {
                failures.push(format!("{}: {error}", check.id));
                report
                    .push(json!({"id":check.id,"description":check.description,"status":"failed"}));
            },
        }
    }
    fs::create_dir_all(repo.runtime())?;
    fs::write(
        repo.runtime().join("validation.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if failures.is_empty() {
        println!("validation passed: {} checks", report.len());
        Ok(())
    } else {
        bail!(failures.join("\n"))
    }
}
