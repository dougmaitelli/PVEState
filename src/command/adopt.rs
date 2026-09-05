mod transaction;

use crate::{
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedState,
    reconcile::{Domain, Operation, Plan},
    resource::{backup, firewall, guest, network},
    utility::{progress::EventSink, runtime_security, yaml_patch},
};
use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

pub(crate) fn run(
    local: &LocalState,
    preview: bool,
    all: bool,
    requested: &[String],
    events: &dyn EventSink,
) -> Result<()> {
    events.section(if preview {
        "Previewing captured drift"
    } else {
        "Adopting captured state into local configuration"
    });
    runtime_security::prepare(&local.runtime())?;
    let captured = CapturedState::load(local, chrono::Duration::minutes(30))?;
    let plan = Plan::from_slice(
        &runtime_security::read(
            &local
                .runtime()
                .join(crate::config::artifacts::PRODUCTION_PLAN),
        )
        .context("run plan first")?,
    )?;
    plan.verify()?;
    if plan.capture_id != captured.id().as_str() {
        bail!(
            "plan was created from capture {}, but the current capture is {}; run plan again",
            plan.capture_id,
            captured.id().as_str()
        )
    }
    let candidates = candidates(local, &captured, &plan)?;

    if preview {
        events.finish(true);
        events.output(&serde_json::to_string_pretty(&candidates)?);
        return Ok(());
    }
    let selected = selection(&candidates, all, requested)?;
    let known = candidates
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<BTreeSet<_>>();
    let unknown = selected.difference(&known).collect::<Vec<_>>();
    if !unknown.is_empty() {
        bail!("unknown adoption IDs: {unknown:?}")
    }

    let mut patches = Vec::new();
    for candidate in candidates
        .iter()
        .filter(|candidate| selected.contains(&candidate.id))
    {
        events.operation(&format!(
            "{}: {} -> {}",
            candidate.id, candidate.local, candidate.captured
        ));
        let candidate_patches = candidate.patches().with_context(|| {
            format!(
                "{} cannot be adopted: {}",
                candidate.id,
                candidate.reason.as_deref().unwrap_or("unsupported")
            )
        })?;
        patches.extend_from_slice(candidate_patches);
    }
    let documents = apply_local_patches(local, patches)?;
    transaction::validate(local, &documents)?;
    transaction::publish(local, &documents)?;
    events.finish(true);
    events.output(&format!(
        "adopted {} captured value(s) into {}",
        selected.len(),
        documents.keys().cloned().collect::<Vec<_>>().join(", ")
    ));
    Ok(())
}

fn selection(
    candidates: &[AdoptionCandidate],
    all: bool,
    requested: &[String],
) -> Result<BTreeSet<String>> {
    if all {
        let selected = candidates
            .iter()
            .filter(|candidate| candidate.is_adoptable())
            .map(|candidate| candidate.id.clone())
            .collect::<BTreeSet<_>>();
        if selected.is_empty() {
            bail!("there are no adoptable captured values")
        }
        return Ok(selected);
    }
    if requested.is_empty() {
        bail!("adopt requires --preview, --all, or at least one ID")
    }
    Ok(requested.iter().cloned().collect())
}

fn candidates(
    local: &LocalState,
    captured: &CapturedState,
    plan: &Plan,
) -> Result<Vec<AdoptionCandidate>> {
    let mut candidates = Vec::new();
    for operation in &plan.operations {
        let mut produced = match operation {
            Operation::ApiMutation {
                domain: Domain::Guest,
                ..
            }
            | Operation::GrowDisk {
                domain: Domain::Guest,
                ..
            } => guest::adopt::candidates(local, captured, operation)?,
            Operation::ApiMutation {
                domain: Domain::Dns,
                ..
            } => network::adopt::candidates(local, captured, operation)?,
            Operation::ApiMutation {
                domain: Domain::Backup | Domain::Pbs,
                ..
            } => backup::adopt::candidates(local, captured, operation)?,
            Operation::WriteFile {
                domain: Domain::Network,
                ..
            }
            | Operation::DeleteFile {
                domain: Domain::Network,
                ..
            } => vec![network::adopt::native_candidate(
                local,
                &captured.native,
                operation,
            )?],
            Operation::WriteFile {
                domain: Domain::Firewall,
                ..
            }
            | Operation::DeleteFile {
                domain: Domain::Firewall,
                ..
            } => {
                vec![firewall::adopt::candidate(
                    local,
                    &captured.native,
                    operation,
                )?]
            },
            operation => vec![AdoptionCandidate::blocked(
                operation.resource(),
                "operation",
                "local state",
                "captured state",
                format!("{} has no adoption adapter", operation.domain()),
            )],
        };
        if produced.is_empty() {
            bail!(
                "adoption adapter returned no candidate for {}",
                operation.description()
            )
        }
        candidates.append(&mut produced);
    }
    Ok(candidates)
}

