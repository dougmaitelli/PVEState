mod output;
mod validation;

pub(crate) use output::print_human;

use crate::{
    client::{PbsClient, PveClient},
    config::LocalState,
    discovery::CapturedState,
    reconcile::{Plan, PlanBuilder},
    resource::{backup, dns, firewall, guest, network},
    utility::{atomic_file, progress, runtime_security},
};
use anyhow::Result;

pub(crate) fn run(repo: &LocalState, events: &dyn progress::EventSink) -> Result<Plan> {
    events.section("Comparing local configuration with live state");
    runtime_security::prepare(&repo.runtime())?;
    validation::validate(repo, events)?;
    let captured = CapturedState::load(repo, chrono::Duration::minutes(30))?;
    build_captured(repo, &captured, events)
}

fn build_captured(
    repo: &LocalState,
    captured: &CapturedState,
    events: &dyn progress::EventSink,
) -> Result<Plan> {
    build(
        repo,
        &captured.pve,
        &captured.pbs,
        captured.id().as_str(),
        events,
    )
}

pub(super) fn build(
    repo: &LocalState,
    api: &dyn PveClient,
    pbs: &dyn PbsClient,
    capture_id: &str,
    events: &dyn progress::EventSink,
) -> Result<Plan> {
    let mut builder = PlanBuilder::new(capture_id, api.endpoint(), pbs.endpoint());
    guest::plan(repo, api, &mut builder)?;
    dns::plan::plan(repo, api, &mut builder)?;
    backup::plan(repo, api, pbs, &mut builder)?;
    network::plan::plan(repo, &mut builder)?;
    firewall::plan::plan(repo, &mut builder)?;

    let plan = builder.finish()?;
    for operation in &plan.operations {
        events.operation(&operation.description());
    }
    events.detail(&format!("{} blocker(s)", plan.blockers.len()));
    atomic_file::write_json(
        &repo
            .runtime()
            .join(crate::config::artifacts::PRODUCTION_PLAN),
        &plan,
    )?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::{
        client::{PbsClient, PveClient},
        discovery::{CaptureManifest, SourceEvidence, collect_artifacts},
        reconcile::{ApiMethod, ApiTarget, Domain, ManagedFile, Operation, ResourceId},
    };
    use anyhow::{Context, bail};
    use chrono::Utc;
    use serde::Deserialize;
    use serde_json::Value;
    use std::{collections::BTreeMap, fs, path::Path};

    #[derive(Deserialize)]
    struct LiveFixture {
        pve: BTreeMap<String, Value>,
        pbs: BTreeMap<String, Value>,
    }

    struct FixtureClient<'a> {
        endpoint: &'static str,
        responses: &'a BTreeMap<String, Value>,
    }

    impl FixtureClient<'_> {
        fn response(&self, path: &str) -> Result<Value> {
            self.responses
                .get(path)
                .cloned()
                .with_context(|| format!("fixture has no response for {path}"))
        }
    }

    impl PveClient for FixtureClient<'_> {
        fn endpoint(&self) -> &str {
            self.endpoint
        }
        fn get(&self, path: &str) -> Result<Value> {
            self.response(path)
        }
        fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("planner mutated PVE")
        }
        fn post(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("planner mutated PVE")
        }
        fn delete(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("planner mutated PVE")
        }
    }

    impl PbsClient for FixtureClient<'_> {
        fn endpoint(&self) -> &str {
            self.endpoint
        }
        fn get(&self, path: &str) -> Result<Value> {
            self.response(path)
        }
        fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("planner mutated PBS")
        }
        fn post(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("planner mutated PBS")
        }
        fn delete(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("planner mutated PBS")
        }
    }

    fn complete_capture(root: &Path) {
        let observed = root.join("observed/production");
        let manifest = CaptureManifest::new(
            Utc::now(),
            BTreeMap::from([(
                "fixture".into(),
                SourceEvidence {
                    endpoint: "fixture".into(),
                    required: true,
                    complete: true,
                    failures: Vec::new(),
                },
            )]),
            collect_artifacts(&observed).unwrap(),
        );
        fs::write(
            observed.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn operation_summary(operation: &Operation) -> Value {
        match operation {
            Operation::ApiMutation { .. } | Operation::GrowDisk { .. } => {
                serde_json::to_value(operation).unwrap()
            },
            Operation::WriteFile {
                domain,
                resource,
                target,
                ..
            } => serde_json::json!({
                "action": "write-file", "domain": domain, "resource": resource,
                "target": target,
            }),
            Operation::DeleteFile {
                domain,
                resource,
                target,
                ..
            } => serde_json::json!({
                "action": "delete-file", "domain": domain, "resource": resource,
                "target": target,
            }),
        }
    }

    #[test]
    fn plan_hash_detects_tampering() {
        let mut plan = Plan {
            schema_version: 3,
            created_at: Utc::now(),
            capture_id: "fixture-capture".into(),
            target: "https://pve.example:8006".into(),
            pbs_target: "https://pbs.example:8007".into(),
            operations: Vec::new(),
            blockers: Vec::new(),
            plan_sha256: String::new(),
        };
        plan.plan_sha256 = plan.calculate_hash().unwrap();
        assert!(plan.verify().is_ok());
        plan.capture_id.push_str("-tampered");
        assert!(plan.verify().is_err());
    }

    #[test]
    fn plan_verification_rejects_cross_domain_operations() {
        let mut plan = Plan {
            schema_version: 3,
            created_at: Utc::now(),
            capture_id: "fixture-capture".into(),
            target: "https://pve.example:8006".into(),
            pbs_target: "https://pbs.example:8007".into(),
            operations: vec![Operation::ApiMutation {
                target: ApiTarget::Pbs,
                method: ApiMethod::Put,
                domain: Domain::Guest,
                resource: ResourceId::parse("lxc/101"),
                endpoint: "/nodes/pve/lxc/101/config".into(),
                changes: BTreeMap::new(),
                environment_changes: BTreeMap::new(),
                digest: None,
            }],
            blockers: Vec::new(),
            plan_sha256: String::new(),
        };
        plan.plan_sha256 = plan.calculate_hash().unwrap();

        let error = plan.verify().unwrap_err();
        assert!(error.to_string().contains("incompatible"));
    }

    #[test]
    fn plan_verification_rejects_managed_file_identity_mismatch() {
        let mut plan = Plan {
            schema_version: 3,
            created_at: Utc::now(),
            capture_id: "fixture-capture".into(),
            target: "https://pve.example:8006".into(),
            pbs_target: "https://pbs.example:8007".into(),
            operations: vec![Operation::DeleteFile {
                domain: Domain::Firewall,
                resource: ResourceId::Guest("lxc/102".parse().unwrap()),
                target: ManagedFile::GuestFirewall { vmid: 101 },
                before_content: String::new(),
                before_sha256: String::new(),
            }],
            blockers: Vec::new(),
            plan_sha256: String::new(),
        };
        plan.plan_sha256 = plan.calculate_hash().unwrap();

        let error = plan.verify().unwrap_err();
        assert!(error.to_string().contains("incompatible"));
    }

    #[test]
    fn legacy_arbitrary_file_paths_are_not_deserializable() {
        let operation = serde_json::json!({
            "action": "delete-file",
            "domain": "firewall",
            "resource": "cluster",
            "path": "/etc/shadow",
            "before_sha256": "digest"
        });

        assert!(serde_json::from_value::<Operation>(operation).is_err());
    }

    #[test]
    fn old_plan_schema_requests_regeneration_before_operation_decoding() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "operations": [{
                "action": "delete-file",
                "domain": "firewall",
                "resource": "cluster",
                "path": "/etc/shadow",
                "before_sha256": "digest"
            }]
        }))
        .unwrap();

        let error = Plan::from_slice(&bytes).unwrap_err();
        assert_eq!(
            error.to_string(),
            "unsupported plan schema 2; run plan again"
        );
    }

    #[test]
    fn fixture_plans_guest_backup_dns_and_file_mutations() {
        let temp = tempfile::tempdir().unwrap();
        config::scaffold::initialize(temp.path()).unwrap();
        fs::create_dir_all(temp.path().join("observed/production/pve/firewall")).unwrap();
        fs::write(
            temp.path().join("observed/production/pve/firewall/201.fw"),
            "[OPTIONS]\n\nenable: 1\n",
        )
        .unwrap();
        complete_capture(temp.path());
        let repo = config::open(temp.path()).unwrap();
        let fixture: LiveFixture =
            serde_json::from_str(include_str!("../../../tests/fixtures/planner/live.json"))
                .unwrap();
        let pve = FixtureClient {
            endpoint: "https://pve.test:8006",
            responses: &fixture.pve,
        };
        let pbs = FixtureClient {
            endpoint: "https://pbs.test:8007",
            responses: &fixture.pbs,
        };

        let plan = build(
            &repo,
            &pve,
            &pbs,
            "fixture-capture",
            &crate::utility::progress::NullEventSink,
        )
        .unwrap();
        assert!(plan.blockers.is_empty());
        let actual = plan
            .operations
            .iter()
            .map(operation_summary)
            .collect::<Vec<_>>();
        let expected: Vec<Value> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/planner/expected-plan.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);

        let mut domains = plan
            .operations
            .iter()
            .map(Operation::domain)
            .collect::<Vec<_>>();
        domains.dedup();
        assert_eq!(
            domains,
            [
                Domain::Guest,
                Domain::Dns,
                Domain::Backup,
                Domain::Pbs,
                Domain::Network,
                Domain::Firewall,
            ]
        );
    }
}
