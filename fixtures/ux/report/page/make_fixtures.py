#!/usr/bin/env python3
"""Generate and verify synthetic fixtures for UX.REPORT.PAGE.

License: CC-BY-4.0 (synthetic fixtures for AC-2 and AC-3 of fid-design page-check).

Usage:
    python3 fixtures/ux/report/page/make_fixtures.py
    python3 fixtures/ux/report/page/make_fixtures.py --check
"""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def base_grids() -> list[dict]:
    return [
        {
            "id": "a4-portrait",
            "page": {"width_mm": "210.0", "height_mm": "297.0"},
            "margins": {"top_mm": "15.0", "right_mm": "15.0", "bottom_mm": "15.0", "left_mm": "18.0"},
            "safe_margin_mm": "10.0",
            "columns": 12,
            "column_width_mm": "12.0",
            "gutter_mm": "3.0",
            "content_width_mm": "177.0",
            "header_band_mm": "12.0",
            "footer_band_mm": "10.0",
            "content_height_mm": "245.0",
        },
        {
            "id": "a4-landscape",
            "page": {"width_mm": "297.0", "height_mm": "210.0"},
            "margins": {"top_mm": "15.0", "right_mm": "15.0", "bottom_mm": "15.0", "left_mm": "18.0"},
            "safe_margin_mm": "10.0",
            "columns": 12,
            "column_width_mm": "19.25",
            "gutter_mm": "3.0",
            "content_width_mm": "264.0",
            "header_band_mm": "12.0",
            "footer_band_mm": "10.0",
            "content_height_mm": "158.0",
        },
        {
            "id": "letter-portrait",
            "page": {"width_mm": "215.9", "height_mm": "279.4"},
            "margins": {"top_mm": "15.0", "right_mm": "14.95", "bottom_mm": "15.0", "left_mm": "17.95"},
            "safe_margin_mm": "10.0",
            "columns": 12,
            "column_width_mm": "12.5",
            "gutter_mm": "3.0",
            "content_width_mm": "183.0",
            "header_band_mm": "12.0",
            "footer_band_mm": "10.0",
            "content_height_mm": "227.4",
        },
        {
            "id": "letter-landscape",
            "page": {"width_mm": "279.4", "height_mm": "215.9"},
            "margins": {"top_mm": "15.0", "right_mm": "15.2", "bottom_mm": "15.0", "left_mm": "18.2"},
            "safe_margin_mm": "10.0",
            "columns": 12,
            "column_width_mm": "17.75",
            "gutter_mm": "3.0",
            "content_width_mm": "246.0",
            "header_band_mm": "12.0",
            "footer_band_mm": "10.0",
            "content_height_mm": "163.9",
        },
    ]


def pretty(obj: object) -> str:
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def build_files() -> dict[str, str]:
    # AC-2: narrow-margin.json (8.0 mm right margin on a4-portrait; content_width & columns
    # adjusted consistently so ONLY PAGE-MARGIN a4-portrait triggers).
    narrow = copy.deepcopy(base_grids())
    # 210 - 10 - 8 = 192; 12 * 13.25 + 11 * 3.0 = 159 + 33 = 192.0 mm exact.
    narrow[0]["margins"]["left_mm"] = "10.0"
    narrow[0]["margins"]["right_mm"] = "8.0"
    narrow[0]["content_width_mm"] = "192.0"
    narrow[0]["column_width_mm"] = "13.25"

    # AC-3: bad-width.json (columns and gutters do not sum to content width on a4-portrait:
    # 12 * 12.5 + 11 * 3.0 = 183.0 != 177.0 mm).
    bad_w = copy.deepcopy(base_grids())
    bad_w[0]["column_width_mm"] = "12.5"

    return {
        "narrow-margin.json": pretty({"format": "page-grids@1", "grids": narrow}),
        "bad-width.json": pretty({"format": "page-grids@1", "grids": bad_w}),
    }


def main() -> int:
    files = build_files()
    if "--check" in sys.argv[1:]:
        errors: list[str] = []
        for rel, want in sorted(files.items()):
            p = ROOT / rel
            if not p.is_file():
                errors.append(f"missing fixture file: {rel}")
                continue
            if p.read_text(encoding="utf-8") != want:
                errors.append(f"fixture drift: {rel}")
        if errors:
            for e in errors:
                print(e, file=sys.stderr)
            return 1
        return 0

    for rel, content in sorted(files.items()):
        (ROOT / rel).write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
