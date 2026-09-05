mod journal;

use self::journal::ApplyJournal;
use crate::{
    client::{Pbs, PbsClient, Pve, PveClient, RemoteHost, Ssh},
    config::LocalState,
    reconcile::{ApiMethod, ApiTarget, Operation, Plan},
    settings::{ApplySettings, Settings},
    utility::{authorization, progress::EventSink, remote_file, runtime_security, shell},
};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub(crate) struct ApplyReport {
    pub(crate) journal_id: String,
    pub(crate) journal_path: String,
    pub(crate) completed: usize,
    pub(crate) failed: Option<String>,
}

struct MutationClients {
    pve: Box<dyn PveClient>,
    pbs: Option<Box<dyn PbsClient>>,
    ssh: Option<Box<dyn RemoteHost>>,
}

impl MutationClients {
    fn configured(settings: &Settings) -> Result<Self> {
        Ok(Self {
            pve: Box::new(Pve::mutation(&settings.pve)?),
            pbs: settings
                .pbs
                .mutation
                .as_ref()
                .map(|_| Pbs::mutation(&settings.pbs))
                .transpose()?
                .map(|client| Box::new(client) as Box<dyn PbsClient>),
            ssh: settings
                .ssh
                .mutation
                .as_ref()
                .map(|target| Box::new(Ssh::new(target)) as Box<dyn RemoteHost>),
        })
    }
}

pub(crate) fn run(
    repo: &LocalState,
    settings: &Settings,
    events: &dyn EventSink,
) -> Result<ApplyReport> {
    run_with_factory(
        repo,
        &settings.apply,
        || MutationClients::configured(settings),
        events,
    )
}

fn run_with_factory(
    repo: &LocalState,
    settings: &ApplySettings,
    factory: impl FnOnce() -> Result<MutationClients>,
    events: &dyn EventSink,
) -> Result<ApplyReport> {
    events.section("Applying local configuration to live system");
    runtime_security::prepare(&repo.runtime())?;
    let plan = authorize(repo, settings)?;
    let mut journal = ApplyJournal::new(&repo.runtime(), &plan);
    journal.persist()?;

    let result = factory().and_then(|clients| {
        execute(
            &plan,
            settings,
            clients.pve.as_ref(),
            clients.pbs.as_deref(),
            clients.ssh.as_deref(),
            &mut journal,
            events,
        )
    });
    match result {
        Ok(()) => {
            journal.succeed();
            journal.persist()?;
            events.finish(true);
            Ok(journal.report())
        },
        Err(error) => {
            if journal.failure.is_none() {
                journal.fail(&error);
            }
            let journal_path = journal.path().display().to_string();
            if let Err(journal_error) = journal.persist() {
                return Err(error).context(format!(
                    "apply failed; journal {journal_path} also failed to persist: {journal_error:#}"
                ));
            }
            Err(error).context(format!(
                "apply failed; partial execution journal: {journal_path}"
            ))
        },
    }
}

fn authorize(repo: &LocalState, settings: &ApplySettings) -> Result<Plan> {
    let plan = Plan::from_slice(
        &runtime_security::read(
            &repo
                .runtime()
                .join(crate::config::artifacts::PRODUCTION_PLAN),
        )
        .context("run plan first")?,
    )?;
    plan.verify()?;
    let target = settings
        .target
        .as_ref()
        .map(|target| target.as_str().trim_end_matches('/'))
        .unwrap_or_default();
    authorization::authorize(
        &plan.plan_sha256,
        &plan.target,
        plan.created_at,
        &plan.blockers,
        authorization::Policy {
            enabled: settings.enabled,
            enabled_error: crate::settings::env::ENABLE_PRODUCTION_APPLY_ERROR,
            confirmation: settings.confirm_plan_sha.as_deref(),
            confirmation_error: "plan SHA confirmation mismatch",
            requested_target: target,
            target_error: "apply target does not match plan target",
            max_age: chrono::Duration::minutes(30),
            stale_error: "plan is stale; capture and plan again",
            max_future_skew: chrono::Duration::minutes(2),
            future_error: "plan timestamp is too far in the future; check system clocks",
            blocker_prefix: "plan has blockers: ",
            blocker_separator: "; ",
        },
    )?;
    let allowed = &settings.domains;
    let needed: BTreeSet<_> = plan.operations.iter().map(|x| x.domain()).collect();
    if !needed.is_subset(allowed) {
        bail!(
            "unapproved domains: {:?}",
            needed.difference(allowed).collect::<Vec<_>>()
        )
    }
    Ok(plan)
}

