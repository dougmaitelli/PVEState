use crate::{api::Pve, config::Repository, pbs::Pbs, ssh::Ssh};
use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use std::{collections::BTreeMap, fs};
pub fn run(repo: &Repository) -> Result<()> {
    let api = Pve::discovery()?;
    let ssh = Ssh::discovery(&repo.root)?;
    fs::create_dir_all(repo.runtime())?;
    fs::create_dir_all(repo.observed().join("pve/firewall"))?;
    fs::create_dir_all(repo.observed().join("network"))?;
    let pbs = Pbs::discovery()?;
    let pbs_failures = pbs.write_snapshot(&repo.runtime())?;
    if !pbs_failures.is_empty() {
        eprintln!("some PBS API endpoints were unavailable to the discovery token:");
        for failure in pbs_failures {
            eprintln!("  - {failure}");
        }
    }
    let mut guests = BTreeMap::new();
    for id in repo.guests.lxcs.keys() {
        guests.insert(
            format!("lxc/{id}"),
            api.get(&format!("/nodes/{}/lxc/{id}/config", repo.guests.node))?,
        );
    }
    for id in repo.guests.vms.keys() {
        guests.insert(
            format!("qemu/{id}"),
            api.get(&format!("/nodes/{}/qemu/{id}/config", repo.guests.node))?,
        );
    }
    let snapshot = json!({"schema_version":1,"captured_at":Utc::now(),"endpoint":api.endpoint(),"dns":api.get(&format!("/nodes/{}/dns",repo.guests.node))?,"network":api.get(&format!("/nodes/{}/network",repo.guests.node))?,"storage":api.get("/storage")?,"backup_jobs":api.get("/cluster/backup")?,"guests":guests});
    fs::write(
        repo.runtime().join("observed-latest.json"),
        serde_json::to_vec_pretty(&snapshot)?,
    )?;
    export(
        &ssh,
        "/etc/network/interfaces",
        &repo.observed().join("network/interfaces"),
    )?;
    for (local, remote) in [
        ("network/hosts", "/etc/hosts"),
        ("storage/fstab", "/etc/fstab"),
        ("pve/storage.cfg", "/etc/pve/storage.cfg"),
        ("pve/jobs.cfg", "/etc/pve/jobs.cfg"),
        ("pve/datacenter.cfg", "/etc/pve/datacenter.cfg"),
    ] {
        export(&ssh, remote, &repo.observed().join(local))?;
    }
    for id in repo.guests.lxcs.keys() {
        export(
            &ssh,
            &format!("/etc/pve/lxc/{id}.conf"),
            &repo.observed().join(format!("pve/lxc/{id}.conf")),
        )?;
    }
    for id in repo.guests.vms.keys() {
        export(
            &ssh,
            &format!("/etc/pve/qemu-server/{id}.conf"),
            &repo.observed().join(format!("pve/qemu-server/{id}.conf")),
        )?;
    }
    for (local, remote) in [
        ("pbs/datastore.cfg", "/etc/proxmox-backup/datastore.cfg"),
        ("pbs/node.cfg", "/etc/proxmox-backup/node.cfg"),
        ("pbs/prune.cfg", "/etc/proxmox-backup/prune.cfg"),
        (
            "pbs/verification.cfg",
            "/etc/proxmox-backup/verification.cfg",
        ),
    ] {
        export_command(
            &ssh,
            &format!("pct exec 111 -- cat {remote}"),
            &repo.observed().join(local),
        )?;
    }
    export(
        &ssh,
        "/etc/pve/firewall/cluster.fw",
        &repo.observed().join("pve/firewall/cluster.fw"),
    )?;
    for id in repo.guests.lxcs.keys().chain(repo.guests.vms.keys()) {
        let remote = format!("/etc/pve/firewall/{id}.fw");
        let local = repo.observed().join(format!("pve/firewall/{id}.fw"));
        match ssh.run(&format!("cat {remote}")) {
            Ok(s) => fs::write(local, s)?,
            Err(_) => {
                if local.exists() {
                    fs::remove_file(local)?
                }
            },
        }
    }
    let manifest = json!({"schema_version":1,"exported_at":Utc::now(),"source":api.endpoint(),"scope":"network and PVE cluster/managed-guest firewalls"});
    fs::write(
        repo.observed().join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    let mut host = BTreeMap::new();
    for (name, command) in [
        ("identity", "hostname; uname -a; pveversion -v"),
        ("network", "ip -json address show; ip -json route show"),
        (
            "block_devices",
            "lsblk --json --bytes -o NAME,PATH,SIZE,TYPE,FSTYPE,MOUNTPOINTS,MODEL,SERIAL",
        ),
        (
            "mounts",
            "findmnt --json --bytes -o TARGET,SOURCE,FSTYPE,OPTIONS,SIZE,USED,AVAIL",
        ),
        (
            "zfs",
            "zpool status; zfs list -Hp -o name,used,available,referenced,mountpoint",
        ),
        (
            "pbs",
            "pct exec 111 -- proxmox-backup-manager datastore list --output-format json",
        ),
    ] {
        host.insert(name, ssh.run(command)?);
    }
    fs::write(
        repo.runtime().join("host-latest.json"),
        serde_json::to_vec_pretty(&host)?,
    )?;
    println!("captured production into {}", repo.root.display());
    Ok(())
}

fn export(ssh: &Ssh, remote: &str, local: &std::path::Path) -> Result<()> {
    export_command(ssh, &format!("cat {remote}"), local).with_context(|| remote.to_string())
}

fn export_command(ssh: &Ssh, command: &str, local: &std::path::Path) -> Result<()> {
    let data = sanitize(&ssh.run(command)?);
    if let Some(p) = local.parent() {
        fs::create_dir_all(p)?
    }
    fs::write(local, data)?;
    Ok(())
}

fn sanitize(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let lower = line.trim_start().to_ascii_lowercase();
            if [
                "password",
                "passwd",
                "secret",
                "secret-key",
                "token",
                "api-key",
                "private-key",
            ]
            .iter()
            .any(|x| lower.starts_with(x))
            {
                format!(
                    "{}: [REDACTED]",
                    line.split([':', ' ', '=']).next().unwrap_or("secret")
                )
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
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
        anyhow::bail!(failures.join("\n"))
    }
}
