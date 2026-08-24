#!/usr/bin/env python3
"""Build a read-only plan by comparing managed guest fields with live PVE."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
from pve_discover import PVEClient  # noqa: E402


def parse_options(value: str | None) -> dict[str, str]:
    if not value:
        return {}
    result: dict[str, str] = {}
    first = True
    for item in value.split(","):
        if "=" in item:
            key, val = item.split("=", 1)
            result[key] = val
        elif first:
            result["volume"] = item
        first = False
    return result


def bool_value(value: Any) -> bool:
    return str(value).lower() in {"1", "true", "yes", "on"}


def compare(path: str, desired: Any, actual: Any, drift: list[dict[str, Any]]) -> None:
    if desired != actual:
        drift.append({"field": path, "desired": desired, "actual": actual})


def lxc_plan(vmid: str, desired: dict[str, Any], actual: dict[str, Any]) -> list[dict[str, Any]]:
    drift: list[dict[str, Any]] = []
    direct = {
        "hostname": desired["hostname"],
        "ostype": desired["os"],
        "cores": desired["cores"],
        "memory": desired["memory_mb"],
        "swap": desired["swap_mb"],
    }
    for key, value in direct.items():
        compare(f"lxc.{vmid}.{key}", value, actual.get(key), drift)
    compare(f"lxc.{vmid}.unprivileged", desired["unprivileged"], bool_value(actual.get("unprivileged", 0)), drift)
    compare(f"lxc.{vmid}.onboot", desired["start"]["onboot"], bool_value(actual.get("onboot", 0)), drift)

    rootfs = parse_options(actual.get("rootfs"))
    volume = rootfs.get("volume", "")
    compare(f"lxc.{vmid}.rootfs.storage", desired["rootfs"]["storage"], volume.split(":", 1)[0], drift)
    size = rootfs.get("size", "").removesuffix("G")
    compare(f"lxc.{vmid}.rootfs.size_gb", desired["rootfs"]["size_gb"], int(size) if size.isdigit() else size, drift)

    network = parse_options(actual.get("net0"))
    mapping = {"name": "name", "mac": "hwaddr", "bridge": "bridge", "ipv4": "ip", "gateway4": "gw", "ipv6": "ip6", "gateway6": "gw6"}
    for desired_key, actual_key in mapping.items():
        if desired_key in desired["network"]:
            compare(f"lxc.{vmid}.network.{desired_key}", desired["network"][desired_key], network.get(actual_key), drift)
    if "firewall" in desired["network"]:
        compare(f"lxc.{vmid}.network.firewall", desired["network"]["firewall"], bool_value(network.get("firewall", 0)), drift)

    actual_additional = []
    for key in sorted(k for k in actual if re.fullmatch(r"net[1-9]\d*", k)):
        options = parse_options(actual[key])
        actual_additional.append({
            "name": key,
            "mac": options.get("hwaddr"),
            "bridge": options.get("bridge"),
            "firewall": bool_value(options.get("firewall", 0)),
            "ipv4": options.get("ip"),
        })
    compare(f"lxc.{vmid}.additional_networks", desired.get("additional_networks", []), actual_additional, drift)

    startup = parse_options(actual.get("startup"))
    compare(f"lxc.{vmid}.start.order", desired["start"]["order"], int(startup.get("order", -1)), drift)
    if "delay_seconds" in desired["start"]:
        compare(f"lxc.{vmid}.start.delay_seconds", desired["start"]["delay_seconds"], int(startup.get("up", -1)), drift)

    actual_mounts = []
    for key, value in actual.items():
        if re.fullmatch(r"mp\d+", key):
            options = parse_options(value)
            actual_mounts.append({"source": options.get("volume"), "target": options.get("mp")})
    desired_mounts = [{"source": item["source"], "target": item["target"]} for item in desired.get("bind_mounts", [])]
    compare(f"lxc.{vmid}.bind_mounts", desired_mounts, actual_mounts, drift)
    return drift


def qemu_plan(vmid: str, desired: dict[str, Any], actual: dict[str, Any]) -> list[dict[str, Any]]:
    drift: list[dict[str, Any]] = []
    for key, value in {
        "name": desired["name"],
        "machine": desired["machine"],
        "bios": desired["bios"],
        "cores": desired["cpu"]["cores"],
        "sockets": desired["cpu"]["sockets"],
        "memory": desired["memory_mb"],
    }.items():
        compare(f"vm.{vmid}.{key}", value, int(actual[key]) if key in {"cores", "sockets", "memory"} else actual.get(key), drift)
    compare(f"vm.{vmid}.cpu.type", desired["cpu"]["type"], actual.get("cpu"), drift)
    compare(f"vm.{vmid}.onboot", desired["start"]["onboot"], bool_value(actual.get("onboot", 0)), drift)
    compare(f"vm.{vmid}.qemu_guest_agent", desired["qemu_guest_agent"], bool_value(actual.get("agent", 0)), drift)

    disk = parse_options(actual.get("scsi0"))
    compare(f"vm.{vmid}.disk.storage", desired["disk"]["storage"], disk.get("volume", "").split(":", 1)[0], drift)
    compare(f"vm.{vmid}.disk.size_gb", desired["disk"]["size_gb"], int(disk.get("size", "").removesuffix("G")), drift)

    actual_networks = []
    for key in sorted(k for k in actual if re.fullmatch(r"net\d+", k)):
        options = parse_options(actual[key])
        actual_networks.append({
            "bridge": options.get("bridge"),
            **({"vlan": int(options["tag"])} if "tag" in options else {}),
        })
    desired_networks = [
        {key: value for key, value in item.items() if key in {"bridge", "vlan"}}
        for item in desired["networks"]
    ]
    compare(f"vm.{vmid}.networks", desired_networks, actual_networks, drift)
    return drift


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--desired", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    desired = yaml.safe_load(args.desired.read_text())
    client = PVEClient(
        os.environ["PVE_ENDPOINT"],
        os.environ["PVE_API_TOKEN_ID"],
        os.environ["PVE_API_TOKEN_SECRET"],
    )
    node = desired["node"]
    drift: list[dict[str, Any]] = []
    for vmid, config in desired.get("lxcs", {}).items():
        actual = client.get(f"/nodes/{node}/lxc/{vmid}/config")
        drift.extend(lxc_plan(str(vmid), config, actual))
    for vmid, config in desired.get("vms", {}).items():
        actual = client.get(f"/nodes/{node}/qemu/{vmid}/config")
        drift.extend(qemu_plan(str(vmid), config, actual))

    result = {"mode": "read-only", "managed_guests": 8, "drift_count": len(drift), "drift": drift}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