fn execute(
    plan: &Plan,
    settings: &ApplySettings,
    api: &dyn PveClient,
    pbs: Option<&dyn PbsClient>,
    ssh: Option<&dyn RemoteHost>,
    journal: &mut ApplyJournal,
    events: &dyn EventSink,
) -> Result<()> {
    if api.endpoint() != plan.target {
        bail!("mutation API endpoint differs from plan target")
    }
    for (index, op) in plan.operations.iter().enumerate() {
        events.operation(&format!(
            "[{}/{}] {}",
            index + 1,
            plan.operations.len(),
            op.description()
        ));
        journal.start(index)?;
        journal.persist()?;
        let result = (|| -> Result<()> {
            match op {
                Operation::ApiMutation {
                    target,
                    method,
                    endpoint,
                    changes,
                    environment_changes,
                    digest,
                    ..
                } => {
                    let mut x = changes.clone();
                    for (parameter, variable) in environment_changes {
                        x.insert(
                            parameter.clone(),
                            settings
                                .secret(variable.as_str())
                                .with_context(|| format!("resolve {variable} for {parameter}"))?
                                .into(),
                        );
                    }
                    if let Some(d) = digest {
                        x.insert("digest".into(), d.clone());
                    }
                    match target {
                        ApiTarget::Pve => mutate_pve(api, *method, endpoint, &x)?,
                        ApiTarget::Pbs => {
                            let client = pbs.context("plan requires PBS mutation credentials")?;
                            if client.endpoint() != plan.pbs_target {
                                bail!("mutation PBS endpoint differs from plan target")
                            }
                            mutate_pbs(client, *method, endpoint, &x)?
                        },
                    };
                    Ok(())
                },
                Operation::GrowDisk {
                    endpoint,
                    disk,
                    size_gb,
                    ..
                } => {
                    api.put(
                        endpoint,
                        &BTreeMap::from([
                            ("disk".into(), disk.to_string()),
                            ("size".into(), format!("{size_gb}G")),
                        ]),
                    )?;
                    Ok(())
                },
                Operation::WriteFile {
                    target,
                    content,
                    before_sha256,
                    ..
                } => {
                    let s = ssh.context("plan requires apply SSH settings")?;
                    let path = target.path();
                    remote_file::write(
                        s,
                        &path,
                        content,
                        remote_file::WriteOptions {
                            mode: target.mode(),
                            expected_sha256: before_sha256.as_deref(),
                            verify_expected: true,
                            backup_existing: true,
                        },
                    )?;
                    if target.requires_activation() && settings.activate_network {
                        s.run("ifreload -a")?;
                    }
                    Ok(())
                },
                Operation::DeleteFile {
                    target,
                    before_sha256,
                    ..
                } => {
                    let s = ssh.context("plan requires apply SSH settings")?;
                    let path = target.path();
                    shell::verify_remote_file(s, &path, Some(before_sha256))?;
                    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
                    let backup = format!("/root/pves-preapply/{stamp}{path}");
                    let parent = std::path::Path::new(&backup)
                        .parent()
                        .and_then(std::path::Path::to_str)
                        .context("backup parent")?;
                    s.run(&format!(
                        "install -d {} && cp -a {} {} && rm -f {}",
                        shell::quote(parent),
                        shell::quote(&path),
                        shell::quote(&backup),
                        shell::quote(&path)
                    ))?;
                    Ok(())
                },
            }
        })();
        match result {
            Ok(()) => {
                events.detail("completed");
                journal.applied(index)?;
                journal.persist()?;
            },
            Err(error) => {
                events.detail(&format!("failed: {error:#}"));
                journal.operation_failed(index, &error)?;
                journal.persist()?;
                return Err(error);
            },
        }
    }
    Ok(())
}

fn resource(o: &Operation) -> String {
    match o {
        Operation::ApiMutation { resource, .. }
        | Operation::GrowDisk { resource, .. }
        | Operation::WriteFile { resource, .. }
        | Operation::DeleteFile { resource, .. } => resource.to_string(),
    }
}

fn mutate_pve(
    client: &dyn PveClient,
    method: ApiMethod,
    endpoint: &str,
    data: &BTreeMap<String, String>,
) -> Result<()> {
    match method {
        ApiMethod::Post => client.post(endpoint, data),
        ApiMethod::Put => client.put(endpoint, data),
        ApiMethod::Delete => client.delete(endpoint, data),
    }
}

