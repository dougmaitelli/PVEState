use super::{agent::QemuAgentOptions, render};
use crate::{
    command::plan::{ApiMethod, ApiTarget, Operation},
    model::{GuestField, GuestKind, GuestRef, Lxc, Vm},
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn lxc(
    node: &str,
    id: u32,
    desired: &Lxc,
    actual: &Value,
    operations: &mut Vec<Operation>,
    blockers: &mut Vec<String>,
) -> Result<()> {
    let mut wanted = BTreeMap::from([
        (GuestField::Hostname.api_name(), desired.hostname.clone()),
        (GuestField::OsType.api_name(), desired.os.clone()),
        (GuestField::Cores.api_name(), desired.cores.to_string()),
        (GuestField::Memory.api_name(), desired.memory_mb.to_string()),
        (GuestField::Swap.api_name(), desired.swap_mb.to_string()),
        (
            GuestField::OnBoot.api_name(),
            u8::from(desired.start.onboot).to_string(),
        ),
        (
            GuestField::Startup.api_name(),
            format!(
                "order={}{}",
                desired.start.order,
                desired
                    .start
                    .delay_seconds
                    .map(|seconds| format!(",up={seconds}"))
                    .unwrap_or_default()
            ),
        ),
    ]);
    for (index, nic) in desired.networks.iter().enumerate() {
        wanted.insert(
            GuestField::Network(index as u8).api_name(),
            render::lxc_nic(nic),
        );
    }
    for (index, mount) in desired.bind_mounts.iter().enumerate() {
        wanted.insert(
            GuestField::BindMount(index as u8).api_name(),
            render::bind_mount(mount),
        );
    }
    let mut changes = changed(&wanted, actual);
    add_removed(actual, &wanted, &["net", "mp"], &mut changes);
    let actual_unprivileged = actual
        .get("unprivileged")
        .map(value_string)
        .unwrap_or_else(|| "0".into());
    if actual_unprivileged != if desired.unprivileged { "1" } else { "0" } {
        blockers.push(format!(
            "lxc/{id}: privileged/unprivileged conversion is not automatic"
        ));
    }
    let guest = GuestRef::new(GuestKind::Lxc, id);
    push_update(node, guest, changes, actual, operations);
    disk(
        node,
        guest,
        (
            &GuestField::RootFs.api_name(),
            &desired.rootfs.storage,
            desired.rootfs.size_gb,
        ),
        actual,
        operations,
        blockers,
    )
}

pub(crate) fn vm(
    node: &str,
    id: u32,
    desired: &Vm,
    actual: &Value,
    operations: &mut Vec<Operation>,
    blockers: &mut Vec<String>,
) -> Result<()> {
    let mut wanted = BTreeMap::from([
        (GuestField::Name.api_name(), desired.name.clone()),
        (GuestField::Machine.api_name(), desired.machine.clone()),
        (GuestField::Bios.api_name(), desired.bios.clone()),
        (GuestField::Cores.api_name(), desired.cpu.cores.to_string()),
        (
            GuestField::Sockets.api_name(),
            desired.cpu.sockets.to_string(),
        ),
        (GuestField::Memory.api_name(), desired.memory_mb.to_string()),
        (GuestField::Cpu.api_name(), desired.cpu.r#type.clone()),
        (
            GuestField::OnBoot.api_name(),
            u8::from(desired.start.onboot).to_string(),
        ),
        (
            GuestField::Startup.api_name(),
            format!(
                "order={}{}",
                desired.start.order,
                desired
                    .start
                    .delay_seconds
                    .map(|seconds| format!(",up={seconds}"))
                    .unwrap_or_default()
            ),
        ),
    ]);
    for (slot, nic) in &desired.networks {
        wanted.insert(slot.to_string(), render::vm_nic(nic));
    }
    for (slot, usb) in &desired.usb_devices {
        wanted.insert(slot.to_string(), format!("host={}", usb.host));
    }
    for (slot, desired_disk) in &desired.disks {
        let key = slot.to_string();
        wanted.insert(key.clone(), desired_disk_value(actual, &key, desired_disk)?);
    }
    for (slot, efi) in &desired.efi_disks {
        let key = slot.to_string();
        wanted.insert(
            key.clone(),
            desired_efi(
                actual,
                &key,
                &efi.storage,
                efi.pre_enrolled_keys,
                blockers,
                id,
            ),
        );
    }
    let mut changes = changed(&wanted, actual);
    if let Some(agent) = agent_change(desired.qemu_guest_agent, actual.get("agent"))? {
        changes.insert(GuestField::Agent.api_name(), agent);
    }
    add_removed_vm(actual, &wanted, &mut changes);
    let guest = GuestRef::new(GuestKind::Qemu, id);
    push_update(node, guest, changes, actual, operations);
    for (slot, desired) in &desired.disks {
        let key = slot.to_string();
        if actual.get(&key).is_some() {
            disk(
                node,
                guest,
                (&key, &desired.storage, desired.size_gb),
                actual,
                operations,
                blockers,
            )?;
        }
    }
    Ok(())
}

fn agent_change(desired: bool, actual: Option<&Value>) -> Result<Option<String>> {
    let mut options = QemuAgentOptions::from_api(actual)?;
    if options.enabled == desired {
        return Ok(None);
    }
    options.enabled = desired;
    Ok(Some(options.render()))
}

fn push_update(
    node: &str,
    guest: GuestRef,
    changes: BTreeMap<String, String>,
    actual: &Value,
    operations: &mut Vec<Operation>,
) {
    if !changes.is_empty() {
        operations.push(Operation::ApiMutation {
            target: ApiTarget::Pve,
            method: ApiMethod::Put,
            domain: crate::command::plan::Domain::Guest,
            resource: guest.to_string().into(),
            endpoint: guest.config_endpoint(node).into(),
            changes,
            environment_changes: BTreeMap::new(),
            digest: actual["digest"].as_str().map(str::to_string),
        });
    }
}

fn disk(
    node: &str,
    guest: GuestRef,
    desired: (&str, &str, u64),
    actual: &Value,
    operations: &mut Vec<Operation>,
    blockers: &mut Vec<String>,
) -> Result<()> {
    let (key, storage, size) = desired;
    let options = parse_options(actual[key].as_str().context("disk config")?);
    let volume = options.get("volume").cloned().unwrap_or_default();
    let actual_storage = volume.split(':').next().unwrap_or("");
    let current = options
        .get("size")
        .and_then(|value| value.trim_end_matches('G').parse::<u64>().ok())
        .unwrap_or(0);
    if actual_storage != storage {
        blockers.push(format!("{guest}: storage moves are not automatic"));
    }
    if size < current {
        blockers.push(format!("{guest}: disk shrinking is forbidden"));
    } else if size > current {
        operations.push(Operation::GrowDisk {
            domain: crate::command::plan::Domain::Guest,
            resource: format!("{guest}/{key}").into(),
            endpoint: guest.resize_endpoint(node).into(),
            disk: key.into(),
            size_gb: size,
        });
    }
    Ok(())
}

fn changed(wanted: &BTreeMap<String, String>, actual: &Value) -> BTreeMap<String, String> {
    wanted
        .iter()
        .filter(|(key, value)| {
            if structured(key) {
                let have = parse_options(actual.get(*key).and_then(Value::as_str).unwrap_or(""));
                let want = parse_options(value);
                want.iter().any(|(option, expected)| {
                    have.get(option).map(String::as_str).unwrap_or(
                        if option == "firewall" || option == "backup" {
                            "0"
                        } else {
                            ""
                        },
                    ) != expected
                })
            } else {
                actual.get(*key).map(value_string).unwrap_or_else(|| {
                    if *key == "agent" {
                        "0".into()
                    } else {
                        "".into()
                    }
                }) != **value
            }
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn structured(key: &str) -> bool {
    ["net", "mp", "scsi", "sata", "virtio", "ide", "efidisk"]
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

fn desired_disk_value(actual: &Value, key: &str, desired: &super::VmDisk) -> Result<String> {
    let Some(current) = actual.get(key).and_then(Value::as_str) else {
        let mut value = format!("{}:{}", desired.storage, desired.size_gb);
        if desired.discard {
            value.push_str(",discard=on");
        }
        return Ok(value);
    };
    let mut options = parse_options(current);
    if desired.discard {
        options.insert("discard".into(), "on".into());
    } else {
        options.remove("discard");
    }
    Ok(render_options(&options))
}

fn desired_efi(
    actual: &Value,
    key: &str,
    storage: &str,
    pre_enrolled_keys: bool,
    blockers: &mut Vec<String>,
    id: u32,
) -> String {
    let mut options = parse_options(actual[key].as_str().unwrap_or(""));
    let volume = options
        .get("volume")
        .cloned()
        .unwrap_or_else(|| format!("{storage}:0"));
    let actual_storage = volume.split(':').next().unwrap_or("");
    if !actual_storage.is_empty() && actual_storage != storage {
        blockers.push(format!("qemu/{id}: EFI storage moves are not automatic"));
    }
    options.insert("volume".into(), volume);
    options
        .entry("efitype".into())
        .or_insert_with(|| "4m".into());
    options.insert(
        "pre-enrolled-keys".into(),
        u8::from(pre_enrolled_keys).to_string(),
    );
    render_options(&options)
}

fn add_removed_vm(
    actual: &Value,
    wanted: &BTreeMap<String, String>,
    changes: &mut BTreeMap<String, String>,
) {
    let removed = actual
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter(|(key, _)| {
            ["net", "usb", "efidisk"]
                .iter()
                .any(|prefix| numbered(key, prefix))
                || key.parse::<crate::model::DiskInterface>().is_ok()
        })
        .filter(|(key, value)| {
            !wanted.contains_key(*key)
                && !is_unmanaged_special_disk(key, value.as_str().unwrap_or_default())
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    if !removed.is_empty() {
        changes.insert("delete".into(), removed.join(","));
    }
}

pub(super) fn is_unmanaged_special_disk(key: &str, value: &str) -> bool {
    if key.parse::<crate::model::DiskInterface>().is_err() {
        return false;
    }
    let options = parse_options(value);
    options.get("media").is_some_and(|media| media == "cdrom")
        || options
            .get("volume")
            .is_some_and(|volume| volume.contains("cloudinit"))
}

fn render_options(options: &BTreeMap<String, String>) -> String {
    let mut values = Vec::new();
    if let Some(volume) = options.get("volume") {
        values.push(volume.clone());
    }
    values.extend(
        options
            .iter()
            .filter(|(key, _)| key.as_str() != "volume")
            .map(|(key, value)| format!("{key}={value}")),
    );
    values.join(",")
}

fn add_removed(
    actual: &Value,
    wanted: &BTreeMap<String, String>,
    prefixes: &[&str],
    changes: &mut BTreeMap<String, String>,
) {
    let removed = actual
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .filter(|key| prefixes.iter().any(|prefix| numbered(key, prefix)))
        .filter(|key| !wanted.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    if !removed.is_empty() {
        changes.insert("delete".into(), removed.join(","));
    }
}

fn numbered(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
}

fn parse_options(value: &str) -> BTreeMap<String, String> {
    value
        .split(',')
        .enumerate()
        .filter_map(|(index, item)| {
            item.split_once('=')
                .map(|(key, value)| (key.into(), value.into()))
                .or_else(|| (index == 0).then(|| ("volume".into(), item.into())))
        })
        .collect()
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
    fn parses_proxmox_volume_and_options() {
        let parsed = parse_options("VMs:vm-107-disk-1,discard=on,size=64G");
        assert_eq!(parsed["volume"], "VMs:vm-107-disk-1");
        assert_eq!(parsed["size"], "64G");
    }

    #[test]
    fn removed_devices_are_sent_through_delete() {
        let actual = serde_json::json!({"net0":"wanted", "net1":"stale", "usb2":"stale"});
        let wanted = BTreeMap::from([("net0".into(), "wanted".into())]);
        let mut changes = BTreeMap::new();
        add_removed(&actual, &wanted, &["net", "usb"], &mut changes);
        assert_eq!(changes["delete"], "net1,usb2");
    }

    #[test]
    fn guest_without_networks_removes_every_live_nic() {
        let actual = serde_json::json!({"net0":"first", "net1":"second", "cores":2});
        let mut changes = BTreeMap::new();

        add_removed(&actual, &BTreeMap::new(), &["net"], &mut changes);

        assert_eq!(changes["delete"], "net0,net1");
    }

    #[test]
    fn compound_agent_values_converge_without_discarding_options() {
        let live = serde_json::json!("1,fstrim_cloned_disks=1,type=virtio");
        assert_eq!(agent_change(true, Some(&live)).unwrap(), None);

        let mutation = agent_change(false, Some(&live)).unwrap().unwrap();
        assert_eq!(mutation, "0,fstrim_cloned_disks=1,type=virtio");
        let applied = serde_json::json!(mutation);
        assert_eq!(agent_change(false, Some(&applied)).unwrap(), None);
    }

    #[test]
    fn arbitrary_owned_disks_are_added_modified_and_removed() {
        let actual = serde_json::json!({
            "scsi0": "local-lvm:vm-201-disk-0,size=16G",
            "sata1": "data:vm-201-disk-1,size=64G,discard=off",
            "virtio2": "stale:vm-201-disk-2,size=8G"
        });
        let mut wanted = BTreeMap::new();
        wanted.insert(
            "scsi0".into(),
            desired_disk_value(
                &actual,
                "scsi0",
                &crate::resource::guest::VmDisk {
                    storage: "local-lvm".into(),
                    size_gb: 32,
                    discard: false,
                },
            )
            .unwrap(),
        );
        wanted.insert(
            "sata1".into(),
            desired_disk_value(
                &actual,
                "sata1",
                &crate::resource::guest::VmDisk {
                    storage: "data".into(),
                    size_gb: 64,
                    discard: true,
                },
            )
            .unwrap(),
        );
        wanted.insert(
            "ide3".into(),
            desired_disk_value(
                &actual,
                "ide3",
                &crate::resource::guest::VmDisk {
                    storage: "archive".into(),
                    size_gb: 20,
                    discard: false,
                },
            )
            .unwrap(),
        );

        let mut changes = changed(&wanted, &actual);
        add_removed_vm(&actual, &wanted, &mut changes);

        assert_eq!(changes["sata1"], "data:vm-201-disk-1,discard=on,size=64G");
        assert_eq!(changes["ide3"], "archive:20");
        assert_eq!(changes["delete"], "virtio2");
    }

    #[test]
    fn cdrom_and_cloud_init_drives_are_preserved_when_unmodeled() {
        let actual = serde_json::json!({
            "ide2": "local:iso/installer.iso,media=cdrom",
            "scsi1": "local-lvm:vm-201-cloudinit,media=cdrom",
            "sata2": "data:vm-201-disk-2,size=8G"
        });
        let mut changes = BTreeMap::new();

        add_removed_vm(&actual, &BTreeMap::new(), &mut changes);

        assert_eq!(changes["delete"], "sata2");
    }

    #[test]
    fn existing_disks_only_grow_and_never_move_or_shrink_automatically() {
        let actual = serde_json::json!({"scsi0": "local-lvm:vm-201-disk-0,size=32G"});
        let guest = GuestRef::new(GuestKind::Qemu, 201);
        let mut operations = Vec::new();
        let mut blockers = Vec::new();

        disk(
            "pve",
            guest,
            ("scsi0", "other-storage", 16),
            &actual,
            &mut operations,
            &mut blockers,
        )
        .unwrap();

        assert!(operations.is_empty());
        assert!(blockers.iter().any(|value| value.contains("storage moves")));
        assert!(blockers.iter().any(|value| value.contains("shrinking")));

        blockers.clear();
        disk(
            "pve",
            guest,
            ("scsi0", "local-lvm", 64),
            &actual,
            &mut operations,
            &mut blockers,
        )
        .unwrap();
        assert!(matches!(
            operations.as_slice(),
            [Operation::GrowDisk { size_gb: 64, .. }]
        ));
        assert!(blockers.is_empty());
    }
}
