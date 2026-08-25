mod host;
mod native;

use crate::{
    client::{Pbs, Pve, Ssh},
    config::Repository,
    discovery::{capture_pve, write_snapshot},
};
use anyhow::{Result, bail};
use chrono::Utc;
use serde_json::json;
use std::fs;

pub fn run(repo: &Repository) -> Result<()> {
    let pve = Pve::discovery()?;
    let pbs = Pbs::discovery()?;
    let ssh = Ssh::discovery(&repo.root)?;

    fs::create_dir_all(repo.runtime())?;
    fs::create_dir_all(repo.observed().join("api"))?;

    let pve_snapshot = capture_pve(&pve);
    write_snapshot(
        "pve",
        pve_snapshot.collected_at,
        &pve_snapshot,
        &repo.runtime(),
        &repo.observed().join("api"),
    )?;
    report_failures("PVE", pve_snapshot.failures());

    let pbs_snapshot = pbs.discover();
    write_snapshot(
        "pbs",
        pbs_snapshot.collected_at,
        &pbs_snapshot,
        &repo.runtime(),
        &repo.observed().join("api"),
    )?;
    report_failures("PBS", pbs_snapshot.failures());

    native::export(repo, &ssh, &pve_snapshot)?;
    host::capture(repo, &ssh)?;

    let manifest = json!({
        "schema_version": 1,
        "exported_at": Utc::now(),
        "source": pve.endpoint(),
        "scope": "complete PVE/PBS API inventory, native guest configuration, DNS, network, storage, backup jobs, and cluster/node/guest firewalls"
    });
    fs::write(
        repo.observed().join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("captured production into {}", repo.root.display());
    Ok(())
}

pub fn validate(repo: &Repository) -> Result<()> {
    let ssh = Ssh::discovery(&repo.root)?;
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

fn report_failures(system: &str, failures: Vec<String>) {
    if failures.is_empty() {
        return;
    }
    eprintln!("some {system} discovery requests were unavailable:");
    for failure in failures {
        eprintln!("  - {failure}");
    }
}
