# Persistent application data map

This map distinguishes declarative configuration from mutable state and assigns
each item a recovery source. All Docker paths listed below are inside LXC 105's
root filesystem unless explicitly noted, so they are included in its PBS guest
backup. That provides crash-consistent filesystem recovery, not necessarily an
application-consistent export.

## Non-Docker guests

| Guest | Service | Primary state | Current recovery source | Follow-up |
|---:|---|---|---|---|
| 102 | Omada | guest root filesystem | PBS backup of LXC 102 | Identify/export controller application backup |
| 103 | AdGuard Home | guest root filesystem | PBS backup of LXC 103 | Export declarative DNS/filter settings |
| 106 | Frigate | rootfs plus `/mnt/security` | rootfs in PBS; recordings have no confirmed backup | Define recording retention/recovery policy |
| 107 | Home Assistant OS | opaque VM disk | PBS backup of VM 107 | Add HA application backup/export and test restore |
| 108 | Pelican panel | guest rootfs, Redis/application data | PBS backup of LXC 108 | Identify database and application backup procedure |
| 111 | PBS | rootfs, S3 datastore, local cache | rootfs circularly in PBS; canonical chunks in S3 | Build independent PBS bootstrap bundle |
| 121 | Wings | guest rootfs and game-server data | PBS backup of LXC 121 | Inventory server data and restore validation |

## Docker LXC 105

### Compose source

Nine Compose projects are version controlled in `hades-server`. Production image
version drift was captured and committed locally on branch
`iac/reconcile-production-images` at commit `d215c45`.

### Named volumes

The following named volumes hold mutable state within `/var/lib/docker/volumes`
and are covered by the LXC 105 PBS backup:

- `bot_shitbot_data`
- `home_freshrss_data`
- `home_open-webui_data`
- `monitoring_dockhand_data`
- `monitoring_goaccess_html`
- `monitoring_goaccess_www`
- `monitoring_pulse_data`
- `monitoring_uptimekuma_data`
- `mqtt_mosquitto_data`
- `mqtt_mosquitto_etc`
- `mqtt_mosquitto_log`
- `network_netalert_data`
- `proxy_certvault-data`
- `proxy_pocketid_data`
- `proxy_tinyauth_data`

Four anonymous volumes are also active for DockDash, Apprise attachments/plugins,
and SkySend. Anonymous volume identifiers are not a stable rebuild contract; the
corresponding Compose definitions should be changed to explicit names if these
volumes contain required state.

### Bind-mounted mutable paths

These paths are inside the Docker LXC rootfs and therefore included in its PBS
backup, but most are intentionally ignored by the application Git repository:

- `/srv/config/apprise`
- `/srv/config/netalertx/config`
- `/srv/config/homepage`
- `/srv/config/retroassembly`
- `/srv/config/prunemate/config`
- `/srv/config/prunemate/logs`
- `/srv/config/trek/data`
- `/srv/config/trek/uploads`
- `/srv/config/freshrss/extensions`
- `/srv/certs`
- `/srv/log/traefik`
- `/srv/uploads`

`/srv/config/apprise/apprise.cfg` is declarative but potentially secret-bearing;
the rest of its `store/` tree is runtime state. The config should become a
sanitized template plus an external secret, while its runtime store remains a
backup item.

### Configuration-only bind mounts

Tracked configuration is mounted for Traefik, CertVault, Homepage, Mosquitto,
Code Server, and other services. Git is the desired restore source after the
known NetAlertX/Mosquitto secret concerns are handled. Until then, PBS is also the
fallback for the currently deployed versions.

### External host bind mounts

`/mnt/pve/backup` is mounted into Docker LXC 105 at `/mnt/backup`. Its contents
are not part of the LXC backup. Current Docker mount metadata did not show an
active container consuming `/mnt/backup`, but the mount must still be classified
before declaring it disposable.

## Recovery tiers

1. **Git:** Compose and sanitized static configuration.
2. **PBS guest backup:** LXC/VM disks, Docker volumes, and rootfs bind paths.
3. **Separate host-data protection:** `/mnt/pve/security` and any required data
   on `/mnt/pve/backup`.
4. **Application exports:** Home Assistant, Omada, Pelican, and other databases
   where crash-consistent guest backups alone are insufficient.
