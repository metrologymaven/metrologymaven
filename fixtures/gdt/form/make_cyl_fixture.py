#!/usr/bin/env python3
"""Deterministic CC0 fixture generator for GDT.FORM.CYL (`spec/gdt/form/cyl/NODE.md`).

Generates `fixtures/gdt/form/cyl_known_mz.json` (AC-1):
- Seed: `0xc71d`
- 2,000 points on a tilted, shifted cylinder of nominal radius r = 10 mm
- Planted alternating radial extremes at both ends giving exact coaxial minimum-zone
  width 0.005 mm (peak index 0, valley index 6) and a unique global MZ axis.

Usage:
  python3 fixtures/gdt/form/make_cyl_fixture.py          # write fixture
  python3 fixtures/gdt/form/make_cyl_fixture.py --check  # verify byte-exact match
"""

from __future__ import annotations

import json
import math
import pathlib
import random
import sys

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / "cyl_known_mz.json"


def normalize(v: list[float]) -> list[float]:
    n = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
    return [v[0] / n, v[1] / n, v[2] / n]


def cross(a: list[float], b: list[float]) -> list[float]:
    return [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]


def build_cyl_known_mz() -> dict:
    rng = random.Random(0xC71D)
    origin = [0.018, -0.011, 0.006]
    axis = normalize([0.16, -0.22, 0.9622889378975529])
    e1 = normalize(cross(axis, [1.0, 0.0, 0.0]))
    e2 = normalize(cross(axis, e1))

    r_nom = 0.010  # 10 mm
    width = 0.000005  # 0.005 mm
    half = 0.5 * width
    z_end = 0.020  # 40 mm axial length

    def cyl_pt(r: float, th: float, z: float) -> list[float]:
        c = math.cos(th)
        s = math.sin(th)
        return [
            origin[0] + r * (c * e1[0] + s * e2[0]) + z * axis[0],
            origin[1] + r * (c * e1[1] + s * e2[1]) + z * axis[1],
            origin[2] + r * (c * e1[2] + s * e2[2]) + z * axis[2],
        ]

    pts: list[list[float]] = []

    # Indices 0..5: 6 exact outer contacts (3 at -z_end, 3 at +z_end). Peak index = 0.
    for z in (-z_end, z_end):
        for k in range(3):
            th = 2.0 * math.pi * k / 3.0
            pts.append(cyl_pt(r_nom + half, th, z))

    # Indices 6..11: 6 exact inner contacts interlaced by 60 deg. Valley index = 6.
    for z in (-z_end, z_end):
        for k in range(3):
            th = 2.0 * math.pi * (k + 0.5) / 3.0
            pts.append(cyl_pt(r_nom - half, th, z))

    # Remaining 1,988 points strictly inside the annulus, with asymmetric noise so the
    # LS cylinder axis is slightly shifted/tilted away from the exact MZ axis (proving
    # the MZ cylinder solver actually moves to the exact MZ optimum).
    for i in range(2000 - len(pts)):
        th = rng.uniform(0.0, 2.0 * math.pi)
        z = rng.uniform(-0.95 * z_end, 0.95 * z_end)
        # Small interior bias (+0.15*half*cos(th)*z/z_end) nudges LS while staying strictly
        # inside (-0.85*half, +0.85*half) so the planted 12 contacts still uniquely define MZ.
        bias = 0.15 * half * math.cos(th) * (z / z_end)
        dr = bias + rng.uniform(-0.65 * half, 0.65 * half)
        pts.append(cyl_pt(r_nom + dr, th, z))

    return {
        "axis_direction": axis,
        "axis_origin": origin,
        "expected_mz_width_m": width,
        "expected_peak_index": 0,
        "expected_valley_index": 6,
        "licence": "CC0-1.0",
        "nominal_radius_m": r_nom,
        "points": pts,
        "seed": "0xc71d",
    }


def main() -> int:
    doc = build_cyl_known_mz()
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
