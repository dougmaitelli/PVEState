# PBS bootstrap and S3 recovery

This procedure breaks the dependency loop created by running PBS as LXC 111 on
the same PVE host it protects. `./bin/iac recover bootstrap-pbs` automates creation of
the replacement container once a template volume is selected. Commands that
claim the existing S3 datastore remain an explicit operator checkpoint because
running two PBS instances against it can corrupt the recovery source.

## Known topology

```text
PVE node pve
└── LXC 111 proxmox-backup-server (Debian 13 / PBS 4)
    ├── rootfs: VMs ZFS pool, 10 GiB
    ├── /mnt/backup: PVE bind mount from /dev/sdd1, ext4
    └── datastore pve-backup
        └── S3 backend
            ├── endpoint id: backblaze
            ├── region: us-west-002
            ├── bucket: hades-bkp
            └── object prefix/datastore identity: pve-backup
```

The local `/mnt/backup` filesystem is an S3 cache. The Backblaze bucket is the
canonical datastore backend. PBS requires a persistent local cache even when the
datastore is S3-backed.

## Required off-host material

Before declaring recovery ready, store these outside PVE, PBS, the Backblaze
bucket, and this Git repository:

- Backblaze access key ID and secret key
- PBS administrative recovery credential
- PVE-to-PBS API token or instructions for recreating it
- PBS TLS private key if identity continuity is required, otherwise a plan to
  trust the replacement certificate/fingerprint
- all relevant encryption keys and recovery passwords
- a copy of this repository and `hades-server`

Identifiers are tracked in `config/required-secrets.yml`; values are never stored
here.

## Phase A — recover PVE prerequisites

1. Install PVE and restore networking from `exports/production/network/`.
2. Recreate/mount host storage using `config/host.yml`,
   `exports/production/storage/fstab`, and `exports/production/pve/storage.cfg`.
3. Confirm `/mnt/pve/backup` exists on a persistent ext4 filesystem. If the old
   cache disk is lost, provision a replacement cache with adequate capacity.
4. Confirm DNS for `pbs.h4des.dev` can point to the replacement PBS instance.

Do not blindly copy `/etc/pve` over a newly installed host. Review and restore the
individual exported definitions.

## Phase B — bootstrap PBS LXC 111 without PBS

1. Download a Debian 13 LXC template to PVE local template storage.
2. Recreate LXC 111 using `exports/production/pve/lxc/111.conf` and
   `config/guests.yml` as the reviewed hardware/network definition.
3. Mount the host cache path into the guest:

   ```text
   /mnt/pve/backup -> /mnt/backup
   ```

4. Start LXC 111 and install PBS 4 from the supported Debian 13/Trixie PBS
   repository:

   ```bash
   apt update
   apt install proxmox-backup-server
   ```

5. Verify the API is reachable at `https://pbs.h4des.dev:8007` before attaching
   the datastore.

Set the selected template volume ID in `config/restore.yml`; the guarded
`bootstrap-pbs` playbook then creates LXC 111 without overwriting an existing ID.

## Phase C — reconnect the Backblaze S3 datastore

1. Recreate S3 endpoint `backblaze` using the secret manager values and these
   non-secret settings:

   ```text
   endpoint: {{bucket}}.s3.{{region}}.backblazeb2.com
   region: us-west-002
   provider quirk: skip-if-none-match-header
   put rate limit: 10
   ```

2. Validate credentials before claiming the datastore:

   ```bash
   proxmox-backup-manager s3 check backblaze hades-bkp \
     --store-prefix pve-backup
   ```

3. Confirm the original PBS instance is permanently offline. Only then recreate
   the datastore with the exact original name:

   ```bash
   proxmox-backup-manager datastore create \
     pve-backup /mnt/backup \
     --backend type=s3,client=backblaze,bucket=hades-bkp \
     --reuse-datastore true \
     --overwrite-in-use true
   ```

`--overwrite-in-use` transfers ownership of the S3 datastore. Never run two PBS
instances against the same datastore and never run this command merely to test
the runbook.

4. Refresh the local cache/index from S3:

   ```bash
   proxmox-backup-manager datastore s3-refresh pve-backup
   ```

5. Validate the expected eight backup groups exist:

   ```text
   ct/102 ct/103 ct/105 ct/106 ct/108 ct/111 ct/121 vm/107
   ```

6. Restore pruning and verification job definitions from the sanitized exports,
   then recreate users, ACLs, and API tokens manually from the secret runbook.

## Phase D — reconnect PVE and restore guests

1. Add PBS storage `pbs` back to PVE using:

   ```text
   server: pbs.h4des.dev
   datastore: pve-backup
   content: backup
   ```

2. Update and verify the PBS TLS fingerprint or trusted CA.
3. Confirm PVE can list backups before attempting any restore.
4. Restore DNS LXC 103 first, then infrastructure/applications in documented
   startup order. Do not restore PBS LXC 111 over the newly bootstrapped PBS.
5. Treat `/mnt/pve/security` and other host bind mounts separately; they are not
   inside guest backups.

## Acceptance checks

- PBS API responds on port 8007
- `pve-backup` reports backend type `s3`
- all eight expected backup groups are visible
- at least one verified recovery point is visible for every guest
- PVE can browse the datastore through storage `pbs`
- an isolated restore of a small LXC succeeds before production VMIDs are used
- garbage collection, pruning, and verification schedules are restored

## Important architecture note

Proxmox recommends a separate physical PBS system for production so backups
remain reachable when the hypervisor fails. This runbook makes the current
single-host design recoverable, but does not remove that availability limitation.
