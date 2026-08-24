#!/usr/bin/env python3
"""Export an allowlist of non-secret PVE/PBS configuration over verified SSH."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


PVE_FILES = {
    "network/interfaces": "/etc/network/interfaces",
    "network/hosts": "/etc/hosts",
    "storage/fstab": "/etc/fstab",
    "pve/storage.cfg": "/etc/pve/storage.cfg",
    "pve/jobs.cfg": "/etc/pve/jobs.cfg",
    "pve/datacenter.cfg": "/etc/pve/datacenter.cfg",
    "pve/lxc/102.conf": "/etc/pve/lxc/102.conf",
    "pve/lxc/103.conf": "/etc/pve/lxc/103.conf",
    "pve/lxc/105.conf": "/etc/pve/lxc/105.conf",
    "pve/lxc/106.conf": "/etc/pve/lxc/106.conf",
    "pve/lxc/108.conf": "/etc/pve/lxc/108.conf",
    "pve/lxc/111.conf": "/etc/pve/lxc/111.conf",
    "pve/lxc/121.conf": "/etc/pve/lxc/121.conf",
    "pve/qemu-server/107.conf": "/etc/pve/qemu-server/107.conf",
    "pve/firewall/cluster.fw": "/etc/pve/firewall/cluster.fw",
    "pve/firewall/pve-host.fw": "/etc/pve/nodes/pve/host.fw",
}

PBS_FILES = {
    "pbs/datastore.cfg": "/etc/proxmox-backup/datastore.cfg",
    "pbs/node.cfg": "/etc/proxmox-backup/node.cfg",
    "pbs/prune.cfg": "/etc/proxmox-backup/prune.cfg",
    "pbs/verification.cfg": "/etc/proxmox-backup/verification.cfg",
    "pbs/sync.cfg": "/etc/proxmox-backup/sync.cfg",
}

SECRET_LINE = re.compile(
    r"(?i)^\s*(?:password|passwd|secret|secret-key|token|api[_-]?key|private[_-]?key)\s*[ :=]"
)


def base(args: argparse.Namespace) -> list[str]:
    return [
        "ssh", "-p", str(args.port),
        "-o", "BatchMode=yes",
        "-o", "IdentitiesOnly=yes",
        "-o", "StrictHostKeyChecking=yes",
        "-o", f"UserKnownHostsFile={args.known_hosts}",
        "-i", str(args.identity),
        f"root@{args.host}",
    ]


def read(base_command: list[str], path: str, pbs: bool = False) -> tuple[bool, str]:
    command = f"pct exec 111 -- cat {path}" if pbs else f"cat {path}"
    result = subprocess.run(
        base_command + [command], check=False, capture_output=True, text=True, timeout=60
    )
    if result.returncode:
        return False, result.stderr.strip()
    return True, result.stdout


def discover_firewall_files(base_command: list[str]) -> dict[str, str]:
    """List only PVE firewall files at the cluster, node, and guest levels."""
    command = (
        "find /etc/pve/firewall /etc/pve/nodes -type f "
        "\\( -path '/etc/pve/firewall/*.fw' -o -path '/etc/pve/nodes/*/host.fw' \\) -print"
    )
    result = subprocess.run(
        base_command + [command], check=False, capture_output=True, text=True, timeout=60
    )
    if result.returncode:
        raise SystemExit(f"unable to enumerate PVE firewall files: {result.stderr.strip()}")
    files: dict[str, str] = {}
    for path in result.stdout.splitlines():
        if re.fullmatch(r"/etc/pve/firewall/(?:cluster|\d+)\.fw", path):
            files[f"pve/firewall/{Path(path).name}"] = path
        elif match := re.fullmatch(r"/etc/pve/nodes/([A-Za-z0-9_.-]+)/host\.fw", path):
            files[f"pve/firewall/nodes/{match.group(1)}/host.fw"] = path
    return files


def sanitize(content: str) -> tuple[str, int]:
    output: list[str] = []
    redactions = 0
    for line in content.splitlines():
        if SECRET_LINE.search(line):
            key = re.split(r"[ :=]", line.strip(), maxsplit=1)[0]
            output.append(f"{key}: [REDACTED]")
            redactions += 1
        else:
            output.append(line)
    return "\n".join(output) + ("\n" if content else ""), redactions


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="pve.h4des.dev")
    parser.add_argument("--port", default="22")
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--known-hosts", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()

    ssh = base(args)
    manifest: dict[str, Any] = {
        "exported_at": datetime.now(timezone.utc).isoformat(),
        "files": {},
        "omitted_secret_paths": [
            "/etc/pve/priv/**",
            "/etc/proxmox-backup/s3.cfg",
            "/etc/proxmox-backup/token.shadow",
            "/etc/proxmox-backup/shadow.json",
        ],
    }

    firewall_files = discover_firewall_files(ssh)
    files = {**PVE_FILES, **firewall_files, **PBS_FILES}
    manifest["firewall_scope"] = {
        "enumerated": True,
        "files": sorted(firewall_files),
        "note": "PVE cluster, node, and guest firewall files; excludes guest OS and upstream network firewalls",
    }

    for relative, remote_path in files.items():
        is_pbs = relative.startswith("pbs/")
        ok, content = read(ssh, remote_path, pbs=is_pbs)
        if not ok:
            manifest["files"][relative] = {"status": "absent-or-unreadable", "detail": content}
            continue
        sanitized, redactions = sanitize(content)
        destination = args.output_dir / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(sanitized)
        manifest["files"][relative] = {
            "status": "exported",
            "sha256": hashlib.sha256(sanitized.encode()).hexdigest(),
            "redactions": redactions,
        }

    (args.output_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    exported = sum(1 for item in manifest["files"].values() if item["status"] == "exported")
    print(f"exported {exported} sanitized configuration files to {args.output_dir}")
    for name, item in manifest["files"].items():
        print(f"{name}: {item['status']} redactions={item.get('redactions', 0)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
