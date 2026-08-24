# h4des Proxmox infrastructure and recovery

This repository is the source of truth for rebuilding the existing production
Proxmox environment at `https://pve.h4des.dev:8006`.

The project starts in **discovery-only mode**. The first milestone is to record
the current PVE topology without changing it. Provisioning and restoration code
will be added only after the discovered inventory has been reviewed.

## Safety model

- No CI/CD system is authorized to apply infrastructure changes.
- Discovery uses only HTTP `GET` requests.
- Apply and restore operations will require explicit local commands and confirmation.
- API secrets remain outside Git.
- Existing production guests are never implicitly adopted, replaced, or destroyed.
- OpenTofu state is not created until an import strategy is reviewed.

## Current layout

```text
.
├── ansible/                 future desired configuration and playbooks
├── artifacts/discovery/     ignored API snapshots
├── bin/iac                  operator entry point
├── config/site.yml          non-secret site facts
├── config/backup.yml        discovered non-secret backup topology
├── config/host.yml          discovered non-secret host/storage topology
├── config/guests.yml        discovered guest adoption baseline
├── config/network.yml       replacement-host bridge configuration
├── config/firewall.yml      declarative PVE cluster firewall policy
├── config/storage.yml       required pools, mounts, and PVE storage definitions
├── config/restore.yml       guarded recovery inputs and archive selections
├── config/services.yml      discovered service/persistence inventory
├── docs/current-state.md     reviewed production baseline
├── docs/restore-plan.md     staged disaster-recovery plan
└── scripts/pve_discover.py  read-only PVE API collector
```

## First use

Set up the repository-local tooling once:

```bash
./bin/iac setup
```

You do not need to activate or deactivate a Python virtual environment. The
`./bin/iac` wrapper invokes the repository's own Python and Ansible binaries
directly, leaving your shell environment unchanged.

Create a least-privilege, privilege-separated PVE API token with audit-only
permissions. A token's permissions are constrained by both the backing user and
the token ACL, so grant the token only `PVEAuditor` access needed for discovery.

```bash
cp .env.example .env
chmod 600 .env
# edit .env locally

./bin/iac capture
./bin/iac plan
./bin/iac validate
```

For a disaster rebuild, fill the deliberately unresolved values in
`config/restore.yml`, generate `./bin/iac recover plan RECOVERY_HOST`, and follow
the hash-confirmed staged procedure in
[docs/operator-workflow.md](docs/operator-workflow.md). Rebuild commands are
fail-closed and cannot overwrite existing guest IDs.

For routine configuration management:

```bash
./bin/iac capture
./bin/iac plan
# edit config/*.yml, repeat plan, then review
./bin/iac save "reviewed desired-state change"
# set the separate apply identity and exact plan confirmations
./bin/iac apply
```

Observed state and desired state are intentionally separate: `capture` refreshes
ignored evidence and sanitized `exports/production/`, while human edits live in
`config/`. See [docs/operator-workflow.md](docs/operator-workflow.md) for the
apply gates and managed-field boundaries.

`discover` writes a timestamped JSON snapshot beneath `artifacts/discovery/` and
updates `latest.json`. Those files are ignored because they describe the private
production topology. Discovery includes node, storage, backup-job, guest
configuration, and guest snapshot metadata. It does not enter guests or read
their filesystems.

The wrapper loads `.env` without printing it. The Python client sends the token
through an HTTP authorization header and never writes it to disk or command-line
arguments.

## What comes next

1. Run and review read-only discovery.
2. Classify every guest as rebuildable, stateful, or infrastructure-critical.
3. Document PBS placement, datastore name, encryption material, S3 endpoint, and
   local-cache recovery requirements without committing secrets.
4. Export current guest and host configuration into reviewed Ansible data.
5. Add idempotent configuration roles one service at a time.
6. Test restores into isolated IDs/storage/networking before declaring the plan ready.

See [docs/restore-plan.md](docs/restore-plan.md) for the recovery model.
The first discovery findings are recorded in
[docs/current-state.md](docs/current-state.md).
The application repository findings are recorded in
[docs/application-source-audit.md](docs/application-source-audit.md).
Persistent state and its current recovery sources are mapped in
[docs/persistence-map.md](docs/persistence-map.md).
Manual plan/apply operations are documented in
[docs/operator-workflow.md](docs/operator-workflow.md).
The independent PBS recovery sequence is documented in
[docs/pbs-bootstrap.md](docs/pbs-bootstrap.md).
Service checks and their limitations are documented in
[docs/service-recovery-checks.md](docs/service-recovery-checks.md).
