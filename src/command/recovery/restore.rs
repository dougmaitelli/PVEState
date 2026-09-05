use crate::{
    client::RemoteHost,
    config::LocalState,
    resource::{firewall, native_paths},
    utility::{progress::EventSink, remote_file, shell},
};
use anyhow::{Context, Result, bail};

pub(super) fn run(repo: &LocalState, ssh: &dyn RemoteHost, events: &dyn EventSink) -> Result<()> {
    for id in &repo.restore.restore_order {
        let id = *id;
        let archive = repo
            .restore
            .archives
            .get(&id)
            .and_then(Option::as_deref)
            .context("archive")?;
        events.operation(&format!("restore VMID {id} from {archive}"));
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
        events.operation(&format!(
            "reattach VMID {} mount {}",
            mount.vmid, mount.index
        ));
        ssh.run(&format!(
            "pct set {} -mp{} {},mp={}",
            mount.vmid,
            mount.index,
            shell::quote(&mount.source),
            shell::quote(&mount.target)
        ))?;
    }

    for (id, policy) in repo
        .guests
        .lxcs
        .iter()
        .map(|(id, guest)| (id, guest.firewall.as_ref()))
        .chain(
            repo.guests
                .vms
                .iter()
                .map(|(id, guest)| (id, guest.firewall.as_ref())),
        )
    {
        if let Some(policy) = policy {
            remote_file::write(
                ssh,
                &native_paths::guest_firewall_remote(id),
                &firewall::render::render(policy),
                remote_file::WriteOptions {
                    mode: "0640",
                    expected_sha256: None,
                    verify_expected: false,
                    backup_existing: true,
                },
            )?;
        }
    }

    for id in &repo.restore.restore_order {
        ssh.run(&format!("qm start {id} 2>/dev/null || pct start {id}"))?;
    }
    Ok(())
}
