#!/usr/bin/env python3
"""Capture allowlisted production Compose drift without changing the guest."""

from __future__ import annotations

import argparse
import difflib
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


FILES = (
    "stacks/home/docker-compose.yml",
    "stacks/monitoring/docker-compose.yml",
    "stacks/network/docker-compose.yml",
    "stacks/proxy/docker-compose.yml",
)


def ssh_base(args: argparse.Namespace) -> list[str]:
    return [
        "ssh", "-p", str(args.port),
        "-o", "BatchMode=yes",
        "-o", "IdentitiesOnly=yes",
        "-o", "StrictHostKeyChecking=yes",
        "-o", f"UserKnownHostsFile={args.known_hosts}",
        "-i", str(args.identity),
        f"root@{args.host}",
    ]


def remote(base: list[str], command: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(base + [command], check=False, capture_output=True, text=True, timeout=60)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="pve.h4des.dev")
    parser.add_argument("--port", default="22")
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--known-hosts", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()

    base = ssh_base(args)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    manifest: dict[str, Any] = {
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "files": {},
        "apprise_files": [],
    }
    patch_lines: list[str] = []

    for relative in FILES:
        result = remote(base, f"pct exec 105 -- cat /srv/{relative}")
        if result.returncode:
            raise SystemExit(f"failed to read {relative}: {result.stderr.strip()}")
        live = result.stdout
        destination = args.output_dir / "live" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(live)
        repository = (args.source / relative).read_text()
        diff = list(difflib.unified_diff(
            repository.splitlines(keepends=True),
            live.splitlines(keepends=True),
            fromfile=f"a/{relative}",
            tofile=f"b/{relative}",
        ))
        patch_lines.extend(diff)
        manifest["files"][relative] = {
            "different": repository != live,
            "added_lines": sum(1 for line in diff if line.startswith("+") and not line.startswith("+++")),
            "removed_lines": sum(1 for line in diff if line.startswith("-") and not line.startswith("---")),
        }

    listing = remote(
        base,
        "pct exec 105 -- find /srv/config/apprise -maxdepth 4 -type f -printf '%P|%s bytes\\n' 2>/dev/null",
    )
    if listing.returncode == 0:
        manifest["apprise_files"] = sorted(line for line in listing.stdout.splitlines() if line)

    (args.output_dir / "hades-server-production.patch").write_text("".join(patch_lines))
    (args.output_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
