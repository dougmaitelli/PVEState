use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};
use tempfile::Builder;

pub fn write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().context("atomic file parent")?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .context("atomic file name")?
        .to_string_lossy();
    // `tempfile` uses randomized names and exclusive create-new semantics. An
    // existing file or symlink is never opened or followed.
    let mut temporary = Builder::new()
        .prefix(&format!(".{name}."))
        .suffix(".pves-tmp")
        .tempfile_in(parent)
        .with_context(|| format!("create temporary file in {}", parent.display()))?;
    restrict_permissions(temporary.as_file())?;
    let file = temporary.as_file_mut();
    file.write_all(content)?;
    if !content.ends_with(b"\n") {
        file.write_all(b"\n")?;
    }
    file.sync_all()?;
    publish(temporary, path)?;
    sync_parent(parent)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(file: &File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_: &File) -> Result<()> {
    Ok(())
}

fn publish(temporary: tempfile::NamedTempFile, path: &Path) -> Result<()> {
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| error.error)
        .with_context(|| format!("publish {}", path.display()))
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
        assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".pves-tmp")
        }));
    }

    #[cfg(unix)]
    #[test]
    fn publishes_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("plan.json");
        write(&path, b"sensitive").unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn publication_replaces_a_symlink_without_following_it() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let victim = temp.path().join("victim");
        let path = temp.path().join("plan.json");
        fs::write(&victim, "unchanged").unwrap();
        symlink(&victim, &path).unwrap();

        write(&path, b"plan").unwrap();

        assert_eq!(fs::read_to_string(victim).unwrap(), "unchanged");
        assert_eq!(fs::read_to_string(path).unwrap(), "plan\n");
    }
}
