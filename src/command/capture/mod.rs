mod host;
mod native;

use crate::{
    client::{PbsClient, PveClient, RemoteHost},
    config::LocalState,
    discovery::{
        CaptureManifest, CaptureStatus, SourceEvidence, capture_pbs, capture_pve,
        collect_artifacts, write_snapshot,
    },
    utility::{atomic_file, progress::EventSink, runtime_security},
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) fn run(
    repo: &LocalState,
    pve: &dyn PveClient,
    pbs: &dyn PbsClient,
    ssh: &dyn RemoteHost,
    events: &dyn EventSink,
) -> Result<()> {
    events.section("Capturing live state");
    events.detail(&format!("configuration: {}", repo.root().display()));
    runtime_security::prepare(&repo.runtime())?;
    fs::create_dir_all(repo.root().join("observed"))?;
    recover_observed(repo, events)?;
    let staging = tempfile::Builder::new()
        .prefix(".capture-")
        .tempdir_in(repo.root().join("observed"))?;
    let staged_observed = staging.path().join("production");
    fs::create_dir_all(staged_observed.join("api"))?;

    let started = Utc::now();
    let initial = CaptureManifest::new(
        started,
        BTreeMap::from([(
            "capture".into(),
            source("local", vec!["capture did not complete".into()]),
        )]),
        BTreeMap::new(),
    );
    write_runtime_manifest(repo, &initial)?;

    let sources = match perform(repo, &staged_observed, pve, pbs, ssh, events) {
        Ok(sources) => sources,
        Err(error) => {
            let failed = CaptureManifest::new(
                Utc::now(),
                BTreeMap::from([(
                    "capture".into(),
                    source("local", vec![format!("{error:#}")]),
                )]),
                collect_artifacts(&staged_observed)?,
            );
            write_runtime_manifest(repo, &failed)?;
            return Err(error);
        },
    };

    let manifest = CaptureManifest::new(Utc::now(), sources, collect_artifacts(&staged_observed)?);
    if manifest.status == CaptureStatus::Partial {
        write_runtime_manifest(repo, &manifest)?;
        bail!("capture is partial: {}", manifest.failures.join("; "))
    }
    write_observed_manifest(&staged_observed, &manifest)?;
    publish_observed(repo, &staged_observed, &manifest.capture_id, events)?;
    write_runtime_manifest(repo, &manifest)?;
    events.finish(true);
    events.output(&format!(
        "captured live state into {}",
        repo.root().display()
    ));
    Ok(())
}

fn perform(
    repo: &LocalState,
    observed: &Path,
    pve: &dyn PveClient,
    pbs: &dyn PbsClient,
    ssh: &dyn RemoteHost,
    events: &dyn EventSink,
) -> Result<BTreeMap<String, SourceEvidence>> {
    let mut sources = BTreeMap::new();

    events.section("Proxmox VE API");
    let pve_snapshot = capture_pve(pve, events);
    write_snapshot(
        "pve",
        pve_snapshot.collected_at,
        &pve_snapshot,
        &repo.runtime(),
        &observed.join("api"),
    )?;
    let pve_failures = pve_snapshot.failures();
    events.finish(pve_failures.is_empty());
    sources.insert("pve-api".into(), source(pve.endpoint(), pve_failures));

    events.section("Proxmox Backup Server API");
    let pbs_snapshot = capture_pbs(pbs, events);
    write_snapshot(
        "pbs",
        pbs_snapshot.collected_at,
        &pbs_snapshot,
        &repo.runtime(),
        &observed.join("api"),
    )?;
    let pbs_failures = pbs_snapshot.failures();
    events.finish(pbs_failures.is_empty());
    sources.insert("pbs-api".into(), source(pbs.endpoint(), pbs_failures));

    events.section("Native configuration files");
    let native_failures = native::export(repo, observed, ssh, &pve_snapshot, events)
        .err()
        .map(|error| vec![format!("{error:#}")])
        .unwrap_or_default();
    events.finish(native_failures.is_empty());
    sources.insert("native-ssh".into(), source(pve.endpoint(), native_failures));

    events.section("Host and PBS diagnostics");
    let host_failures = host::capture(repo, ssh, events)
        .map_err(|error| anyhow!("host evidence capture: {error:#}"))?;
    events.finish(host_failures.is_empty());
    sources.insert("host-ssh".into(), source(pve.endpoint(), host_failures));
    Ok(sources)
}

fn source(endpoint: &str, failures: Vec<String>) -> SourceEvidence {
    SourceEvidence {
        endpoint: endpoint.into(),
        required: true,
        complete: failures.is_empty(),
        failures,
    }
}

fn write_observed_manifest(observed: &Path, manifest: &CaptureManifest) -> Result<()> {
    let content = serde_json::to_vec_pretty(manifest)?;
    atomic_file::write(
        &observed.join(crate::config::artifacts::CAPTURE_MANIFEST),
        &content,
    )
}

