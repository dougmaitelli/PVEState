use crate::{
    command::plan::Operation,
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedState,
    model::{BindMount, GuestKind, GuestRef, Nic, VmNic},
    utility::yaml_patch::Segment,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn candidates(
    local: &LocalState,
    captured: &CapturedState,
    operation: &Operation,
) -> Result<Vec<AdoptionCandidate>> {
    let resource = operation.resource();
    let guest = resource.guest().context("guest operation resource")?;
    let actual = captured
        .pve
        .response(&guest.config_endpoint(&local.guests.node))?;
    let parsed = match guest.kind {
        GuestKind::Lxc => captured_lxc(local, guest, &actual)
            .and_then(|value| serde_yaml::to_value(value).map_err(Into::into)),
        GuestKind::Qemu => captured_vm(local, guest, &actual)
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

fn captured_lxc(local: &LocalState, guest: GuestRef, actual: &Value) -> Result<super::Lxc> {
    let mut result = local.guests.lxcs[&guest.vmid].clone();
    assign_string(actual, "hostname", &mut result.hostname);
    assign_string(actual, "ostype", &mut result.os);
    assign_bool(actual, "unprivileged", &mut result.unprivileged);
    assign_number(actual, "cores", &mut result.cores);
    assign_number(actual, "memory", &mut result.memory_mb);
    assign_number(actual, "swap", &mut result.swap_mb);
    assign_bool(actual, "onboot", &mut result.start.onboot);
    assign_start(actual, &mut result.start);
    if let Some(rootfs) = actual.get("rootfs").and_then(Value::as_str) {
        let options = options(rootfs);
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
    Ok(result)
}

fn captured_vm(local: &LocalState, guest: GuestRef, actual: &Value) -> Result<super::Vm> {
    let mut result = local.guests.vms[&guest.vmid].clone();
    assign_string(actual, "name", &mut result.name);
    assign_string(actual, "machine", &mut result.machine);
    assign_string(actual, "bios", &mut result.bios);
    assign_string(actual, "cpu", &mut result.cpu.r#type);
    assign_number(actual, "cores", &mut result.cpu.cores);
    assign_number(actual, "sockets", &mut result.cpu.sockets);
    assign_number(actual, "memory", &mut result.memory_mb);
    assign_bool(actual, "agent", &mut result.qemu_guest_agent);
    assign_bool(actual, "onboot", &mut result.start.onboot);
    assign_start(actual, &mut result.start);
    let disk_key = result.disk.interface.to_string();
    if let Some(disk) = actual.get(&disk_key).and_then(Value::as_str) {
        let options = options(disk);
        if let Some(volume) = options.get("volume") {
            result.disk.storage = volume.split(':').next().unwrap_or(volume).into();
        }
        if let Some(size) = options.get("size").and_then(|size| gigabytes(size)) {
            result.disk.size_gb = size;
        }
        result.disk.discard = options.get("discard").is_some_and(|value| *value == "on");
    }
    if let Some(efi) = actual.get("efidisk0").and_then(Value::as_str) {
        let options = options(efi);
        if let Some(volume) = options.get("volume") {
            result.efi.storage = volume.split(':').next().unwrap_or(volume).into();
        }
        result.efi.pre_enrolled_keys = options
            .get("pre-enrolled-keys")
            .is_some_and(|value| matches!(*value, "1" | "true"));
    }
    result.networks = numbered(actual, "net")
        .into_iter()
        .map(|(_, value)| parse_vm_nic(value))
        .collect::<Result<_>>()?;
    result.usb_passthrough = numbered(actual, "usb")
        .into_iter()
        .map(|(index, value)| {
            Ok(super::Usb {
                slot: format!("usb{index}").parse()?,
                host: options(value)
                    .get("host")
                    .copied()
                    .unwrap_or(value.split(',').next().unwrap_or_default())
                    .into(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(result)
}

fn parse_lxc_nic(value: &str) -> Result<Nic> {
    let item = options(value);
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
    let item = options(value);
    let (model, mac) = item
        .iter()
        .find(|(key, _)| !matches!(**key, "bridge" | "firewall" | "tag"))
        .context("captured VM NIC model")?;
    Ok(VmNic {
        model: (*model).into(),
        mac: mac.parse()?,
        bridge: required(&item, "bridge")?.into(),
        firewall: flag(&item, "firewall"),
        vlan: item.get("tag").and_then(|tag| tag.parse().ok()),
    })
}

fn parse_mount(value: &str) -> Result<BindMount> {
    let item = options(value);
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

fn options(value: &str) -> BTreeMap<&str, &str> {
    value
        .split(',')
        .enumerate()
        .filter_map(|(index, item)| {
            item.split_once('=')
                .or_else(|| (index == 0).then_some(("volume", item)))
        })
        .collect()
}

fn required<'a>(options: &'a BTreeMap<&str, &str>, key: &str) -> Result<&'a str> {
    options
        .get(key)
        .copied()
        .with_context(|| format!("captured guest option {key}"))
}

fn flag(options: &BTreeMap<&str, &str>, key: &str) -> bool {
    options
        .get(key)
        .is_some_and(|value| matches!(*value, "1" | "true" | "on"))
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

fn assign_start(value: &Value, target: &mut super::Start) {
    if let Some(startup) = value.get("startup").and_then(Value::as_str) {
        let options = options(startup);
        if let Some(order) = options.get("order").and_then(|v| v.parse().ok()) {
            target.order = order;
        }
        target.delay_seconds = options.get("up").and_then(|v| v.parse().ok());
    }
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
}
