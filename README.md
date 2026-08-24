# pve-iac

`pve-iac` is a safety-focused, standalone Rust CLI for capturing, planning, and
applying explicitly owned Proxmox VE configuration. Environment configuration
lives in a separate repository; the binary contains no site-specific state.

## Install

Linux x86_64 and macOS:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/dougmaitelli/pve-iac/main/install.sh | sh
```

The installer downloads the latest GitHub Release, verifies its SHA-256 file,
and installs `pve-iac` under `${HOME}/.local/bin` by default. Override the
destination with `PVE_IAC_INSTALL_DIR`.

Rust users may also build from source:

```bash
cargo install --git https://github.com/dougmaitelli/pve-iac
```

## Configuration repository

Create or select a separate environment repository:

```bash
pve-iac init ./my-proxmox
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
pve-iac capture
pve-iac plan

# Edit config/*.yml and review the plan again.
pve-iac plan

# Commit changes with normal Git commands in the configuration repository.
git diff
git add config observed
git commit -m "Describe the infrastructure change"

pve-iac apply
pve-iac validate
```

Every command also accepts `--config-dir PATH`.

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
pve-iac apply
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