fn write_runtime_manifest(repo: &LocalState, manifest: &CaptureManifest) -> Result<()> {
    let content = serde_json::to_vec_pretty(manifest)?;
    atomic_file::write(
        &repo
            .runtime()
            .join(format!("capture-manifest-{}.json", manifest.capture_id)),
        &content,
    )?;
    Ok(())
}

fn publish_observed(
    repo: &LocalState,
    staged: &Path,
    capture_id: &str,
    events: &dyn EventSink,
) -> Result<()> {
    let observed_root = repo.root().join("observed");
    let current = repo.observed();
    let previous_name = format!(".previous-{capture_id}");
    let previous = observed_root.join(&previous_name);
    let staged_name = staged
        .strip_prefix(&observed_root)
        .context("capture staging directory is outside observed root")?
        .to_string_lossy()
        .into_owned();
    let mut transaction = CaptureTransaction {
        schema_version: 1,
        capture_id: capture_id.into(),
        state: CaptureTransactionState::Ready,
        staged: staged_name,
        previous: previous_name,
        had_current: current.exists(),
    };
    transaction.persist(repo)?;

    let had_current = current.exists();
    if had_current {
        fs::rename(&current, &previous)?;
        atomic_file::sync_directory(&observed_root)?;
    }
    transaction.state = CaptureTransactionState::Publishing;
    transaction.persist(repo)?;
    if let Err(error) = fs::rename(staged, &current) {
        if had_current {
            fs::rename(&previous, &current)?;
            atomic_file::sync_directory(&observed_root)?;
        }
        return Err(error.into());
    }
    atomic_file::sync_directory(&observed_root)?;
    if had_current && let Err(error) = fs::remove_dir_all(&previous) {
        events.detail(&format!(
            "could not remove previous capture {}: {error}",
            previous.display()
        ));
    }
    transaction.state = CaptureTransactionState::Complete;
    transaction.persist(repo)?;
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct CaptureTransaction {
    schema_version: u8,
    capture_id: String,
    state: CaptureTransactionState,
    staged: String,
    previous: String,
    had_current: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum CaptureTransactionState {
    Ready,
    Publishing,
    Complete,
}

impl CaptureTransaction {
    fn path(repo: &LocalState) -> std::path::PathBuf {
        repo.runtime()
            .join(crate::config::artifacts::CAPTURE_TRANSACTION)
    }

    fn persist(&self, repo: &LocalState) -> Result<()> {
        atomic_file::write_json(&Self::path(repo), self)
    }
}

fn recover_observed(repo: &LocalState, events: &dyn EventSink) -> Result<()> {
    let journal_path = CaptureTransaction::path(repo);
    if !journal_path.exists() {
        return Ok(());
    }
    let mut transaction: CaptureTransaction = serde_json::from_slice(
        &fs::read(&journal_path).context("read interrupted capture transaction")?,
    )
    .context("parse interrupted capture transaction")?;
    if transaction.schema_version != 1 {
        bail!("unsupported capture transaction schema; manual recovery required")
    }
    validate_capture_path(&transaction.staged)?;
    validate_capture_path(&transaction.previous)?;
    if transaction.state == CaptureTransactionState::Complete {
        return Ok(());
    }

    events.detail(&format!(
        "recovering interrupted capture {}",
        transaction.capture_id
    ));
    let observed_root = repo.root().join("observed");
    let current = repo.observed();
    let staged = observed_root.join(&transaction.staged);
    let previous = observed_root.join(&transaction.previous);

    if staged.exists() {
        if current.exists() && !previous.exists() && transaction.had_current {
            fs::rename(&current, &previous)?;
            atomic_file::sync_directory(&observed_root)?;
        }
        if !current.exists() {
            fs::rename(&staged, &current)?;
            atomic_file::sync_directory(&observed_root)?;
        }
    } else if !current.exists() {
        if previous.exists() {
            fs::rename(&previous, &current)?;
            bail!(
                "interrupted capture {} lacked its staged generation; restored previous capture",
                transaction.capture_id
            )
        } else {
            bail!(
                "interrupted capture {} has no current, staged, or previous generation",
                transaction.capture_id
            )
        }
    }
    if previous.exists() {
        fs::remove_dir_all(&previous)?;
        atomic_file::sync_directory(&observed_root)?;
    }
    transaction.state = CaptureTransactionState::Complete;
    transaction.persist(repo)
}

fn validate_capture_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        bail!("unsafe capture transaction path {}", path.display())
    }
    Ok(())
}

