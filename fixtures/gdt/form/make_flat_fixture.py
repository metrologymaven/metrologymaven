#!/usr/bin/env python3
"""Generate seeded CC0 fixture `fixtures/gdt/form/flat_known_mz.json` for GDT.FORM.FLAT (AC-1).

Seed `0xf1a7`, 500 points on a tilted 3D plane with planted 3+1 extremes giving exact
minimum-zone flatness width `0.008 mm` (`8e-6 m`).

Run: python3 fixtures/gdt/form/make_flat_fixture.py [--check]
Licence: CC0-1.0.
"""

from __future__ import annotations

import json
import math
import random
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent / "flat_known_mz.json"
SEED = 0xF1A7
N_POINTS = 500
HALF_WIDTH = 0.000004  # 0.004 mm -> total MZ width 0.008 mm = 8e-6 m


def build() -> str:
    rng = random.Random(SEED)
    # Tilted plane normal (canonical positive first non-zero component)
    n_raw = (0.24, -0.32, 0.916515138991168)
    nl = math.sqrt(sum(c * c for c in n_raw))
    n = tuple(c / nl for c in n_raw)
    # Orthonormal basis in plane
    e1_raw = (n[2], 0.0, -n[0])
    l1 = math.sqrt(sum(c * c for c in e1_raw))
    e1 = tuple(c / l1 for c in e1_raw)
    e2 = (
        n[1] * e1[2] - n[2] * e1[1],
        n[2] * e1[0] - n[0] * e1[2],
        n[0] * e1[1] - n[1] * e1[0],
    )
    origin = (0.05, -0.03, 0.02)

    # Planted 3+1 extremes guaranteeing exact unique MZ plane normal == n and width == 0.008 mm:
    # - 3 peak points at +0.004 mm forming a large equilateral triangle around (0, 0)
    # - 1 valley point at -0.004 mm strictly inside that triangle at (0, 0)
    # Index 0 = first peak (+0.004 mm), Index 1 = second peak, Index 2 = third peak,
    # Index 3 = unique valley (-0.004 mm).
    planted = [
        (0.040, 0.000, +HALF_WIDTH),
        (-0.020, 0.035, +HALF_WIDTH),
        (-0.020, -0.035, +HALF_WIDTH),
        (0.000, 0.000, -HALF_WIDTH),
    ]
    pts = []
    for u, v, w in planted:
        pts.append([
            origin[k] + u * e1[k] + v * e2[k] + w * n[k]
            for k in range(3)
        ])

    # Remaining 496 interior points strictly inside (-0.0032 mm, +0.0032 mm) and inside the triangle
    for _ in range(N_POINTS - len(planted)):
        u = rng.uniform(-0.015, 0.015)
        v = rng.uniform(-0.015, 0.015)
        w = rng.uniform(-0.8 * HALF_WIDTH, 0.8 * HALF_WIDTH)
        pts.append([
            origin[k] + u * e1[k] + v * e2[k] + w * n[k]
            for k in range(3)
        ])

    doc = {
        "expected_mz_width_m": 2.0 * HALF_WIDTH,
        "expected_peak_index": 0,
        "expected_valley_index": 3,
        "licence": "CC0-1.0",
        "plane_normal": list(n),
        "plane_origin": list(origin),
        "points": pts,
        "seed": "0xf1a7",
    }
    return json.dumps(doc, indent=2, sort_keys=True) + "\n"


def main() -> int:
    content = build()
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != content:
            print(f"mismatch: {OUT}", file=sys.stderr)
            return 1
        print(f"ok: {OUT}")
        return 0
    OUT.write_text(content, encoding="utf-8")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
