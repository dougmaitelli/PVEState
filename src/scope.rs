use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ManagementClass {
    ProductionManaged,
    RecoveryOnly,
    ValidationOnly,
    DeclaredOnly,
    Metadata,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ScopeEntry {
    pub document: &'static str,
    pub fields: &'static str,
    pub class: ManagementClass,
    pub behavior: &'static str,
}

const ENTRIES: &[ScopeEntry] = &[
    entry(
        "pves.yml",
        "*",
        ManagementClass::Metadata,
        "Repository format and tool compatibility metadata.",
    ),
    entry(
        "cluster.yml",
        "{cluster,proxmox,workload_profile,backup}",
        ManagementClass::Metadata,
        "Environment inventory and operator-facing context.",
    ),
    entry(
        "node.yml",
        "node.*",
        ManagementClass::Metadata,
        "Observed host identity and version expectations.",
    ),
    entry(
        "node.yml",
        "storage_topology.*",
        ManagementClass::DeclaredOnly,
        "Documents storage prerequisites, but recovery does not currently consume them.",
    ),
    entry(
        "guests.yml",
        "node",
        ManagementClass::ProductionManaged,
        "Selects the PVE node used by guest planning and DNS reconciliation.",
    ),
    entry(
        "guests.yml",
        "lxcs.*.{hostname,os,cores,memory_mb,swap_mb,networks,bind_mounts,start,firewall}",
        ManagementClass::ProductionManaged,
        "Compared with the live LXC API; listed devices are updated and removed devices are deleted.",
    ),
    entry(
        "guests.yml",
        "lxcs.*.rootfs",
        ManagementClass::ProductionManaged,
        "Storage moves and shrinking are blocked; growth is applied.",
    ),
    entry(
        "guests.yml",
        "lxcs.*.unprivileged",
        ManagementClass::DeclaredOnly,
        "Captured in desired state but not reconciled during production apply.",
    ),
    entry(
        "guests.yml",
        "vms.*.{name,machine,bios,cpu,memory_mb,networks,usb_passthrough,qemu_guest_agent,start}",
        ManagementClass::ProductionManaged,
        "Compared with the live QEMU API; listed NIC/USB devices are updated and removed devices are deleted.",
    ),
    entry(
        "guests.yml",
        "vms.*.{disk,efi}",
        ManagementClass::ProductionManaged,
        "Disk growth, discard, and EFI options are managed; storage moves and shrinking are blocked.",
    ),
    entry(
        "network.yml",
        "dns.*",
        ManagementClass::ProductionManaged,
        "Search domain and listed DNS servers are updated through the PVE API.",
    ),
    entry(
        "network.yml",
        "{interfaces,bridges}",
        ManagementClass::ProductionManaged,
        "Renders /etc/network/interfaces; activation requires an additional confirmation.",
    ),
    entry(
        "network.yml",
        "{host,management_address,management_gateway}",
        ManagementClass::DeclaredOnly,
        "Describes the management endpoint but is not independently reconciled.",
    ),
    entry(
        "cluster.yml",
        "firewall",
        ManagementClass::ProductionManaged,
        "Creates, replaces, or removes the cluster firewall file with concurrent-change checks.",
    ),
    entry(
        "node.yml",
        "firewall",
        ManagementClass::ProductionManaged,
        "Creates, replaces, or removes the managed node firewall file with concurrent-change checks.",
    ),
    entry(
        "storage.yml",
        "*",
        ManagementClass::DeclaredOnly,
        "Documents mounts, pools, and PVE storage definitions but is not consumed by apply or recovery.",
    ),
    entry(
        "backup.yml",
        "{pbs.datastore,pbs.s3_endpoint,pbs.jobs,pve_backup_jobs,absent_pve_backup_jobs}",
        ManagementClass::ProductionManaged,
        "Reconciles PVE backup jobs and PBS datastore, S3 endpoint, prune, verify, and sync jobs; removals require explicit absent IDs.",
    ),
    entry(
        "backup.yml",
        "{pbs.endpoint,pbs.version,pbs.guest}",
        ManagementClass::Metadata,
        "Describes the PBS deployment; connection settings and credentials come from the environment.",
    ),
    entry(
        "restore.yml",
        "{target.production_address,pbs_bootstrap.lxc_template,pbs_bootstrap.storage_attached_to_pve,archives,restore_order,reattach_mounts,application.configure_command}",
        ManagementClass::RecoveryOnly,
        "Controls guarded replacement-host recovery stages.",
    ),
    entry(
        "restore.yml",
        "{target.expected_hostname,target.plan_max_age_minutes,pbs_bootstrap.vmid,pbs_bootstrap.datastore,pbs_bootstrap.cache_path,pbs_bootstrap.s3_endpoint_id,pbs_bootstrap.bucket,pbs_bootstrap.region,protected_vmids,application.repository,application.docker_guest_vmid,application.configure_playbook}",
        ManagementClass::DeclaredOnly,
        "Accepted by the recovery model but not currently consumed by recovery execution.",
    ),
    entry(
        "recovery-checks.yml",
        "*",
        ManagementClass::ValidationOnly,
        "Commands executed read-only by pves validate.",
    ),
    entry(
        "services.yml",
        "*",
        ManagementClass::DeclaredOnly,
        "Application inventory and persistence guidance; no command currently consumes it.",
    ),
    entry(
        "required-secrets.yml",
        "*",
        ManagementClass::Metadata,
        "Inventory of off-host secrets; values are never managed by PVE State.",
    ),
];

const fn entry(
    document: &'static str,
    fields: &'static str,
    class: ManagementClass,
    behavior: &'static str,
) -> ScopeEntry {
    ScopeEntry {
        document,
        fields,
        class,
        behavior,
    }
}

pub fn entries() -> &'static [ScopeEntry] {
    ENTRIES
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_configuration_document_has_a_scope() {
        let actual: BTreeSet<_> = entries().iter().map(|entry| entry.document).collect();
        let expected = BTreeSet::from([
            "pves.yml",
            "cluster.yml",
            "node.yml",
            "guests.yml",
            "network.yml",
            "storage.yml",
            "backup.yml",
            "restore.yml",
            "recovery-checks.yml",
            "services.yml",
            "required-secrets.yml",
        ]);

        assert_eq!(actual, expected);
    }

    #[test]
    fn every_scope_has_an_explanation() {
        assert!(entries().iter().all(|entry| !entry.behavior.is_empty()));
    }
}
