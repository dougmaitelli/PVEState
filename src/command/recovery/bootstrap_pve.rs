use crate::{
    client::RemoteHost,
    config::LocalState,
    resource::{firewall, native_paths, network},
    utility::{progress::EventSink, remote_file, shell},
};
use anyhow::Result;

pub(super) fn run(repo: &LocalState, ssh: &dyn RemoteHost, events: &dyn EventSink) -> Result<()> {
    events.operation("verify replacement PVE host");
    ssh.run("pveversion")?;

    for pool in &repo.storage.pools {
        ssh.run(&format!(
            "zpool list -H -o name {}",
            shell::quote(&pool.zpool)
        ))?;
    }
    for mount in repo
        .storage
        .host_mounts
        .iter()
        .filter(|mount| mount.required)
    {
        ssh.run(&format!("test -d {}", shell::quote(&mount.path)))?;
    }

    write(
        ssh,
        native_paths::NETWORK_REMOTE,
        &network::render::render(&repo.network),
        "0644",
    )?;
    if let Some(policy) = &repo.cluster.firewall {
        write(
            ssh,
            native_paths::CLUSTER_FIREWALL_REMOTE,
            &firewall::render::render(policy),
            "0640",
        )?;
    }
    if let Some(policy) = &repo.node.firewall {
        write(
            ssh,
            &native_paths::node_firewall_remote(&repo.node.node.name),
            &firewall::render::render(policy),
            "0640",
        )?;
    }
    events.finish(true);
    Ok(())
}

fn write(ssh: &dyn RemoteHost, path: &str, content: &str, mode: &str) -> Result<()> {
    remote_file::write(
        ssh,
        path,
        content,
        remote_file::WriteOptions {
            mode,
            expected_sha256: None,
            verify_expected: false,
            backup_existing: true,
        },
    )
}
