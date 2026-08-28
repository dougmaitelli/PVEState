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

```bash
pves init ./my-proxmox
export PVES_CONFIG_DIR="$PWD/my-proxmox"
```

The current Hades environment is stored separately at `/root/pveconf`.

```text
my-proxmox/
├── config/                 desired YAML
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
native configuration read, and host probe.

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

### Configuration scope

Loading and validating a configuration file does not by itself mean every field
is changed by `apply`. PVE State classifies configuration as:

- `production-managed`: compared with production and emitted into a guarded plan;
- `recovery-only`: consumed only by replacement-host recovery or validation;
- `declared-only`: represented in desired state but not yet reconciled;
- `metadata`: descriptive repository or environment information.

The field-level source of truth is `scope::entries()` in the tool. `pves schema`
publishes it as `management-scope.json`; the checked-in copy lives alongside the
JSON Schemas. PVE/PBS backup jobs, node firewalls, embedded guest firewall policies,
LXC bind mounts, and guest device removals are production-managed. Remaining
gaps, such as privileged/unprivileged LXC conversion, are explicitly marked
`declared-only`.

### Safety model

`apply` requires a plan less than 30 minutes old, an exact plan SHA, an exact
target, explicitly approved domains, and separate mutation credentials. Guest
deletion/replacement, disk shrinking, and implicit storage moves are not modeled.
Proxmox config digests and remote file hashes reject concurrent changes.
Network files are not activated unless `PVES_APPLY_NETWORK_NOW=YES`.

Every authorized apply attempt creates `.runtime/apply-<id>.json` and updates
`.runtime/apply-latest.json` before initializing mutation clients. Operations
are durably journaled as `pending`, `running`, `applied`, or `failed` after each
transition. A partial failure preserves earlier successes, the current error,
later pending work, timestamps, targets, and the confirmed plan digest. Journal
files are published with write, flush, filesystem sync, and rename; apply errors
always report the attempt-specific journal path.

Recovery plans pin both `restore.target.expected_hostname` and
`restore.target.expected_host_key_sha256`. Before any recovery mutation, `pves`
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
- `capture`: refresh API observations and sanitized host/firewall exports.
- `plan`: validate desired state and create a deterministic guarded plan.
- `adopt`: preview production values or copy explicitly selected values into
  desired state.
- `apply`: execute exactly the confirmed plan; removals are generated only from
  explicit desired-state absence or `absent_*` identifiers.
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
