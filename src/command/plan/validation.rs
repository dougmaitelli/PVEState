use crate::config::Repository;
use anyhow::{Result, bail};
use std::collections::BTreeSet;

pub(super) fn validate(repo: &Repository) -> Result<()> {
    let bridges: BTreeSet<_> = repo
        .network
        .bridges
        .iter()
        .map(|bridge| &bridge.name)
        .collect();
    let mut macs = BTreeSet::new();
    for (id, lxc) in &repo.guests.lxcs {
        let mut names = BTreeSet::new();
        for nic in std::iter::once(&lxc.network).chain(&lxc.additional_networks) {
            if !bridges.contains(&nic.bridge) {
                bail!("lxc/{id}: unknown bridge {}", nic.bridge);
            }
            if !names.insert(&nic.name) {
                bail!("lxc/{id}: duplicate NIC name {}", nic.name);
            }
            if !macs.insert(nic.mac.to_uppercase()) {
                bail!("duplicate MAC: {}", nic.mac);
            }
        }
    }
    for (id, vm) in &repo.guests.vms {
        let mut slots = BTreeSet::new();
        for nic in &vm.networks {
            if !bridges.contains(&nic.bridge) {
                bail!("qemu/{id}: unknown bridge {}", nic.bridge);
            }
            if !macs.insert(nic.mac.to_uppercase()) {
                bail!("duplicate MAC: {}", nic.mac);
            }
        }
        for usb in &vm.usb_passthrough {
            if !slots.insert(&usb.slot) {
                bail!("qemu/{id}: duplicate USB slot {}", usb.slot);
            }
        }
    }
    let managed: BTreeSet<_> = repo
        .guests
        .lxcs
        .keys()
        .chain(repo.guests.vms.keys())
        .collect();
    if let Some(id) = repo.firewall.guests.keys().find(|id| !managed.contains(id)) {
        bail!("firewall policy references unmanaged VMID {id}");
    }
    println!(
        "configuration valid: {} guests, {} NICs",
        managed.len(),
        macs.len()
    );
    Ok(())
}
