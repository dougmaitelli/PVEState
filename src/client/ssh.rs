use super::RemoteHost;
use crate::settings::SshTarget;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    io::Write,
    process::{Command, Stdio},
};
pub(crate) struct Ssh {
    host: String,
    port: String,
    user: String,
    key: String,
    known: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SshOutput {
    pub(crate) ok: bool,
    pub(crate) return_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Ssh {
    pub(crate) fn new(settings: &SshTarget) -> Self {
        Self {
            host: settings.host.clone(),
            port: settings.port.to_string(),
            user: settings.user.clone(),
            key: settings.key.display().to_string(),
            known: settings.known_hosts.display().to_string(),
        }
    }
    fn command(&self, remote: &str) -> Command {
        self.command_with_options(remote, false)
    }

    fn command_with_options(&self, remote: &str, verbose: bool) -> Command {
        let mut c = Command::new("ssh");
        if verbose {
            c.arg("-v");
        }
        c.args([
            "-p",
            &self.port,
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            &format!("UserKnownHostsFile={}", self.known),
            "-i",
            &self.key,
            &format!("{}@{}", self.user, self.host),
            remote,
        ]);
        c
    }

    pub(crate) fn host_key_fingerprint(&self) -> Result<String> {
        let output = self
            .command_with_options("true", true)
            .output()
            .context("probe SSH host key")?;
        if !output.status.success() {
            bail!(
                "SSH host-key probe failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }
        let stderr = String::from_utf8(output.stderr)?;
        stderr
            .lines()
            .find_map(|line| {
                line.split_whitespace()
                    .find(|field| field.starts_with("SHA256:"))
                    .map(str::to_owned)
            })
            .context("SSH did not report the negotiated host-key fingerprint")
    }
    pub(crate) fn run(&self, remote: &str) -> Result<String> {
        let output = self.probe(remote)?;
        if !output.ok {
            bail!("ssh failed: {}", output.stderr)
        }
        Ok(output.stdout)
    }

    pub(crate) fn probe(&self, remote: &str) -> Result<SshOutput> {
        let output = self.command(remote).output().context("start ssh")?;
        Ok(SshOutput {
            ok: output.status.success(),
            return_code: output.status.code(),
            stdout: String::from_utf8(output.stdout)?,
            stderr: String::from_utf8(output.stderr)?,
        })
    }
    pub(crate) fn stdin(&self, remote: &str, input: &[u8]) -> Result<()> {
        let mut c = self.command(remote);
        let mut p = c.stdin(Stdio::piped()).spawn()?;
        p.stdin.take().unwrap().write_all(input)?;
        if !p.wait()?.success() {
            bail!("ssh operation failed")
        }
        Ok(())
    }
}

impl RemoteHost for Ssh {
    fn host_key_fingerprint(&self) -> Result<String> {
        self.host_key_fingerprint()
    }
    fn run(&self, remote: &str) -> Result<String> {
        self.run(remote)
    }
    fn probe(&self, remote: &str) -> Result<SshOutput> {
        self.probe(remote)
    }
    fn stdin(&self, remote: &str, input: &[u8]) -> Result<()> {
        self.stdin(remote, input)
    }
}
