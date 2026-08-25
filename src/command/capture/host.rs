use crate::{client::Ssh, config::Repository};
use anyhow::Result;
use std::{collections::BTreeMap, fs};

pub(super) fn capture(repo: &Repository, ssh: &Ssh) -> Result<()> {
    let pbs_vmid = repo.backup.pbs.guest.vmid;
    let mut host = BTreeMap::new();
    for (name, command) in [
        ("identity", "id; hostname; uname -a; pveversion -v".into()),
        (
            "network",
            "ip -json address show; ip -json route show; cat /etc/network/interfaces".into(),
        ),
        (
            "block_devices",
            "lsblk --json --bytes -o NAME,PATH,SIZE,TYPE,FSTYPE,MOUNTPOINTS,MODEL,SERIAL".into(),
        ),
        (
            "mounts",
            "findmnt --json --bytes -o TARGET,SOURCE,FSTYPE,OPTIONS,SIZE,USED,AVAIL".into(),
        ),
        ("zpool", "zpool status 2>&1; zpool list -Hp 2>&1".into()),
        (
            "zfs",
            "zfs list -Hp -o name,used,available,referenced,mountpoint 2>&1".into(),
        ),
        ("pve_storage", "pvesm status".into()),
        ("pve_cluster", "pvecm status 2>&1".into()),
        (
            "services",
            "systemctl --no-pager --plain --state=failed 2>&1; systemctl is-enabled pveproxy pvedaemon pvestatd pve-cluster".into(),
        ),
        (
            "pbs_identity",
            format!("pct exec {pbs_vmid} -- sh -c 'hostname; uname -a; proxmox-backup-manager versions --output-format json'"),
        ),
        (
            "pbs_datastores",
            format!("pct exec {pbs_vmid} -- proxmox-backup-manager datastore list --output-format json"),
        ),
        (
            "pbs_mounts",
            format!("pct exec {pbs_vmid} -- findmnt --json --bytes -o TARGET,SOURCE,FSTYPE,OPTIONS,SIZE,USED,AVAIL"),
        ),
    ] {
        host.insert(name, ssh.probe(&command)?);
    }
    fs::write(
        repo.runtime().join("host-latest.json"),
        serde_json::to_vec_pretty(&host)?,
    )?;
    Ok(())
}
