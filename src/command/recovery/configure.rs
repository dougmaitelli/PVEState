use crate::{client::RemoteHost, config::LocalState};
use anyhow::{Context, Result};

pub(super) fn run(repo: &LocalState, ssh: &dyn RemoteHost) -> Result<()> {
    let command = repo
        .restore
        .application
        .configure_command
        .as_deref()
        .context("application.configure_command")?;
    ssh.run(command)?;
    Ok(())
}
