use crate::{client::RemoteHost, config::Repository, discovery::PveSnapshot, utility::progress};
use anyhow::{Context, Result};
use std::{collections::BTreeSet, fs, path::Path};

pub(super) fn export(
    repo: &Repository,
    ssh: &dyn RemoteHost,
    snapshot: &PveSnapshot,
) -> Result<()> {
    for (local, remote) in [
        ("network/interfaces", "/etc/network/interfaces"),
        ("network/hosts", "/etc/hosts"),
        ("storage/fstab", "/etc/fstab"),
        ("pve/storage.cfg", "/etc/pve/storage.cfg"),
        ("pve/jobs.cfg", "/etc/pve/jobs.cfg"),
        ("pve/datacenter.cfg", "/etc/pve/datacenter.cfg"),
        ("pve/firewall/cluster.fw", "/etc/pve/firewall/cluster.fw"),
    ] {
        required(ssh, remote, &repo.observed().join(local))?;
    }

    let mut guest_ids = BTreeSet::new();
    for (node_name, node) in &snapshot.nodes {
        optional(
            ssh,
            &format!("cat /etc/pve/nodes/{node_name}/host.fw"),
            &repo
                .observed()
                .join(format!("pve/firewall/{node_name}-host.fw")),
        )?;
        for id in node.lxcs.keys() {
            guest_ids.insert(id.clone());
            let local = if node_name == &repo.guests.node {
                repo.observed().join(format!("pve/lxc/{id}.conf"))
            } else {
                repo.observed()
                    .join(format!("pve/nodes/{node_name}/lxc/{id}.conf"))
            };
            required(
                ssh,
                &format!("/etc/pve/nodes/{node_name}/lxc/{id}.conf"),
                &local,
            )?;
        }
        for id in node.vms.keys() {
            guest_ids.insert(id.clone());
            let local = if node_name == &repo.guests.node {
                repo.observed().join(format!("pve/qemu-server/{id}.conf"))
            } else {
                repo.observed()
                    .join(format!("pve/nodes/{node_name}/qemu-server/{id}.conf"))
            };
            required(
                ssh,
                &format!("/etc/pve/nodes/{node_name}/qemu-server/{id}.conf"),
                &local,
            )?;
        }
    }
    for id in guest_ids {
        optional(
            ssh,
            &format!("cat /etc/pve/firewall/{id}.fw"),
            &repo.observed().join(format!("pve/firewall/{id}.fw")),
        )?;
    }

    let pbs_vmid = repo.backup.pbs.guest.vmid;
    for (local, remote) in [
        ("pbs/datastore.cfg", "/etc/proxmox-backup/datastore.cfg"),
        ("pbs/node.cfg", "/etc/proxmox-backup/node.cfg"),
        ("pbs/prune.cfg", "/etc/proxmox-backup/prune.cfg"),
        ("pbs/sync.cfg", "/etc/proxmox-backup/sync.cfg"),
        (
            "pbs/verification.cfg",
            "/etc/proxmox-backup/verification.cfg",
        ),
    ] {
        optional(
            ssh,
            &format!("pct exec {pbs_vmid} -- cat {remote}"),
            &repo.observed().join(local),
        )?;
    }
    Ok(())
}

fn required(ssh: &dyn RemoteHost, remote: &str, local: &Path) -> Result<()> {
    progress::operation(format!("read {remote}"));
    let command = format!("cat {remote}");
    let data = ssh.run(&command).with_context(|| remote.to_string())?;
    write(local, &data)
}

fn optional(ssh: &dyn RemoteHost, command: &str, local: &Path) -> Result<()> {
    progress::operation(format!("read {}", local.display()));
    match ssh.run(command) {
        Ok(data) => write(local, &data)?,
        Err(_) if local.exists() => fs::remove_file(local)?,
        Err(_) => {},
    }
    Ok(())
}

fn write(local: &Path, data: &str) -> Result<()> {
    if let Some(parent) = local.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(local, sanitize(data))?;
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
            .any(|sensitive| lower.starts_with(sensitive))
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
