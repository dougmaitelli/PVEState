# Production restore plan

## Objectives

The system must support two deliberate recovery paths:

1. **Fast recovery:** restore an existing VM/LXC backup from PBS.
2. **Clean rebuild:** recreate infrastructure and configuration from Git, then
   restore only persistent application data.

Neither path should depend on an unattended CI/CD runner.

## Recovery order

1. Reinstall and harden the PVE host or cluster.
2. Restore the minimum host configuration: networking, storage, certificates,
   users, ACLs, and required cluster settings.
3. Recover PBS access and its S3-backed datastore.
4. Restore infrastructure-critical guests, including PBS if it is itself a guest.
5. Restore stateful services and validate their data.
6. Rebuild disposable services from Ansible definitions.
7. Validate networking, DNS, certificates, monitoring, and scheduled backups.

## PBS/S3 bootstrap risk

PBS runs inside the Proxmox environment, so recovery has a dependency loop:

```text
PVE is needed to run PBS
       ↓
PBS is needed for fast PVE guest restores
```

The repository must eventually document a tested bootstrap path for the PBS
guest. At minimum, preserve outside the failed PVE system:

- PBS installer/version and guest hardware definition
- PBS datastore name and S3 backend metadata
- S3 endpoint, bucket, region, and addressing mode
- required local persistent cache layout and sizing
- PBS user/ACL definitions
- encryption keys and recovery passwords, stored in a separate secret manager
- TLS CA/fingerprints needed to reconnect securely

Secrets do not belong in this repository.

## Production adoption gates

- [ ] Read-only API discovery reviewed
- [x] Initial read-only API discovery captured
- [x] PBS datastore, S3 backend, pruning, and verification jobs discovered
- [x] Current PBS guest backup coverage enumerated
- [x] PVE host storage, mounts, ZFS health, and PBS guest metadata discovered
- [x] Guest operating systems, services, listeners, and Docker topology discovered
- [x] Docker application Git source and production drift audited
- [ ] Known Mosquitto credential issue remediated (explicitly deferred)
- [ ] Known NetAlertX credential issue remediated (explicitly deferred)
- [ ] Production Compose drift reviewed and reconciled
- [x] Production Compose drift captured on an unpushed local review branch
- [ ] Ignored application state assigned to a tested backup mechanism
- [x] Initial persistent-data ownership and recovery-source map created
- [x] Sanitized host/PBS configuration export created
- [x] Manual read-only plan implemented and validated with zero drift
- [x] Apply workflow implemented with a tested safety lock
- [x] Read-only service recovery validation implemented; 15/15 checks passing
- [ ] Every guest classified: rebuildable, stateful, or infrastructure-critical
- [ ] Bind mounts, passthrough devices, and external storage documented
- [ ] PVE host-level configuration backup defined
- [ ] PBS bootstrap procedure documented
- [x] PBS-from-S3 bootstrap procedure documented
- [ ] S3-backed datastore recovery tested
- [ ] One LXC restored to isolated networking and a non-production VMID
- [ ] The sole VM restored to isolated networking and a non-production VMID
- [ ] Application-level integrity checks documented
- [ ] Recovery time and recovery point objectives recorded

## Non-goals for the discovery phase

- Creating, modifying, stopping, or deleting guests
- Importing guests into OpenTofu state
- Changing PVE/PBS storage or backup jobs
- Reading or exporting application secrets
- Testing restores against production VMIDs or networks
