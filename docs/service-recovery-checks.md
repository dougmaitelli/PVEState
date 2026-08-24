# Service recovery checks

Run the read-only validation suite after normal maintenance, a guest restore, or
a full disaster recovery:

```bash
./bin/iac validate
./bin/iac audit
```

`validate` executes the allowlisted commands in `config/recovery-checks.yml` over
the verified PVE SSH connection. It does not restart services, write files, alter
containers, or change PVE/PBS configuration.

## Current coverage

- all seven LXCs and Home Assistant VM 107 are running
- Omada and AdGuard Home system services
- DNS listener on port 53
- Docker daemon, expected container count, and health state
- Traefik HTTP/HTTPS listeners
- Frigate and Docker proxy state
- NVR recording disk mounted from the expected host device
- Pelican panel dependencies
- PBS daemon/proxy and expected S3 datastore backend
- PVE visibility of PBS storage
- Pelican Wings and Docker

The initial production run on 2026-08-23 passed all 15 checks.

## Limitations

These checks establish basic operational readiness, not application data
correctness. They do not currently prove:

- successful login or a representative transaction in web applications
- Home Assistant integrations or USB device functionality
- Frigate camera streams, detection, or recording playback
- correctness of Omada, AdGuard, Pelican, or Docker application databases
- restoreability of `/mnt/pve/security`
- application-consistent database exports

Those require isolated restore drills and application-specific acceptance tests.
PBS recovery-point recency and verification are assessed separately by
`./bin/iac audit`.
