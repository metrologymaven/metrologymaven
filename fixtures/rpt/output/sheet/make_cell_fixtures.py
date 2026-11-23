#!/usr/bin/env python3
"""Generate and check RPT.OUTPUT.SHEET.CELL fixtures (`injection.json`).

Synthetic formula-injection test fixture, CC-BY-4.0, stdlib only.
Usage:
    python fixtures/rpt/output/sheet/make_cell_fixtures.py
    python fixtures/rpt/output/sheet/make_cell_fixtures.py --check
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent


def jcs(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256_prefixed(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def build_fixtures() -> dict[str, bytes]:
    head_digest = sha256_prefixed(b"fixture:sheet:injection:head")
    block_id = "019a9f10-0001-7000-8000-000000000101"
    page_id = "019a9f10-0001-7000-8000-000000000011"

    hostile_samples = [
        "=HYPERLINK(\"https://example.invalid\",\"click\")",
        "+1+2",
        "-2+3",
        "@SUM(A1:A10)",
        "\t=CMD|' /C calc'!A0",
        "\r=1+1",
        "\uff1d1+1",
        "\uff0b2+2",
        "\uff0d3+3",
        "\uff20SUM(1,2)",
        "Safe label",
    ]
    rows = [
        [
            {"kind": "text", "text": f"C{i+1:02d}"},
            {"kind": "text", "text": sample},
        ]
        for i, sample in enumerate(hostile_samples)
    ]

    content = {
        "pages": [
            {
                "blocks": [
                    {
                        "frame": None,
                        "id": block_id,
                        "kind": "table",
                        "kind_version": "1",
                        "payload": {
                            "header": ["ID", "Comment"],
                            "rows": rows,
                            "schema": "resolved_table@1",
                        },
                    }
                ],
                "id": page_id,
            }
        ],
        "provenance": {
            "formatters": [],
            "head": head_digest,
            "kernel_version": "0.1.0",
            "locale": "en",
            "locale_release": "i18n-locale@1",
            "template": None,
            "units": None,
        },
        "schema": "resolved_report@1",
        "schema_version": "1",
        "warnings": [],
    }
    digest = sha256_prefixed(jcs(content))
    resolved = dict(content, digest=digest)
    return {"injection.json": jcs(resolved)}


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate or check RPT.OUTPUT.SHEET.CELL fixtures.")
    parser.add_argument("--check", action="store_true", help="Verify fixture bytes.")
    args = parser.parse_args()

    fixtures = build_fixtures()
    mismatches = []
    for name, expected in sorted(fixtures.items()):
        path = OUT_DIR / name
        if args.check:
            if not path.exists() or path.read_bytes() != expected:
                mismatches.append(name)
        else:
            path.write_bytes(expected)

    if args.check:
        if mismatches:
            print("RPT.OUTPUT.SHEET.CELL fixture mismatch:", ", ".join(mismatches), file=sys.stderr)
            return 1
        print(f"RPT.OUTPUT.SHEET.CELL fixtures OK ({len(fixtures)} files)")
    else:
        print(f"Wrote {len(fixtures)} RPT.OUTPUT.SHEET.CELL fixtures to {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
