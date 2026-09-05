use crate::{
    config::{self, LocalState},
    utility::atomic_file,
};
use anyhow::{Context, Result};
use std::{collections::BTreeMap, fs};

pub(super) fn validate(repo: &LocalState, documents: &BTreeMap<String, String>) -> Result<()> {
    crate::config::transaction::validate_paths(documents)?;
    let staging = tempfile::Builder::new()
        .prefix("adopt-validation-")
        .tempdir_in(repo.runtime())?;
    fs::create_dir(staging.path().join("config"))?;
    fs::copy(
        repo.root().join("pves.yml"),
        staging.path().join("pves.yml"),
    )?;
    for entry in fs::read_dir(repo.root().join("config"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(
                entry.path(),
                staging.path().join("config").join(entry.file_name()),
            )?;
        }
    }
    for (path, content) in documents {
        atomic_file::write(&staging.path().join(path), content.as_bytes())?;
    }
    config::open(staging.path()).context("validate complete adopted configuration")?;
    Ok(())
}

pub(super) fn publish(repo: &LocalState, documents: &BTreeMap<String, String>) -> Result<()> {
    crate::config::transaction::publish(&repo.layout, documents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use std::cell::Cell;

    fn repository() -> (tempfile::TempDir, LocalState) {
        let temp = tempfile::tempdir().unwrap();
        config::scaffold::initialize(temp.path()).unwrap();
        let repo = config::open(temp.path()).unwrap();
        (temp, repo)
    }

    #[test]
    fn second_document_failure_restores_every_original() {
        let (_temp, repo) = repository();
        let paths = ["config/cluster.yml", "config/node.yml"];
        let before = paths.map(|path| fs::read(repo.root().join(path)).unwrap());
        let documents = BTreeMap::from([
            (
                paths[0].to_string(),
                "cluster: invalid-but-publishable".into(),
            ),
            (paths[1].to_string(), "node: invalid-but-publishable".into()),
        ]);
        let calls = Cell::new(0);

        let error =
            crate::config::transaction::publish_with(&repo.layout, &documents, |path, content| {
                let call = calls.get() + 1;
                calls.set(call);
                if call == 2 {
                    bail!("injected second-file failure")
                }
                atomic_file::write(path, content)
            })
            .unwrap_err();

        assert!(format!("{error:#}").contains("injected second-file failure"));
        assert_eq!(fs::read(repo.root().join(paths[0])).unwrap(), before[0]);
        assert_eq!(fs::read(repo.root().join(paths[1])).unwrap(), before[1]);
        let journal: serde_json::Value = serde_json::from_slice(
            &fs::read(repo.runtime().join(crate::config::artifacts::ADOPT_LATEST)).unwrap(),
        )
        .unwrap();
        assert_eq!(journal["state"], "failed");
        assert_eq!(journal["files"][0]["state"], "rolled-back");
    }

    #[test]
    fn validation_failure_does_not_publish_any_document() {
        let (_temp, repo) = repository();
        let path = "config/guests.yml";
        let before = fs::read(repo.root().join(path)).unwrap();
        let documents = BTreeMap::from([(path.to_string(), "not: valid guests".into())]);

        assert!(validate(&repo, &documents).is_err());
        assert_eq!(fs::read(repo.root().join(path)).unwrap(), before);
        assert!(
            !repo
                .runtime()
                .join(crate::config::artifacts::ADOPT_LATEST)
                .exists()
        );
    }

    #[test]
    fn successful_multi_document_publication_replaces_every_file() {
        let (_temp, repo) = repository();
        let cluster = fs::read_to_string(repo.root().join("config/cluster.yml"))
            .unwrap()
            .replace("example", "adopted");
        let node = fs::read_to_string(repo.root().join("config/node.yml"))
            .unwrap()
            .replace("name: pve", "name: adopted-node");
        let documents = BTreeMap::from([
            ("config/cluster.yml".into(), cluster.clone()),
            ("config/node.yml".into(), node.clone()),
        ]);

        validate(&repo, &documents).unwrap();
        publish(&repo, &documents).unwrap();

        assert_eq!(
            fs::read_to_string(repo.root().join("config/cluster.yml")).unwrap(),
            cluster
        );
        assert_eq!(
            fs::read_to_string(repo.root().join("config/node.yml")).unwrap(),
            node
        );
        let journal: serde_json::Value = serde_json::from_slice(
            &fs::read(repo.runtime().join(crate::config::artifacts::ADOPT_LATEST)).unwrap(),
        )
        .unwrap();
        assert_eq!(journal["state"], "complete");
    }
}
