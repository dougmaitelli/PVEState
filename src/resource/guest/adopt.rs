use super::agent::QemuAgentOptions;
use crate::{
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedState,
    model::{BindMount, DiskInterface, EfiSlot, GuestKind, NetworkSlot, Nic, UsbSlot, VmNic},
    reconcile::Operation,
    utility::yaml_patch::Segment,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn option_candidates(
    local: &LocalState,
    captured: &CapturedState,
) -> Result<Vec<AdoptionCandidate>> {
    let mut candidates = Vec::new();
    for (vmid, lxc) in &local.guests.lxcs {
        if lxc.options.is_some() {
            continue;
        }
        let guest = crate::model::GuestRef::new(GuestKind::Lxc, *vmid);
        let options = super::plan::lxc_options(captured.guest(guest)?.config())?;
        if options.is_empty() {
            continue;
        }
        let resource = crate::reconcile::ResourceId::Guest(guest);
        candidates.push(AdoptionCandidate::adoptable(
            &resource,
            "options",
            "not locally managed",
            format!("{} captured option(s)", options.len()),
            vec![LocalPatch::ReplaceResource {
                document: ConfigDocument::Guests,
                path: vec![
                    Segment::Key("lxcs".into()),
                    Segment::Key(vmid.to_string()),
                    Segment::Key("options".into()),
                ],
                value: serde_yaml::to_value(options)?,
            }],
        ));
    }
    Ok(candidates)
}

pub(crate) fn candidates(
    local: &LocalState,
    captured: &CapturedState,
    operation: &Operation,
) -> Result<Vec<AdoptionCandidate>> {
    let resource = operation.resource();
    let guest = resource.guest().context("guest operation resource")?;
    let managed = captured.guest(guest)?;
    debug_assert_eq!(managed.reference, guest);
    let parsed = match guest.kind {
        GuestKind::Lxc => captured_lxc(local, managed)
            .and_then(|value| serde_yaml::to_value(value).map_err(Into::into)),
        GuestKind::Qemu => captured_vm(local, managed)
            .and_then(|value| serde_yaml::to_value(value).map_err(Into::into)),
    };
    let value = match parsed {
        Ok(value) => value,
        Err(error) => {
            return Ok(vec![AdoptionCandidate::blocked(
                resource,
                operation_fields(operation),
                "local guest configuration",
                "captured guest configuration",
                format!("captured guest cannot be represented safely: {error:#}"),
            )]);
        },
    };
    let patches = vec![LocalPatch::ReplaceResource {
        document: ConfigDocument::Guests,
        path: vec![
            Segment::Key(guest.kind.collection_name().into()),
            Segment::Key(guest.vmid.to_string()),
        ],
        value,
    }];
    Ok(vec![AdoptionCandidate::adoptable(
        resource,
        operation_fields(operation),
        "local guest configuration",
        "captured guest configuration",
        patches,
    )])
}

fn captured_lxc(
    local: &LocalState,
    captured: &crate::discovery::managed::CapturedGuest,
) -> Result<super::Lxc> {
    let guest = captured.reference;
    let actual = captured.config();
    let mut result = local.guests.lxcs[&guest.vmid].clone();
    result.hostname = captured.name.clone();
    result.cores = captured.cores;
    result.memory_mb = captured.memory_mb;
    assign_string(actual, "ostype", &mut result.os);
    assign_bool(actual, "unprivileged", &mut result.unprivileged);
    assign_number(actual, "swap", &mut result.swap_mb);
    assign_bool(actual, "onboot", &mut result.start.onboot);
    assign_start(actual, &mut result.start)?;
    if let Some(rootfs) = actual.get("rootfs").and_then(Value::as_str) {
        let options = super::property::parse(rootfs)?;
        if let Some(volume) = options.get("volume") {
            result.rootfs.storage = volume.split(':').next().unwrap_or(volume).into();
        }
        if let Some(size) = options.get("size").and_then(|size| gigabytes(size)) {
            result.rootfs.size_gb = size;
        }
    }
    result.networks = numbered(actual, "net")
        .into_iter()
        .map(|(_, value)| parse_lxc_nic(value))
        .collect::<Result<_>>()?;
    result.bind_mounts = numbered(actual, "mp")
        .into_iter()
        .map(|(_, value)| parse_mount(value))
        .collect::<Result<_>>()?;
    result.options = Some(super::plan::lxc_options(actual)?);
    Ok(result)
}

fn captured_vm(
    local: &LocalState,
    captured: &crate::discovery::managed::CapturedGuest,
) -> Result<super::Vm> {
    let guest = captured.reference;
    let actual = captured.config();
    let mut result = local.guests.vms[&guest.vmid].clone();
    result.name = captured.name.clone();
    result.cpu.cores = captured.cores;
    result.memory_mb = captured.memory_mb;
    assign_string(actual, "machine", &mut result.machine);
    assign_string(actual, "bios", &mut result.bios);
    assign_string(actual, "cpu", &mut result.cpu.r#type);
    assign_number(actual, "sockets", &mut result.cpu.sockets);
    result.qemu_guest_agent = QemuAgentOptions::from_api(actual.get("agent"))?.enabled;
    assign_bool(actual, "onboot", &mut result.start.onboot);
    assign_start(actual, &mut result.start)?;
    result.disks = actual
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(key, value)| Some((key.parse::<DiskInterface>().ok()?, value.as_str()?)))
        .map(|(slot, value)| {
            if super::plan::is_unmanaged_special_disk(&slot.to_string(), value)? {
                Ok(None)
            } else {
                Ok(Some((slot, parse_vm_disk(value)?)))
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    result.efi_disks = numbered(actual, "efidisk")
        .into_iter()
        .map(|(index, value)| Ok((EfiSlot(index as u8), parse_efi(value)?)))
        .collect::<Result<_>>()?;
    result.networks = numbered(actual, "net")
        .into_iter()
        .map(|(index, value)| Ok((NetworkSlot(index as u8), parse_vm_nic(value)?)))
        .collect::<Result<_>>()?;
    result.usb_devices = numbered(actual, "usb")
        .into_iter()
        .map(|(index, value)| {
            let property = super::property::parse(value)?;
            Ok((
                UsbSlot(index as u8),
                super::Usb {
                    host: property
                        .get("host")
                        .or_else(|| property.get("volume"))
                        .cloned()
                        .context("captured USB host")?,
                },
            ))
        })
        .collect::<Result<_>>()?;
    Ok(result)
}

fn parse_vm_disk(value: &str) -> Result<super::VmDisk> {
    let item = super::property::parse(value)?;
    let volume = required(&item, "volume")?;
    Ok(super::VmDisk {
        storage: volume.split(':').next().unwrap_or(volume).into(),
        size_gb: item
            .get("size")
            .and_then(|size| gigabytes(size))
            .context("captured VM disk size")?,
        discard: item.get("discard").is_some_and(|value| *value == "on"),
    })
}

fn parse_efi(value: &str) -> Result<super::Efi> {
    let item = super::property::parse(value)?;
    let volume = required(&item, "volume")?;
    Ok(super::Efi {
        storage: volume.split(':').next().unwrap_or(volume).into(),
        pre_enrolled_keys: item
            .get("pre-enrolled-keys")
            .is_some_and(|value| matches!(value.as_str(), "1" | "true")),
    })
}

fn parse_lxc_nic(value: &str) -> Result<Nic> {
    let item = super::property::parse(value)?;
    Ok(Nic {
        name: required(&item, "name")?.into(),
        mac: required(&item, "hwaddr")?.parse()?,
        bridge: required(&item, "bridge")?.into(),
        firewall: flag(&item, "firewall"),
        ipv4: required(&item, "ip")?.into(),
        gateway4: item.get("gw").map(ToString::to_string),
        ipv6: item.get("ip6").map(ToString::to_string),
        gateway6: item.get("gw6").map(ToString::to_string),
    })
}

fn parse_vm_nic(value: &str) -> Result<VmNic> {
    let item = super::property::parse(value)?;
    let (model, mac) = item
        .iter()
        .find(|(key, _)| !matches!(key.as_str(), "volume" | "bridge" | "firewall" | "tag"))
        .context("captured VM NIC model")?;
    Ok(VmNic {
        model: model.clone(),
        mac: mac.parse()?,
        bridge: required(&item, "bridge")?.into(),
        firewall: flag(&item, "firewall"),
        vlan: item.get("tag").and_then(|tag| tag.parse().ok()),
    })
}

fn parse_mount(value: &str) -> Result<BindMount> {
    let item = super::property::parse(value)?;
    Ok(BindMount {
        source: required(&item, "volume")?.into(),
        target: required(&item, "mp")?.into(),
        backed_up_by_pve: flag(&item, "backup"),
    })
}

fn numbered<'a>(actual: &'a Value, prefix: &str) -> Vec<(usize, &'a str)> {
    let mut values = actual
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(key, value)| key.strip_prefix(prefix)?.parse().ok().zip(value.as_str()))
        .collect::<Vec<_>>();
    values.sort_by_key(|(index, _)| *index);
    values
}

fn required<'a>(options: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    options
        .get(key)
        .map(String::as_str)
        .with_context(|| format!("captured guest option {key}"))
}

fn flag(options: &BTreeMap<String, String>, key: &str) -> bool {
    options
        .get(key)
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "on"))
}

