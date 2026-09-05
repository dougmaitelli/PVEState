use super::{ApplyReport, resource};
use crate::{
    reconcile::{Operation, Plan},
    utility::atomic_file,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ApplyStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum OperationStatus {
    Pending,
    Running,
    Applied,
    Failed,
}

#[derive(Debug, Serialize)]
pub(super) struct ApplyJournal {
    pub(crate) schema_version: u8,
    pub(crate) apply_id: String,
    pub(crate) plan_sha256: String,
    pub(crate) target: String,
    pub(crate) pbs_target: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) finished_at: Option<DateTime<Utc>>,
    pub(crate) status: ApplyStatus,
    pub(crate) failure: Option<String>,
    pub(crate) operations: Vec<JournalOperation>,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    latest: PathBuf,
}

#[derive(Debug, Serialize)]
pub(super) struct JournalOperation {
    pub(crate) index: usize,
    pub(crate) domain: String,
    pub(crate) resource: String,
    pub(crate) action: &'static str,
    pub(crate) status: OperationStatus,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) finished_at: Option<DateTime<Utc>>,
    pub(crate) error: Option<String>,
}

impl ApplyJournal {
    pub(crate) fn new(runtime: &Path, plan: &Plan) -> Self {
        let started_at = Utc::now();
        let apply_id = started_at.format("%Y%m%dT%H%M%S%.fZ").to_string();
        Self {
            schema_version: 1,
            path: runtime.join(format!("apply-{apply_id}.json")),
            latest: runtime.join(crate::config::artifacts::APPLY_LATEST),
            apply_id,
            plan_sha256: plan.plan_sha256.clone(),
            target: plan.target.clone(),
            pbs_target: plan.pbs_target.clone(),
            started_at,
            finished_at: None,
            status: ApplyStatus::Running,
            failure: None,
            operations: plan
                .operations
                .iter()
                .enumerate()
                .map(|(index, operation)| JournalOperation {
                    index,
                    domain: operation.domain().into(),
                    resource: resource(operation),
                    action: action(operation),
                    status: OperationStatus::Pending,
                    started_at: None,
                    finished_at: None,
                    error: None,
                })
                .collect(),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn start(&mut self, index: usize) -> Result<()> {
        let operation = self
            .operations
            .get_mut(index)
            .context("journal operation")?;
        operation.status = OperationStatus::Running;
        operation.started_at = Some(Utc::now());
        Ok(())
    }

    pub(crate) fn applied(&mut self, index: usize) -> Result<()> {
        let operation = self
            .operations
            .get_mut(index)
            .context("journal operation")?;
        operation.status = OperationStatus::Applied;
        operation.finished_at = Some(Utc::now());
        Ok(())
    }

    pub(crate) fn operation_failed(&mut self, index: usize, error: &anyhow::Error) -> Result<()> {
        let operation = self
            .operations
            .get_mut(index)
            .context("journal operation")?;
        operation.status = OperationStatus::Failed;
        operation.finished_at = Some(Utc::now());
        operation.error = Some(format!("{error:#}"));
        self.fail(error);
        Ok(())
    }

    pub(crate) fn succeed(&mut self) {
        self.status = ApplyStatus::Succeeded;
        self.finished_at = Some(Utc::now());
    }

    pub(crate) fn fail(&mut self, error: &anyhow::Error) {
        self.status = ApplyStatus::Failed;
        self.finished_at = Some(Utc::now());
        self.failure = Some(format!("{error:#}"));
    }

    pub(crate) fn persist(&self) -> Result<()> {
        atomic_file::write_json(&self.path, self)?;
        atomic_file::write_json(&self.latest, self)?;
        Ok(())
    }

    pub(crate) fn report(&self) -> ApplyReport {
        ApplyReport {
            journal_id: self.apply_id.clone(),
            journal_path: self.path.display().to_string(),
            completed: self
                .operations
                .iter()
                .filter(|operation| operation.status == OperationStatus::Applied)
                .count(),
            failed: self.failure.clone(),
        }
    }
}

fn action(operation: &Operation) -> &'static str {
    match operation {
        Operation::ApiMutation { .. } => "api-mutation",
        Operation::GrowDisk { .. } => "grow-disk",
        Operation::WriteFile { .. } => "write-file",
        Operation::DeleteFile { .. } => "delete-file",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconcile::Plan;
    use std::fs;

    fn plan() -> Plan {
        Plan {
            schema_version: 3,
            created_at: Utc::now(),
            capture_id: "fixture-capture".into(),
            target: "https://pve.test:8006".into(),
            pbs_target: "https://pbs.test:8007".into(),
            operations: Vec::new(),
            blockers: Vec::new(),
            plan_sha256: "digest".into(),
        }
    }

    fn plan_with_operation() -> Plan {
        let mut plan = plan();
        plan.operations.push(Operation::DeleteFile {
            domain: crate::reconcile::Domain::Firewall,
            resource: "guest/101".into(),
            target: crate::reconcile::ManagedFile::GuestFirewall { vmid: 101 },
            before_content: "old firewall".into(),
            before_sha256: "before".into(),
        });
        plan
    }

    #[test]
    fn completed_journal_is_persisted_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let mut journal = ApplyJournal::new(temp.path(), &plan());
        journal.persist().unwrap();
        journal.succeed();
        journal.persist().unwrap();

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(journal.path()).unwrap()).unwrap();
        assert_eq!(value["status"], "succeeded");
        assert!(
            temp.path()
                .join(crate::config::artifacts::APPLY_LATEST)
                .is_file()
        );
    }

    #[test]
    fn failed_operation_preserves_partial_progress() {
        let temp = tempfile::tempdir().unwrap();
        let mut journal = ApplyJournal::new(temp.path(), &plan_with_operation());
        journal.persist().unwrap();
        journal.start(0).unwrap();
        journal.persist().unwrap();
        journal
            .operation_failed(0, &anyhow::anyhow!("remote rejected mutation"))
            .unwrap();
        journal.persist().unwrap();

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(journal.path()).unwrap()).unwrap();
        assert_eq!(value["status"], "failed");
        assert_eq!(value["operations"][0]["status"], "failed");
        assert_eq!(value["operations"][0]["error"], "remote rejected mutation");
    }
}
