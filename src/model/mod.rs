mod cluster;
mod manifest;
mod node;
mod proxmox;
mod recovery_check;
mod restore;
mod secrets;
mod services;
mod storage;

pub use crate::resource::{
    backup::{
        BackupConfig, BackupGuest, BackupJobs, Datastore, PbsBackup, PruneJob, PveBackupJob,
        Retention, S3Endpoint, SyncJob, VerifyJob,
    },
    firewall::{
        FirewallAlias, FirewallIpSet, FirewallIpSetEntry, FirewallPolicy, FirewallRule,
        FirewallSecurityGroup,
    },
    guest::{BindMount, Cpu, Disk, Efi, Guests, Lxc, Nic, Start, Usb, Vm, VmDisk, VmNic},
    network::{Bridge, Dns, Interface, Network},
};
pub use cluster::*;
pub use manifest::*;
pub use node::*;
pub use proxmox::*;
pub use recovery_check::*;
pub use restore::*;
pub use secrets::*;
pub use services::*;
pub use storage::*;
