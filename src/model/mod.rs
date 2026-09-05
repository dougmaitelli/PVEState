mod cluster;
mod manifest;
mod node;
mod proxmox;
mod recovery_check;
mod restore;
mod secrets;
mod services;
mod storage;

pub(crate) use crate::resource::{
    backup::{BackupConfig, PruneJob, SyncJob, VerifyJob},
    firewall::{
        FirewallAction, FirewallAlias, FirewallDirection, FirewallIpSet, FirewallIpSetEntry,
        FirewallLogLevel, FirewallPolicy, FirewallProtocol, FirewallRule, FirewallSecurityGroup,
    },
    guest::{BindMount, Guests, Lxc, Nic, Vm, VmNic},
    network::{Bridge, Interface, Network},
};
pub(crate) use cluster::ClusterConfig;
pub(crate) use manifest::RepositoryManifest;
pub(crate) use node::NodeConfig;
pub(crate) use proxmox::{
    DiskInterface, EfiSlot, GuestField, GuestKind, GuestRef, NetworkSlot, UsbSlot,
};
pub(crate) use recovery_check::RecoveryChecks;
pub(crate) use restore::{RestoreConfig, RestoreMount};
pub(crate) use secrets::RequiredSecretsConfig;
pub(crate) use services::ServicesConfig;
pub(crate) use storage::StorageConfig;
