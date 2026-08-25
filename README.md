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
export IAC_CONFIG_DIR="$PWD/my-proxmox"
```

The current Hades environment is stored separately at `/root/pveconf`.

```text
my-proxmox/
├── config/                 desired YAML
├── observed/production/    sanitized, reviewable native exports
├── .runtime/               ignored raw observations and plans
├── .env                    ignored API credentials
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

PVE State uses one canonical host for both API and SSH access:

```bash
PVE_HOST=pve.example.test
PVE_API_PORT=8006       # optional; default 8006
PVE_API_SCHEME=https    # optional; default https
PVE_SSH_PORT=22         # optional; default 22
PVE_SSH_USER=root       # optional; default root
```

Discovery and mutation use separate credentials, but both address `PVE_HOST`.

PBS API discovery runs as part of every capture and requires:

```bash
PBS_ENDPOINT=https://pbs.example.test:8007
PBS_API_TOKEN_ID=iac-auditor@pbs!discovery
PBS_API_TOKEN_SECRET=secret
PBS_VERIFY_TLS=true
# PBS_CA_FILE=/absolute/path/to/private-ca.pem
```

### Safety model

`apply` requires a plan less than 30 minutes old, an exact plan SHA, an exact
target, explicitly approved domains, and separate mutation credentials. Guest
deletion/replacement, disk shrinking, and implicit storage moves are not modeled.
Proxmox config digests and remote file hashes reject concurrent changes.
Network files are not activated unless `IAC_APPLY_NETWORK_NOW=YES`.

```bash
export IAC_ENABLE_PRODUCTION_APPLY=YES
export IAC_CONFIRM_PLAN_SHA='<sha from .runtime/production-plan.json>'
export IAC_APPLY_TARGET='https://pve.example:8006'
export IAC_APPLY_DOMAINS='guests,firewall,dns'
pves apply
```

## Commands

- `init PATH`: scaffold an environment repository.
- `capture`: refresh API observations and sanitized host/firewall exports.
- `plan`: validate desired state and create a deterministic guarded plan.
- `apply`: execute exactly the confirmed non-destructive plan.
- `validate`: verify all managed guests are running.
- `recover ACTION TARGET`: grouped disaster-recovery interface.
- `schema`: emit JSON Schemas for the repository manifest and every configuration document.

## Development

The local quality gate follows the same format/lint/test pattern as `/root/ada`:

```bash
make check
make build
```

CI runs `rustfmt`, Clippy with warnings denied, tests, and release builds on
Linux, macOS, and Windows. Pushing a `v*` tag creates checksummed GitHub Release
archives consumed by `install.sh`.

## Security

Never commit `.env`, `.secrets`, raw runtime observations, API tokens, private
keys, application secrets, or PBS S3 credentials. Use a read-only discovery
identity and a distinct, narrowly scoped mutation identity.

## License

MIT
