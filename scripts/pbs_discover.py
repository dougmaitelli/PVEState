#!/usr/bin/env python3
"""Collect a read-only snapshot of a Proxmox Backup Server environment."""

from __future__ import annotations

import argparse
import json
import os
import ssl
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


STATIC_ENDPOINTS = {
    "version": "/version",
    "datastore_usage": "/status/datastore-usage",
    "datastores": "/config/datastore",
    "s3_endpoints": "/config/s3",
    "remotes": "/config/remote",
    "sync_jobs": "/config/sync",
    "prune_jobs": "/config/prune",
    "verify_jobs": "/config/verify",
    "node_status": "/nodes/localhost/status",
}


def env_bool(name: str, default: bool = True) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


class PBSClient:
    def __init__(self, endpoint: str, token_id: str, token_secret: str) -> None:
        self.base_url = endpoint.rstrip("/") + "/api2/json"
        self.authorization = f"PBSAPIToken {token_id}:{token_secret}"

        if env_bool("PBS_VERIFY_TLS", True):
            ca_file = os.getenv("PBS_CA_FILE")
            self.ssl_context = ssl.create_default_context(cafile=ca_file)
        else:
            self.ssl_context = ssl._create_unverified_context()  # noqa: SLF001

    def get(self, path: str) -> Any:
        request = Request(
            self.base_url + path,
            headers={
                "Authorization": self.authorization,
                "Accept": "application/json",
                "User-Agent": "h4des-iac-pbs-discovery/0.1",
            },
            method="GET",
        )
        with urlopen(request, context=self.ssl_context, timeout=30) as response:
            payload = json.load(response)
        return payload.get("data")


def safe_get(client: PBSClient, path: str) -> dict[str, Any]:
    try:
        return {"ok": True, "path": path, "data": client.get(path)}
    except HTTPError as error:
        return {"ok": False, "path": path, "error": f"HTTP {error.code}: {error.reason}"}
    except (URLError, TimeoutError, ssl.SSLError) as error:
        return {"ok": False, "path": path, "error": str(error)}


def discover(client: PBSClient) -> dict[str, Any]:
    result: dict[str, Any] = {
        "schema_version": 1,
        "collected_at": datetime.now(timezone.utc).isoformat(),
        "mode": "read-only",
        "requests": {},
        "datastores": {},
    }

    for name, path in STATIC_ENDPOINTS.items():
        result["requests"][name] = safe_get(client, path)

    response = result["requests"]["datastores"]
    if response["ok"]:
        for datastore in response["data"] or []:
            name = datastore.get("name") or datastore.get("store")
            if not name:
                continue
            encoded = quote(str(name), safe="")
            result["datastores"][str(name)] = {
                "config": datastore,
                "status": safe_get(client, f"/admin/datastore/{encoded}/status"),
                "groups": safe_get(client, f"/admin/datastore/{encoded}/groups"),
                "snapshots": safe_get(client, f"/admin/datastore/{encoded}/snapshots"),
            }

    return result


def required_env(name: str) -> str:
    value = os.getenv(name)
    if not value:
        raise SystemExit(f"missing required environment variable: {name}")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()

    client = PBSClient(
        required_env("PBS_ENDPOINT"),
        required_env("PBS_API_TOKEN_ID"),
        required_env("PBS_API_TOKEN_SECRET"),
    )
    snapshot = discover(client)

    args.output_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    destination = args.output_dir / f"pbs-{stamp}.json"
    destination.write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n")
    (args.output_dir / "pbs-latest.json").write_text(destination.read_text())

    failures: list[str] = []
    for name, response in snapshot["requests"].items():
        if not response["ok"]:
            failures.append(f"{name}: {response['error']}")
    for datastore, details in snapshot["datastores"].items():
        for name in ("status", "groups", "snapshots"):
            response = details[name]
            if not response["ok"]:
                failures.append(f"{datastore}/{name}: {response['error']}")

    print(f"wrote read-only PBS snapshot: {destination}")
    if failures:
        print("some PBS endpoints were unavailable to this token:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
