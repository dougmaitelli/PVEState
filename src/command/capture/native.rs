use crate::{
    client::RemoteHost,
    config::LocalState,
    discovery::PveSnapshot,
    utility::{progress, shell},
};
use anyhow::{Context, Result, bail};
use std::{collections::BTreeSet, fs, path::Path};

pub(super) fn export(
    repo: &LocalState,
    observed: &Path,
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
    ] {
        required(ssh, remote, &observed.join(local))?;
    }
    optional_file(
        ssh,
        "/etc/pve/firewall/cluster.fw",
        &observed.join("pve/firewall/cluster.fw"),
    )?;

    let mut guest_ids = BTreeSet::new();
    for (node_name, node) in &snapshot.nodes {
        optional_file(
            ssh,
            &format!("/etc/pve/nodes/{node_name}/host.fw"),
            &observed.join(format!("pve/firewall/{node_name}-host.fw")),
        )?;
        for id in node.lxcs.keys() {
            guest_ids.insert(id.clone());
            let local = if node_name == &repo.guests.node {
                observed.join(format!("pve/lxc/{id}.conf"))
            } else {
                observed.join(format!("pve/nodes/{node_name}/lxc/{id}.conf"))
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
                observed.join(format!("pve/qemu-server/{id}.conf"))
            } else {
                observed.join(format!("pve/nodes/{node_name}/qemu-server/{id}.conf"))
            };
            required(
                ssh,
                &format!("/etc/pve/nodes/{node_name}/qemu-server/{id}.conf"),
                &local,
            )?;
        }
    }
    for id in guest_ids {
        optional_file(
            ssh,
            &format!("/etc/pve/firewall/{id}.fw"),
            &observed.join(format!("pve/firewall/{id}.fw")),
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
        optional_container_file(ssh, pbs_vmid, remote, &observed.join(local))?;
    }
    Ok(())
}

fn required(ssh: &dyn RemoteHost, remote: &str, local: &Path) -> Result<()> {
    progress::operation(format!("read {remote}"));
    let command = format!("cat {remote}");
    let data = ssh.run(&command).with_context(|| remote.to_string())?;
    write(local, &data)
}

fn optional_file(ssh: &dyn RemoteHost, remote: &str, local: &Path) -> Result<()> {
    let remote = shell::quote(remote);
    optional_command(
        ssh,
        &format!("if test -e {remote}; then cat {remote}; else exit 44; fi"),
        local,
    )
}

fn optional_container_file(
    ssh: &dyn RemoteHost,
    vmid: u32,
    remote: &str,
    local: &Path,
) -> Result<()> {
    let remote = shell::quote(remote);
    let command = format!("if test -e {remote}; then cat {remote}; else exit 44; fi");
    optional_command(
        ssh,
        &format!("pct exec {vmid} -- sh -c {}", shell::quote(&command)),
        local,
    )
}

fn optional_command(ssh: &dyn RemoteHost, command: &str, local: &Path) -> Result<()> {
    progress::operation(format!("read {}", local.display()));
    let output = ssh.probe(command)?;
    if output.ok {
        return write(local, &output.stdout);
    }
    if output.return_code == Some(44) {
        if local.exists() {
            fs::remove_file(local)?;
        }
        return Ok(());
    }
    bail!(
        "optional native capture failed with exit {:?}: {}",
        output.return_code,
        output.stderr.trim()
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SshOutput;

    struct FakeSsh {
        output: SshOutput,
    }

    impl RemoteHost for FakeSsh {
        fn host_key_fingerprint(&self) -> Result<String> {
            unreachable!()
        }

        fn run(&self, _: &str) -> Result<String> {
            unreachable!()
        }

        fn probe(&self, _: &str) -> Result<SshOutput> {
            Ok(SshOutput {
                ok: self.output.ok,
                return_code: self.output.return_code,
                stdout: self.output.stdout.clone(),
                stderr: self.output.stderr.clone(),
            })
        }

        fn stdin(&self, _: &str, _: &[u8]) -> Result<()> {
            unreachable!()
        }
    }

    fn output(ok: bool, return_code: i32, stdout: &str, stderr: &str) -> FakeSsh {
        FakeSsh {
            output: SshOutput {
                ok,
                return_code: Some(return_code),
                stdout: stdout.into(),
                stderr: stderr.into(),
            },
        }
    }

    #[test]
    fn confirmed_absence_removes_previous_optional_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let local = temp.path().join("guest.fw");
        fs::write(&local, "previous").unwrap();

        optional_file(&output(false, 44, "", ""), "/guest.fw", &local).unwrap();

        assert!(!local.exists());
    }

    #[test]
    fn operational_failure_preserves_previous_optional_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let local = temp.path().join("guest.fw");
        fs::write(&local, "previous").unwrap();

        let error = optional_file(
            &output(false, 255, "", "Permission denied"),
            "/guest.fw",
            &local,
        )
        .unwrap_err();

        assert!(error.to_string().contains("Permission denied"));
        assert_eq!(fs::read_to_string(local).unwrap(), "previous");
    }
}
