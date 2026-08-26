use crate::{
    client::RemoteHost,
    config::Repository,
    render,
    settings::RecoverySettings,
    utility::{
        atomic_file, authorization,
        plan_envelope::{self, PlanEnvelope},
        progress, shell,
    },
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Clone, Copy)]
pub enum Stage {
    Plan,
    BootstrapPve,
    BootstrapPbs,
    Restore,
    Configure,
    All,
}

#[derive(Clone, Serialize, Deserialize)]
struct RecoveryPlan {
    schema_version: u8,
    created_at: DateTime<Utc>,
    target: String,
    blockers: Vec<String>,
    plan_sha256: String,
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

pub fn run(
    repo: &Repository,
    stage: Stage,
    target: &str,
    settings: &RecoverySettings,
    ssh: Option<&dyn RemoteHost>,
) -> Result<()> {
    progress::section(format!("Recovery: {}", stage.name()));
    if matches!(stage, Stage::Plan) {
        return create_plan(repo, target);
    }
    let plan: RecoveryPlan = serde_json::from_slice(
        &fs::read(repo.runtime().join("recovery-plan.json")).context("run recover plan first")?,
    )?;
    authorize(&plan, target, settings)?;
    if repo.restore.target.production_address == target && !settings.allow_production_target {
        bail!("recovery target is production; PVES_ALLOW_PRODUCTION_TARGET must equal YES")
    }
    let ssh = ssh.context("recovery SSH settings are required")?;
    match stage {
        Stage::BootstrapPve => bootstrap_pve(repo, ssh),
        Stage::BootstrapPbs => bootstrap_pbs(repo, ssh),
        Stage::Restore => restore(repo, ssh),
        Stage::Configure => configure(repo, ssh),
        Stage::All => {
            bootstrap_pve(repo, ssh)?;
            bootstrap_pbs(repo, ssh)?;
            restore(repo, ssh)?;
            configure(repo, ssh)
        },
        Stage::Plan => unreachable!(),
    }
}

impl Stage {
    const fn name(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::BootstrapPve => "bootstrap PVE",
            Self::BootstrapPbs => "bootstrap PBS",
            Self::Restore => "restore guests",
            Self::Configure => "configure applications",
            Self::All => "all stages",
        }
    }
}

fn create_plan(repo: &Repository, target: &str) -> Result<()> {
    let mut blockers = Vec::new();
    if repo.restore.pbs_bootstrap.lxc_template.is_none() {
        blockers.push("pbs_bootstrap.lxc_template".into());
    }
    if !repo.restore.pbs_bootstrap.storage_attached_to_pve {
        blockers.push("pbs_bootstrap.storage_attached_to_pve".into());
    }
    for id in &repo.restore.restore_order {
        if repo.restore.archives.get(id).is_none_or(Option::is_none) {
            blockers.push(format!("archives.{id}"));
        }
    }
    if repo.restore.application.configure_command.is_none() {
        blockers.push("application.configure_command".into());
    }
    let mut plan = RecoveryPlan {
        schema_version: 1,
        created_at: Utc::now(),
        target: target.into(),
        blockers,
        plan_sha256: String::new(),
    };
    plan_envelope::sign(&mut plan)?;
    atomic_file::write_json(&repo.runtime().join("recovery-plan.json"), &plan)?;
    println!("{}", serde_json::to_string_pretty(&plan)?);
    Ok(())
}

fn authorize(plan: &RecoveryPlan, target: &str, settings: &RecoverySettings) -> Result<()> {
    plan.verify()?;
    authorization::authorize(
        &plan.plan_sha256,
        &plan.target,
        plan.created_at,
        &plan.blockers,
        authorization::Policy {
            enabled: settings.enabled,
            enabled_error: "PVES_ENABLE_RECOVERY must equal YES",
            confirmation: settings.confirm_plan_sha.as_deref(),
            confirmation_error: "recovery plan SHA mismatch",
            requested_target: target,
            target_error: "recovery target mismatch",
            max_age: chrono::Duration::minutes(30),
            stale_error: "recovery plan is stale",
            blocker_prefix: "recovery blockers: ",
            blocker_separator: ", ",
        },
    )
}

fn bootstrap_pve(repo: &Repository, ssh: &dyn RemoteHost) -> Result<()> {
    progress::operation("verify replacement PVE host");
    ssh.run("pveversion")?;
    ssh.run("zpool list -H -o name VMs; zpool list -H -o name Data; test -d /mnt/pve/backup; test -d /mnt/pve/security")?;
    write(
        ssh,
        "/etc/network/interfaces",
        &render::network(&repo.network),
        "0644",
    )?;
    write(
        ssh,
        "/etc/pve/firewall/cluster.fw",
        &render::firewall_policy(&repo.firewall.cluster),
        "0640",
    )?;
    println!("replacement PVE configuration staged; activate networking only with console access");
    Ok(())
}