fn gigabytes(value: &str) -> Option<u64> {
    value.trim_end_matches('G').parse().ok()
}

fn assign_string(value: &Value, key: &str, target: &mut String) {
    if let Some(value) = value.get(key).and_then(Value::as_str) {
        *target = value.into();
    }
}

fn assign_bool(value: &Value, key: &str, target: &mut bool) {
    if let Some(value) = value.get(key) {
        *target = value
            .as_bool()
            .unwrap_or_else(|| matches!(value.as_str(), Some("1" | "true")));
    }
}

fn assign_number<T: std::str::FromStr>(value: &Value, key: &str, target: &mut T) {
    if let Some(parsed) = value
        .get(key)
        .and_then(|value| {
            value
                .as_u64()
                .map(|v| v.to_string())
                .or_else(|| value.as_str().map(str::to_owned))
        })
        .and_then(|value| value.parse().ok())
    {
        *target = parsed;
    }
}

fn assign_start(value: &Value, target: &mut super::Start) -> Result<()> {
    if let Some(startup) = value.get("startup").and_then(Value::as_str) {
        let options = super::property::parse(startup)?;
        if let Some(order) = options.get("order").and_then(|v| v.parse().ok()) {
            target.order = order;
        }
        target.delay_seconds = options.get("up").and_then(|v| v.parse().ok());
    }
    Ok(())
}

