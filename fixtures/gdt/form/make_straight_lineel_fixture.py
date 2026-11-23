#!/usr/bin/env python3
"""Deterministic CC0 fixture generator for GDT.FORM.STRAIGHT.LINEEL
(`spec/gdt/form/straight/lineel/NODE.md`).

Generates `fixtures/gdt/form/straight_lines_planted.json` (AC-1 & AC-2):
- Planar strip with 5 line elements (indices 1..=5) at 2 mm spacing along +Y (plane_normal = [0,1,0]),
  origin = [0, 0, 0], line_direction = [1, 0, 0], nominal band half-width = 0.25 mm.
- Element 3 has exact planted 2D minimum-zone straightness = 0.006 mm; elements 1, 2, 4, 5 have
  exact planted 2D minimum-zone straightness = 0.002 mm.
- On element 5 (y = 10 mm), only 1 point lies within +/- 0.05 mm of y = 10 mm while the rest
  lie at y = 10 mm +/- 0.15 mm (still inside 0.25 mm for AC-1, but narrowing band_half_width
  to 0.05 mm leaves element 5 with exactly 1 point for AC-2!).

Usage:
  python3 fixtures/gdt/form/make_straight_lineel_fixture.py          # write fixture
  python3 fixtures/gdt/form/make_straight_lineel_fixture.py --check  # verify byte-exact match
"""

from __future__ import annotations

import json
import pathlib
import random
import sys

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / "straight_lines_planted.json"


def build_fixture() -> dict:
    rng = random.Random(0x57AA)
    spacing = 0.002  # 2 mm
    band_hw = 0.00025  # 0.25 mm
    planted = {1: 0.000002, 2: 0.000002, 3: 0.000006, 4: 0.000002, 5: 0.000002}
    pts: list[list[float]] = []

    for k in range(1, 6):
        y0 = k * spacing
        w = planted[k]
        half = 0.5 * w
        # 2 lower endpoints + 1 upper midpoint -> exact parallel strip of width `w` along X.
        # On element 5, put point 0 at exact y0 and all other points at y0 +/- 0.15 mm so that
        # AC-1 (band 0.25 mm) includes all points, while AC-2 (band 0.05 mm) keeps exactly 1 point.
        dy_other = 0.00015 if k == 5 else 0.00002
        pts.append([0.000, y0, -half])
        pts.append([0.050, y0 + dy_other, -half])
        pts.append([0.025, y0 - dy_other, half])
        # Interior points strictly inside (-0.8*half, +0.8*half).
        for j in range(37):
            x = 0.001 + 0.048 * (j / 36.0)
            if k == 5:
                dy = (0.00015 if j % 2 == 0 else -0.00015) + rng.uniform(-0.00001, 0.00001)
            else:
                dy = rng.uniform(-0.00002, 0.00002)
            z = rng.uniform(-0.75 * half, 0.75 * half)
            pts.append([x, y0 + dy, z])

    return {
        "ac2_narrow_band_half_width_m": 0.00005,
        "band_half_width_m": band_hw,
        "expected_attaining_element": 3,
        "expected_max_width_m": 0.000006,
        "expected_per_element_m": {str(k): v for k, v in planted.items()},
        "licence": "CC0-1.0",
        "line_direction": [1.0, 0.0, 0.0],
        "origin": [0.0, 0.0, 0.0],
        "plane_normal": [0.0, 1.0, 0.0],
        "points": pts,
        "range_end": 5,
        "range_start": 1,
        "seed": "0x57AA",
        "spacing_m": spacing,
    }


def main() -> int:
    doc = build_fixture()
    payload = json.dumps(doc, sort_keys=True, separators=(",", ":")) + "\n"
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != payload:
            sys.stderr.write(f"mismatch: {OUT}\n")
            return 1
        print(f"ok: {OUT}")
        return 0
    OUT.write_text(payload, encoding="utf-8")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
