use super::RepositoryLayout;
use crate::utility::atomic_file;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

const SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum TransactionState {
    Preparing,
    Ready,
    Publishing,
    Complete,
    RollingBack,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum FileState {
    Prepared,
    Published,
    RolledBack,
}

#[derive(Debug, Deserialize, Serialize)]
struct PreparedFile {
    path: String,
    staged: String,
    backup: Option<String>,
    state: FileState,
}

#[derive(Debug, Deserialize, Serialize)]
struct LocalTransactionJournal {
    schema_version: u8,
    id: String,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    state: TransactionState,
    failure: Option<String>,
    files: Vec<PreparedFile>,
}

impl LocalTransactionJournal {
    fn path(layout: &RepositoryLayout) -> std::path::PathBuf {
        layout
            .runtime()
            .join(crate::config::artifacts::ADOPT_TRANSACTION)
    }

    fn persist(&self, layout: &RepositoryLayout) -> Result<()> {
        atomic_file::write_json(&Self::path(layout), self)
    }

    fn transaction_dir(&self, layout: &RepositoryLayout) -> std::path::PathBuf {
        layout.runtime().join(format!("adopt-{}", self.id))
    }
}

pub(crate) fn publish(
    layout: &RepositoryLayout,
    documents: &BTreeMap<String, String>,
) -> Result<()> {
    publish_with(layout, documents, atomic_file::write)
}

pub(crate) fn publish_with(
    layout: &RepositoryLayout,
    documents: &BTreeMap<String, String>,
    mut writer: impl FnMut(&Path, &[u8]) -> Result<()>,
) -> Result<()> {
    validate_paths(documents)?;
    recover_with(layout, &mut writer)?;

    let started_at = Utc::now();
    let id = started_at.format("%Y%m%dT%H%M%S%.fZ").to_string();
    let mut journal = LocalTransactionJournal {
        schema_version: SCHEMA_VERSION,
        id,
        started_at,
        finished_at: None,
        state: TransactionState::Preparing,
        failure: None,
        files: Vec::new(),
    };
    journal.persist(layout)?;
    let transaction_dir = journal.transaction_dir(layout);
    fs::create_dir_all(&transaction_dir)?;

    for (index, (path, content)) in documents.iter().enumerate() {
        let staged = format!("new-{index}");
        let backup = format!("old-{index}");
        atomic_file::write(&transaction_dir.join(&staged), content.as_bytes())?;
        let target = layout.root().join(path);
        let backup = if target.exists() {
            atomic_file::write(&transaction_dir.join(&backup), &fs::read(&target)?)?;
            Some(backup)
        } else {
            None
        };
        journal.files.push(PreparedFile {
            path: path.clone(),
            staged,
            backup,
            state: FileState::Prepared,
        });
        journal.persist(layout)?;
    }

    journal.state = TransactionState::Ready;
    journal.persist(layout)?;
    if let Err(error) = finish_publication(layout, &mut journal, &mut writer) {
        journal.state = TransactionState::RollingBack;
        journal.failure = Some(format!("{error:#}"));
        let _ = journal.persist(layout);
        rollback(layout, &mut journal, &mut writer)?;
        journal.state = TransactionState::Failed;
        journal.finished_at = Some(Utc::now());
        journal.persist(layout)?;
        persist_latest(layout, &journal)?;
        return Err(error).context("publish adopted documents; originals restored");
    }
    persist_latest(layout, &journal)
}

pub(crate) fn recover(layout: &RepositoryLayout) -> Result<()> {
    recover_with(layout, &mut atomic_file::write)
}

fn recover_with(
    layout: &RepositoryLayout,
    writer: &mut impl FnMut(&Path, &[u8]) -> Result<()>,
) -> Result<()> {
    let path = LocalTransactionJournal::path(layout);
    if !path.exists() {
        return Ok(());
    }
    let mut journal: LocalTransactionJournal =
        serde_json::from_slice(&fs::read(&path).context("read interrupted adoption transaction")?)
            .context("parse interrupted adoption transaction")?;
    if journal.schema_version != SCHEMA_VERSION {
        bail!("unsupported adoption transaction schema; manual recovery required")
    }
    validate_journal(&journal)?;
    match journal.state {
        TransactionState::Ready | TransactionState::Publishing => {
            finish_publication(layout, &mut journal, writer)?;
            persist_latest(layout, &journal)?;
        },
        TransactionState::Preparing | TransactionState::RollingBack => {
            rollback(layout, &mut journal, writer)?;
            journal.state = TransactionState::Failed;
            journal.finished_at = Some(Utc::now());
            journal.persist(layout)?;
            persist_latest(layout, &journal)?;
        },
        TransactionState::Complete | TransactionState::Failed => {},
    }
    Ok(())
}

fn finish_publication(
    layout: &RepositoryLayout,
    journal: &mut LocalTransactionJournal,
    writer: &mut impl FnMut(&Path, &[u8]) -> Result<()>,
) -> Result<()> {
    journal.state = TransactionState::Publishing;
    journal.persist(layout)?;
    let transaction_dir = journal.transaction_dir(layout);
    for index in 0..journal.files.len() {
        if journal.files[index].state == FileState::Published {
            continue;
        }
        let staged = fs::read(transaction_dir.join(&journal.files[index].staged))?;
        writer(&layout.root().join(&journal.files[index].path), &staged)?;
        journal.files[index].state = FileState::Published;
        journal.persist(layout)?;
    }
    journal.state = TransactionState::Complete;
    journal.finished_at = Some(Utc::now());
    journal.persist(layout)
}

fn rollback(
    layout: &RepositoryLayout,
    journal: &mut LocalTransactionJournal,
    writer: &mut impl FnMut(&Path, &[u8]) -> Result<()>,
) -> Result<()> {
    let transaction_dir = journal.transaction_dir(layout);
    for index in (0..journal.files.len()).rev() {
        let file = &journal.files[index];
        if file.state == FileState::RolledBack {
            continue;
        }
        let target = layout.root().join(&file.path);
        if let Some(backup) = &file.backup {
            writer(&target, &fs::read(transaction_dir.join(backup))?)?;
        } else if target.exists() {
            fs::remove_file(&target)?;
        }
        journal.files[index].state = FileState::RolledBack;
        journal.persist(layout)?;
    }
    Ok(())
}

fn persist_latest(layout: &RepositoryLayout, journal: &LocalTransactionJournal) -> Result<()> {
    atomic_file::write_json(
        &layout
            .runtime()
            .join(crate::config::artifacts::ADOPT_LATEST),
        journal,
    )
}

fn validate_journal(journal: &LocalTransactionJournal) -> Result<()> {
    let documents = journal
        .files
        .iter()
        .map(|file| (file.path.clone(), String::new()))
        .collect::<BTreeMap<_, _>>();
    validate_paths(&documents)?;
    for file in &journal.files {
        validate_artifact_name(&file.staged)?;
        if let Some(backup) = &file.backup {
            validate_artifact_name(backup)?;
        }
    }
    Ok(())
}

fn validate_artifact_name(name: &str) -> Result<()> {
    let path = Path::new(name);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        bail!("unsafe adoption transaction artifact {name}")
    }
    Ok(())
}

