use crate::{
    client::RemoteHost,
    config::LocalState,
    resource::{firewall, network},
    utility::{progress, remote_file, shell},
};
use anyhow::Result;

pub(super) fn run(repo: &LocalState, ssh: &dyn RemoteHost) -> Result<()> {
    progress::operation("verify replacement PVE host");
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
        "/etc/network/interfaces",
        &network::render::render(&repo.network),
        "0644",
    )?;
    if let Some(policy) = &repo.cluster.firewall {
        write(
            ssh,
            "/etc/pve/firewall/cluster.fw",
            &firewall::render::render(policy),
            "0640",
        )?;
    }
    if let Some(policy) = &repo.node.firewall {
        write(
            ssh,
            &format!("/etc/pve/nodes/{}/host.fw", repo.node.node.name),
            &firewall::render::render(policy),
            "0640",
        )?;
    }
    progress::finish(true);
    println!("replacement PVE configuration staged; activate networking only with console access");
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
