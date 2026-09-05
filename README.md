# PVE State

`pves` is a safety-focused, standalone Rust CLI for capturing, planning, and
applying explicitly owned Proxmox VE configuration. Environment configuration
lives in a separate repository; the binary contains no site-specific state.

## Install

Linux x86_64 and macOS:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/dougmaitelli/pvestate/main/install.sh | sh
```

The installer downloads the latest GitHub Release, verifies its SHA-256 file,
and installs `pves` under `${HOME}/.local/bin` by default. Override the
destination with `PVESTATE_INSTALL_DIR`.

Rust users may also build from source:

```bash
cargo install --git https://github.com/dougmaitelli/pvestate
```

## Configuration repository

Create or select a separate environment repository:

CLI output uses `live` for the running PVE/PBS system, `captured` for the latest
snapshot under `observed/production`, and `local` for editable files under
`config/`.

```bash
pves init ./my-proxmox
export PVES_CONFIG_DIR="$PWD/my-proxmox"
```

The current Hades environment is stored separately at `/root/pveconf`.

```text
my-proxmox/
├── config/                 local editable YAML
├── observed/production/    sanitized, reviewable API snapshots and native exports
├── .runtime/               ignored raw observations and plans
├── .pves.env               ignored PVE State credentials and connection settings
└── .secrets/               ignored SSH material
```

## Workflow

```bash
pves capture
pves plan

# Edit config/*.yml and review the plan again.
pves plan

# Commit changes with normal Git commands in the configuration repository.
git diff
git add config observed
git commit -m "Describe the infrastructure change"

