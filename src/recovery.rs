use crate::{config::Repository, render, ssh::Ssh};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    fn hash(&self) -> Result<String> {
        let mut unsigned = self.clone();
        unsigned.plan_sha256.clear();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&unsigned)?)))
    }

    fn verify(&self) -> Result<()> {
        if self.hash()? != self.plan_sha256 {
            bail!("recovery plan integrity check failed")
        }
        Ok(())
    }
}

pub fn run(repo: &Repository, stage: Stage, target: &str) -> Result<()> {
    if matches!(stage, Stage::Plan) {
        return create_plan(repo, target);
    }
    let plan: RecoveryPlan = serde_json::from_slice(
        &fs::read(repo.runtime().join("recovery-plan.json")).context("run recover plan first")?,
    )?;
    authorize(&plan, target)?;
    if repo.restore.target.production_address == target
        && std::env::var("IAC_ALLOW_PRODUCTION_TARGET").as_deref() != Ok("YES")
    {
        bail!("recovery target is production; IAC_ALLOW_PRODUCTION_TARGET must equal YES")
    }
    let ssh = Ssh::recovery(target)?;
    match stage {
        Stage::BootstrapPve => bootstrap_pve(repo, &ssh),
        Stage::BootstrapPbs => bootstrap_pbs(repo, &ssh),
        Stage::Restore => restore(repo, &ssh),
        Stage::Configure => configure(repo, &ssh),
        Stage::All => {
            bootstrap_pve(repo, &ssh)?;
            bootstrap_pbs(repo, &ssh)?;
            restore(repo, &ssh)?;
            configure(repo, &ssh)
        },
        Stage::Plan => unreachable!(),
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
    plan.plan_sha256 = plan.hash()?;
    fs::create_dir_all(repo.runtime())?;
    fs::write(
        repo.runtime().join("recovery-plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&plan)?);
    Ok(())
}

fn authorize(plan: &RecoveryPlan, target: &str) -> Result<()> {
    plan.verify()?;
    if std::env::var("IAC_ENABLE_RECOVERY").as_deref() != Ok("YES") {
        bail!("IAC_ENABLE_RECOVERY must equal YES")
    }
    if std::env::var("IAC_CONFIRM_PLAN_SHA").as_deref() != Ok(&plan.plan_sha256) {
        bail!("recovery plan SHA mismatch")
    }
    if plan.target != target {
        bail!("recovery target mismatch")
    }
    if Utc::now() - plan.created_at > chrono::Duration::minutes(30) {
        bail!("recovery plan is stale")
    }
    if !plan.blockers.is_empty() {
        bail!("recovery blockers: {}", plan.blockers.join(", "))
    }
    Ok(())
}

fn bootstrap_pve(repo: &Repository, ssh: &Ssh) -> Result<()> {
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

fn bootstrap_pbs(repo: &Repository, ssh: &Ssh) -> Result<()> {
    let template = repo
        .restore
        .pbs_bootstrap
        .lxc_template
        .as_deref()
        .context("PBS template")?;
    let guest = repo.guests.lxcs.get(&111).context("LXC 111")?;
    let create = format!(
        "pct status 111 >/dev/null 2>&1 || pct create 111 {} --hostname {} --cores {} --memory {} --swap {} --rootfs {}:{} --net0 {} --unprivileged 0 --onboot 1",
        quote(template),
        quote(&guest.hostname),
        guest.cores,
        guest.memory_mb,
        guest.swap_mb,
        quote(&guest.rootfs.storage),
        guest.rootfs.size_gb,
        quote(&render::lxc_nic(&guest.network))
    );
    ssh.run(&create)?;
    ssh.run("pct set 111 -mp0 /mnt/pve/backup,mp=/mnt/backup; pct start 111 2>/dev/null || true; pct exec 111 -- sh -c 'apt-get update && apt-get install -y proxmox-backup-server'")?;
    Ok(())
}

fn restore(repo: &Repository, ssh: &Ssh) -> Result<()> {
    for id in &repo.restore.restore_order {
        let id = *id;
        let archive = repo
            .restore
            .archives
            .get(&id)
            .and_then(Option::as_deref)
            .context("archive")?;
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
                quote(archive),
                quote(&guest.rootfs.storage)
            ))?;
        } else if let Some(guest) = repo.guests.vms.get(&id) {
            ssh.run(&format!(
                "qmrestore {} {id} --storage {}",
                quote(archive),
                quote(&guest.disk.storage)
            ))?;
        }
    }
    for mount in &repo.restore.reattach_mounts {
        let id = mount.vmid;
        let index = mount.index;
        let source = &mount.source;
        let target = &mount.target;
        ssh.run(&format!(
            "pct set {id} -mp{index} {},mp={}",
            quote(source),
            quote(target)
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

fn configure(repo: &Repository, ssh: &Ssh) -> Result<()> {
    let command = repo
        .restore
        .application
        .configure_command
        .as_deref()
        .context("application.configure_command")?;
    ssh.run(command)?;
    Ok(())
}

fn write(ssh: &Ssh, path: &str, content: &str, mode: &str) -> Result<()> {
    let encoded = STANDARD.encode(content);
    ssh.stdin(
        &format!(
            "base64 -d > {}.iac-new && install -m {} {}.iac-new {}",
            quote(path),
            mode,
            quote(path),
            quote(path)
        ),
        encoded.as_bytes(),
    )
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
        plan.plan_sha256 = plan.hash().unwrap();
        assert!(plan.verify().is_ok());
        plan.target = "other".into();
        assert!(plan.verify().is_err());
    }
}
