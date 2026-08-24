#!/usr/bin/env python3
"""Collect a read-only snapshot of an existing Proxmox VE environment."""

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
from urllib.parse import urlencode
from urllib.request import Request, urlopen


STATIC_ENDPOINTS = {
    "version": "/version",
    "cluster_status": "/cluster/status",
    "cluster_resources": "/cluster/resources",
    "cluster_backup_jobs": "/cluster/backup",
    "cluster_ha_status": "/cluster/ha/status/current",
    "pools": "/pools",
    "storage": "/storage",
}


def env_bool(name: str, default: bool = True) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


class PVEClient:
    def __init__(self, endpoint: str, token_id: str, token_secret: str) -> None:
        self.base_url = endpoint.rstrip("/") + "/api2/json"
        self.authorization = f"PVEAPIToken={token_id}={token_secret}"

        if env_bool("PVE_VERIFY_TLS", True):
            ca_file = os.getenv("PVE_CA_FILE")
            self.ssl_context = ssl.create_default_context(cafile=ca_file)
        else:
            self.ssl_context = ssl._create_unverified_context()  # noqa: SLF001

    def get(self, path: str, params: dict[str, str] | None = None) -> Any:
        url = self.base_url + path
        if params:
            url += "?" + urlencode(params)
        request = Request(
            url,
            headers={
                "Authorization": self.authorization,
                "Accept": "application/json",
                "User-Agent": "h4des-iac-discovery/0.1",
            },
            method="GET",
        )
        with urlopen(request, context=self.ssl_context, timeout=30) as response:
            payload = json.load(response)
        return payload.get("data")

    def put(self, path: str, data: dict[str, str]) -> Any:
        request = Request(
            self.base_url + path,
            data=urlencode(data).encode(),
            headers={
                "Authorization": self.authorization,
                "Accept": "application/json",
                "Content-Type": "application/x-www-form-urlencoded",
                "User-Agent": "h4des-iac-apply/1.0",
            },
            method="PUT",
        )
        with urlopen(request, context=self.ssl_context, timeout=30) as response:
            return json.load(response).get("data")


def safe_get(client: PVEClient, path: str) -> dict[str, Any]:
    try:
        return {"ok": True, "path": path, "data": client.get(path)}
    except HTTPError as error:
        return {"ok": False, "path": path, "error": f"HTTP {error.code}: {error.reason}"}
    except (URLError, TimeoutError, ssl.SSLError) as error:
        return {"ok": False, "path": path, "error": str(error)}


def discover(client: PVEClient) -> dict[str, Any]:
    collected_at = datetime.now(timezone.utc)
    result: dict[str, Any] = {
        "schema_version": 1,
        "collected_at": collected_at.isoformat(),
        "mode": "read-only",
        "requests": {},
        "nodes": {},
    }

    for name, path in STATIC_ENDPOINTS.items():
        result["requests"][name] = safe_get(client, path)

    node_names: list[str] = []
    cluster_status = result["requests"]["cluster_status"]
    if cluster_status["ok"]:
        node_names = sorted(
            item["name"]
            for item in cluster_status["data"] or []
            if item.get("type") == "node" and item.get("name")
        )

    for node in node_names:
        result["nodes"][node] = {
            "status": safe_get(client, f"/nodes/{node}/status"),
            "network": safe_get(client, f"/nodes/{node}/network"),
            "dns": safe_get(client, f"/nodes/{node}/dns"),
            "storage": safe_get(client, f"/nodes/{node}/storage"),
            "lxc": safe_get(client, f"/nodes/{node}/lxc"),
            "qemu": safe_get(client, f"/nodes/{node}/qemu"),
            "guests": {"lxc": {}, "qemu": {}},
        }

        for guest_type in ("lxc", "qemu"):
            guests = result["nodes"][node][guest_type]
            if not guests["ok"]:
                continue
            for guest in guests["data"] or []:
                vmid = str(guest.get("vmid", ""))
                if not vmid:
                    continue
                base = f"/nodes/{node}/{guest_type}/{vmid}"
                result["nodes"][node]["guests"][guest_type][vmid] = {
                    "summary": guest,
                    "config": safe_get(client, f"{base}/config"),
                    "snapshots": safe_get(client, f"{base}/snapshot"),
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

    endpoint = required_env("PVE_ENDPOINT")
    token_id = required_env("PVE_API_TOKEN_ID")
    token_secret = required_env("PVE_API_TOKEN_SECRET")

    client = PVEClient(endpoint, token_id, token_secret)
    snapshot = discover(client)

    args.output_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    destination = args.output_dir / f"pve-{stamp}.json"
    destination.write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n")

    latest = args.output_dir / "latest.json"
    latest.write_text(destination.read_text())

    failures = []
    for name, response in snapshot["requests"].items():
        if not response["ok"]:
            failures.append(f"{name}: {response['error']}")
    for node, responses in snapshot["nodes"].items():
        for name, response in responses.items():
            if name == "guests":
                for guest_type, guests in response.items():
                    for vmid, details in guests.items():
                        for detail_name in ("config", "snapshots"):
                            detail = details[detail_name]
                            if not detail["ok"]:
                                failures.append(
                                    f"{node}/{guest_type}/{vmid}/{detail_name}: "
                                    f"{detail['error']}"
                                )
                continue
            if not response["ok"]:
                failures.append(f"{node}/{name}: {response['error']}")

    print(f"wrote read-only snapshot: {destination}")
    if failures:
        print("some endpoints were unavailable to this token:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
