#!/usr/bin/env python3
"""Cross-check discovered PVE guests against PBS recovery points."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def load(path: Path) -> dict[str, Any]:
    if not path.is_file():
        raise SystemExit(f"required discovery snapshot not found: {path}")
    return json.loads(path.read_text())


def iso(timestamp: int | None) -> str:
    if not timestamp:
        return "unknown"
    return datetime.fromtimestamp(timestamp, timezone.utc).isoformat()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--discovery-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    pve = load(args.discovery_dir / "latest.json")
    pbs = load(args.discovery_dir / "pbs-latest.json")
    now = datetime.now(timezone.utc).timestamp()

    guests: dict[tuple[str, str], str] = {}
    bind_mounts: list[tuple[str, str, str]] = []
    for node in pve.get("nodes", {}).values():
        for kind in ("lxc", "qemu"):
            for vmid, details in node.get("guests", {}).get(kind, {}).items():
                config = details.get("config", {}).get("data") or {}
                name = config.get("hostname") or config.get("name") or "unknown"
                backup_type = "ct" if kind == "lxc" else "vm"
                guests[(backup_type, vmid)] = name
                for key, value in config.items():
                    if key.startswith("mp") and isinstance(value, str) and value.startswith("/"):
                        bind_mounts.append((vmid, name, value.split(",", 1)[0]))

    snapshots: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for datastore in pbs.get("datastores", {}).values():
        response = datastore.get("snapshots", {})
        if not response.get("ok"):
            continue
        for snapshot in response.get("data") or []:
            key = (str(snapshot.get("backup-type")), str(snapshot.get("backup-id")))
            snapshots[key].append(snapshot)

    lines = [
        "# Recovery readiness audit",
        "",
        f"Generated: {datetime.now(timezone.utc).isoformat()}",
        "",
        "| Guest | Recovery points | Latest | Age | Latest verification | Result |",
        "|---|---:|---|---:|---|---|",
    ]
    warnings: list[str] = []

    for key, name in sorted(guests.items(), key=lambda item: int(item[0][1])):
        points = sorted(snapshots.get(key, []), key=lambda item: item.get("backup-time", 0))
        if not points:
            lines.append(f"| {key[1]} `{name}` | 0 | — | — | — | FAIL |")
            warnings.append(f"guest {key[1]} ({name}) has no PBS recovery point")
            continue

        latest = points[-1]
        timestamp = latest.get("backup-time")
        age_days = (now - timestamp) / 86400 if timestamp else float("inf")
        verification = latest.get("verification")
        state = verification.get("state", "unknown") if isinstance(verification, dict) else "unverified"
        result = "PASS"
        if age_days > 8:
            result = "WARN"
            warnings.append(f"guest {key[1]} ({name}) latest backup is {age_days:.1f} days old")
        if state != "ok":
            result = "WARN"
            warnings.append(f"guest {key[1]} ({name}) latest verification state is {state}")

        lines.append(
            f"| {key[1]} `{name}` | {len(points)} | {iso(timestamp)} | "
            f"{age_days:.1f} d | {state} | {result} |"
        )

        failed = [
            point for point in points
            if isinstance(point.get("verification"), dict)
            and point["verification"].get("state") == "failed"
        ]
        for point in failed:
            warnings.append(
                f"guest {key[1]} ({name}) has a failed historical verification at "
                f"{iso(point.get('backup-time'))}"
            )

    lines.extend(["", "## Bind mounts outside guest backups", ""])
    if bind_mounts:
        for vmid, name, source in bind_mounts:
            lines.append(f"- `{vmid}` `{name}`: `{source}`")
            warnings.append(f"guest {vmid} ({name}) bind mount requires separate recovery: {source}")
    else:
        lines.append("None discovered.")

    lines.extend(["", "## Findings", ""])
    if warnings:
        lines.extend(f"- {warning}" for warning in warnings)
    else:
        lines.append("No automated findings.")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines) + "\n")
    print(args.output.read_text())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
