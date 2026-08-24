use anyhow::{Context, Result, bail};
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

impl Ssh {
    pub fn discovery(root: &Path) -> Result<Self> {
        Ok(Self {
            host: req("PVE_HOST")?,
            port: std::env::var("PVE_SSH_PORT").unwrap_or_else(|_| "22".into()),
            user: std::env::var("PVE_SSH_USER").unwrap_or_else(|_| "root".into()),
            key: root.join(".secrets/pve_discovery").display().to_string(),
            known: root.join(".secrets/known_hosts").display().to_string(),
        })
    }
    pub fn mutation() -> Result<Self> {
        Ok(Self {
            host: req("PVE_HOST")?,
            port: std::env::var("PVE_SSH_PORT").unwrap_or_else(|_| "22".into()),
            user: std::env::var("PVE_SSH_USER").unwrap_or_else(|_| "root".into()),
            key: req("IAC_APPLY_SSH_KEY")?,
            known: req("IAC_APPLY_KNOWN_HOSTS")?,
        })
    }
    pub fn recovery(target: &str) -> Result<Self> {
        Ok(Self {
            host: target.into(),
            port: std::env::var("IAC_TARGET_SSH_PORT").unwrap_or_else(|_| "22".into()),
            user: std::env::var("IAC_TARGET_SSH_USER").unwrap_or_else(|_| "root".into()),
            key: req("IAC_TARGET_SSH_KEY")?,
            known: req("IAC_TARGET_KNOWN_HOSTS")?,
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
        let o = self.command(remote).output().context("start ssh")?;
        if !o.status.success() {
            bail!("ssh failed: {}", String::from_utf8_lossy(&o.stderr))
        }
        Ok(String::from_utf8(o.stdout)?)
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

fn req(n: &str) -> Result<String> {
    std::env::var(n).with_context(|| format!("missing {n}"))
}
