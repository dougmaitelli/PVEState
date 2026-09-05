use crate::{client::RemoteHost, utility::shell};
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;

pub(crate) struct WriteOptions<'a> {
    pub(crate) mode: &'a str,
    pub(crate) expected_sha256: Option<&'a str>,
    pub(crate) verify_expected: bool,
    pub(crate) backup_existing: bool,
}

pub(crate) fn write(
    ssh: &dyn RemoteHost,
    path: &str,
    content: &str,
    options: WriteOptions<'_>,
) -> Result<()> {
    if options.verify_expected {
        shell::verify_remote_file(ssh, path, options.expected_sha256)?;
    }

    let encoded = STANDARD.encode(content);
    let temporary = format!("{path}.pves-new");
    let backup = format!(
        "/root/pves-preapply/{}{path}",
        Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let backup_command = if options.backup_existing {
        let parent = std::path::Path::new(&backup)
            .parent()
            .and_then(std::path::Path::to_str)
            .context("backup parent")?;
        format!(
            "install -d {} && if test -e {}; then cp -a {} {}; fi && ",
            shell::quote(parent),
            shell::quote(path),
            shell::quote(path),
            shell::quote(&backup),
        )
    } else {
        String::new()
    };
    let command = format!(
        "{backup_command}base64 -d > {} && install -m {} {} {} && rm -f {}",
        shell::quote(&temporary),
        options.mode,
        shell::quote(&temporary),
        shell::quote(path),
        shell::quote(&temporary),
    );
    ssh.stdin(&command, encoded.as_bytes())
}
