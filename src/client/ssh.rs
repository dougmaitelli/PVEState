use crate::config::env;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
pub struct Ssh {
    host: String,
    port: String,
    user: String,
    key: String,
    known: String,
}

#[derive(Debug, Serialize)]
pub struct SshOutput {
    pub ok: bool,
    pub return_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Ssh {
    pub fn discovery(root: &Path) -> Result<Self> {
        Ok(Self {
            host: env("PVE_HOST", None)?,
            port: env("PVE_SSH_PORT", Some("22"))?,
            user: env("PVE_SSH_USER", Some("root"))?,
            key: root.join(".secrets/pve_discovery").display().to_string(),
            known: root.join(".secrets/known_hosts").display().to_string(),
        })
    }
    pub fn mutation() -> Result<Self> {
        Ok(Self {
            host: env("PVE_HOST", None)?,
            port: env("PVE_SSH_PORT", Some("22"))?,
            user: env("PVE_SSH_USER", Some("root"))?,
            key: env("IAC_APPLY_SSH_KEY", None)?,
            known: env("IAC_APPLY_KNOWN_HOSTS", None)?,
        })
    }
    pub fn recovery(target: &str) -> Result<Self> {
        Ok(Self {
            host: target.into(),
            port: std::env::var("IAC_TARGET_SSH_PORT").unwrap_or_else(|_| "22".into()),
            user: std::env::var("IAC_TARGET_SSH_USER").unwrap_or_else(|_| "root".into()),
            key: env("IAC_TARGET_SSH_KEY", None)?,
            known: env("IAC_TARGET_KNOWN_HOSTS", None)?,
        })
    }
    fn command(&self, remote: &str) -> Command {
        let mut c = Command::new("ssh");
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
    pub fn run(&self, remote: &str) -> Result<String> {
        let output = self.probe(remote)?;
        if !output.ok {
            bail!("ssh failed: {}", output.stderr)
        }
        Ok(output.stdout)
    }

    pub fn probe(&self, remote: &str) -> Result<SshOutput> {
        let output = self.command(remote).output().context("start ssh")?;
        Ok(SshOutput {
            ok: output.status.success(),
            return_code: output.status.code(),
            stdout: String::from_utf8(output.stdout)?,
            stderr: String::from_utf8(output.stderr)?,
        })
    }
    pub fn stdin(&self, remote: &str, input: &[u8]) -> Result<()> {
        let mut c = self.command(remote);
        let mut p = c.stdin(Stdio::piped()).spawn()?;
        p.stdin.take().unwrap().write_all(input)?;
        if !p.wait()?.success() {
            bail!("ssh operation failed")
        }
        Ok(())
    }
}
