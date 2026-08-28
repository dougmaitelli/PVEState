mod yaml;

use crate::{
    command::plan::{ApiTarget, Operation, Plan},
    config::Repository,
    discovery::CaptureManifest,
    model::{GuestField, GuestKind, GuestRef},
    utility::{atomic_file, progress, runtime_security},
};
use anyhow::{Context, Result, bail};
use chrono::Duration;
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeSet, fs};

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub id: String,
    pub resource: String,
    pub field: String,
    pub desired: String,
    pub production: String,
    pub adoptable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub fn run(repo: &Repository, preview: bool, all: bool, requested: &[String]) -> Result<()> {
    progress::section(if preview {
        "Previewing adoptable drift"
    } else {
        "Adopting production state"
    });
    repo.secure_runtime()?;
    let manifest: CaptureManifest = serde_json::from_slice(
        &fs::read(repo.observed().join("manifest.json")).context("run capture first")?,
    )?;
    manifest.verify(&repo.observed(), Duration::minutes(30))?;
    let plan: Plan = serde_json::from_slice(
        &runtime_security::read(&repo.runtime().join("production-plan.json"))
            .context("run plan first")?,
    )?;
    plan.verify()?;
    let observed: Value = serde_json::from_slice(&fs::read(repo.observed().join("api/pve.json"))?)?;
    let candidates = candidates(repo, &plan, &observed)?;

    if preview {
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

    let path = repo.root.join("config/guests.yml");
    let mut content = fs::read_to_string(&path)?;
    for candidate in candidates
        .iter()
        .filter(|candidate| selected.contains(&candidate.id))
    {
        progress::operation(format!(
            "{}: {} -> {}",
            candidate.id, candidate.desired, candidate.production
        ));
        if !candidate.adoptable {
            bail!(
                "{} cannot be adopted: {}",
                candidate.id,
                candidate.reason.as_deref().unwrap_or("unsupported")
            )
        }
        content = apply_candidate(&content, candidate)?;
    }
    serde_yaml::from_str::<crate::model::Guests>(&content)
        .context("validate adopted config/guests.yml")?;
    atomic_file::write(&path, content.as_bytes())?;
    println!(
        "adopted {} production value(s) into config/guests.yml",
        selected.len()
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
            bail!("there are no adoptable production values")
        }
        return Ok(selected);
    }
    if requested.is_empty() {
        bail!("adopt requires --preview, --all, or at least one --id")
    }
    Ok(requested.iter().cloned().collect())
}

fn candidates(repo: &Repository, plan: &Plan, observed: &Value) -> Result<Vec<Candidate>> {
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
        let guest: GuestRef = resource.parse()?;
        let actual = guest_config(observed, &repo.guests.node, guest)?;
        for (field, desired) in changes {
            if field == "delete" {
                result.push(candidate(
                    resource,
                    field,
                    desired,
                    "present in production",
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
            result.push(candidate(
                resource,
                &candidate_field,
                &desired,
                &production,
                adoptable,
                (!adoptable).then_some("field has no unambiguous desired-state mapping"),
            ));
        }
    }
    for operation in &plan.operations {
        if let Operation::GrowDisk { resource, disk, .. } = operation {
            let mut parts = resource.split('/');
            let (Some(kind), Some(id)) = (parts.next(), parts.next()) else {
                continue;
            };
            let guest = GuestRef::new(kind.parse()?, id.parse()?);
            let actual = guest_config(observed, &repo.guests.node, guest)?;
            let disk_config = value_string(actual.get(disk).unwrap_or(&Value::Null));
            let production = option(&disk_config, "size").unwrap_or_default();
            result.push(candidate(
                resource,
                &GuestField::DiskSize.api_name(),
                "larger",
                production.trim_end_matches('G'),
                true,
                None,
            ));
        }
    }
    for operation in &plan.operations {
        match operation {
            Operation::WriteFile { resource, path, .. }
            | Operation::DeleteFile { resource, path, .. } => result.push(candidate(
                resource,
                "file",
                "desired rendering",
                path,
                false,
                "adopting rendered host files is not supported",
            )),
            _ => {},
        }
    }
    Ok(result)
}

fn candidate(
    resource: &str,
    field: &str,
    desired: &str,
    production: &str,
    adoptable: bool,
    reason: impl Into<Option<&'static str>>,
) -> Candidate {
    Candidate {
        id: format!("{resource}:{field}"),
        resource: resource.into(),
        field: field.into(),
        desired: desired.into(),
        production: production.into(),
        adoptable,
        reason: reason.into().map(str::to_owned),
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

fn apply_candidate(content: &str, candidate: &Candidate) -> Result<String> {
    let reference: GuestRef = candidate.resource.parse()?;
    let field = candidate_field(&candidate.field)?;
    let mut prefix = vec![
        yaml::Segment::Key(reference.kind.collection_name().into()),
        yaml::Segment::Key(reference.vmid.to_string()),
    ];
    let yaml::Patch::Set(mut path, value) =
        adoption_patch(reference.kind, field, &candidate.production)?
    else {
        unreachable!("guest field adoption only creates set patches")
    };
    prefix.append(&mut path);
    yaml::apply_patches(content, &[yaml::Patch::Set(prefix, value)])
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

fn adoption_patch(kind: GuestKind, field: GuestField, production: &str) -> Result<yaml::Patch> {
    use yaml::Segment::{Index, Key};
    if let (GuestKind::Lxc, GuestField::BindMountBackup(index)) = (kind, field) {
        return Ok(yaml::Patch::Set(
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
    Ok(yaml::Patch::Set(path, value))
}

fn guest_config<'a>(observed: &'a Value, node: &str, guest: GuestRef) -> Result<&'a Value> {
    let collection = guest.kind.collection_name();
    observed
        .pointer(&format!(
            "/nodes/{node}/{collection}/{}/config/data",
            guest.vmid
        ))
        .with_context(|| format!("captured config for {guest}"))
}

fn candidate_field(value: &str) -> Result<GuestField> {
    if let Some(index) = value
        .strip_prefix("mp")
        .and_then(|value| value.strip_suffix(".backed_up_by_pve"))
        .and_then(|value| value.parse().ok())
    {
        return Ok(GuestField::BindMountBackup(index));
    }
    GuestField::from_api(value).with_context(|| format!("unknown guest field {value}"))
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
        let candidate = Candidate {
            id: "lxc/101:mp0.backed_up_by_pve".into(),
            resource: "lxc/101".into(),
            field: "mp0.backed_up_by_pve".into(),
            desired: "0".into(),
            production: "1".into(),
            adoptable: true,
            reason: None,
        };

        let after = apply_candidate(before, &candidate).unwrap();
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
                desired: "2".into(),
                production: "4".into(),
                adoptable: true,
                reason: None,
            },
            Candidate {
                id: "no".into(),
                resource: "cluster".into(),
                field: "file".into(),
                desired: "wanted".into(),
                production: "live".into(),
                adoptable: false,
                reason: Some("unsupported".into()),
            },
        ];
        assert_eq!(
            selection(&candidates, true, &[]).unwrap(),
            BTreeSet::from(["yes".into()])
        );
    }
}
