#!/usr/bin/env python3
"""Deterministic generator for fixtures/rpt/balloon/auto/ocr_candidates_5.json (CC-BY-4.0).

Five synthetic OCR callout boxes inside an A4 portrait scanned sheet (21000 x 29700 cmm)
for RPT.BALLOON.AUTO.PROPOSE AC-3.

Usage:
  python3 fixtures/rpt/balloon/auto/make_ocr_fixture.py          # write fixture
  python3 fixtures/rpt/balloon/auto/make_ocr_fixture.py --check  # verify byte-for-byte
"""

import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def build_ocr_fixture() -> dict:
    return {
        "version": 1,
        "sheet_bounds_cmm": {"width": 21000, "height": 29700},
        "candidates": [
            {
                "text": "\u00d810.00 \u00b10.05",
                "bbox": {"x0": 3000, "y0": 4000, "x1": 6200, "y1": 4350},
                "font_h_cmm": 350,
            },
            {
                "text": "45.00 +0.02/-0.01",
                "bbox": {"x0": 11000, "y0": 4000, "x1": 15200, "y1": 4350},
                "font_h_cmm": 350,
            },
            {
                "text": "2X R3.00",
                "bbox": {"x0": 3000, "y0": 10000, "x1": 5400, "y1": 10350},
                "font_h_cmm": 350,
            },
            {
                "text": "\u23e5 | 0.01",
                "bbox": {"x0": 11000, "y0": 10000, "x1": 14000, "y1": 10350},
                "font_h_cmm": 350,
            },
            {
                "text": "\u22a5 | 0.02 | A",
                "bbox": {"x0": 3000, "y0": 16000, "x1": 6600, "y1": 16350},
                "font_h_cmm": 350,
            },
        ],
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    want = (json.dumps(build_ocr_fixture(), indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")
    path = HERE / "ocr_candidates_5.json"
    if check:
        if not path.exists() or path.read_bytes() != want:
            print(f"DRIFT: {path}", file=sys.stderr)
            return 1
    else:
        path.write_bytes(want)
    print("rpt-balloon-auto ocr_candidates_5 ok (5 candidates)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