fn bootstrap_pbs(repo: &Repository, ssh: &dyn RemoteHost) -> Result<()> {
    progress::operation("create and provision PBS guest");
    let template = repo
        .restore
        .pbs_bootstrap
        .lxc_template
        .as_deref()
        .context("PBS template")?;
    let guest = repo.guests.lxcs.get(&111).context("LXC 111")?;
    let network = guest
        .networks
        .first()
        .map(|nic| format!(" --net0 {}", shell::quote(&render::lxc_nic(nic))))
        .unwrap_or_default();
    let create = format!(
        "pct status 111 >/dev/null 2>&1 || pct create 111 {} --hostname {} --cores {} --memory {} --swap {} --rootfs {}:{}{} --unprivileged 0 --onboot 1",
        shell::quote(template),
        shell::quote(&guest.hostname),
        guest.cores,
        guest.memory_mb,
        guest.swap_mb,
        shell::quote(&guest.rootfs.storage),
        guest.rootfs.size_gb,
        network
    );
    ssh.run(&create)?;
    ssh.run("pct set 111 -mp0 /mnt/pve/backup,mp=/mnt/backup; pct start 111 2>/dev/null || true; pct exec 111 -- sh -c 'apt-get update && apt-get install -y proxmox-backup-server'")?;
    Ok(())
}

fn restore(repo: &Repository, ssh: &dyn RemoteHost) -> Result<()> {
    for id in &repo.restore.restore_order {
        let id = *id;
        let archive = repo
            .restore
            .archives
            .get(&id)
            .and_then(Option::as_deref)
            .context("archive")?;
        progress::operation(format!("restore VMID {id} from {archive}"));
        if ssh
            .run(&format!(
                "qm status {id} 2>/dev/null || pct status {id} 2>/dev/null"
            ))
            .is_ok()
        {
            bail!("refusing to overwrite existing VMID {id}")
        }
        if let Some(guest) = repo.guests.lxcs.get(&id) {
            ssh.run(&format!(
                "pct restore {id} {} --storage {}",
                shell::quote(archive),
                shell::quote(&guest.rootfs.storage)
            ))?;
        } else if let Some(guest) = repo.guests.vms.get(&id) {
            ssh.run(&format!(
                "qmrestore {} {id} --storage {}",
                shell::quote(archive),
                shell::quote(&guest.disk.storage)
            ))?;
        }
    }
    for mount in &repo.restore.reattach_mounts {
        let id = mount.vmid;
        let index = mount.index;
        let source = &mount.source;
        let target = &mount.target;
        progress::operation(format!("reattach VMID {id} mount {index}"));
        ssh.run(&format!(
            "pct set {id} -mp{index} {},mp={}",
            shell::quote(source),
            shell::quote(target)
        ))?;
    }
    for (id, policy) in &repo.firewall.guests {
        write(
            ssh,
            &format!("/etc/pve/firewall/{id}.fw"),
            &render::firewall_policy(policy),
            "0640",
        )?;
    }
    for id in &repo.restore.restore_order {
        ssh.run(&format!("qm start {id} 2>/dev/null || pct start {id}"))?;
    }
    Ok(())
}

fn configure(repo: &Repository, ssh: &dyn RemoteHost) -> Result<()> {
    let command = repo
        .restore
        .application
        .configure_command
        .as_deref()
        .context("application.configure_command")?;
    ssh.run(command)?;
    Ok(())
}

fn write(ssh: &dyn RemoteHost, path: &str, content: &str, mode: &str) -> Result<()> {
    let encoded = STANDARD.encode(content);
    ssh.stdin(
        &format!(
            "base64 -d > {}.pves-new && install -m {} {}.pves-new {}",
            shell::quote(path),
            mode,
            shell::quote(path),
            shell::quote(path)
        ),
        encoded.as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_plan_detects_tampering() {
        let mut plan = RecoveryPlan {
            schema_version: 1,
            created_at: Utc::now(),
            target: "host".into(),
            blockers: Vec::new(),
            plan_sha256: String::new(),
        };
        plan_envelope::sign(&mut plan).unwrap();
        assert!(plan.verify().is_ok());
        plan.target = "other".into();
        assert!(plan.verify().is_err());
    }
}
