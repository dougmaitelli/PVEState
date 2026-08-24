# Application source-of-truth audit

Audited 2026-08-23 against private repository
`dougmaitelli/hades-server`, branch `master`, commit
`79a15780840108dabea4649df5b48115f52141a2`.

No credential values were copied into this repository or printed in discovery
artifacts.

## What is working

- The GitHub repository is private.
- Docker LXC 105 is checked out at the same commit as GitHub.
- The root `.gitignore` correctly ignores `*.env` files.
- All deployed stack `.env` files reported by Git are ignored.
- Compose definitions, most static application configuration, and Docker/Podman
  migration definitions are version controlled.

## Tracked secret material

The Homepage Proxmox `token` field is an identifier rather than a credential.
Its `secret` field and the Homepage service `password` field use Homepage's
double-brace environment templating and are not literal secrets. They were
incorrectly classified by the initial scanner, which recognized `${VAR}` but not
Homepage's template syntax.

Tracked files with known, deferred credential concerns include:

- NetAlertX API/client credentials, an enable password, and encryption material
- plaintext Mosquitto credentials in `config/mosquitto/pass.txt`
- hashed Mosquitto credentials in `config/mosquitto/passwd`

The Mosquitto files are a known issue accepted for deferred remediation. Their
contents must not be copied into the IaC repository or printed in discovery
artifacts. This does not block documenting or restoring the rest of the system.

The NetAlertX values are likewise a known issue accepted for deferred
remediation. They must not be copied into the IaC repository or discovery
artifacts and do not block the remaining IaC work.

Additional tracked files contain secret-shaped fields that are blank,
environment references, metadata, or require application-specific review.

Even in a private repository, tracked secrets are copied into Git history,
developer clones, backups, and potentially CI logs. Removing them only from the
latest commit does not invalidate existing credentials or erase history.

## Production drift

The live checkout at `/srv` has modified tracked files:

- `stacks/home/docker-compose.yml`
- `stacks/monitoring/docker-compose.yml`
- `stacks/network/docker-compose.yml`
- `stacks/proxy/docker-compose.yml`

It also has an untracked `config/apprise/` directory. Therefore GitHub is not yet
a complete reproduction of the running Docker host.

Ignored runtime/persistent paths were also observed, including certificates,
application stores/logs, SQLite data, stack `.env` files, and the Trek stack.
These are expected to remain outside Git, but each needs an explicit restore
source.

## Required remediation before IaC adoption

1. Later, rotate the known NetAlertX and Mosquitto credentials; do not assume making the repository private
   or deleting the current value is sufficient.
2. Replace tracked literal secret values with environment or secret-file references.
3. Remove plaintext password files from tracking and add narrow ignore rules.
4. Decide whether to rewrite Git history after rotation and coordinate that with
   every clone of the repository.
5. Review and reconcile the four production-modified Compose files into Git.
6. Decide whether `config/apprise/` contains declarative configuration that should
   be sanitized and committed or mutable data that should be backed up.
7. Inventory every ignored persistent path and assign it to PBS, application-level
   export, or a separate encrypted backup.

Credential rotation and Git-history rewriting are intentionally not performed by
the read-only discovery workflow.
