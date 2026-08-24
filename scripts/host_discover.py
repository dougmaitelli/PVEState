#!/usr/bin/env python3
"""Collect an allowlisted, read-only PVE/PBS host snapshot over SSH."""

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


# Every command is fixed here for auditability. Do not add secret-bearing paths
# such as /etc/pve/priv, token.shadow, shadow.json, or PBS S3 credential files.
COMMANDS = {
    "identity": "id; hostname; uname -a; pveversion -v",
    "network": "ip -json address show; ip -json route show; cat /etc/network/interfaces",
    "block_devices": "lsblk --json --bytes -o NAME,PATH,SIZE,TYPE,FSTYPE,MOUNTPOINTS,MODEL,SERIAL",
    "mounts": "findmnt --json --bytes -o TARGET,SOURCE,FSTYPE,OPTIONS,SIZE,USED,AVAIL",
    "zpool": "zpool status 2>&1; zpool list -Hp 2>&1",
    "zfs": "zfs list -Hp -o name,used,available,referenced,mountpoint 2>&1",
    "pve_storage": "pvesm status",
    "pve_cluster": "pvecm status 2>&1",
    "services": (
        "systemctl --no-pager --plain --state=failed 2>&1; "
        "systemctl is-enabled pveproxy pvedaemon pvestatd pve-cluster"
    ),
    "bind_mount_sources": (
        "findmnt -T /mnt/pve/backup -o TARGET,SOURCE,FSTYPE,OPTIONS,SIZE,USED,AVAIL 2>&1; "
        "findmnt -T /mnt/pve/security -o TARGET,SOURCE,FSTYPE,OPTIONS,SIZE,USED,AVAIL 2>&1"
    ),
    "pbs_identity": "pct exec 111 -- sh -c 'hostname; uname -a; proxmox-backup-manager versions --output-format json'",
    "pbs_datastores": "pct exec 111 -- proxmox-backup-manager datastore list --output-format json",
    "pbs_disks": "pct exec 111 -- lsblk --json --bytes -o NAME,PATH,SIZE,TYPE,FSTYPE,MOUNTPOINTS,MODEL",
    "pbs_mounts": "pct exec 111 -- findmnt --json --bytes -o TARGET,SOURCE,FSTYPE,OPTIONS,SIZE,USED,AVAIL",
}


def run_ssh(base: list[str], command: str) -> dict[str, Any]:
    completed = subprocess.run(
        [*base, command],
        check=False,
        capture_output=True,
        text=True,
        timeout=90,
    )
    return {
        "ok": completed.returncode == 0,
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", required=True)
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--known-hosts", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()

    for path in (args.identity, args.known_hosts):
        if not path.is_file():
            raise SystemExit(f"required SSH file not found: {path}")

    base = [
        "ssh",
        "-p", str(args.port),
        "-o", "BatchMode=yes",
        "-o", "IdentitiesOnly=yes",
        "-o", "StrictHostKeyChecking=yes",
        "-o", f"UserKnownHostsFile={args.known_hosts}",
        "-o", "ConnectTimeout=10",
        "-i", str(args.identity),
        f"root@{args.host}",
    ]

    snapshot = {
        "schema_version": 1,
        "collected_at": datetime.now(timezone.utc).isoformat(),
        "mode": "read-only-allowlist",
        "commands": {name: run_ssh(base, command) for name, command in COMMANDS.items()},
    }

    args.output_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    destination = args.output_dir / f"host-{stamp}.json"
    destination.write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n")
    (args.output_dir / "host-latest.json").write_text(destination.read_text())

    failures = [name for name, result in snapshot["commands"].items() if not result["ok"]]
    print(f"wrote read-only host snapshot: {destination}")
    if failures:
        print("commands with non-zero status: " + ", ".join(failures))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
