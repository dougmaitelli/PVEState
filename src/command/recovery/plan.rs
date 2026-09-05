use crate::{
    config::LocalState,
    settings::RecoverySettings,
    utility::{
        atomic_file, authorization,
        plan_envelope::{self, PlanEnvelope},
        progress, runtime_security,
    },
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct RecoveryPlan {
    schema_version: u8,
    pub(super) created_at: DateTime<Utc>,
    pub(super) target: String,
    pub(super) expected_hostname: String,
    pub(super) expected_host_key_sha256: String,
    max_age_minutes: u32,
    pub(super) blockers: Vec<String>,
    pub(super) plan_sha256: String,
}

impl RecoveryPlan {
    fn verify(&self) -> Result<()> {
        plan_envelope::verify(self, "recovery plan integrity check failed")
    }
}

impl PlanEnvelope for RecoveryPlan {
    fn integrity(&self) -> &str {
        &self.plan_sha256
    }

    fn set_integrity(&mut self, value: String) {
        self.plan_sha256 = value;
    }
}

pub(super) fn create(repo: &LocalState, target: &str) -> Result<()> {
    let mut blockers = Vec::new();
    if repo.restore.target.expected_host_key_sha256.is_none() {
        blockers.push("target.expected_host_key_sha256".into());
    }
    if repo.restore.pbs_bootstrap.lxc_template.is_none() {
        blockers.push("pbs_bootstrap.lxc_template".into());
    }
    if !repo.restore.pbs_bootstrap.storage_attached_to_pve {
        blockers.push("pbs_bootstrap.storage_attached_to_pve".into());
    }
    if super::bootstrap_pbs::cache_mount(repo).is_none() {
        blockers.push("reattach_mounts entry for PBS cache_path".into());
    }
    for id in &repo.restore.restore_order {
        if repo.restore.archives.get(id).is_none_or(Option::is_none) {
            blockers.push(format!("archives.{id}"));
        }
        if !repo.restore.protected_vmids.contains(id) {
            blockers.push(format!("protected_vmids does not include {id}"));
        }
        if !repo.guests.lxcs.contains_key(id) && !repo.guests.vms.contains_key(id) {
            blockers.push(format!("guest configuration for VMID {id}"));
        }
    }
    if repo.restore.application.configure_command.is_none() {
        blockers.push("application.configure_command".into());
    }

    let mut plan = RecoveryPlan {
        schema_version: 2,
        created_at: Utc::now(),
        target: target.into(),
        expected_hostname: repo.restore.target.expected_hostname.clone(),
        expected_host_key_sha256: repo
            .restore
            .target
            .expected_host_key_sha256
            .clone()
            .unwrap_or_default(),
        max_age_minutes: repo.restore.target.plan_max_age_minutes,
        blockers,
        plan_sha256: String::new(),
    };
    plan_envelope::sign(&mut plan)?;
    atomic_file::write_json(
        &repo.runtime().join(crate::config::artifacts::RECOVERY_PLAN),
        &plan,
    )?;
    progress::finish(true);
    println!("{}", serde_json::to_string_pretty(&plan)?);
    Ok(())
}

pub(super) fn load(repo: &LocalState) -> Result<RecoveryPlan> {
    Ok(serde_json::from_slice(
        &runtime_security::read(&repo.runtime().join(crate::config::artifacts::RECOVERY_PLAN))
            .context("run recover plan first")?,
    )?)
}

pub(super) fn authorize(
    plan: &RecoveryPlan,
    target: &str,
    settings: &RecoverySettings,
) -> Result<()> {
    plan.verify()?;
    authorization::authorize(
        &plan.plan_sha256,
        &plan.target,
        plan.created_at,
        &plan.blockers,
        authorization::Policy {
            enabled: settings.enabled,
            enabled_error: crate::settings::env::ENABLE_RECOVERY_ERROR,
            confirmation: settings.confirm_plan_sha.as_deref(),
            confirmation_error: "recovery plan SHA mismatch",
            requested_target: target,
            target_error: "recovery target mismatch",
            max_age: chrono::Duration::minutes(i64::from(plan.max_age_minutes)),
            stale_error: "recovery plan is stale",
            max_future_skew: chrono::Duration::minutes(2),
            future_error: "recovery plan timestamp is too far in the future; check system clocks",
            blocker_prefix: "recovery blockers: ",
            blocker_separator: ", ",
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_plan_detects_tampering() {
        let mut plan = RecoveryPlan {
            schema_version: 2,
            created_at: Utc::now(),
            target: "host".into(),
            expected_hostname: "replacement".into(),
            expected_host_key_sha256: "SHA256:fixture".into(),
            max_age_minutes: 30,
            blockers: Vec::new(),
            plan_sha256: String::new(),
        };
        plan_envelope::sign(&mut plan).unwrap();
        assert!(plan.verify().is_ok());
        plan.target = "other".into();
        assert!(plan.verify().is_err());
    }
}
