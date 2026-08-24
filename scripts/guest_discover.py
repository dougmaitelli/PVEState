#!/usr/bin/env python3
"""Collect allowlisted, non-secret service metadata from PVE guests."""

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LXC_IDS = (102, 103, 105, 106, 108, 111, 121)
QEMU_IDS = (107,)


LXC_PROBES = {
    "identity": "cat /etc/os-release 2>/dev/null; uname -a; hostname",
    "services": (
        "if command -v systemctl >/dev/null 2>&1; then "
        "systemctl --no-pager --plain list-units --type=service --state=running; "
        "systemctl --no-pager --plain list-unit-files --state=enabled; "
        "elif command -v rc-status >/dev/null 2>&1; then rc-status -a; fi"
    ),
    "listeners": "ss -H -lntup 2>/dev/null || netstat -lntup 2>/dev/null || true",
    "filesystems": (
        "findmnt --json --bytes -o TARGET,SOURCE,FSTYPE,OPTIONS,SIZE,USED,AVAIL 2>/dev/null; "
        "df -B1 -P 2>/dev/null"
    ),
    "docker": (
        "if command -v docker >/dev/null 2>&1; then "
        "docker version --format 'server={{.Server.Version}}' 2>/dev/null; "
        "docker ps -a --format 'container={{.Names}}|image={{.Image}}|status={{.Status}}|ports={{.Ports}}'; "
        "docker volume ls --format 'volume={{.Name}}|driver={{.Driver}}'; "
        "docker network ls --format 'network={{.Name}}|driver={{.Driver}}|scope={{.Scope}}'; "
        "fi"
    ),
    "compose_locations": (
        "find /opt /srv /root /home -xdev -maxdepth 5 -type f "
        "\\( -name compose.yml -o -name compose.yaml -o -name docker-compose.yml "
        "-o -name docker-compose.yaml \\) -print 2>/dev/null | sort | head -200"
    ),
}


def ssh_base(args: argparse.Namespace) -> list[str]:
    control_path = args.identity.parent / "ssh-control-%C"
    return [
        "ssh",
        "-p", str(args.port),
        "-o", "BatchMode=yes",
        "-o", "IdentitiesOnly=yes",
        "-o", "StrictHostKeyChecking=yes",
        "-o", f"UserKnownHostsFile={args.known_hosts}",
        "-o", "ConnectTimeout=10",
        "-o", "ControlMaster=auto",
        "-o", "ControlPersist=60",
        "-o", f"ControlPath={control_path}",
        "-i", str(args.identity),
        f"root@{args.host}",
    ]


def run(base: list[str], command: str) -> dict[str, Any]:
    try:
        completed = subprocess.run(
            [*base, command],
            check=False,
            capture_output=True,
            text=True,
            timeout=120,
        )
        return {
            "ok": completed.returncode == 0,
            "returncode": completed.returncode,
            "stdout": completed.stdout,
            "stderr": completed.stderr,
        }
    except subprocess.TimeoutExpired as error:
        return {
            "ok": False,
            "returncode": None,
            "stdout": error.stdout or "",
            "stderr": "probe timed out",
        }


def lxc_probe(base: list[str], vmid: int, probe: str) -> dict[str, Any]:
    # Probe strings are fixed constants defined above; no user input enters the shell.
    escaped = probe.replace("'", "'\"'\"'")
    return run(base, f"pct exec {vmid} -- sh -c '{escaped}'")


def discover(args: argparse.Namespace) -> dict[str, Any]:
    base = ssh_base(args)
    result: dict[str, Any] = {
        "schema_version": 1,
        "collected_at": datetime.now(timezone.utc).isoformat(),
        "mode": "read-only-allowlist-no-secrets",
        "lxc": {},
        "qemu": {},
    }

    for vmid in LXC_IDS:
        result["lxc"][str(vmid)] = {
            name: lxc_probe(base, vmid, probe) for name, probe in LXC_PROBES.items()
        }

    for vmid in QEMU_IDS:
        result["qemu"][str(vmid)] = {
            "os_info": run(base, f"qm guest cmd {vmid} get-osinfo"),
            "interfaces": run(base, f"qm guest cmd {vmid} network-get-interfaces"),
            "filesystems": run(base, f"qm guest cmd {vmid} get-fsinfo"),
        }
    return result


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

    snapshot = discover(args)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    destination = args.output_dir / f"guests-{stamp}.json"
    destination.write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n")
    (args.output_dir / "guests-latest.json").write_text(destination.read_text())

    failures: list[str] = []
    for guest_type in ("lxc", "qemu"):
        for vmid, probes in snapshot[guest_type].items():
            for name, response in probes.items():
                if not response["ok"]:
                    failures.append(f"{guest_type}/{vmid}/{name}")

    print(f"wrote read-only guest snapshot: {destination}")
    if failures:
        print("unavailable probes: " + ", ".join(failures))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
