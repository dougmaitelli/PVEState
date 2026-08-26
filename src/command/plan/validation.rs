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
        for nic in &lxc.networks {
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
    if let Some(id) = repo
        .firewall
        .absent_guest_files
        .iter()
        .find(|id| repo.firewall.guests.contains_key(id))
    {
        bail!("guest firewall {id} is both managed and explicitly absent");
    }
    validate_present_absent(
        "PVE backup job",
        repo.backup.pve_backup_jobs.keys(),
        &repo.backup.absent_pve_backup_jobs,
    )?;
    validate_present_absent(
        "PBS prune job",
        repo.backup.pbs.jobs.prune.keys(),
        &repo.backup.pbs.jobs.absent_prune,
    )?;
    validate_present_absent(
        "PBS verify job",
        repo.backup.pbs.jobs.verify.keys(),
        &repo.backup.pbs.jobs.absent_verify,
    )?;
    validate_present_absent(
        "PBS sync job",
        repo.backup.pbs.jobs.sync.keys(),
        &repo.backup.pbs.jobs.absent_sync,
    )?;
    println!(
        "configuration structurally valid: {} guests, {} NICs; management scope is documented in management-scope.json",
        managed.len(),
        macs.len()
    );
    Ok(())
}

fn validate_present_absent<'a>(
    kind: &str,
    present: impl IntoIterator<Item = &'a String>,
    absent: &[String],
) -> Result<()> {
    let present: BTreeSet<_> = present.into_iter().collect();
    if let Some(id) = absent.iter().find(|id| present.contains(id)) {
        bail!("{kind} {id} is both managed and explicitly absent");
    }
    Ok(())
}
