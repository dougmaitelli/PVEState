mod builder;
pub(crate) mod file;
mod output;
mod types;
mod validation;

pub(crate) use builder::PlanBuilder;
pub(crate) use output::print_human;
pub(crate) use types::{ApiPath, DiskId, Domain, ManagedFile, ResourceId, SecretName};

use crate::{
    client::{PbsClient, PveClient},
    config::LocalState,
    discovery::CapturedState,
    resource::{backup, dns, firewall, guest, network},
    utility::{
        atomic_file,
        plan_envelope::{self, PlanEnvelope},
        progress, runtime_security,
    },
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub(crate) enum Operation {
    ApiMutation {
        target: ApiTarget,
        method: ApiMethod,
        domain: Domain,
        resource: ResourceId,
        endpoint: ApiPath,
        changes: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        environment_changes: BTreeMap<String, SecretName>,
        digest: Option<String>,
    },
    GrowDisk {
        domain: Domain,
        resource: ResourceId,
        endpoint: ApiPath,
        disk: DiskId,
        size_gb: u64,
    },
    WriteFile {
        domain: Domain,
        resource: ResourceId,
        target: ManagedFile,
        content: String,
        #[serde(skip)]
        before_content: Option<String>,
        before_sha256: Option<String>,
    },
    DeleteFile {
        domain: Domain,
        resource: ResourceId,
        target: ManagedFile,
        #[serde(skip)]
        before_content: String,
        before_sha256: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ApiTarget {
    Pve,
    Pbs,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ApiMethod {
    Post,
    Put,
    Delete,
}

impl Operation {
    pub(crate) const fn resource(&self) -> &ResourceId {
        match self {
            Self::ApiMutation { resource, .. }
            | Self::GrowDisk { resource, .. }
            | Self::WriteFile { resource, .. }
            | Self::DeleteFile { resource, .. } => resource,
        }
    }

    pub(crate) fn domain(&self) -> Domain {
        match self {
            Self::ApiMutation { domain, .. }
            | Self::GrowDisk { domain, .. }
            | Self::WriteFile { domain, .. }
            | Self::DeleteFile { domain, .. } => *domain,
        }
    }

    pub(crate) fn description(&self) -> String {
        match self {
            Self::ApiMutation {
                target,
                method,
                resource,
                changes,
                ..
            } => {
                format!(
                    "{method:?} {target:?} {resource} ({} change(s))",
                    changes.len()
                )
            },
            Self::GrowDisk {
                resource, size_gb, ..
            } => format!("grow {resource} to {size_gb} GiB"),
            Self::WriteFile { target, .. } => format!("write {}", target.path()),
            Self::DeleteFile { target, .. } => format!("delete {}", target.path()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub(crate) schema_version: u8,
    pub(crate) created_at: DateTime<Utc>,
    #[serde(default)]
    pub(crate) capture_id: String,
    pub(crate) target: String,
    pub(crate) pbs_target: String,
    pub(crate) operations: Vec<Operation>,
    pub(crate) blockers: Vec<String>,
    pub(crate) plan_sha256: String,
}

impl Plan {
    pub(crate) fn from_slice(bytes: &[u8]) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        if schema_version != 3 {
            bail!("unsupported plan schema {schema_version}; run plan again")
        }
        Ok(serde_json::from_value(value)?)
    }

    #[cfg(test)]
    pub(crate) fn calculate_hash(&self) -> Result<String> {
        let mut signed = self.clone();
        plan_envelope::sign(&mut signed)?;
        Ok(signed.plan_sha256)
    }

    pub(crate) fn verify(&self) -> Result<()> {
        if self.schema_version != 3 {
            bail!(
                "unsupported plan schema {}; run plan again",
                self.schema_version
            )
        }
        plan_envelope::verify(self, "plan file integrity check failed")?;
        for operation in &self.operations {
            match operation {
                Operation::ApiMutation {
                    target,
                    domain,
                    resource,
                    endpoint,
                    environment_changes,
                    ..
                } => {
                    types::validate_operation(*domain, *target, resource)?;
                    if !endpoint.is_valid() {
                        bail!("invalid API path {endpoint}")
                    }
                    if let Some(secret) =
                        environment_changes.values().find(|value| !value.is_valid())
                    {
                        bail!("invalid secret environment variable {secret}")
                    }
                },
                Operation::GrowDisk {
                    domain,
                    resource,
                    endpoint,
                    disk,
                    ..
                } => {
                    types::validate_operation(*domain, ApiTarget::Pve, resource)?;
                    if !endpoint.is_valid() {
                        bail!("invalid API path {endpoint}")
                    }
                    if !disk.is_valid() {
                        bail!("invalid guest disk identifier {disk}")
                    }
                },
                Operation::WriteFile {
                    domain,
                    resource,
                    target,
                    ..
                }
                | Operation::DeleteFile {
                    domain,
                    resource,
                    target,
                    ..
                } => {
                    types::validate_operation(*domain, ApiTarget::Pve, resource)?;
                    if !target.matches(*domain, resource) {
                        bail!(
                            "managed file {} is incompatible with domain {domain} and resource {resource}",
                            target.path()
                        )
                    }
                },
            }
        }
        Ok(())
    }
}

impl PlanEnvelope for Plan {
    fn integrity(&self) -> &str {
        &self.plan_sha256
    }
    fn set_integrity(&mut self, value: String) {
        self.plan_sha256 = value;
    }
}

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
    };
    use anyhow::{Context, bail};
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