fn apply_local_patches(
    local: &LocalState,
    patches: Vec<LocalPatch>,
) -> Result<BTreeMap<String, String>> {
    let mut grouped = BTreeMap::<ConfigDocument, Vec<yaml_patch::Patch>>::new();
    for patch in patches {
        grouped
            .entry(patch.document())
            .or_default()
            .push(patch.into_yaml_patch());
    }
    let mut documents = BTreeMap::new();
    for (document, patches) in grouped {
        let path = document.path();
        let content = fs::read_to_string(local.root().join(path))?;
        documents.insert(path.into(), yaml_patch::apply_patches(&content, &patches)?);
    }
    Ok(documents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::{PbsClient, PveClient},
        reconcile::ResourceId,
    };
    use anyhow::{Result, bail};
    use serde_json::Value;
    use std::collections::BTreeMap;

    struct FixtureClient<'a> {
        endpoint: &'static str,
        responses: &'a BTreeMap<String, Value>,
    }

    impl PveClient for FixtureClient<'_> {
        fn endpoint(&self) -> &str {
            self.endpoint
        }

        fn get(&self, path: &str) -> Result<Value> {
            self.responses
                .get(path)
                .cloned()
                .with_context(|| format!("fixture response {path}"))
        }

        fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("fixture must not mutate")
        }

        fn post(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("fixture must not mutate")
        }

        fn delete(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("fixture must not mutate")
        }
    }

    impl PbsClient for FixtureClient<'_> {
        fn endpoint(&self) -> &str {
            self.endpoint
        }

        fn get(&self, path: &str) -> Result<Value> {
            self.responses
                .get(path)
                .cloned()
                .with_context(|| format!("fixture response {path}"))
        }

        fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("fixture must not mutate")
        }

        fn post(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("fixture must not mutate")
        }

        fn delete(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            bail!("fixture must not mutate")
        }
    }

    #[test]
    fn all_selects_only_candidates_with_patches() {
        let resource = ResourceId::Named("fixture".into());
        let candidates = [
            AdoptionCandidate::adoptable(
                &resource,
                "field",
                "local",
                "captured",
                vec![LocalPatch::RemoveResource {
                    document: ConfigDocument::Backup,
                    path: vec![],
                }],
            ),
            AdoptionCandidate::blocked(&resource, "blocked", "local", "captured", "reason"),
        ];
        assert_eq!(selection(&candidates, true, &[]).unwrap().len(), 1);
    }

    #[test]
    fn every_fixture_plan_operation_produces_a_patch_or_documented_blocker() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            pve: BTreeMap<String, Value>,
            pbs: BTreeMap<String, Value>,
        }

        let temp = tempfile::tempdir().unwrap();
        crate::config::scaffold::initialize(temp.path()).unwrap();
        let local = crate::config::open(temp.path()).unwrap();
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../tests/fixtures/planner/live.json")).unwrap();
        let pve = FixtureClient {
            endpoint: "https://pve.test:8006",
            responses: &fixture.pve,
        };
        let pbs = FixtureClient {
            endpoint: "https://pbs.test:8007",
            responses: &fixture.pbs,
        };
        let plan = super::super::plan::build(
            &local,
            &pve,
            &pbs,
            "fixture-capture",
            &crate::utility::progress::NullEventSink,
        )
        .unwrap();
        let captured = CapturedState::fixture(
            "fixture-capture",
            pve.endpoint,
            fixture.pve,
            pbs.endpoint,
            fixture.pbs,
            local.observed(),
        );

        for operation in plan.operations.iter().filter(|operation| {
            matches!(
                operation,
                Operation::ApiMutation { .. } | Operation::GrowDisk { .. }
            )
        }) {
            let produced = candidates(
                &local,
                &captured,
                &Plan {
                    operations: vec![operation.clone()],
                    ..plan.clone()
                },
            )
            .unwrap();
            assert_eq!(produced.len(), 1, "{}", operation.description());
            assert!(
                produced[0].is_adoptable() || produced[0].reason.is_some(),
                "{}",
                operation.description()
            );
        }
    }

    #[test]
    fn adopting_fixture_patches_then_reloading_converges() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            pve: BTreeMap<String, Value>,
            pbs: BTreeMap<String, Value>,
        }

        let temp = tempfile::tempdir().unwrap();
        crate::config::scaffold::initialize(temp.path()).unwrap();
        let local = crate::config::open(temp.path()).unwrap();
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../tests/fixtures/planner/live.json")).unwrap();
        let pve = FixtureClient {
            endpoint: "https://pve.test:8006",
            responses: &fixture.pve,
        };
        let pbs = FixtureClient {
            endpoint: "https://pbs.test:8007",
            responses: &fixture.pbs,
        };
        let plan = super::super::plan::build(
            &local,
            &pve,
            &pbs,
            "fixture-capture",
            &crate::utility::progress::NullEventSink,
        )
        .unwrap();
        let captured = CapturedState::fixture(
            "fixture-capture",
            pve.endpoint,
            fixture.pve.clone(),
            pbs.endpoint,
            fixture.pbs.clone(),
            local.observed(),
        );
        let mut patches = Vec::new();
        for operation in plan
            .operations
            .iter()
            .filter(|operation| matches!(operation.domain(), Domain::Backup | Domain::Pbs))
        {
            for candidate in backup::adopt::candidates(&local, &captured, operation).unwrap() {
                patches.extend_from_slice(candidate.patches().unwrap());
            }
        }
        let documents = apply_local_patches(&local, patches).unwrap();
        for (path, content) in documents {
            fs::write(temp.path().join(path), content).unwrap();
        }
        let adopted = crate::config::open(temp.path()).unwrap();
        assert!(adopted.backup.pbs.datastore.is_some());
        assert!(adopted.backup.pbs.s3_endpoint.is_none());

        let replanned = super::super::plan::build(
            &adopted,
            &pve,
            &pbs,
            "fixture-capture",
            &crate::utility::progress::NullEventSink,
        )
        .unwrap();

        let backup_operations = replanned
            .operations
            .iter()
            .filter(|operation| matches!(operation.domain(), Domain::Backup | Domain::Pbs))
            .collect::<Vec<_>>();
        assert!(backup_operations.is_empty(), "{backup_operations:#?}");
    }
}
