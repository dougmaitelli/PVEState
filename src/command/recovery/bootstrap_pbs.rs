use crate::{
    client::RemoteHost,
    config::LocalState,
    model::RestoreMount,
    render,
    utility::{progress, shell},
};
use anyhow::{Context, Result};

pub(super) fn run(repo: &LocalState, ssh: &dyn RemoteHost) -> Result<()> {
    progress::operation("create and provision PBS guest");
    let bootstrap = &repo.restore.pbs_bootstrap;
    let id = bootstrap.vmid;
    let template = bootstrap.lxc_template.as_deref().context("PBS template")?;
    let guest = repo
        .guests
        .lxcs
        .get(&id)
        .with_context(|| format!("PBS LXC {id}"))?;
    let mount = cache_mount(repo).context("PBS cache mount")?;
    let network = guest
        .networks
        .first()
        .map(|nic| format!(" --net0 {}", shell::quote(&render::lxc_nic(nic))))
        .unwrap_or_default();
    let create = format!(
        "pct status {id} >/dev/null 2>&1 || pct create {id} {} --hostname {} --cores {} --memory {} --swap {} --rootfs {}:{}{} --unprivileged {} --onboot {}",
        shell::quote(template),
        shell::quote(&guest.hostname),
        guest.cores,
        guest.memory_mb,
        guest.swap_mb,
        shell::quote(&guest.rootfs.storage),
        guest.rootfs.size_gb,
        network,
        u8::from(guest.unprivileged),
        u8::from(guest.start.onboot),
    );
    ssh.run(&create)?;
    ssh.run(&format!(
        "pct set {id} -mp{} {},mp={}",
        mount.index,
        shell::quote(&mount.source),
        shell::quote(&mount.target),
    ))?;
    ssh.run(&format!("pct start {id} 2>/dev/null || true"))?;
    ssh.run(&format!(
        "pct exec {id} -- sh -c 'apt-get update && apt-get install -y proxmox-backup-server'"
    ))?;
    Ok(())
}

pub(super) fn cache_mount(repo: &LocalState) -> Option<&RestoreMount> {
    let bootstrap = &repo.restore.pbs_bootstrap;
    repo.restore
        .reattach_mounts
        .iter()
        .find(|mount| mount.vmid == bootstrap.vmid && mount.target == bootstrap.cache_path)
}
