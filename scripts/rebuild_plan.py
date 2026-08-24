#!/usr/bin/env python3
"""Validate recovery inputs and write a deterministic, confirmation-ready plan."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path

import yaml


FILES = ("site.yml", "host.yml", "network.yml", "firewall.yml", "storage.yml", "guests.yml", "backup.yml", "restore.yml")


def load(path: Path):
    with path.open(encoding="utf-8") as handle:
        return yaml.safe_load(handle)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config-dir", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    documents = {name: load(args.config_dir / name) for name in FILES}
    restore = documents["restore.yml"]
    guests = documents["guests.yml"]
    vmids = sorted([int(v) for v in guests.get("lxcs", {})] + [int(v) for v in guests.get("vms", {})])
    protected = sorted(int(v) for v in restore["protected_vmids"])
    if vmids != protected:
        raise SystemExit(f"protected_vmids must exactly match managed guests: {vmids}")
    if not documents["network.yml"].get("management_address"):
        raise SystemExit("network.management_address is required")
    if not documents["firewall.yml"].get("cluster", {}).get("enabled"):
        raise SystemExit("refusing a recovery plan with the captured cluster firewall disabled")

    missing_archives = [str(v) for v in restore["restore_order"] if not restore["archives"].get(v)]
    missing_bootstrap = [] if restore["pbs_bootstrap"].get("lxc_template") else ["pbs_bootstrap.lxc_template"]
    missing_restore = [] if restore["pbs_bootstrap"].get("storage_attached_to_pve") else ["pbs_bootstrap.storage_attached_to_pve"]
    missing_configure = [] if restore["application"].get("configure_playbook") else ["application.configure_playbook"]
    canonical = json.dumps(documents, sort_keys=True, separators=(",", ":"))
    digest = hashlib.sha256((args.target + "\n" + canonical).encode()).hexdigest()
    result = {
        "schema_version": 1,
        "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "target": args.target,
        "plan_sha256": digest,
        "managed_vmids": vmids,
        "protected_vmids": protected,
        "stages": ["preflight", "bootstrap-pve", "bootstrap-pbs", "restore-guests", "configure", "validate"],
        "blocking_inputs": {
            "missing_archives": missing_archives,
            "missing_bootstrap": missing_bootstrap,
            "missing_restore": missing_restore,
            "missing_configure": missing_configure,
        },
        "ready_for_execution": not any((missing_archives, missing_bootstrap, missing_restore, missing_configure)),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