fn mutate_pbs(
    client: &dyn PbsClient,
    method: ApiMethod,
    endpoint: &str,
    data: &BTreeMap<String, String>,
) -> Result<()> {
    match method {
        ApiMethod::Post => client.post(endpoint, data),
        ApiMethod::Put => client.put(endpoint, data),
        ApiMethod::Delete => client.delete(endpoint, data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::settings::ApplySettings;
    use anyhow::anyhow;
    use reqwest::Url;
    use serde_json::Value;
    use std::cell::Cell;
    use std::fs;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
    struct RecordedCall {
        client: String,
        method: String,
        path: String,
        data: BTreeMap<String, String>,
    }

    #[derive(serde::Deserialize)]
    struct MutationFixture {
        pve_target: String,
        pbs_target: String,
        operations: Vec<Operation>,
    }

    struct RecordingClient<'a> {
        name: &'static str,
        endpoint: &'a str,
        calls: &'a Mutex<Vec<RecordedCall>>,
    }

    impl RecordingClient<'_> {
        fn record(&self, method: &str, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
            self.calls.lock().unwrap().push(RecordedCall {
                client: self.name.into(),
                method: method.into(),
                path: path.into(),
                data: data.clone(),
            });
            Ok(())
        }
    }

    macro_rules! recording_client {
        ($trait:ident) => {
            impl $trait for RecordingClient<'_> {
                fn endpoint(&self) -> &str {
                    self.endpoint
                }
                fn get(&self, _: &str) -> Result<Value> {
                    unreachable!()
                }
                fn put(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
                    self.record("put", path, data)
                }
                fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
                    self.record("post", path, data)
                }
                fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
                    self.record("delete", path, data)
                }
            }
        };
    }
    recording_client!(PveClient);
    recording_client!(PbsClient);

    struct FakePve {
        calls: AtomicUsize,
    }

    impl PveClient for FakePve {
        fn endpoint(&self) -> &str {
            "https://pve.test:8006"
        }

        fn get(&self, _: &str) -> Result<Value> {
            unreachable!()
        }

        fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                Err(anyhow!("injected second-operation failure"))
            } else {
                Ok(())
            }
        }

        fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
            self.put(path, data)
        }

        fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
            self.put(path, data)
        }
    }

    fn operation(resource: &str) -> Operation {
        Operation::ApiMutation {
            target: ApiTarget::Pve,
            method: ApiMethod::Put,
            domain: "guests".into(),
            resource: resource.into(),
            endpoint: format!("/{resource}").into(),
            changes: BTreeMap::new(),
            environment_changes: BTreeMap::new(),
            digest: None,
        }
    }

    fn authorized_apply() -> (tempfile::TempDir, LocalState, ApplySettings) {
        let temp = tempfile::tempdir().unwrap();
        config::scaffold::initialize(temp.path()).unwrap();
        let repo = config::open(temp.path()).unwrap();
        let mut plan = Plan {
            schema_version: 3,
            created_at: Utc::now(),
            capture_id: "fixture-capture".into(),
            target: "https://pve.test:8006".into(),
            pbs_target: "https://pbs.test:8007".into(),
            operations: Vec::new(),
            blockers: Vec::new(),
            plan_sha256: String::new(),
        };
        plan.plan_sha256 = plan.calculate_hash().unwrap();
        crate::utility::atomic_file::write_json(
            &repo
                .runtime()
                .join(crate::config::artifacts::PRODUCTION_PLAN),
            &plan,
        )
        .unwrap();
        let settings = ApplySettings {
            enabled: true,
            confirm_plan_sha: Some(plan.plan_sha256),
            target: Some(Url::parse(&plan.target).unwrap()),
            domains: BTreeSet::new(),
            activate_network: false,
            secrets: BTreeMap::new(),
        };
        (temp, repo, settings)
    }

    #[test]
    fn authorization_failure_does_not_construct_mutation_clients() {
        let (_temp, repo, mut settings) = authorized_apply();
        settings.enabled = false;
        let factory_called = Cell::new(false);

        let error = run_with_factory(
            &repo,
            &settings,
            || -> Result<MutationClients> {
                factory_called.set(true);
                bail!("factory must not run")
            },
            &crate::utility::progress::NullEventSink,
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("PVES_ENABLE_PRODUCTION_APPLY"));
        assert!(!factory_called.get());
        assert!(
            !repo
                .runtime()
                .join(crate::config::artifacts::APPLY_LATEST)
                .exists()
        );
    }

    #[test]
    fn successful_service_returns_an_apply_report() {
        let (_temp, repo, settings) = authorized_apply();

        let report = run_with_factory(
            &repo,
            &settings,
            || {
                Ok(MutationClients {
                    pve: Box::new(FakePve {
                        calls: AtomicUsize::new(0),
                    }),
                    pbs: None,
                    ssh: None,
                })
            },
            &crate::utility::progress::NullEventSink,
        )
        .unwrap();

        assert_eq!(report.completed, 0);
        assert_eq!(report.failed, None);
        assert!(std::path::Path::new(&report.journal_path).is_file());
        assert!(!report.journal_id.is_empty());
    }

    #[test]
    fn client_initialization_failure_is_persisted_in_apply_journal() {
        let (_temp, repo, settings) = authorized_apply();

        let error = run_with_factory(
            &repo,
            &settings,
            || -> Result<MutationClients> { bail!("injected client initialization failure") },
            &crate::utility::progress::NullEventSink,
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("injected client initialization failure"));
        let journal: Value = serde_json::from_slice(
            &fs::read(repo.runtime().join(crate::config::artifacts::APPLY_LATEST)).unwrap(),
        )
        .unwrap();
        assert_eq!(journal["status"], "failed");
        assert!(
            journal["failure"]
                .as_str()
                .unwrap()
                .contains("injected client initialization failure")
        );
    }

    #[test]
    fn injected_client_failure_is_journaled_without_network() {
        let temp = tempfile::tempdir().unwrap();
        let plan = Plan {
            schema_version: 3,
            created_at: Utc::now(),
            capture_id: "fixture-capture".into(),
            target: "https://pve.test:8006".into(),
            pbs_target: "https://pbs.test:8007".into(),
            operations: vec![operation("first"), operation("second"), operation("third")],
            blockers: Vec::new(),
            plan_sha256: "digest".into(),
        };
        let settings = ApplySettings {
            enabled: true,
            confirm_plan_sha: Some("digest".into()),
            target: Some(Url::parse("https://pve.test:8006").unwrap()),
            domains: BTreeSet::from(["guests".into()]),
            activate_network: false,
            secrets: BTreeMap::new(),
        };
        let client = FakePve {
            calls: AtomicUsize::new(0),
        };
        let mut journal = ApplyJournal::new(temp.path(), &plan);
        journal.persist().unwrap();

        let error = execute(
            &plan,
            &settings,
            &client,
            None,
            None,
            &mut journal,
            &crate::utility::progress::NullEventSink,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("injected second-operation failure"));
        journal.persist().unwrap();
        let value: Value = serde_json::from_slice(&fs::read(journal.path()).unwrap()).unwrap();
        assert_eq!(value["operations"][0]["status"], "applied");
        assert_eq!(value["operations"][1]["status"], "failed");
        assert_eq!(value["operations"][2]["status"], "pending");
    }

    #[test]
    fn fixture_executes_exact_mutations_in_order() {
        let fixture: MutationFixture =
            serde_json::from_str(include_str!("../../tests/fixtures/mutation/plan.json")).unwrap();
        let expected: Vec<RecordedCall> = serde_json::from_str(include_str!(
            "../../tests/fixtures/mutation/expected-calls.json"
        ))
        .unwrap();
        let plan = Plan {
            schema_version: 3,
            created_at: Utc::now(),
            capture_id: "fixture-capture".into(),
            target: fixture.pve_target.clone(),
            pbs_target: fixture.pbs_target.clone(),
            operations: fixture.operations,
            blockers: Vec::new(),
            plan_sha256: "fixture".into(),
        };
        let calls = Mutex::new(Vec::new());
        let pve = RecordingClient {
            name: "pve",
            endpoint: &fixture.pve_target,
            calls: &calls,
        };
        let pbs = RecordingClient {
            name: "pbs",
            endpoint: &fixture.pbs_target,
            calls: &calls,
        };
        let settings = ApplySettings {
            enabled: true,
            confirm_plan_sha: Some("fixture".into()),
            target: Some(Url::parse(&fixture.pve_target).unwrap()),
            domains: BTreeSet::from(["guests".into(), "pbs".into()]),
            activate_network: false,
            secrets: BTreeMap::from([(
                "PBS_APPLY_S3_ACCESS_KEY".into(),
                "fixture-access-key".into(),
            )]),
        };
        let temp = tempfile::tempdir().unwrap();
        let mut journal = ApplyJournal::new(temp.path(), &plan);

        execute(
            &plan,
            &settings,
            &pve,
            Some(&pbs),
            None,
            &mut journal,
            &crate::utility::progress::NullEventSink,
        )
        .unwrap();

        assert_eq!(*calls.lock().unwrap(), expected);
    }
}