pves apply
pves validate
```

Every command also accepts `--config-dir PATH`.

High-level progress is shown by default. Use `-v` to show each operation and
`-vv` for additional diagnostic detail. Progress is written to stderr so JSON
output on stdout remains safe to pipe or redirect. For example, `pves capture`
shows its major stages, while `pves capture -v` also shows every API request,
native configuration read, and host probe. Interactive terminals show colored
spinners, live operation counts, elapsed times, and completion markers. CI,
redirected output, and `TERM=dumb` automatically use stable plain-text output;
terminal coloring also respects `NO_COLOR`.

Native `/etc/pve/firewall/*.fw` files are the authoritative firewall
representation for comparison, adoption, and apply. The PVE firewall API is
captured as supporting evidence, but is not mixed into mutation planning.
Options, aliases, IP sets, security groups, and ordered rules are modeled from
the native files. A plan is blocked if a captured file contains syntax that the
local model cannot reproduce without loss.

PVE State uses one canonical host for both API and SSH access:

```bash
PVE_HOST=pve.example.test
PVE_API_PORT=8006       # optional; default 8006
PVE_API_SCHEME=https    # optional; default https
PVE_SSH_PORT=22         # optional; default 22
PVE_SSH_USER=root       # optional; default root
```

Discovery and mutation use separate credentials, but both address `PVE_HOST`.

Environment values are loaded once into typed settings before a command runs.
Ports are parsed as integers, endpoints as URLs, TLS flags as booleans, domains
as a set, and file locations as paths. Credential pairs are validated together;
read-only roles do not require mutation credentials.

PBS API discovery runs as part of every capture and requires:

```bash
PBS_ENDPOINT=https://pbs.example.test:8007
PBS_API_TOKEN_ID=iac-auditor@pbs!discovery
PBS_API_TOKEN_SECRET=secret
PBS_VERIFY_TLS=true
# PBS_CA_FILE=/absolute/path/to/private-ca.pem
```

PBS mutations use a separate narrowly scoped identity through
`PBS_APPLY_API_TOKEN_ID` and `PBS_APPLY_API_TOKEN_SECRET`. Creating a missing S3
endpoint additionally resolves `PBS_APPLY_S3_ACCESS_KEY` and
`PBS_APPLY_S3_SECRET_KEY` at apply time; those values are never written to YAML
or plan files.

`pves capture` dynamically enumerates every PVE node, VM, and LXC. The captured
PVE snapshot includes each guest's full API configuration and native config,
including allocated CPU and memory, disks, NICs, mount points, passthrough
devices, snapshots, and guest firewall options/rules. It also records host and
cluster firewall configuration, DNS, storage, pools, HA state, and PVE backup
jobs (including guest selection).

The PBS snapshot includes datastore configuration and usage, S3 endpoint and
bucket settings, remotes, sync jobs, prune jobs, verification jobs, backup
groups, and snapshots. Sanitized stable snapshots are written to
`observed/production/api/`; detailed timestamped snapshots are written beneath
ignored `.runtime/`.

Every capture writes a typed evidence manifest. A capture is `complete` only
when all required PVE API, PBS API, native SSH, and host SSH requests succeed.
The manifest records the exact sanitized artifact set, SHA-256 hashes, sizes,
source endpoints, and failures. Partial captures remain available for diagnosis,
but `plan` rejects partial, stale, missing, unexpected, or modified evidence.
Capture artifacts are assembled in a staging directory and promoted only after
every required source succeeds; a failed capture therefore leaves the last
complete `observed/production` snapshot intact.

`pves plan` reads only that verified captured snapshot; it does not contact PVE
or PBS. The capture ID is embedded in the signed plan, and `pves adopt` rejects
the plan if a newer or different capture is currently selected.

### Configuration scope

Loading and validating a configuration file does not by itself mean every field
is changed by `apply`. PVE State classifies configuration by workflow and by
its highest implemented management level. The levels are:

- `archived`: retained as evidence or inventory without reconciliation;
- `declared`: represented in typed local configuration;
- `planned`: compared and emitted as drift;
- `adoptable`: captured drift can be written into local configuration;
- `applicable`: guarded execution exists for the entry's named workflow.

The workflow classes remain:

- `production-managed`: compared with production and emitted into a guarded plan;
- `recovery-only`: consumed only by replacement-host recovery or validation;
- `declared-only`: represented in desired state but not yet reconciled;
- `metadata`: descriptive repository or environment information.

The field-level source of truth is `scope::manifest()` in the tool. `pves schema`
publishes it as `management-scope.json`; the checked-in copy lives alongside the
JSON Schemas. PVE/PBS backup jobs, node firewalls, embedded guest firewall policies,
LXC bind mounts, and guest device removals are production-managed and applicable.
Remaining gaps, such as privileged/unprivileged LXC conversion, guest resource
creation/deletion, storage topology, and host services, are explicitly marked
`declared-only` and never advertised as applicable.

Guest ownership is intentionally conservative: only IDs declared in `guests.yml`
are reconciled. A declared guest missing from the capture blocks the plan because
creation is unsupported. Extra live guests remain archived evidence, are outside
ownership, and are never implicitly deleted.

VM hardware is keyed by its Proxmox slot (`scsi0`, `efidisk0`, `net0`, `usb0`),
so non-contiguous and mixed-bus devices retain their identity. The former singular
`disk`/`efi` fields and list-based network/USB fields remain accepted when loading
older repositories and are normalized to the slot-keyed format when rewritten.
Normal disks absent from the local slot map are removed through the guarded guest
plan; unmodeled CD-ROM and cloud-init drives are preserved.

### Safety model

`apply` requires a plan less than 30 minutes old, an exact plan SHA, an exact
target, explicitly approved domains, and separate mutation credentials. Guest
deletion/replacement, disk shrinking, and implicit storage moves are not modeled.
Proxmox config digests and remote file hashes reject concurrent changes.
Network files are not activated unless `PVES_APPLY_NETWORK_NOW=YES`.
Plans and capture evidence more than two minutes in the future are rejected to
allow minor clock skew without permitting future-dated freshness bypasses.

Every authorized apply attempt creates `.runtime/apply-<id>.json` and updates
`.runtime/apply-latest.json` before initializing mutation clients. Operations
are durably journaled as `pending`, `running`, `applied`, or `failed` after each
transition. A partial failure preserves earlier successes, the current error,
later pending work, timestamps, targets, and the confirmed plan digest. Journal
files are published with write, flush, filesystem sync, and rename; apply errors
always report the attempt-specific journal path.

The runtime directory is created with owner-only `0700` permissions and runtime
artifacts with `0600` permissions on Unix. Commands reject a symlinked runtime
directory, artifacts that are not regular files, ownership mismatches, and any
group- or world-accessible runtime permissions.

Recovery plans pin both `restore.target.expected_hostname` and
`restore.target.expected_host_key_sha256`. The fingerprint may be omitted from
configuration used for normal capture and planning, but its absence blocks every
recovery mutation. Before any recovery mutation, `pves`
verifies the selected key from the recovery `known_hosts` file and checks the
hostname reported by the authenticated machine. A replacement may intentionally
reuse production DNS or IP addresses; safety depends on the signed replacement
host-key pin rather than an address-based override.

```bash
export PVES_ENABLE_PRODUCTION_APPLY=YES
export PVES_CONFIRM_PLAN_SHA='<sha from .runtime/production-plan.json>'
export PVES_APPLY_TARGET='https://pve.example:8006'
export PVES_APPLY_DOMAINS='guests,firewall,dns,backup,pbs'
pves apply
```

## Commands

- `init PATH`: scaffold an environment repository.
- `capture`: refresh the captured snapshot of live API and native configuration.
- `plan`: compare local configuration with live state, create a deterministic
  guarded plan, and show its changes grouped by domain. Use `--json` for the
  complete machine-readable plan envelope.
- `adopt`: preview captured values or copy explicitly selected values into the
  local configuration.
- `apply`: execute exactly the confirmed local-to-live plan; removals are
  generated only from explicit local absence or `absent_*` identifiers.
- `validate`: verify all managed guests are running.
- `recover ACTION TARGET`: grouped disaster-recovery interface.
- `schema`: emit JSON Schemas for the repository manifest and every configuration document.

Preview values that can be adopted from the latest verified capture and plan:

```bash
pves --config-dir ./environment adopt --preview
pves --config-dir ./environment adopt lxc/106:mp0.backed_up_by_pve
pves --config-dir ./environment adopt --all
```

Candidate IDs are positional arguments. `--preview`, positional IDs, and `--all`
are mutually exclusive modes validated by the CLI parser. Bulk adoption selects
only candidates marked adoptable; unsupported or ambiguous fields are reported
but skipped.

## Development

The local quality gate follows the same format/lint/test pattern as `/root/ada`:

```bash
make check
make build
```

CI runs `rustfmt`, Clippy with warnings denied, tests, and release builds on
Linux, macOS, and Windows. Pushing a `v*` tag creates checksummed GitHub Release
archives consumed by `install.sh`.

Command logic depends on `PveClient`, `PbsClient`, and `RemoteHost` capabilities,
not concrete HTTP or SSH implementations. `main` is the composition root that
constructs real clients from typed settings. Tests inject in-memory clients to
exercise request generation, discovery, and partial failures without network or
production access.

Safety-critical plumbing is shared across commands: plan envelopes use one
signing and integrity-verification implementation, apply and recovery use one
authorization policy engine, remote commands use one shell-quoting and file
verification module, and plans and execution journals use durable atomic writes.

## Security

Never commit `.pves.env`, `.secrets`, raw runtime observations, API tokens, private
keys, application secrets, or PBS S3 credentials. Use a read-only discovery
identity and a distinct, narrowly scoped mutation identity.

## License

MIT
