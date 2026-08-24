# Discovered production baseline

Captured read-only from PVE on 2026-08-23. The machine-readable snapshot is in
the ignored `artifacts/discovery/latest.json` file.

## Platform

- PVE: 9.2.11
- Node: `pve`
- Guests: 7 LXCs and 1 QEMU VM
- Visible storage definitions: 5
- Guest snapshots: none
- Guest protection flag: disabled on every guest
- Cluster mode: standalone node; no Corosync configuration
- Failed systemd units at discovery: none

The API currently reports 7 LXCs, rather than the earlier estimate of more than
10. This baseline follows the API and should be checked for intentionally removed,
hidden, or otherwise inaccessible containers.

## Guests

| VMID | Type | Name | CPU | Memory | Root disk | Start order | Classification |
|---:|---|---|---:|---:|---:|---:|---|
| 102 | LXC | omada | 1 | 8 GiB | 8 GiB | 1 | network/stateful |
| 103 | LXC | dns | 1 | 512 MiB | 8 GiB | 1 | network/critical |
| 105 | LXC | docker | 4 | 12 GiB | 32 GiB | 2 | multi-service/stateful |
| 107 | VM | hass | 4 | 24 GiB | 64 GiB | 3 | home automation/stateful |
| 106 | LXC | nvr | 4 | 8 GiB | 16 GiB | 4 | surveillance/stateful |
| 108 | LXC | pelican | 1 | 1 GiB | 8 GiB | 5 | game management/stateful |
| 111 | LXC | proxmox-backup-server | 2 | 2 GiB | 10 GiB | 6 | backup/critical bootstrap |
| 121 | LXC | p-wing-1 | 8 | 12 GiB | 32 GiB | 10 | game workload/stateful |

The classifications are provisional and must be confirmed against the data and
services actually running inside each guest.

## Service inventory

- 102 `omada`: Omada controller services
- 103 `dns`: AdGuard Home
- 105 `docker`: 27 active containers across 9 Compose projects, with 19 named
  and 4 anonymous Docker volumes
- 106 `nvr`: Frigate plus a Docker socket proxy
- 107 `hass`: Home Assistant OS; QEMU guest agent is not configured
- 108 `pelican`: Nginx, PHP-FPM, Redis, and Pelican
- 111 `proxmox-backup-server`: PBS daemon and API proxy
- 121 `p-wing-1`: Pelican Wings and Docker

Application configuration values and environment files were deliberately not
read during discovery. The Docker socket proxy in LXC 106 publishes TCP port
2375 on all IPv4 and IPv6 interfaces. Its authorization/environment policy must
be reviewed before deciding whether this is an acceptable exposure.

The Docker application's Git repository is private and the deployed checkout is
at the same commit as GitHub, but it is not currently a clean or secret-free
source of truth. Four Compose files have uncommitted production modifications,
an Apprise configuration directory is untracked, and several tracked files hold
literal credentials. See `docs/application-source-audit.md`.

## Storage and backups

Visible PVE storage definitions:

- `local` (`dir`)
- `local-lvm` (`lvmthin`)
- `Data` (`zfspool`)
- `VMs` (`zfspool`)
- `pbs` (`pbs`), server `pbs.h4des.dev`, datastore `pve-backup`

One enabled backup job runs Sundays at 01:00 in snapshot mode. It includes all
eight discovered guests and retains the latest three backups.

PBS 4.2 exposes `pve-backup` as an S3-backed datastore:

- S3 provider: Backblaze B2
- Region: `us-west-002`
- Bucket: `hades-bkp`
- PBS local cache: `/mnt/backup/`
- Garbage collection: daily
- Pruning: daily
- Verification: Mondays at 23:00
- Backup groups: all 8 discovered guests
- Recovery points at discovery: 24 total, three for each guest

The datastore currently contains backups from August 9, 16, and 23, 2026. Of
the 24 snapshots, 15 report successful verification, 8 are not yet verified,
and one failed verification. The failed item is the August 9 backup of VM 107
(`hass`). Its August 16 backup verified successfully; its August 23 backup was
not yet verified when discovery ran.

There are no PBS sync jobs or configured PBS remotes. Backblaze S3 is therefore
the datastore backend itself, not a secondary sync target.

## Physical storage topology

| Purpose | Layout | Redundancy | Recovery implication |
|---|---|---|---|
| PVE system, `local`, `local-lvm` | NVMe/LVM-thin | none | Host reinstall is required after disk loss |
| `VMs` guest storage | single-device ZFS pool | none | Guest disks depend on PBS backups after device loss |
| `Data` | two-device ZFS mirror | mirror | Tolerates one member failure |
| `/mnt/pve/backup` | 1 TB ext4 HDD | none | PBS S3 cache and shared bind-mount data |
| `/mnt/pve/security` | 2 TB ext4 HDD | none | NVR data is outside `vzdump` backups |

Both ZFS pools were online with no known data errors at discovery. Their latest
reported scrub completed August 9, 2026 without errors. PVE reports that some
supported pool feature flags are not enabled; this is recorded only and should
not be changed as part of recovery work without compatibility review.

PBS package metadata reports 4.2.5-1 installed while the daemon identified its
running version as 4.2.4. This commonly means the service has not restarted since
an update, but should be verified during a maintenance window rather than changed
by discovery automation.

## Bind-mount recovery gaps

Three LXCs have host bind mounts:

| VMID | Guest | Host source | Guest target | PVE option |
|---:|---|---|---|---|
| 105 | docker | `/mnt/pve/backup` | `/mnt/backup` | default |
| 106 | nvr | `/mnt/pve/security` | `/mnt/security` | `backup=1` |
| 111 | proxmox-backup-server | `/mnt/pve/backup` | `/mnt/backup` | default |

PVE does not back up the contents of bind mounts with `vzdump`; the `backup`
option applies only to storage-backed volume mount points. The NVR data therefore
needs a separately verified backup even though its configuration says `backup=1`.

## PVE firewall coverage

The 2026-08-24 read-only enumeration captured every PVE firewall file at the
cluster, node, and guest scopes:

- cluster policy: enabled, with TCP 45876 (Beszel) and 61208 (Glances);
- node `pve`: no `host.fw` file;
- guest policies: VMIDs 102, 103, 105, 106, 107, and 108;
- no guest policy file: VMIDs 111 and 121;
- disabled rules are preserved for Copyparty on 105 and Govee on 107.

The structured source of truth is `config/firewall.yml`; exact sanitized native
files are under `exports/production/pve/firewall/`. This covers PVE firewall
configuration only. Guest OS firewalls, Docker port publication, router/NAT,
and physical switch ACLs require separate collection methods.

## Highest-priority recovery questions

1. What data is stored in `/mnt/pve/backup` and `/mnt/pve/security`, and how is
   each underlying filesystem or remote mount protected?
2. Where are the PBS S3 settings, datastore definition, TLS material, and any
   encryption keys backed up outside this PVE host?
3. Can PBS LXC 111 be bootstrapped without first restoring it from the PBS service
   that it provides?
4. Are three weekly recovery points sufficient for every guest and application?
5. Why did VM 107's August 9 backup fail verification, and has a test restore of
   a newer verified recovery point succeeded?
6. Is the single-device `VMs` pool an accepted availability risk, given that all
   production guest root disks depend on it?
