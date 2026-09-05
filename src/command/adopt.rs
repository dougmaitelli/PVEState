mod transaction;

use crate::{
    command::plan::{ApiTarget, Operation, Plan},
    config::{ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedState,
    model::{GuestField, GuestKind, GuestRef},
    resource::native,
    utility::{progress, runtime_security, yaml_patch},
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

#[derive(Debug, Serialize)]
pub(crate) struct Candidate {
    pub(crate) id: String,
    pub(crate) resource: String,
    pub(crate) field: String,
    pub(crate) local: String,
    pub(crate) captured: String,
    pub(crate) adoptable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
    #[serde(skip)]
    patches: Vec<LocalPatch>,
}

pub(crate) fn run(repo: &LocalState, preview: bool, all: bool, requested: &[String]) -> Result<()> {
    progress::section(if preview {
        "Previewing captured drift"
    } else {
        "Adopting captured state into local configuration"
    });
    runtime_security::prepare(&repo.runtime())?;
    let captured = CapturedState::load(repo, chrono::Duration::minutes(30))?;
    let plan: Plan = serde_json::from_slice(
        &runtime_security::read(&repo.runtime().join("production-plan.json"))
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
    let candidates = candidates(repo, &captured, &plan)?;

    if preview {
        progress::finish(true);
        println!("{}", serde_json::to_string_pretty(&candidates)?);
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
        progress::operation(format!(
            "{}: {} -> {}",
            candidate.id, candidate.local, candidate.captured
        ));
        if !candidate.adoptable {
            bail!(
                "{} cannot be adopted: {}",
                candidate.id,
                candidate.reason.as_deref().unwrap_or("unsupported")
            )
        }
        patches.extend(candidate.patches.clone());
    }
    let documents = apply_local_patches(repo, patches)?;
    transaction::validate(repo, &documents)?;
    transaction::publish(repo, &documents)?;
    progress::finish(true);
    println!(
        "adopted {} captured value(s) into {}",
        selected.len(),
        documents.keys().cloned().collect::<Vec<_>>().join(", ")
    );
    Ok(())
}

fn selection(
    candidates: &[Candidate],
    all: bool,
    requested: &[String],
) -> Result<BTreeSet<String>> {
    if all {
        let selected = candidates
            .iter()
            .filter(|candidate| candidate.adoptable)
            .map(|candidate| candidate.id.clone())
            .collect::<BTreeSet<_>>();
        if selected.is_empty() {
            bail!("there are no adoptable captured values")
        }
        return Ok(selected);
    }
    if requested.is_empty() {
        bail!("adopt requires --preview, --all, or at least one --id")
    }
    Ok(requested.iter().cloned().collect())
}

fn candidates(repo: &LocalState, captured: &CapturedState, plan: &Plan) -> Result<Vec<Candidate>> {
    let mut result = Vec::new();
    for operation in &plan.operations {
        let Operation::ApiMutation {
            target,
            domain,
            resource,
            changes,
            ..
        } = operation
        else {
            continue;
        };
        if *target != ApiTarget::Pve || domain != "guests" {
            for (field, desired) in changes {
                result.push(candidate(
                    resource,
                    field,
                    desired,
                    "captured",
                    false,
                    "this domain does not yet have an unambiguous local writer",
                ));
            }
            continue;
        }
        let guest = resource.guest().context("guest operation resource")?;
        let actual = guest_config(captured, &repo.guests.node, guest)?;
        for (field, desired) in changes {
            if field == "delete" {
                result.push(candidate(
                    resource,
                    field,
                    desired,
                    "present in captured state",
                    false,
                    "adopting additional devices is not yet supported",
                ));
                continue;
            }
            let production = actual.get(field).map(value_string).unwrap_or_default();
            let typed_field = adoption_field(guest.kind, field, &production);
            let adoptable = typed_field.is_some();
            let candidate_field = typed_field
                .map(GuestField::api_name)
                .unwrap_or_else(|| field.clone());
            let (desired, production) = if candidate_field.ends_with(".backed_up_by_pve") {
                (
                    option(desired, "backup").unwrap_or("0").to_string(),
                    option(&production, "backup").unwrap_or("0").to_string(),
                )
            } else {
                (desired.clone(), production)
            };
            let mut value = candidate(
                resource,
                &candidate_field,
                &desired,
                &production,
                adoptable,
                (!adoptable).then_some("field has no unambiguous desired-state mapping"),
            );
            if let Some(field) = typed_field {
                value
                    .patches
                    .push(guest_patch(guest, field, &value.captured)?);
            }
            result.push(value);
        }
    }
    for operation in &plan.operations {
        if let Operation::GrowDisk { resource, disk, .. } = operation {
            let Some(guest) = resource.guest() else {
                continue;
            };
            let actual = guest_config(captured, &repo.guests.node, guest)?;
            let disk_config = value_string(actual.get(disk.as_str()).unwrap_or(&Value::Null));
            let production = option(&disk_config, "size").unwrap_or_default();
            let mut value = candidate(
                resource,
                &GuestField::DiskSize.api_name(),
                "larger",
                production.trim_end_matches('G'),
                true,
                None,
            );
            value.patches.push(guest_patch(
                guest,
                GuestField::DiskSize,
                production.trim_end_matches('G'),
            )?);
            result.push(value);
        }
    }
    for operation in &plan.operations {
        match operation {
            Operation::WriteFile {
                domain, resource, ..
            }
            | Operation::DeleteFile {
                domain, resource, ..
            } if domain == "network" || domain == "firewall" => {
                let target = if domain == "network" {
                    native::Target::Network
                } else {
                    native::Target::Firewall {
                        resource: resource.to_string(),
                    }
                };
                let mut value = candidate(
                    resource,
                    "file",
                    "configured state",
                    "captured state",
                    true,
                    None,
                );
                match native::adoption_patches(repo, &captured.native, &target) {
                    Ok(patches) => value.patches = patches,
                    Err(error) => {
                        value.adoptable = false;
                        value.reason = Some(format!("{error:#}"));
                    },
                }
                result.push(value);
            },
            Operation::WriteFile { resource, path, .. }
            | Operation::DeleteFile { resource, path, .. } => result.push(candidate(
                resource,
                "file",
                "local rendering",
                path,
                false,
                "this native configuration format has no typed adoption adapter",
            )),
            _ => {},
        }
    }
    Ok(result)
}

fn candidate(
    resource: &impl ToString,
    field: &str,
    local: &str,
    captured: &str,
    adoptable: bool,
    reason: impl Into<Option<&'static str>>,
) -> Candidate {
    Candidate {
        id: format!("{}:{field}", resource.to_string()),
        resource: resource.to_string(),
        field: field.into(),
        local: local.into(),
        captured: captured.into(),
        adoptable,
        reason: reason.into().map(str::to_owned),
        patches: Vec::new(),
    }
}

fn adoption_field(kind: GuestKind, field: &str, production: &str) -> Option<GuestField> {
    let field = GuestField::from_api(field)?;
    match (kind, field) {
        (GuestKind::Lxc, GuestField::BindMount(index))
            if option(production, "backup").is_some() =>
        {
            Some(GuestField::BindMountBackup(index))
        },
        (
            GuestKind::Lxc,
            GuestField::Hostname
            | GuestField::Cores
            | GuestField::Memory
            | GuestField::Swap
            | GuestField::OnBoot,
        ) => Some(field),
        (
            GuestKind::Qemu,
            GuestField::Name
            | GuestField::Machine
            | GuestField::Bios
            | GuestField::Cores
            | GuestField::Sockets
            | GuestField::Memory
            | GuestField::Cpu
            | GuestField::Agent
            | GuestField::OnBoot,
        ) => Some(field),
        _ => None,
    }
}

fn guest_patch(reference: GuestRef, field: GuestField, captured: &str) -> Result<LocalPatch> {
    let mut prefix = vec![
        yaml_patch::Segment::Key(reference.kind.collection_name().into()),
        yaml_patch::Segment::Key(reference.vmid.to_string()),
    ];
    let yaml_patch::Patch::Set(mut path, value) = adoption_patch(reference.kind, field, captured)?
    else {
        unreachable!("guest field adoption only creates set patches")
    };
    prefix.append(&mut path);
    Ok(LocalPatch::SetScalar {
        document: ConfigDocument::Guests,
        path: prefix,
        value,
    })
}

#[derive(Clone, Copy)]
enum FieldValue {
    String,
    Integer,
    Boolean,
}

struct FieldMapping {
    guest: GuestKind,
    api: &'static str,
    yaml: &'static [&'static str],
    value: FieldValue,
}

const FIELD_MAPPINGS: &[FieldMapping] = &[
    FieldMapping {
        guest: GuestKind::Lxc,
        api: "hostname",
        yaml: &["hostname"],
        value: FieldValue::String,
    },
    FieldMapping {
        guest: GuestKind::Lxc,
        api: "cores",
        yaml: &["cores"],
        value: FieldValue::Integer,
    },
    FieldMapping {
        guest: GuestKind::Lxc,
        api: "memory",
        yaml: &["memory_mb"],
        value: FieldValue::Integer,
    },
    FieldMapping {
        guest: GuestKind::Lxc,
        api: "swap",
        yaml: &["swap_mb"],
        value: FieldValue::Integer,
    },
    FieldMapping {
        guest: GuestKind::Lxc,
        api: "onboot",
        yaml: &["start", "onboot"],
        value: FieldValue::Boolean,
    },
    FieldMapping {
        guest: GuestKind::Lxc,
        api: "size_gb",
        yaml: &["rootfs", "size_gb"],
        value: FieldValue::Integer,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "name",
        yaml: &["name"],
        value: FieldValue::String,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "machine",
        yaml: &["machine"],
        value: FieldValue::String,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "bios",
        yaml: &["bios"],
        value: FieldValue::String,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "cores",
        yaml: &["cpu", "cores"],
        value: FieldValue::Integer,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "sockets",
        yaml: &["cpu", "sockets"],
        value: FieldValue::Integer,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "memory",
        yaml: &["memory_mb"],
        value: FieldValue::Integer,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "cpu",
        yaml: &["cpu", "type"],
        value: FieldValue::String,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "agent",
        yaml: &["qemu_guest_agent"],
        value: FieldValue::Boolean,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "onboot",
        yaml: &["start", "onboot"],
        value: FieldValue::Boolean,
    },
    FieldMapping {
        guest: GuestKind::Qemu,
        api: "size_gb",
        yaml: &["disk", "size_gb"],
        value: FieldValue::Integer,
    },
];

fn adoption_patch(
    kind: GuestKind,
    field: GuestField,
    production: &str,
) -> Result<yaml_patch::Patch> {
    use yaml_patch::Segment::{Index, Key};
    if let (GuestKind::Lxc, GuestField::BindMountBackup(index)) = (kind, field) {
        return Ok(yaml_patch::Patch::Set(
            vec![
                Key("bind_mounts".into()),
                Index(index.into()),
                Key("backed_up_by_pve".into()),
            ],
            serde_yaml::Value::Bool(bool_value(production)?),
        ));
    }
    let api = field.api_name();
    let mapping = FIELD_MAPPINGS
        .iter()
        .find(|mapping| mapping.guest == kind && mapping.api == api)
        .with_context(|| format!("unsupported adoption field {api}"))?;
    let path = mapping.yaml.iter().map(|key| Key((*key).into())).collect();
    let value = match mapping.value {
        FieldValue::String => serde_yaml::Value::String(production.into()),
        FieldValue::Integer => serde_yaml::from_str(production)?,
        FieldValue::Boolean => serde_yaml::Value::Bool(bool_value(production)?),
    };
    Ok(yaml_patch::Patch::Set(path, value))
}

fn guest_config(captured: &CapturedState, node: &str, guest: GuestRef) -> Result<Value> {
    captured.pve.response(&guest.config_endpoint(node))
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

fn option<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value
        .split(',')
        .filter_map(|part| part.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn bool_value(value: &str) -> Result<bool> {
    match value {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => bail!("invalid boolean {value}"),
    }
}

fn value_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adopts_lxc_mount_backup_flag() {
        let before = include_str!("../../examples/basic/config/guests.yml");
        let patch = guest_patch(
            GuestRef::new(GuestKind::Lxc, 101),
            GuestField::BindMountBackup(0),
            "1",
        )
        .unwrap()
        .into_yaml_patch();
        let after = yaml_patch::apply_patches(before, &[patch]).unwrap();
        let guests: crate::model::Guests = serde_yaml::from_str(&after).unwrap();

        assert!(guests.lxcs[&101].bind_mounts[0].backed_up_by_pve);
        assert_eq!(
            before.replace("backed_up_by_pve: false", "backed_up_by_pve: true"),
            after
        );
    }

    #[test]
    fn classifies_device_removals_as_unsupported() {
        assert_eq!(adoption_field(GuestKind::Lxc, "delete", "net1"), None);
    }

    #[test]
    fn all_selects_only_adoptable_candidates() {
        let candidates = [
            Candidate {
                id: "yes".into(),
                resource: "lxc/1".into(),
                field: "cores".into(),
                local: "2".into(),
                captured: "4".into(),
                adoptable: true,
                reason: None,
                patches: Vec::new(),
            },
            Candidate {
                id: "no".into(),
                resource: "cluster".into(),
                field: "file".into(),
                local: "wanted".into(),
                captured: "live".into(),
                adoptable: false,
                reason: Some("unsupported".into()),
                patches: Vec::new(),
            },
        ];
        assert_eq!(
            selection(&candidates, true, &[]).unwrap(),
            BTreeSet::from(["yes".into()])
        );
    }
}