pub(crate) fn validate_paths(documents: &BTreeMap<String, String>) -> Result<()> {
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
    use crate::config;

    fn interrupted_transaction(
        state: TransactionState,
        first_file_state: FileState,
    ) -> (tempfile::TempDir, RepositoryLayout) {
        let temp = tempfile::tempdir().unwrap();
        config::scaffold::initialize(temp.path()).unwrap();
        let layout = RepositoryLayout::new(temp.path());
        let transaction_dir = layout.runtime().join("adopt-interrupted");
        fs::create_dir_all(&transaction_dir).unwrap();
        let paths = ["config/cluster.yml", "config/node.yml"];
        let mut files = Vec::new();

        for (index, path) in paths.iter().enumerate() {
            let staged = format!("new-{index}");
            let backup = format!("old-{index}");
            let original = fs::read_to_string(layout.root().join(path)).unwrap();
            let desired = if index == 0 {
                original.replace("example", "adopted")
            } else {
                original.replace("name: pve", "name: adopted-node")
            };
            atomic_file::write(&transaction_dir.join(&staged), desired.as_bytes()).unwrap();
            atomic_file::write(
                &transaction_dir.join(&backup),
                &fs::read(layout.root().join(path)).unwrap(),
            )
            .unwrap();
            files.push(PreparedFile {
                path: (*path).into(),
                staged,
                backup: Some(backup),
                state: if index == 0 {
                    first_file_state
                } else {
                    FileState::Prepared
                },
            });
        }
        if first_file_state == FileState::Published {
            let desired = fs::read(transaction_dir.join("new-0")).unwrap();
            atomic_file::write(&layout.root().join(paths[0]), &desired).unwrap();
        }
        LocalTransactionJournal {
            schema_version: SCHEMA_VERSION,
            id: "interrupted".into(),
            started_at: Utc::now(),
            finished_at: None,
            state,
            failure: None,
            files,
        }
        .persist(&layout)
        .unwrap();
        (temp, layout)
    }

    #[test]
    fn loader_finishes_interrupted_publication_idempotently() {
        let (_temp, layout) =
            interrupted_transaction(TransactionState::Publishing, FileState::Published);

        config::open(layout.root()).unwrap();
        recover(&layout).unwrap();

        assert!(
            fs::read_to_string(layout.root().join("config/cluster.yml"))
                .unwrap()
                .contains("adopted")
        );
        assert!(
            fs::read_to_string(layout.root().join("config/node.yml"))
                .unwrap()
                .contains("adopted-node")
        );
        let journal: LocalTransactionJournal =
            serde_json::from_slice(&fs::read(LocalTransactionJournal::path(&layout)).unwrap())
                .unwrap();
        assert_eq!(journal.state, TransactionState::Complete);
    }

    #[test]
    fn loader_finishes_ready_transaction_before_loading_documents() {
        let (_temp, layout) = interrupted_transaction(TransactionState::Ready, FileState::Prepared);

        recover(&layout).unwrap();

        assert!(
            fs::read_to_string(layout.root().join("config/cluster.yml"))
                .unwrap()
                .contains("adopted")
        );
        assert!(
            fs::read_to_string(layout.root().join("config/node.yml"))
                .unwrap()
                .contains("adopted-node")
        );
    }

    #[test]
    fn loader_rolls_back_interrupted_rollback_idempotently() {
        let (_temp, layout) =
            interrupted_transaction(TransactionState::RollingBack, FileState::Published);
        let original = fs::read(layout.runtime().join("adopt-interrupted/old-0")).unwrap();

        recover(&layout).unwrap();
        recover(&layout).unwrap();

        assert_eq!(
            fs::read(layout.root().join("config/cluster.yml")).unwrap(),
            original
        );
    }
}
