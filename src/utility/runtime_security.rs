use anyhow::{Context, Result, bail};
use std::{fs, path::Path};

pub(crate) fn prepare(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => {},
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => create(path)?,
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
    }
    validate_directory(path)
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    let parent = path.parent().context("runtime file parent")?;
    prepare(parent)?;
    validate_file(path)?;
    fs::read(path).with_context(|| format!("read secure runtime file {}", path.display()))
}

#[cfg(unix)]
fn create(path: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(path)
        .with_context(|| format!("create secure runtime directory {}", path.display()))
}

#[cfg(not(unix))]
fn create(path: &Path) -> Result<()> {
    fs::create_dir(path)
        .with_context(|| format!("create secure runtime directory {}", path.display()))
}

fn validate_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("runtime path must be a real directory: {}", path.display())
    }
    validate_unix_metadata(path, &metadata, true)
}

fn validate_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "runtime artifact must be a regular file: {}",
            path.display()
        )
    }
    validate_unix_metadata(path, &metadata, false)
}

#[cfg(unix)]
fn validate_unix_metadata(path: &Path, metadata: &fs::Metadata, directory: bool) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let expected_uid = rustix::process::geteuid().as_raw();
    if metadata.uid() != expected_uid {
        bail!(
            "{} is owned by uid {}, expected {}: {}",
            if directory {
                "runtime directory"
            } else {
                "runtime artifact"
            },
            metadata.uid(),
            expected_uid,
            path.display()
        )
    }
    let mode = metadata.mode() & 0o777;
    if mode & 0o077 != 0 {
        bail!(
            "{} permissions are too broad ({mode:o}): {}",
            if directory {
                "runtime directory"
            } else {
                "runtime artifact"
            },
            path.display()
        )
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_unix_metadata(_: &Path, _: &fs::Metadata, _: bool) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn creates_owner_only_runtime_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join(".runtime");
        prepare(&runtime).unwrap();
        assert_eq!(
            fs::metadata(runtime).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_permissive_runtime_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join(".runtime");
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            prepare(&runtime)
                .unwrap_err()
                .to_string()
                .contains("too broad")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_runtime_directory_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let runtime = temp.path().join(".runtime");
        fs::create_dir(&real).unwrap();
        symlink(real, &runtime).unwrap();
        assert!(
            prepare(&runtime)
                .unwrap_err()
                .to_string()
                .contains("real directory")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_permissive_runtime_artifact() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join(".runtime");
        let artifact = runtime.join(crate::config::artifacts::PRODUCTION_PLAN);
        prepare(&runtime).unwrap();
        fs::write(&artifact, "plan").unwrap();
        fs::set_permissions(&artifact, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            read(&artifact)
                .unwrap_err()
                .to_string()
                .contains("too broad")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_runtime_artifact_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join(".runtime");
        let victim = temp.path().join("victim");
        let artifact = runtime.join(crate::config::artifacts::PRODUCTION_PLAN);
        prepare(&runtime).unwrap();
        fs::write(&victim, "plan").unwrap();
        symlink(victim, &artifact).unwrap();
        assert!(
            read(&artifact)
                .unwrap_err()
                .to_string()
                .contains("regular file")
        );
    }
}
