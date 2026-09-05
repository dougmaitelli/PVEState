use crate::{
    client::RemoteHost,
    config::LocalState,
    utility::{atomic_file, progress::EventSink},
};
use anyhow::Result;
use std::collections::BTreeMap;

pub(super) fn capture(
    repo: &LocalState,
    ssh: &dyn RemoteHost,
    events: &dyn EventSink,
) -> Result<Vec<String>> {
    let pbs_vmid = repo.backup.pbs.guest.vmid;
    let cluster_command = if repo.node.node.standalone {
        "pvecm status 2>&1 || true"
    } else {
        "pvecm status 2>&1"
    };
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
        ("pve_cluster", cluster_command.into()),
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
        events.operation(&format!("probe {name}"));
        host.insert(name, ssh.probe(&command)?);
    }
    let failures = host
        .iter()
        .filter(|(_, output)| !output.ok)
        .map(|(name, output)| {
            format!(
                "{name}: exit {:?}: {}",
                output.return_code,
                output.stderr.trim()
            )
        })
        .collect();
    atomic_file::write(
        &repo.runtime().join(crate::config::artifacts::HOST_LATEST),
        &serde_json::to_vec_pretty(&host)?,
    )?;
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SshOutput;
    use crate::config;
    use std::fs;

    struct FakeSsh;

    impl RemoteHost for FakeSsh {
        fn host_key_fingerprint(&self) -> Result<String> {
            Ok("SHA256:fixture".into())
        }

        fn run(&self, _: &str) -> Result<String> {
            Ok(String::new())
        }

        fn probe(&self, command: &str) -> Result<SshOutput> {
            Ok(SshOutput {
                ok: true,
                return_code: Some(0),
                stdout: format!("fixture for {command}"),
                stderr: String::new(),
            })
        }

        fn stdin(&self, _: &str, _: &[u8]) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn host_capture_accepts_an_injected_ssh_client() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("environment");
        config::scaffold::initialize(&root).unwrap();
        let repo = config::open(&root).unwrap();
        fs::create_dir_all(repo.runtime()).unwrap();

        assert!(
            capture(&repo, &FakeSsh, &crate::utility::progress::NullEventSink)
                .unwrap()
                .is_empty()
        );
        assert!(
            repo.runtime()
                .join(crate::config::artifacts::HOST_LATEST)
                .is_file()
        );
    }
}
