#!/usr/bin/env python3
"""Independent fixture generator for CORE.STO.PRIV (Offline operation and telemetry consent).

Uses only the Python standard library (json, hashlib, pathlib, sys) to emit:
  - telemetry-description.v1.json (matches crates/fid-store/privacy/telemetry-description.json)
  - telemetry-description.v2.json (updated description for AC-5 lapse check)
  - offline_session.json (AC-1 offline session script)
  - expected.json (independently computed RFC 8785 canonical SHA-256 digests)

License: CC-BY-4.0 (per spec/core/sto/priv/NODE.md).
Run with --check to verify committed fixtures and the shipped description are up to date.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[3]
SHIPPED_DESCRIPTION = REPO_ROOT / "crates" / "fid-store" / "privacy" / "telemetry-description.json"


def canonical_json_bytes(obj: object) -> bytes:
    """RFC 8785 canonical JSON for ASCII strings, booleans, arrays, objects and small integers."""
    text = json.dumps(obj, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    return text.encode("utf-8")


def sha256_prefixed(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def build_fixtures() -> dict[str, bytes]:
    desc_v1 = {
        "format": "fid-telemetry-description/1",
        "version": 1,
        "purpose": "Aggregate reliability and feature-usage counts to guide open-source maintenance.",
        "collected": [
            "app_version",
            "command_counts_by_domain",
            "crash_signal_name",
            "os_family",
        ],
        "excluded": [
            "file_paths",
            "geometry_and_measurements",
            "host_and_user_names",
            "project_contents",
            "serial_numbers",
        ],
        "retention_days": 90,
    }

    desc_v2 = {
        "format": "fid-telemetry-description/1",
        "version": 2,
        "purpose": "Aggregate reliability, feature-usage and graphics-tier counts to guide open-source maintenance.",
        "collected": [
            "app_version",
            "command_counts_by_domain",
            "crash_signal_name",
            "gpu_tier",
            "os_family",
        ],
        "excluded": [
            "file_paths",
            "geometry_and_measurements",
            "host_and_user_names",
            "project_contents",
            "serial_numbers",
        ],
        "retention_days": 90,
    }

    import_payload = json.dumps(
        {
            "element_id": "elem-gauge-001",
            "points": [[0, 0, 0], [10, 0, 0], [0, 10, 0], [10, 10, 0]],
            "units": "mm",
        },
        separators=(",", ":"),
        sort_keys=True,
    )
    edit_payload = json.dumps(
        {
            "element_id": "elem-gauge-001",
            "note": "trimmed_edge",
            "points": [[0, 0, 0], [10, 0, 0], [0, 10, 0], [10, 10, 0]],
            "units": "mm",
        },
        separators=(",", ":"),
        sort_keys=True,
    )

    offline_session = {
        "format": "fid-offline-session/1",
        "project_name": "offline_gauge_block",
        "steps": [
            {"op": "show_privacy", "expect_consent": False},
            {"op": "create_project"},
            {
                "op": "import_element",
                "element_id": "elem-gauge-001",
                "kind": "test_kit.plane_points",
                "payload": import_payload,
            },
            {
                "op": "edit_element",
                "element_id": "elem-gauge-001",
                "kind": "test_kit.plane_points",
                "payload": edit_payload,
            },
            {"op": "save_project"},
            {"op": "close_project"},
            {"op": "reopen_project", "expect_entries": 2},
        ],
    }

    v1_canon = canonical_json_bytes(desc_v1)
    v2_canon = canonical_json_bytes(desc_v2)
    v1_file = v1_canon + b"\n"
    v2_file = v2_canon + b"\n"
    session_bytes = (json.dumps(offline_session, indent=2, sort_keys=True) + "\n").encode("utf-8")

    expected = {
        "format": "fid-privacy-expected/1",
        "license": "CC-BY-4.0",
        "v1_digest": sha256_prefixed(v1_canon),
        "v2_digest": sha256_prefixed(v2_canon),
        "import_blob_digest": sha256_prefixed(import_payload.encode("utf-8")),
        "edit_blob_digest": sha256_prefixed(edit_payload.encode("utf-8")),
        "offline_steps": len(offline_session["steps"]),
        "offline_entries": 2,
        "offline_blobs": 2,
    }
    expected_bytes = (json.dumps(expected, indent=2, sort_keys=True) + "\n").encode("utf-8")

    return {
        "telemetry-description.v1.json": v1_file,
        "telemetry-description.v2.json": v2_file,
        "offline_session.json": session_bytes,
        "expected.json": expected_bytes,
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv
    fixtures = build_fixtures()
    mismatches: list[str] = []

    for name, content in fixtures.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != content:
                mismatches.append(str(path.relative_to(REPO_ROOT)))
        else:
            path.write_bytes(content)

    # Also check that the shipped crate description matches v1 byte-for-byte.
    if check:
        if not SHIPPED_DESCRIPTION.exists() or SHIPPED_DESCRIPTION.read_bytes() != fixtures["telemetry-description.v1.json"]:
            mismatches.append(str(SHIPPED_DESCRIPTION.relative_to(REPO_ROOT)))
        if mismatches:
            sys.stderr.write("CORE.STO.PRIV fixtures out of date:\n")
            for m in mismatches:
                sys.stderr.write(f"  - {m}\n")
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
