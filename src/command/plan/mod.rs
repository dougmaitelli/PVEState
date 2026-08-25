mod file;
mod guest;
mod validation;

use crate::{client::Pve, config::Repository, render};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Operation {
    ApiUpdate {
        domain: String,
        resource: String,
        endpoint: String,
        changes: BTreeMap<String, String>,
        digest: Option<String>,
    },
    GrowDisk {
        domain: String,
        resource: String,
        endpoint: String,
        disk: String,
        size_gb: u64,
    },
    WriteFile {
        domain: String,
        resource: String,
        path: String,
        content: String,
        before_sha256: String,
        activate: bool,
    },
}

impl Operation {
    pub fn domain(&self) -> &str {
        match self {
            Self::ApiUpdate { domain, .. }
            | Self::GrowDisk { domain, .. }
            | Self::WriteFile { domain, .. } => domain,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub schema_version: u8,
    pub created_at: DateTime<Utc>,
    pub target: String,
    pub operations: Vec<Operation>,
    pub blockers: Vec<String>,
    pub plan_sha256: String,
}

impl Plan {
    pub fn calculate_hash(&self) -> Result<String> {
        let mut unsigned = self.clone();
        unsigned.plan_sha256.clear();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&unsigned)?)))
    }

    pub fn verify(&self) -> Result<()> {
        if self.calculate_hash()? != self.plan_sha256 {
            bail!("plan file integrity check failed")
        }
        Ok(())
    }
}

pub fn run(repo: &Repository) -> Result<Plan> {
    validation::validate(repo)?;
    ensure_observations_are_fresh(repo)?;
    let api = Pve::discovery()?;
    let mut operations = Vec::new();
    let mut blockers = Vec::new();

    for (id, desired) in &repo.guests.lxcs {
        let actual = api.get(&format!("/nodes/{}/lxc/{id}/config", repo.guests.node))?;
        guest::lxc(
            &repo.guests.node,
            *id,
            desired,
            &actual,
            &mut operations,
            &mut blockers,
        )?;
    }
    for (id, desired) in &repo.guests.vms {
        let actual = api.get(&format!("/nodes/{}/qemu/{id}/config", repo.guests.node))?;
        guest::vm(
            &repo.guests.node,
            *id,
            desired,
            &actual,
            &mut operations,
            &mut blockers,
        )?;
    }

    let dns = api.get(&format!("/nodes/{}/dns", repo.guests.node))?;
    let mut changes = BTreeMap::new();
    compare(&mut changes, "search", &repo.network.dns.search, &dns);
    for (index, server) in repo.network.dns.servers.iter().enumerate() {
        compare(&mut changes, &format!("dns{}", index + 1), server, &dns);
    }
    if !changes.is_empty() {
        operations.push(Operation::ApiUpdate {
            domain: "dns".into(),
            resource: repo.guests.node.clone(),
            endpoint: format!("/nodes/{}/dns", repo.guests.node),
            changes,
            digest: None,
        });
    }

    file::operation(
        repo,
        ("network", &repo.guests.node),
        ("network/interfaces", "/etc/network/interfaces"),
        render::network(&repo.network),
        true,
        &mut operations,
    )?;
    file::operation(
        repo,
        ("firewall", "cluster"),
        ("pve/firewall/cluster.fw", "/etc/pve/firewall/cluster.fw"),
        render::firewall_policy(&repo.firewall.cluster),
        false,
        &mut operations,
    )?;
    for (id, policy) in &repo.firewall.guests {
        let id = id.to_string();
        file::operation(
            repo,
            ("firewall", &id),
            (
                &format!("pve/firewall/{id}.fw"),
                &format!("/etc/pve/firewall/{id}.fw"),
            ),
            render::firewall_policy(policy),
            false,
            &mut operations,
        )?;
    }

    let mut plan = Plan {
        schema_version: 1,
        created_at: Utc::now(),
        target: api.endpoint().into(),
        operations,
        blockers,
        plan_sha256: String::new(),
    };
    plan.plan_sha256 = plan.calculate_hash()?;
    fs::create_dir_all(repo.runtime())?;
    fs::write(
        repo.runtime().join("production-plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )?;
    Ok(plan)
}

fn ensure_observations_are_fresh(repo: &Repository) -> Result<()> {
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(repo.observed().join("manifest.json")).context("run capture before plan")?,
    )?;
    let exported_at = DateTime::parse_from_rfc3339(
        manifest["exported_at"]
            .as_str()
            .context("manifest exported_at")?,
    )?
    .with_timezone(&Utc);
    if Utc::now() - exported_at > chrono::Duration::minutes(30) {
        bail!("observed exports are stale; run capture before plan")
    }
    Ok(())
}

fn compare(
    changes: &mut BTreeMap<String, String>,
    key: &str,
    wanted: &impl ToString,
    actual: &serde_json::Value,
) {
    let wanted = wanted.to_string();
    let current = actual.get(key).map(value_string);
    if current.as_deref() != Some(&wanted) {
        changes.insert(key.into(), wanted);
    }
}

fn value_string(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_hash_detects_tampering() {
        let mut plan = Plan {
            schema_version: 1,
            created_at: Utc::now(),
            target: "https://pve.example:8006".into(),
            operations: Vec::new(),
            blockers: Vec::new(),
            plan_sha256: String::new(),
        };
        plan.plan_sha256 = plan.calculate_hash().unwrap();
        assert!(plan.verify().is_ok());
        plan.target.push_str("/tampered");
        assert!(plan.verify().is_err());
    }

    #[test]
    fn firewall_option_order_is_not_drift() {
        let a = "[OPTIONS]\nenable: 1\nlog_level_in: nolog\n";
        let b = "[OPTIONS]\nlog_level_in: nolog\nenable: 1\n";
        assert_eq!(render::firewall_semantic(a), render::firewall_semantic(b));
    }
}
