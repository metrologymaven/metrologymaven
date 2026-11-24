#!/usr/bin/env python3
"""Opaque-box E2E test runner for Metrology Maven (Tiers 1-4 default, Tier 5 opt-in, F1-F7).

Usage:
    python e2e/run_e2e.py [--mode auto|strict] [--tier 1|2|3|4|5|all]
                          [--feature F1..F7] [--out <report.json>] [--keep-workdirs]
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from e2e.lib.binaries import SUT_BINARIES, BinaryManager  # noqa: E402
from e2e.lib.env import probe_capabilities  # noqa: E402
from e2e.lib.harness import execute_suite, print_summary  # noqa: E402
import e2e.tests  # noqa: E402, F401  # registers all Tier 1-4 tests

FEATURES = ("F1", "F2", "F3", "F4", "F5", "F6", "F7")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    ap = argparse.ArgumentParser(description="Run Metrology Maven Tier 1-4 opaque-box E2E tests.")
    ap.add_argument(
        "--mode",
        choices=("auto", "strict"),
        default=None,
        help="Execution mode (default: strict when CI=true, otherwise auto)",
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="Alias for --mode strict (fail if any OS-supported capability is skipped)",
    )
    ap.add_argument(
        "--tier",
        choices=("1", "2", "3", "4", "5", "all"),
        default="all",
        help="Filter by test tier (1, 2, 3, 4, 5, or all [Tiers 1-4])",
    )
    ap.add_argument(
        "--feature",
        choices=FEATURES,
        default=None,
        help="Filter by feature area (F1..F7)",
    )
    ap.add_argument(
        "--out",
        type=Path,
        default=None,
        help="Output path for e2e-report/v1 JSON (defaults to system temp dir)",
    )
    ap.add_argument(
        "--keep-workdirs",
        action="store_true",
        help="Preserve per-test temporary workdirs for debugging",
    )
    ns = ap.parse_args(argv)
    if ns.strict:
        if ns.mode == "auto":
            ap.error("--strict cannot be combined with --mode auto")
        ns.mode = "strict"
    return ns


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    ci_truthy = os.environ.get("CI", "").strip().lower() in ("1", "true", "yes")
    mode = args.mode if args.mode is not None else ("strict" if ci_truthy else "auto")
    tier_filter = None if args.tier == "all" else int(args.tier)

    caps = probe_capabilities(REPO_ROOT)
    binaries = BinaryManager(REPO_ROOT, caps)

    out_path = args.out
    if out_path is None:
        out_path = Path(tempfile.gettempdir()) / "metrologymaven-e2e-report.json"

    report = execute_suite(
        repo_root=REPO_ROOT,
        caps=caps,
        binaries=binaries,
        mode=mode,
        tier_filter=tier_filter,
        feature_filter=args.feature,
        keep_workdirs=args.keep_workdirs,
    )
    # Record all binaries requested/built during this run (filtered runs avoid compiling unneeded crates).
    build_results = binaries.cached_results()
    report["binaries"] = {
        name: {
            "crate": res.crate_name,
            "status": res.status,
            "path": str(res.path) if res.path else None,
            "detail": res.detail,
        }
        for name, res in build_results.items()
    }

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print_summary(report)
    print(f"\nReport written to: {out_path}")

    summary = report["summary"]
    assert isinstance(summary, dict)
    failed = int(summary["failed"])
    non_os_skipped = int(summary["skipped_capability"]) - int(summary["skipped_os_unsupported"])
    if failed > 0:
        return 1
    if mode == "strict" and non_os_skipped > 0:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
