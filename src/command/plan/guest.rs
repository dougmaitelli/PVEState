use super::Operation;
use crate::{
    model::{Lxc, Vm},
    render,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn lxc(
    node: &str,
    id: u32,
    desired: &Lxc,
    actual: &Value,
    operations: &mut Vec<Operation>,
    blockers: &mut Vec<String>,
) -> Result<()> {
    let mut wanted = BTreeMap::from([
        ("hostname".into(), desired.hostname.clone()),
        ("cores".into(), desired.cores.to_string()),
        ("memory".into(), desired.memory_mb.to_string()),
        ("swap".into(), desired.swap_mb.to_string()),
        ("onboot".into(), u8::from(desired.start.onboot).to_string()),
        (
            "startup".into(),
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
        ("net0".into(), render::lxc_nic(&desired.network)),
    ]);
    for (index, nic) in desired.additional_networks.iter().enumerate() {
        wanted.insert(format!("net{}", index + 1), render::lxc_nic(nic));
    }
    push_update(
        node,
        "lxc",
        id,
        changed(&wanted, actual),
        actual,
        operations,
    );
    disk(
        node,
        ("lxc", id),
        ("rootfs", &desired.rootfs.storage, desired.rootfs.size_gb),
        actual,
        operations,
        blockers,
    )
}

pub(super) fn vm(
    node: &str,
    id: u32,
    desired: &Vm,
    actual: &Value,
    operations: &mut Vec<Operation>,
    blockers: &mut Vec<String>,
) -> Result<()> {
    let mut wanted = BTreeMap::from([
        ("name".into(), desired.name.clone()),
        ("machine".into(), desired.machine.clone()),
        ("bios".into(), desired.bios.clone()),
        ("cores".into(), desired.cpu.cores.to_string()),
        ("sockets".into(), desired.cpu.sockets.to_string()),
        ("memory".into(), desired.memory_mb.to_string()),
        ("cpu".into(), desired.cpu.r#type.clone()),
        ("onboot".into(), u8::from(desired.start.onboot).to_string()),
        (
            "agent".into(),
            u8::from(desired.qemu_guest_agent).to_string(),
        ),
        ("startup".into(), format!("order={}", desired.start.order)),
    ]);
    for (index, nic) in desired.networks.iter().enumerate() {
        wanted.insert(format!("net{index}"), render::vm_nic(nic));
    }
    for usb in &desired.usb_passthrough {
        wanted.insert(usb.slot.clone(), format!("host={}", usb.host));
    }
    push_update(
        node,
        "qemu",
        id,
        changed(&wanted, actual),
        actual,
        operations,
    );
    disk(
        node,
        ("qemu", id),
        (
            &desired.disk.interface,
            &desired.disk.storage,
            desired.disk.size_gb,
        ),
        actual,
        operations,
        blockers,
    )
}

fn push_update(
    node: &str,
    kind: &str,
    id: u32,
    changes: BTreeMap<String, String>,
    actual: &Value,
    operations: &mut Vec<Operation>,
) {
    if !changes.is_empty() {
        operations.push(Operation::ApiUpdate {
            domain: "guests".into(),
            resource: format!("{kind}/{id}"),
            endpoint: format!("/nodes/{node}/{kind}/{id}/config"),
            changes,
            digest: actual["digest"].as_str().map(str::to_string),
        });
    }
}

fn disk(
    node: &str,
    identity: (&str, u32),
    desired: (&str, &str, u64),
    actual: &Value,
    operations: &mut Vec<Operation>,
    blockers: &mut Vec<String>,
) -> Result<()> {
    let (kind, id) = identity;
    let (key, storage, size) = desired;
    let options = parse_options(actual[key].as_str().context("disk config")?);
    let volume = options.get("volume").cloned().unwrap_or_default();
    let actual_storage = volume.split(':').next().unwrap_or("");
    let current = options
        .get("size")
        .and_then(|value| value.trim_end_matches('G').parse::<u64>().ok())
        .unwrap_or(0);
    if actual_storage != storage {
        blockers.push(format!("{kind}/{id}: storage moves are not automatic"));
    }
    if size < current {
        blockers.push(format!("{kind}/{id}: disk shrinking is forbidden"));
    } else if size > current {
        operations.push(Operation::GrowDisk {
            domain: "guests".into(),
            resource: format!("{kind}/{id}/{key}"),
            endpoint: format!("/nodes/{node}/{kind}/{id}/resize"),
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
            if key.starts_with("net") {
                let have = parse_options(actual.get(*key).and_then(Value::as_str).unwrap_or(""));
                let want = parse_options(value);
                want.iter().any(|(option, expected)| {
                    have.get(option)
                        .map(String::as_str)
                        .unwrap_or(if option == "firewall" { "0" } else { "" })
                        != expected
                })
            } else {
                actual
                    .get(*key)
                    .map(super::value_string)
                    .unwrap_or_else(|| {
                        if *key == "agent" {
                            "0".into()
                        } else {
                            "".into()
                        }
                    })
                    != **value
            }
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proxmox_volume_and_options() {
        let parsed = parse_options("VMs:vm-107-disk-1,discard=on,size=64G");
        assert_eq!(parsed["volume"], "VMs:vm-107-disk-1");
        assert_eq!(parsed["size"], "64G");
    }
}
