use crate::{config::LocalState, model::FirewallPolicy, utility::progress::EventSink};
use anyhow::{Result, bail};
use std::collections::BTreeSet;

pub(super) fn validate(repo: &LocalState, events: &dyn EventSink) -> Result<()> {
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
            if !macs.insert(nic.mac.to_string()) {
                bail!("duplicate MAC: {}", nic.mac);
            }
        }
    }
    for (id, vm) in &repo.guests.vms {
        for nic in vm.networks.values() {
            if !bridges.contains(&nic.bridge) {
                bail!("qemu/{id}: unknown bridge {}", nic.bridge);
            }
            if !macs.insert(nic.mac.to_string()) {
                bail!("duplicate MAC: {}", nic.mac);
            }
        }
    }
    let managed: BTreeSet<_> = repo
        .guests
        .lxcs
        .keys()
        .chain(repo.guests.vms.keys())
        .collect();
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
    for (resource, policy) in
        std::iter::once(("cluster".to_string(), repo.cluster.firewall.as_ref()))
            .chain(std::iter::once((
                format!("node/{}", repo.guests.node),
                repo.node.firewall.as_ref(),
            )))
            .chain(
                repo.guests
                    .lxcs
                    .iter()
                    .map(|(id, guest)| (format!("lxc/{id}"), guest.firewall.as_ref())),
            )
            .chain(
                repo.guests
                    .vms
                    .iter()
                    .map(|(id, guest)| (format!("qemu/{id}"), guest.firewall.as_ref())),
            )
    {
        if let Some(policy) = policy {
            validate_firewall(&resource, policy)?;
        }
    }
    events.output(&format!(
        "configuration structurally valid: {} guests, {} NICs; management scope is documented in management-scope.json",
        managed.len(),
        macs.len()
    ));
    Ok(())
}

fn validate_firewall(resource: &str, policy: &FirewallPolicy) -> Result<()> {
    for reserved in ["enable", "log_level_in"] {
        if policy.options.contains_key(reserved) {
            bail!("firewall {resource}: option `{reserved}` has a dedicated field");
        }
    }
    unique_names(
        resource,
        "alias",
        policy.aliases.iter().map(|item| item.name.as_str()),
    )?;
    unique_names(
        resource,
        "IP set",
        policy.ip_sets.iter().map(|item| item.name.as_str()),
    )?;
    unique_names(
        resource,
        "security group",
        policy.security_groups.iter().map(|item| item.name.as_str()),
    )
}

fn unique_names<'a>(
    resource: &str,
    kind: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    if let Some(name) = names.into_iter().find(|name| !seen.insert(*name)) {
        bail!("firewall {resource}: duplicate {kind} `{name}`");
    }
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
