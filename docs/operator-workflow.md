# Operator workflow

All production operations are manual. No CI/CD identity has PVE or PBS mutation
credentials.

## Local setup

Run this once after cloning, and again whenever `requirements.txt` or
`ansible/requirements.yml` changes:

```bash
./bin/iac setup
```

There is no activation step. Every command uses the project-local environment
through `./bin/iac`, and there is nothing to deactivate afterward.

## Refresh evidence

```bash
./bin/iac discover
./bin/iac pbs-discover
./bin/iac host-discover
./bin/iac guest-discover
./bin/iac export-config
./bin/iac audit
./bin/iac validate
```

For the routine desired-state workflow, the single command is:

```bash
./bin/iac capture
```

It refreshes timestamped ignored observations and the sanitized native exports.
It never modifies files under `config/`, so production observations cannot
overwrite an intentional desired-state edit. A plan refuses firewall/network
comparison when the sanitized export is more than 30 minutes old.

The raw snapshots and drift patches are intentionally ignored. Sanitized host
configuration under `exports/production/` and reviewed YAML under `config/` are
the durable recovery inputs.

## Plan

```bash
./bin/iac plan
```

The plan sends only PVE API `GET` requests and compares desired YAML plus rendered
native configuration with the fresh production capture:

```bash
./bin/iac check
./bin/iac plan
```

It writes `artifacts/production-plan.json`, including exact operations, current
values, blockers, and a SHA-256 confirmation. Managed domains are guest CPU,
memory, names, startup, NICs, IPs, bridges, MACs, disks (growth only), QEMU USB,
PVE node DNS, host networking, and cluster/guest PVE firewalls.

The initial adoption plan on 2026-08-23 reported:

```text
managed guests: 8
drift: 0
```

Fields not yet represented in `config/guests.yml` are deliberately unmanaged and
will not appear in the plan.

## Save desired configuration

```bash
./bin/iac save "describe the reviewed configuration change"
```

This initializes a local Git repository when necessary and commits only the
allowlisted IaC source, documentation, and sanitized exports. `.env`, `.secrets`,
raw observations, apply reports, the virtualenv, and application checkout remain
ignored. Nothing is pushed automatically.

## Production apply

```bash
./bin/iac apply
```

Create a separate mutation API token and SSH key. Never reuse the discovery
identity. After reviewing a plan less than 30 minutes old:

```bash
export IAC_ENABLE_PRODUCTION_APPLY=YES
export IAC_CONFIRM_PLAN_SHA='<exact SHA from production-plan.json>'
export IAC_APPLY_TARGET='https://pve.h4des.dev:8006'
export IAC_APPLY_DOMAINS='guests,firewall,dns'
./bin/iac apply
```

Add `network` to the approved domains only with physical/remote console access.
Writing the network file does not activate it unless
`IAC_APPLY_NETWORK_NOW=YES`. Before replacing native files, apply creates a
timestamped remote copy beneath `/root/iac-preapply/`; API guest before-state and
per-operation results are retained in ignored plan/report artifacts.

Apply never creates, deletes, or replaces guests. Disk shrinking and implicit
storage moves are blockers. Disk growth is allowed. Any domain present in the
plan but absent from `IAC_APPLY_DOMAINS` stops the entire apply before mutation.

## Docker source reconciliation

The live image-version changes were captured without secrets and committed in the
ignored local clone of `hades-server`:

```text
branch: iac/reconcile-production-images
commit: d215c45
```

Nothing was pushed to GitHub. Review and push that branch from the application
repository workflow when desired.

## Disaster rebuild workflow

The rebuild commands target a freshly installed replacement host over SSH. They
do not use the production inventory or the discovery API token.

First complete the recovery-specific values in `config/restore.yml`: select a
Debian LXC template for PBS, select one reviewed PBS archive for each guest, and
add a reviewed application configuration playbook. Disk device identities and
off-host secrets are never inferred.

Create a plan for the replacement host:

```bash
./bin/iac rebuild-plan RECOVERY_HOST
```

Review `artifacts/rebuild-plan.json`. It lists all blockers and emits a SHA-256
confirmation value. Each mutating stage requires that exact, fresh plan:

```bash
export IAC_ENABLE_MUTATION=YES
export IAC_CONFIRM_PLAN_SHA='<sha256 from the plan>'
export IAC_TARGET_SSH_KEY='/path/to/recovery-key'

./bin/iac bootstrap-pve RECOVERY_HOST
./bin/iac bootstrap-pbs RECOVERY_HOST
./bin/iac restore-guests RECOVERY_HOST
./bin/iac configure RECOVERY_HOST
./bin/iac validate
```

`bootstrap-pve` stages and syntax-checks networking by default. Set
`IAC_COMMIT_NETWORK=true` only when the replacement host console is available;
applying an incorrect management bridge can end the SSH session.

The commands refuse to target `pve.h4des.dev` unless
`IAC_ALLOW_PRODUCTION_TARGET=YES` is also set. Guest restoration refuses any
VMID that already exists and never uses a force/overwrite option.

The convenience command `./bin/iac rebuild RECOVERY_HOST` runs the Ansible
stages in order, but only after every plan blocker is resolved. Running stages
individually is preferred during a real disaster because PBS S3 attachment and
secret recovery require deliberate operator checkpoints.

### What remains external by design

- installation of the base PVE operating system;
- identification/creation of replacement ZFS pools and data filesystems;
- restoration of `/mnt/pve/backup` and `/mnt/pve/security`;
- retrieval of S3, PBS, application `.env`, encryption, and recovery secrets;
- the one-owner S3 datastore takeover described in `pbs-bootstrap.md`;
- DNS changes and physical switch/VLAN configuration.

These inputs cannot safely be derived from a failed host or committed to Git.
