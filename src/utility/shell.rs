use crate::client::RemoteHost;
use anyhow::{Result, bail};

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn verify_remote_file(ssh: &dyn RemoteHost, path: &str, expected: Option<&str>) -> Result<()> {
    let current = ssh.run(&format!(
        "if test -e {}; then sha256sum {} | cut -d ' ' -f 1; else printf absent; fi",
        quote(path),
        quote(path)
    ))?;
    let matches = match expected {
        Some(hash) => current.trim() == hash,
        None => current.trim() == "absent",
    };
    if !matches {
        bail!("remote file changed after plan: {path}")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(quote("a'b"), "'a'\\''b'");
    }
}