pub(crate) fn validate(
    repo: &LocalState,
    ssh: &dyn RemoteHost,
    events: &dyn EventSink,
) -> Result<()> {
    runtime_security::prepare(&repo.runtime())?;
    let mut failures = Vec::new();
    let mut report = Vec::new();
    events.section("Running recovery validation checks");
    for check in &repo.recovery_checks.checks {
        events.operation(&format!("{}: {}", check.id, check.description));
        match ssh.run(&check.command) {
            Ok(_) => {
                events.detail("passed");
                report
                    .push(json!({"id":check.id,"description":check.description,"status":"passed"}))
            },
            Err(error) => {
                events.detail(&format!("failed: {error:#}"));
                failures.push(format!("{}: {error}", check.id));
                report
                    .push(json!({"id":check.id,"description":check.description,"status":"failed"}));
            },
        }
    }
    atomic_file::write(
        &repo.runtime().join(crate::config::artifacts::VALIDATION),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    if failures.is_empty() {
        events.finish(true);
        events.output(&format!("validation passed: {} checks", report.len()));
        Ok(())
    } else {
        bail!(failures.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn complete_capture_replaces_previous_observed_tree() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("environment");
        config::scaffold::initialize(&root).unwrap();
        let repo = config::open(&root).unwrap();
        fs::write(repo.observed().join("previous.txt"), "previous").unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".capture-")
            .tempdir_in(root.join("observed"))
            .unwrap();
        let staged = staging.path().join("production");
        fs::create_dir(&staged).unwrap();
        fs::write(staged.join("current.txt"), "current").unwrap();

        publish_observed(
            &repo,
            &staged,
            "fixture",
            &crate::utility::progress::NullEventSink,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(repo.observed().join("current.txt")).unwrap(),
            "current"
        );
        assert!(!repo.observed().join("previous.txt").exists());
        assert!(!root.join("observed/.previous-fixture").exists());
    }

    #[test]
    fn interrupted_capture_finishes_publication_on_restart() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("environment");
        config::scaffold::initialize(&root).unwrap();
        let repo = config::open(&root).unwrap();
        fs::write(repo.observed().join("old.txt"), "old").unwrap();
        let staged_parent = root.join("observed/.capture-interrupted");
        let staged = staged_parent.join("production");
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("new.txt"), "new").unwrap();
        let previous = root.join("observed/.previous-interrupted");
        fs::rename(repo.observed(), &previous).unwrap();
        CaptureTransaction {
            schema_version: 1,
            capture_id: "interrupted".into(),
            state: CaptureTransactionState::Publishing,
            staged: ".capture-interrupted/production".into(),
            previous: ".previous-interrupted".into(),
            had_current: true,
        }
        .persist(&repo)
        .unwrap();

        recover_observed(&repo, &crate::utility::progress::NullEventSink).unwrap();
        recover_observed(&repo, &crate::utility::progress::NullEventSink).unwrap();

        assert_eq!(
            fs::read_to_string(repo.observed().join("new.txt")).unwrap(),
            "new"
        );
        assert!(!previous.exists());
    }

    #[test]
    fn ready_capture_replaces_the_old_current_generation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("environment");
        config::scaffold::initialize(&root).unwrap();
        let repo = config::open(&root).unwrap();
        fs::write(repo.observed().join("old.txt"), "old").unwrap();
        let staged = root.join("observed/.capture-ready/production");
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("new.txt"), "new").unwrap();
        CaptureTransaction {
            schema_version: 1,
            capture_id: "ready".into(),
            state: CaptureTransactionState::Ready,
            staged: ".capture-ready/production".into(),
            previous: ".previous-ready".into(),
            had_current: true,
        }
        .persist(&repo)
        .unwrap();

        recover_observed(&repo, &crate::utility::progress::NullEventSink).unwrap();

        assert_eq!(
            fs::read_to_string(repo.observed().join("new.txt")).unwrap(),
            "new"
        );
        assert!(!repo.observed().join("old.txt").exists());
        assert!(!root.join("observed/.previous-ready").exists());
    }

    #[test]
    fn interrupted_capture_after_final_rename_only_cleans_previous() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("environment");
        config::scaffold::initialize(&root).unwrap();
        let repo = config::open(&root).unwrap();
        let previous = root.join("observed/.previous-interrupted");
        fs::rename(repo.observed(), &previous).unwrap();
        fs::create_dir(repo.observed()).unwrap();
        fs::write(repo.observed().join("new.txt"), "new").unwrap();
        CaptureTransaction {
            schema_version: 1,
            capture_id: "interrupted".into(),
            state: CaptureTransactionState::Publishing,
            staged: ".capture-interrupted/production".into(),
            previous: ".previous-interrupted".into(),
            had_current: true,
        }
        .persist(&repo)
        .unwrap();

        recover_observed(&repo, &crate::utility::progress::NullEventSink).unwrap();

        assert_eq!(
            fs::read_to_string(repo.observed().join("new.txt")).unwrap(),
            "new"
        );
        assert!(!previous.exists());
    }
}
