use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ManagementLevel {
    Archived,
    Declared,
    Planned,
    Adoptable,
    Applicable,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ManagementClass {
    ProductionManaged,
    RecoveryOnly,
    ValidationOnly,
    DeclaredOnly,
    Metadata,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct ScopeEntry {
    pub(crate) document: &'static str,
    pub(crate) fields: &'static str,
    pub(crate) class: ManagementClass,
    pub(crate) level: ManagementLevel,
    pub(crate) behavior: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct LevelDefinition {
    pub(crate) level: ManagementLevel,
    pub(crate) meaning: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScopeManifest {
    pub(crate) levels: &'static [LevelDefinition],
    pub(crate) entries: &'static [ScopeEntry],
}

const LEVELS: &[LevelDefinition] = &[
    LevelDefinition {
        level: ManagementLevel::Archived,
        meaning: "Retained as evidence or inventory; no local reconciliation is promised.",
    },
    LevelDefinition {
        level: ManagementLevel::Declared,
        meaning: "Represented in typed local configuration but not reconciled.",
    },
    LevelDefinition {
        level: ManagementLevel::Planned,
        meaning: "Compared with captured state and emitted as drift, but not adoptable or applicable.",
    },
    LevelDefinition {
        level: ManagementLevel::Adoptable,
        meaning: "Captured drift can be written into local configuration, but local changes are not applicable.",
    },
    LevelDefinition {
        level: ManagementLevel::Applicable,
        meaning: "Participates in its named workflow through guarded execution.",
    },
];

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
        "{lxcs.*,vms.*} resource existence",
        ManagementClass::DeclaredOnly,
        "Locally declared guest IDs are owned. Missing declared guests are blocked because creation is unsupported; extra live guests are archived outside ownership and are never deleted.",
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
        "vms.*.{name,machine,bios,cpu,memory_mb,networks,usb_devices,qemu_guest_agent,start}",
        ManagementClass::ProductionManaged,
        "Compared with the live QEMU API; listed NIC/USB devices are updated and removed devices are deleted.",
    ),
    entry(
        "guests.yml",
        "vms.*.{disks,efi_disks}",
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
        "{target.expected_hostname,target.expected_host_key_sha256,target.production_address,pbs_bootstrap.lxc_template,pbs_bootstrap.storage_attached_to_pve,archives,restore_order,reattach_mounts}",
        ManagementClass::RecoveryOnly,
        "Controls guarded replacement-host recovery stages.",
    ),
    entry(
        "restore.yml",
        "{target.plan_max_age_minutes,pbs_bootstrap.vmid,pbs_bootstrap.datastore,pbs_bootstrap.cache_path,pbs_bootstrap.s3_endpoint_id,pbs_bootstrap.bucket,pbs_bootstrap.region,protected_vmids}",
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
        level: match class {
            ManagementClass::ProductionManaged
            | ManagementClass::RecoveryOnly
            | ManagementClass::ValidationOnly => ManagementLevel::Applicable,
            ManagementClass::DeclaredOnly => ManagementLevel::Declared,
            ManagementClass::Metadata => ManagementLevel::Archived,
        },
        behavior,
    }
}

#[cfg(test)]
pub(crate) fn entries() -> &'static [ScopeEntry] {
    ENTRIES
}

pub(crate) const fn manifest() -> ScopeManifest {
    ScopeManifest {
        levels: LEVELS,
        entries: ENTRIES,
    }
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

    #[test]
    fn captured_guest_existence_is_not_advertised_as_managed() {
        let existence = entries()
            .iter()
            .find(|entry| entry.fields.contains("resource existence"))
            .unwrap();
        assert_eq!(existence.class, ManagementClass::DeclaredOnly);
        assert_eq!(existence.level, ManagementLevel::Declared);
        assert!(existence.behavior.contains("outside ownership"));
    }

    #[test]
    fn every_management_level_is_defined_in_the_manifest() {
        assert_eq!(manifest().levels.len(), 5);
        assert!(
            manifest()
                .levels
                .iter()
                .all(|definition| !definition.meaning.is_empty())
        );
    }

    #[test]
    fn declared_and_production_classes_have_unambiguous_levels() {
        assert!(entries().iter().all(|entry| match entry.class {
            ManagementClass::DeclaredOnly => entry.level == ManagementLevel::Declared,
            ManagementClass::ProductionManaged => entry.level == ManagementLevel::Applicable,
            _ => true,
        }));

        let storage = entries()
            .iter()
            .find(|entry| entry.document == "storage.yml" && entry.fields == "*")
            .unwrap();
        assert_eq!(storage.level, ManagementLevel::Declared);
    }
}
