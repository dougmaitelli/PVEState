use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

pub fn write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().context("atomic file parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.pves-tmp",
        path.file_name()
            .context("atomic file name")?
            .to_string_lossy()
    ));
    let mut file =
        File::create(&temporary).with_context(|| format!("create {}", temporary.display()))?;
    file.write_all(content)?;
    if !content.ends_with(b"\n") {
        file.write_all(b"\n")?;
    }
    file.sync_all()?;
    publish(&temporary, path)?;
    sync_parent(parent)?;
    Ok(())
}

#[cfg(not(windows))]
fn publish(temporary: &Path, path: &Path) -> Result<()> {
    fs::rename(temporary, path).with_context(|| format!("publish {}", path.display()))
}

#[cfg(windows)]
fn publish(temporary: &Path, path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("replace {}", path.display()))?;
    }
    fs::rename(temporary, path).with_context(|| format!("publish {}", path.display()))
}

#[cfg(not(windows))]
fn sync_parent(parent: &Path) -> Result<()> {
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn sync_parent(_: &Path) -> Result<()> {
    // Windows does not allow opening a directory as a regular File. The
    // temporary file itself is flushed before publication above.
    Ok(())
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write(path, &serde_json::to_vec_pretty(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_existing_file_without_leaving_temporary_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("plan.json");
        write(&path, b"before").unwrap();
        write(&path, b"after").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "after\n");
        assert!(!temp.path().join(".plan.json.pves-tmp").exists());
    }
}
