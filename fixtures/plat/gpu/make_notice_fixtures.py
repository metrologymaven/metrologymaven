#!/usr/bin/env python3
"""Independent fixture generator for PLAT.GPU.NOTICE (stdlib only; licensed with the code).

Generates:
  fixtures/plat/gpu/fallback-valid.toml   Valid gpu-fallback.toml schema 1 (reduced + software tiers)
  fixtures/plat/gpu/notice-software.json  Expected RFC 8785 canonical JSON notice for PLAT.GPU.DETECT AC-2

Usage:
  python3 fixtures/plat/gpu/make_notice_fixtures.py          # write files
  python3 fixtures/plat/gpu/make_notice_fixtures.py --check  # verify byte-for-byte match
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).parent

REDUCED_FEATURES = [
    {"id": "msaa", "effect": "reduced", "note": "2x MSAA instead of 4x"},
    {
        "id": "order_independent_transparency",
        "effect": "disabled",
        "note": "Sorted alpha blending used instead of OIT",
    },
    {"id": "point_splat_shading", "effect": "reduced", "note": "Flat unlit point splats"},
]

SOFTWARE_FEATURES = REDUCED_FEATURES + [
    {"id": "shadows", "effect": "disabled", "note": "Shadows disabled in software rendering"},
    {"id": "lod_budget", "effect": "reduced", "note": "Point cloud LOD budget reduced to 2M points"},
]


def build_fallback_toml() -> str:
    lines = [
        "# Fixture GPU fallback tiers (PLAT.GPU.FALLBK.TIERS schema 1, used by PLAT.GPU.NOTICE).",
        "schema_version = 1",
        "",
    ]
    for feat in REDUCED_FEATURES:
        lines.extend(
            [
                "[[tiers.reduced.features]]",
                f'id = "{feat["id"]}"',
                f'effect = "{feat["effect"]}"',
                f'note = "{feat["note"]}"',
                "",
            ]
        )
    for feat in SOFTWARE_FEATURES:
        lines.extend(
            [
                "[[tiers.software.features]]",
                f'id = "{feat["id"]}"',
                f'effect = "{feat["effect"]}"',
                f'note = "{feat["note"]}"',
                "",
            ]
        )
    lines.extend(
        [
            "[tiers.software.floor]",
            'workload = "render_software_1m"',
            'metric = "fps"',
            "min = 5.0",
            "",
        ]
    )
    return "\n".join(lines)


def build_notice_software_json() -> str:
    reason = (
        "Fixture Basic Display Adapter on dx12: below_minimum; "
        "Fixture Basic Display Adapter on vulkan: below_minimum; "
        "no hardware adapter qualifies; software"
    )
    summary = ", ".join(f'{f["id"]} ({f["effect"]})' for f in SOFTWARE_FEATURES)
    body = (
        "Rendering is using the software fallback tier. "
        f"Reason: {reason}. "
        f"Reduced or disabled features: {summary}. "
        "See docs/hardware.html#fallback."
    )
    doc = {
        "collapsible": True,
        "dismissible": False,
        "entries": [],
        "features": SOFTWARE_FEATURES,
        "id": "gpu.fallback.software",
        "link": "docs/hardware.html#fallback",
        "reason": reason,
        "text": {
            "body": body,
            "title": "Software rendering fallback active",
        },
        "tier": "software",
        "workarounds": [],
    }
    return json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = {
        HERE / "fallback-valid.toml": build_fallback_toml(),
        HERE / "notice-software.json": build_notice_software_json(),
    }
    if check:
        ok = True
        for path, expected in files.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_text(encoding="utf-8") != expected:
                print(f"MISMATCH: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    HERE.mkdir(parents=True, exist_ok=True)
    for path, content in files.items():
        path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