fn operation_fields(operation: &Operation) -> String {
    match operation {
        Operation::ApiMutation { changes, .. } => {
            changes.keys().cloned().collect::<Vec<_>>().join(",")
        },
        Operation::GrowDisk { disk, .. } => disk.to_string(),
        _ => "configuration".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GuestRef;

    #[test]
    fn parses_guest_devices_from_captured_api_values() {
        let lxc = parse_lxc_nic(
            "name=eth0,bridge=vmbr0,firewall=1,gw=192.0.2.1,hwaddr=02:00:00:00:01:01,ip=192.0.2.101/24",
        )
        .unwrap();
        assert_eq!(lxc.name, "eth0");
        assert_eq!(lxc.mac.to_string(), "02:00:00:00:01:01");

        let vm = parse_vm_nic("virtio=02:00:00:00:02:01,bridge=vmbr0,tag=20").unwrap();
        assert_eq!(vm.model, "virtio");
        assert_eq!(vm.vlan, Some(20));

        let mount = parse_mount("/mnt/data,mp=/srv/data,backup=1").unwrap();
        assert_eq!(mount.source, "/mnt/data");
        assert!(mount.backed_up_by_pve);
    }

    #[test]
    fn adoption_reads_enabled_state_from_compound_agent_value() {
        let value = serde_json::json!("1,fstrim_cloned_disks=1,type=virtio");
        assert!(QemuAgentOptions::from_api(Some(&value)).unwrap().enabled);
    }

    #[test]
    fn adopted_compound_agent_state_survives_reload_and_replans_cleanly() {
        let temp = tempfile::tempdir().unwrap();
        crate::config::scaffold::initialize(temp.path()).unwrap();
        let local = crate::config::open(temp.path()).unwrap();
        let fixture: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/planner/live.json"))
                .unwrap();
        let actual = &fixture["pve"]["/nodes/pve/qemu/201/config"];
        let guest = GuestRef::new(GuestKind::Qemu, 201);

        let captured = crate::discovery::managed::CapturedGuest::decode(
            guest,
            actual,
            "/nodes/pve/qemu/201/config",
        )
        .unwrap();
        let adopted = captured_vm(&local, &captured).unwrap();
        let yaml = serde_yaml::to_string(&adopted).unwrap();
        let reloaded: crate::resource::guest::Vm = serde_yaml::from_str(&yaml).unwrap();
        let mut operations = Vec::new();
        let mut blockers = Vec::new();
        crate::resource::guest::plan::vm(
            "pve",
            201,
            &reloaded,
            actual,
            &mut operations,
            &mut blockers,
        )
        .unwrap();

        assert!(operations.iter().all(|operation| match operation {
            Operation::ApiMutation { changes, .. } => !changes.contains_key("agent"),
            _ => true,
        }));
    }

    #[test]
    fn adoption_captures_multiple_disks_and_preserves_special_media() {
        let temp = tempfile::tempdir().unwrap();
        crate::config::scaffold::initialize(temp.path()).unwrap();
        let local = crate::config::open(temp.path()).unwrap();
        let guest = GuestRef::new(GuestKind::Qemu, 201);
        let mut actual: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/planner/live.json"))
                .unwrap();
        let config = &mut actual["pve"]["/nodes/pve/qemu/201/config"];
        config["sata1"] = serde_json::json!("data:vm-201-disk-1,size=64G,discard=on");
        config["ide2"] = serde_json::json!("local:iso/installer.iso,media=cdrom");
        config["scsi2"] = serde_json::json!("local-lvm:cloudinit");

        let captured = crate::discovery::managed::CapturedGuest::decode(
            guest,
            config,
            "/nodes/pve/qemu/201/config",
        )
        .unwrap();
        let adopted = captured_vm(&local, &captured).unwrap();

        assert!(adopted.disks.contains_key(&DiskInterface::Scsi(0)));
        assert!(adopted.disks.contains_key(&DiskInterface::Sata(1)));
        assert!(!adopted.disks.contains_key(&DiskInterface::Ide(2)));
        assert!(!adopted.disks.contains_key(&DiskInterface::Scsi(2)));
        assert_eq!(adopted.disks[&DiskInterface::Sata(1)].size_gb, 64);
    }

    #[test]
    fn lxc_adoption_captures_every_additional_scalar_option() {
        let temp = tempfile::tempdir().unwrap();
        crate::config::scaffold::initialize(temp.path()).unwrap();
        let local = crate::config::open(temp.path()).unwrap();
        let guest = GuestRef::new(GuestKind::Lxc, 101);
        let actual = serde_json::json!({
            "hostname": "apps",
            "cores": 2,
            "memory": 2048,
            "ostype": "debian-13",
            "swap": 512,
            "unprivileged": 1,
            "onboot": 1,
            "startup": "order=20,up=10",
            "rootfs": "local-lvm:subvol-101-disk-0,size=16G",
            "net0": "name=eth0,bridge=vmbr0,hwaddr=02:00:00:00:01:01,ip=192.0.2.101/24",
            "arch": "amd64",
            "description": "application container",
            "features": "nesting=1,keyctl=1",
            "protection": 1,
            "tags": "apps;production",
            "digest": "ignored"
        });
        let captured = crate::discovery::managed::CapturedGuest::decode(
            guest,
            &actual,
            "/nodes/pve/lxc/101/config",
        )
        .unwrap();

        let adopted = captured_lxc(&local, &captured).unwrap();
        let options = adopted.options.unwrap();

        assert_eq!(options["arch"], "amd64");
        assert_eq!(options["description"], "application container");
        assert_eq!(options["features"], "nesting=1,keyctl=1");
        assert_eq!(options["protection"], "1");
        assert_eq!(options["tags"], "apps;production");
        assert!(!options.contains_key("digest"));
    }
}
