#!/usr/bin/env python3
"""Run allowlisted read-only recovery checks and write a Markdown report."""

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import yaml


def ssh_base(args: argparse.Namespace) -> list[str]:
    control_path = args.identity.parent / "ssh-control-validation-%C"
    return [
        "ssh", "-p", str(args.port),
        "-o", "BatchMode=yes",
        "-o", "IdentitiesOnly=yes",
        "-o", "StrictHostKeyChecking=yes",
        "-o", f"UserKnownHostsFile={args.known_hosts}",
        "-o", "ControlMaster=auto",
        "-o", "ControlPersist=60",
        "-o", f"ControlPath={control_path}",
        "-i", str(args.identity),
        f"root@{args.host}",
    ]


def run(base: list[str], command: str) -> dict[str, Any]:
    try:
        result = subprocess.run(
            base + [command], check=False, capture_output=True, text=True, timeout=90
        )
        return {
            "passed": result.returncode == 0,
            "returncode": result.returncode,
            "stdout": result.stdout.strip(),
            "stderr": result.stderr.strip(),
        }
    except subprocess.TimeoutExpired:
        return {"passed": False, "returncode": None, "stdout": "", "stderr": "timed out"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checks", type=Path, required=True)
    parser.add_argument("--host", default="pve.h4des.dev")
    parser.add_argument("--port", default="22")
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--known-hosts", type=Path, required=True)
    parser.add_argument("--json-output", type=Path, required=True)
    parser.add_argument("--markdown-output", type=Path, required=True)
    args = parser.parse_args()

    definitions = yaml.safe_load(args.checks.read_text())["checks"]
    base = ssh_base(args)
    results = []
    for check in definitions:
        result = run(base, check["command"])
        results.append({
            "id": check["id"],
            "description": check["description"],
            **result,
        })

    report = {
        "validated_at": datetime.now(timezone.utc).isoformat(),
        "mode": "read-only",
        "passed": sum(1 for item in results if item["passed"]),
        "failed": sum(1 for item in results if not item["passed"]),
        "checks": results,
    }
    args.json_output.parent.mkdir(parents=True, exist_ok=True)
    args.json_output.write_text(json.dumps(report, indent=2) + "\n")

    lines = [
        "# Service recovery validation",
        "",
        f"Validated: {report['validated_at']}",
        "",
        f"Result: **{report['passed']} passed, {report['failed']} failed**",
        "",
        "| Check | Description | Result |",
        "|---|---|---|",
    ]
    for item in results:
        status = "PASS" if item["passed"] else "FAIL"
        lines.append(f"| `{item['id']}` | {item['description']} | {status} |")
    failed = [item for item in results if not item["passed"]]
    if failed:
        lines.extend(["", "## Failure details", ""])
        for item in failed:
            detail = item["stderr"] or item["stdout"] or f"exit {item['returncode']}"
            lines.append(f"- `{item['id']}`: `{detail[:300]}`")
    args.markdown_output.write_text("\n".join(lines) + "\n")
    print(f"service validation: {report['passed']} passed, {report['failed']} failed")
    for item in results:
        print(f"{'PASS' if item['passed'] else 'FAIL'} {item['id']}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
