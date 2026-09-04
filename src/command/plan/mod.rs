mod captured;
mod file;
mod output;
mod types;
mod validation;

pub use output::print_human;
pub use types::{ApiPath, DiskId, Domain, ResourceId, SecretName};

use crate::{
    client::{PbsClient, PveClient},
    config::Repository,
    discovery::CaptureManifest,
    render,
    resource::{backup, guest},
    utility::{
        atomic_file,
        plan_envelope::{self, PlanEnvelope},
        progress,
    },
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Operation {
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
        path: String,
        content: String,
        #[serde(skip)]
        before_content: Option<String>,
        before_sha256: Option<String>,
        activate: bool,
    },
    DeleteFile {
        domain: Domain,
        resource: ResourceId,
        path: String,
        #[serde(skip)]
        before_content: String,
        before_sha256: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiTarget {
    Pve,
    Pbs,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiMethod {
    Post,
    Put,
    Delete,
}

impl Operation {
    pub fn domain(&self) -> Domain {
        match self {
            Self::ApiMutation { domain, .. }
            | Self::GrowDisk { domain, .. }
            | Self::WriteFile { domain, .. }
            | Self::DeleteFile { domain, .. } => *domain,
        }
    }

    pub fn description(&self) -> String {
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
            Self::WriteFile { path, .. } => format!("write {path}"),
            Self::DeleteFile { path, .. } => format!("delete {path}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub schema_version: u8,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub capture_id: String,
    pub target: String,
    pub pbs_target: String,
    pub operations: Vec<Operation>,
    pub blockers: Vec<String>,
    pub plan_sha256: String,
}

impl Plan {
    pub fn calculate_hash(&self) -> Result<String> {
        let mut signed = self.clone();
        plan_envelope::sign(&mut signed)?;
        Ok(signed.plan_sha256)
    }

    pub fn verify(&self) -> Result<()> {
        if self.schema_version != 2 {
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
                    domain, resource, ..
                }
                | Operation::DeleteFile {
                    domain, resource, ..
                } => {
                    types::validate_operation(*domain, ApiTarget::Pve, resource)?;
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

pub fn run(repo: &Repository) -> Result<Plan> {
    progress::section("Comparing local configuration with live state");
    repo.secure_runtime()?;
    validation::validate(repo)?;
    let manifest = verified_capture(repo)?;
    let clients = captured::CapturedClients::load(repo, &manifest)?;
    build(repo, &clients.pve, &clients.pbs, &manifest.capture_id)
}

fn build(
    repo: &Repository,
    api: &dyn PveClient,
    pbs: &dyn PbsClient,
    capture_id: &str,
) -> Result<Plan> {
    let mut operations = Vec::new();
    let mut blockers = Vec::new();

    for (id, desired) in &repo.guests.lxcs {
        let actual = api.get(
            &crate::model::GuestRef::new(crate::model::GuestKind::Lxc, *id)
                .config_endpoint(&repo.guests.node),
        )?;
        guest::lxc(
            &repo.guests.node,
            *id,
            desired,
            &actual,
            &mut operations,
            &mut blockers,
        )?;
    }
    for (id, desired) in &repo.guests.vms {
        let actual = api.get(
            &crate::model::GuestRef::new(crate::model::GuestKind::Qemu, *id)
                .config_endpoint(&repo.guests.node),
        )?;
        guest::vm(
            &repo.guests.node,
            *id,
            desired,
            &actual,
            &mut operations,
            &mut blockers,
        )?;
    }

    let dns = api.get(&format!("/nodes/{}/dns", repo.guests.node))?;
    let changes = dns_changes(&repo.network.dns.search, &repo.network.dns.servers, &dns);
    if !changes.is_empty() {
        operations.push(Operation::ApiMutation {
            target: ApiTarget::Pve,
            method: ApiMethod::Put,
            domain: "dns".into(),
            resource: ResourceId::Named(repo.guests.node.clone()),
            endpoint: format!("/nodes/{}/dns", repo.guests.node).into(),
            changes,
            environment_changes: BTreeMap::new(),
            digest: None,
        });
    }

    backup::plan(repo, api, pbs, &mut operations, &mut blockers)?;

    if let Ok(captured_network) = fs::read_to_string(repo.observed().join("network/interfaces")) {
        blockers.extend(network_safety_blockers(&captured_network, &repo.network)?);
    }
    for (resource, path) in firewall_artifacts(repo) {
        if let Ok(content) = fs::read_to_string(repo.observed().join(path)) {
            blockers.extend(firewall_safety_blockers(&content, &resource));
        }
    }
    file::operation(
        repo,
        ("network", &repo.guests.node),
        ("network/interfaces", "/etc/network/interfaces"),
        render::network(&repo.network),
        true,
        &mut operations,
    )?;
    if let Some(policy) = &repo.cluster.firewall {
        file::operation(
            repo,
            ("firewall", "cluster"),
            ("pve/firewall/cluster.fw", "/etc/pve/firewall/cluster.fw"),
            render::firewall_policy(policy),
            false,
            &mut operations,
        )?;
    } else {
        file::deletion(
            repo,
            ("firewall", "cluster"),
            ("pve/firewall/cluster.fw", "/etc/pve/firewall/cluster.fw"),
            &mut operations,
        )?;
    }
    let guest_firewalls = repo
        .guests
        .lxcs
        .iter()
        .map(|(id, guest)| (id, guest.firewall.as_ref()))
        .chain(
            repo.guests
                .vms
                .iter()
                .map(|(id, guest)| (id, guest.firewall.as_ref())),
        );
    for (id, policy) in guest_firewalls {
        let id = id.to_string();
        let paths = (
            format!("pve/firewall/{id}.fw"),
            format!("/etc/pve/firewall/{id}.fw"),
        );
        if let Some(policy) = policy {
            file::operation(
                repo,
                ("firewall", &id),
                (&paths.0, &paths.1),
                render::firewall_policy(policy),
                false,
                &mut operations,
            )?;
        } else {
            file::deletion(
                repo,
                ("firewall", &id),
                (&paths.0, &paths.1),
                &mut operations,
            )?;
        }
    }
    let node = &repo.node.node.name;
    let local = format!("pve/firewall/{node}-host.fw");
    let remote = format!("/etc/pve/nodes/{node}/host.fw");
    if let Some(policy) = &repo.node.firewall {
        file::operation(
            repo,
            ("firewall", &format!("node/{node}")),
            (&local, &remote),
            render::firewall_policy(policy),
            false,
            &mut operations,
        )?;
    } else {
        file::deletion(
            repo,
            ("firewall", &format!("node/{node}")),
            (&local, &remote),
            &mut operations,
        )?;
    }

    let mut plan = Plan {
        schema_version: 2,
        created_at: Utc::now(),
        capture_id: capture_id.into(),
        target: api.endpoint().into(),
        pbs_target: pbs.endpoint().into(),
        operations,
        blockers,
        plan_sha256: String::new(),
    };
    plan_envelope::sign(&mut plan)?;
    for operation in &plan.operations {
        progress::operation(operation.description());
    }
    progress::detail(format!("{} blocker(s)", plan.blockers.len()));
    atomic_file::write_json(&repo.runtime().join("production-plan.json"), &plan)?;
    Ok(plan)
}

fn verified_capture(repo: &Repository) -> Result<CaptureManifest> {
    let manifest: CaptureManifest = serde_json::from_slice(
        &fs::read(repo.observed().join("manifest.json")).context("run capture before plan")?,
    )?;
    manifest.verify(&repo.observed(), chrono::Duration::minutes(30))?;
    Ok(manifest)
}

fn compare(
    changes: &mut BTreeMap<String, String>,
    key: &str,
    wanted: &impl ToString,
    actual: &serde_json::Value,
) {
    let wanted = wanted.to_string();
    let current = actual.get(key).map(value_string);
    if current.as_deref() != Some(&wanted) {
        changes.insert(key.into(), wanted);
    }
}

fn network_safety_blockers(content: &str, current: &crate::model::Network) -> Result<Vec<String>> {
    let parsed = crate::resource::native::parse_network(content, current)?;
    Ok(parsed
        .unmodeled
        .iter()
        .map(|directive| {
            format!(
                "network configuration cannot be represented safely: {}",
                directive.diagnostic()
            )
        })
        .collect())
}

fn firewall_artifacts(repo: &Repository) -> Vec<(String, String)> {
    let mut artifacts = vec![
        ("cluster".into(), "pve/firewall/cluster.fw".into()),
        (
            format!("node/{}", repo.guests.node),
            format!("pve/firewall/{}-host.fw", repo.guests.node),
        ),
    ];
    artifacts.extend(
        repo.guests
            .lxcs
            .keys()
            .chain(repo.guests.vms.keys())
            .map(|id| (id.to_string(), format!("pve/firewall/{id}.fw"))),
    );
    artifacts
}

fn firewall_safety_blockers(content: &str, resource: &str) -> Vec<String> {
    crate::resource::native::parse_firewall(content)
        .unmodeled
        .iter()
        .map(|directive| {
            format!(
                "firewall {resource} cannot be represented safely: {}",
                directive.diagnostic()
            )
        })
        .collect()
}

fn dns_changes(
    search: &str,
    servers: &[String],
    actual: &serde_json::Value,
) -> BTreeMap<String, String> {
    let mut changes = BTreeMap::new();
    compare(&mut changes, "search", &search, actual);
    for (index, server) in servers.iter().enumerate() {
        compare(&mut changes, &format!("dns{}", index + 1), server, actual);
    }
    let delete = actual
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .filter(|key| key.starts_with("dns"))
        .filter_map(|key| key[3..].parse::<usize>().ok().map(|index| (key, index)))
        .filter(|(_, index)| *index > servers.len())
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    if !delete.is_empty() {
        changes.insert("delete".into(), delete.join(","));
    }
    changes
}

fn value_string(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::{PbsClient, PveClient},
        discovery::{CaptureManifest, SourceEvidence, collect_artifacts},
    };
    use anyhow::{Context, bail};
    use serde_json::Value;
    use std::{collections::BTreeMap, path::Path};

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
                path,
                activate,
                ..
            } => serde_json::json!({
                "action": "write-file", "domain": domain, "resource": resource,
                "path": path, "activate": activate,
            }),
            Operation::DeleteFile {
                domain,
                resource,
                path,
                ..
            } => serde_json::json!({
                "action": "delete-file", "domain": domain, "resource": resource, "path": path,
            }),
        }
    }

    #[test]
    fn plan_hash_detects_tampering() {
        let mut plan = Plan {
            schema_version: 2,
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
            schema_version: 2,
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
    fn firewall_option_order_is_not_drift() {
        let a = "[OPTIONS]\nenable: 1\nlog_level_in: nolog\n";
        let b = "[OPTIONS]\nlog_level_in: nolog\nenable: 1\n";
        assert_eq!(render::firewall_semantic(a), render::firewall_semantic(b));
    }

    #[test]
    fn firewall_rule_order_remains_semantic() {
        let a = "[RULES]\nIN ACCEPT -dport 22\nIN DROP\n";
        let b = "[RULES]\nIN DROP\nIN ACCEPT -dport 22\n";

        assert_ne!(render::firewall_semantic(a), render::firewall_semantic(b));
    }

    #[test]
    fn extra_dns_servers_are_deleted() {
        let actual =
            serde_json::json!({"search":"example.test", "dns1":"1.1.1.1", "dns2":"8.8.8.8"});
        let changes = dns_changes("example.test", &["1.1.1.1".into()], &actual);
        assert_eq!(changes.get("delete").map(String::as_str), Some("dns2"));
    }

    #[test]
    fn unsupported_native_network_syntax_becomes_a_plan_blocker() {
        let network: crate::model::Network =
            serde_yaml::from_str(include_str!("../../../examples/basic/config/network.yml"))
                .unwrap();
        let blockers = network_safety_blockers(
            "iface vmbr0 inet manual\n    bridge-ports none\n    bridge-vlan-aware yes\n",
            &network,
        )
        .unwrap();

        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("cannot be represented safely"));
        assert!(blockers[0].contains("bridge-vlan-aware yes"));
    }

    #[test]
    fn unsupported_native_firewall_syntax_becomes_a_plan_blocker() {
        let blockers = firewall_safety_blockers(
            "[RULES]\nIN ACCEPT -p tcp -m conntrack -dport 443\n",
            "cluster",
        );

        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("firewall cluster cannot be represented safely"));
        assert!(blockers[0].contains("-m"));
    }

    #[test]
    fn fixture_plans_guest_backup_dns_and_file_mutations() {
        let temp = tempfile::tempdir().unwrap();
        Repository::initialize(temp.path()).unwrap();
        fs::create_dir_all(temp.path().join("observed/production/pve/firewall")).unwrap();
        fs::write(
            temp.path().join("observed/production/pve/firewall/201.fw"),
            "[OPTIONS]\n\nenable: 1\n",
        )
        .unwrap();
        complete_capture(temp.path());
        let repo = Repository::open(temp.path()).unwrap();
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

        let plan = build(&repo, &pve, &pbs, "fixture-capture").unwrap();
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
    }
}
