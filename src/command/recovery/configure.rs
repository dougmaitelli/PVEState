use crate::{client::RemoteHost, config::Repository};
use anyhow::{Context, Result};

pub(super) fn run(repo: &Repository, ssh: &dyn RemoteHost) -> Result<()> {
    let command = repo
        .restore
        .application
        .configure_command
        .as_deref()
        .context("application.configure_command")?;
    ssh.run(command)?;
    Ok(())
}
