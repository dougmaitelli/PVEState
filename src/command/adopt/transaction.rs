use crate::{config::Repository, utility::atomic_file};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

#[derive(Serialize)]
struct Journal {
    schema_version: u8,
    adoption_id: String,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    status: &'static str,
    failure: Option<String>,
    documents: Vec<DocumentStatus>,
}

#[derive(Serialize)]
struct DocumentStatus {
    path: String,
    status: &'static str,
}

impl Journal {
    fn new(documents: &BTreeMap<String, String>) -> Self {
        let started_at = Utc::now();
        Self {
            schema_version: 1,
            adoption_id: started_at.format("%Y%m%dT%H%M%S%.fZ").to_string(),
            started_at,
            finished_at: None,
            status: "publishing",
            failure: None,
            documents: documents
                .keys()
                .map(|path| DocumentStatus {
                    path: path.clone(),
                    status: "pending",
                })
                .collect(),
        }
    }

    fn persist(&self, runtime: &Path) -> Result<()> {
        atomic_file::write_json(
            &runtime.join(format!("adopt-{}.json", self.adoption_id)),
            self,
        )?;
        atomic_file::write_json(&runtime.join("adopt-latest.json"), self)
    }
}

pub(super) fn validate(repo: &Repository, documents: &BTreeMap<String, String>) -> Result<()> {
    validate_paths(documents)?;
    let staging = tempfile::Builder::new()
        .prefix("adopt-validation-")
        .tempdir_in(repo.runtime())?;
    fs::create_dir(staging.path().join("config"))?;
    fs::copy(repo.root.join("pves.yml"), staging.path().join("pves.yml"))?;
    for entry in fs::read_dir(repo.root.join("config"))? {
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
    Repository::open(staging.path()).context("validate complete adopted configuration")?;
    Ok(())
}

pub(super) fn publish(repo: &Repository, documents: &BTreeMap<String, String>) -> Result<()> {
    publish_with(repo, documents, atomic_file::write)
}

fn publish_with(
    repo: &Repository,
    documents: &BTreeMap<String, String>,
    mut writer: impl FnMut(&Path, &[u8]) -> Result<()>,
) -> Result<()> {
    validate_paths(documents)?;
    let originals = documents
        .keys()
        .map(|path| {
            fs::read(repo.root.join(path))
                .map(|content| (path.clone(), content))
                .with_context(|| format!("read original {path}"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut journal = Journal::new(documents);
    journal.persist(&repo.runtime())?;
    let mut published = Vec::new();

    for (index, (path, content)) in documents.iter().enumerate() {
        if let Err(error) = writer(&repo.root.join(path), content.as_bytes()) {
            journal.status = "rolling-back";
            journal.failure = Some(format!("publish {path}: {error:#}"));
            let _ = journal.persist(&repo.runtime());
            let rollback_failures = rollback(
                repo,
                documents,
                &originals,
                &published,
                &mut journal,
                &mut writer,
            );
            journal.status = "failed";
            journal.finished_at = Some(Utc::now());
            journal.persist(&repo.runtime())?;
            if !rollback_failures.is_empty() {
                bail!(
                    "adoption failed publishing {path}; rollback also failed: {}",
                    rollback_failures.join("; ")
                )
            }
            return Err(error).with_context(|| format!("publish adopted document {path}"));
        }
        journal.documents[index].status = "published";
        published.push(path.clone());
        if let Err(error) = journal.persist(&repo.runtime()) {
            journal.status = "rolling-back";
            journal.failure = Some(format!("persist adoption journal: {error:#}"));
            let rollback_failures = rollback(
                repo,
                documents,
                &originals,
                &published,
                &mut journal,
                &mut writer,
            );
            journal.status = "failed";
            journal.finished_at = Some(Utc::now());
            let _ = journal.persist(&repo.runtime());
            if !rollback_failures.is_empty() {
                bail!(
                    "adoption journal failed; rollback also failed: {}",
                    rollback_failures.join("; ")
                )
            }
            return Err(error).context("persist adoption journal; published documents restored");
        }
    }

    journal.status = "succeeded";
    journal.finished_at = Some(Utc::now());
    journal.persist(&repo.runtime())
}

fn rollback(
    repo: &Repository,
    documents: &BTreeMap<String, String>,
    originals: &BTreeMap<String, Vec<u8>>,
    published: &[String],
    journal: &mut Journal,
    writer: &mut impl FnMut(&Path, &[u8]) -> Result<()>,
) -> Vec<String> {
    let mut failures = Vec::new();
    for published_path in published.iter().rev() {
        let original = &originals[published_path];
        match writer(&repo.root.join(published_path), original) {
            Ok(()) => {
                let position = documents
                    .keys()
                    .position(|path| path == published_path)
                    .expect("published document belongs to transaction");
                journal.documents[position].status = "rolled-back";
            },
            Err(error) => failures.push(format!("{published_path}: {error:#}")),
        }
    }
    failures
}

fn validate_paths(documents: &BTreeMap<String, String>) -> Result<()> {
    for path in documents.keys() {
        let path = Path::new(path);
        if path.is_absolute()
            || !path.starts_with("config")
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            bail!("unsafe adopted document path {}", path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn repository() -> (tempfile::TempDir, Repository) {
        let temp = tempfile::tempdir().unwrap();
        Repository::initialize(temp.path()).unwrap();
        let repo = Repository::open(temp.path()).unwrap();
        (temp, repo)
    }

    #[test]
    fn second_document_failure_restores_every_original() {
        let (_temp, repo) = repository();
        let paths = ["config/cluster.yml", "config/node.yml"];
        let before = paths.map(|path| fs::read(repo.root.join(path)).unwrap());
        let documents = BTreeMap::from([
            (
                paths[0].to_string(),
                "cluster: invalid-but-publishable".into(),
            ),
            (paths[1].to_string(), "node: invalid-but-publishable".into()),
        ]);
        let calls = Cell::new(0);

        let error = publish_with(&repo, &documents, |path, content| {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 2 {
                bail!("injected second-file failure")
            }
            atomic_file::write(path, content)
        })
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected second-file failure"));
        assert_eq!(fs::read(repo.root.join(paths[0])).unwrap(), before[0]);
        assert_eq!(fs::read(repo.root.join(paths[1])).unwrap(), before[1]);
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(repo.runtime().join("adopt-latest.json")).unwrap())
                .unwrap();
        assert_eq!(journal["status"], "failed");
        assert_eq!(journal["documents"][0]["status"], "rolled-back");
    }

    #[test]
    fn validation_failure_does_not_publish_any_document() {
        let (_temp, repo) = repository();
        let path = "config/guests.yml";
        let before = fs::read(repo.root.join(path)).unwrap();
        let documents = BTreeMap::from([(path.to_string(), "not: valid guests".into())]);

        assert!(validate(&repo, &documents).is_err());
        assert_eq!(fs::read(repo.root.join(path)).unwrap(), before);
        assert!(!repo.runtime().join("adopt-latest.json").exists());
    }

    #[test]
    fn successful_multi_document_publication_replaces_every_file() {
        let (_temp, repo) = repository();
        let cluster = fs::read_to_string(repo.root.join("config/cluster.yml"))
            .unwrap()
            .replace("example", "adopted");
        let node = fs::read_to_string(repo.root.join("config/node.yml"))
            .unwrap()
            .replace("name: pve", "name: adopted-node");
        let documents = BTreeMap::from([
            ("config/cluster.yml".into(), cluster.clone()),
            ("config/node.yml".into(), node.clone()),
        ]);

        validate(&repo, &documents).unwrap();
        publish(&repo, &documents).unwrap();

        assert_eq!(
            fs::read_to_string(repo.root.join("config/cluster.yml")).unwrap(),
            cluster
        );
        assert_eq!(
            fs::read_to_string(repo.root.join("config/node.yml")).unwrap(),
            node
        );
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(repo.runtime().join("adopt-latest.json")).unwrap())
                .unwrap();
        assert_eq!(journal["status"], "succeeded");
    }
}
