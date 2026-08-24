#!/usr/bin/env python3
"""Fail closed unless a fresh recovery plan and explicit operator consent match."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
from pathlib import Path

import yaml


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--restore-config", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--stage", required=True, choices=["bootstrap-pve", "bootstrap-pbs", "restore-guests", "configure", "rebuild"])
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text(encoding="utf-8"))
    restore = yaml.safe_load(args.restore_config.read_text(encoding="utf-8"))
    if os.environ.get("IAC_ENABLE_MUTATION") != "YES":
        raise SystemExit("set IAC_ENABLE_MUTATION=YES for this one command")
    if os.environ.get("IAC_CONFIRM_PLAN_SHA") != plan.get("plan_sha256"):
        raise SystemExit("IAC_CONFIRM_PLAN_SHA does not match the current rebuild plan")
    if plan.get("target") != args.target:
        raise SystemExit("target does not match the rebuild plan")
    blocking = plan.get("blocking_inputs", {})
    if args.stage in {"bootstrap-pbs", "rebuild"} and blocking.get("missing_bootstrap"):
        raise SystemExit(f"PBS bootstrap has blocking inputs: {blocking['missing_bootstrap']}")
    if args.stage in {"restore-guests", "configure", "rebuild"} and blocking.get("missing_archives"):
        raise SystemExit(f"guest restore has missing archives: {blocking['missing_archives']}")
    if args.stage in {"restore-guests", "configure", "rebuild"} and blocking.get("missing_restore"):
        raise SystemExit(f"PBS attachment has blocking inputs: {blocking['missing_restore']}")
    if args.stage in {"configure", "rebuild"} and blocking.get("missing_configure"):
        raise SystemExit(f"application configuration has blocking inputs: {blocking['missing_configure']}")
    created = dt.datetime.fromisoformat(plan["created_at"])
    age = dt.datetime.now(dt.timezone.utc) - created
    if age > dt.timedelta(minutes=int(restore["target"]["plan_max_age_minutes"])):
        raise SystemExit("rebuild plan is stale; generate a fresh plan")
    production = restore["target"]["production_address"]
    if args.target == production and os.environ.get("IAC_ALLOW_PRODUCTION_TARGET") != "YES":
        raise SystemExit("target is production; also set IAC_ALLOW_PRODUCTION_TARGET=YES")
    print("authorization accepted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
