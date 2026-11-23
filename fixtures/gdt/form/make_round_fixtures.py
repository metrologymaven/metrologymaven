#!/usr/bin/env python3
"""Deterministic CC0 fixture generator for GDT.FORM.ROUND (`spec/gdt/form/round/NODE.md`).

Generates:
1. `fixtures/gdt/form/round_known_mz.json` (AC-1):
   Three explicit 2D circular sections with planted alternating 3-lobe / 4-lobe radial
   extremes giving exact global minimum-zone widths 0.004 mm, 0.006 mm and 0.005 mm
   (worst section = index 1, width = 0.006 mm).
2. `fixtures/gdt/form/round_from_cylinder_cloud.json` (AC-2):
   Seeded 3D point cloud on a nominal cylinder (radius = 10 mm, axis = +Z through origin)
   with measurement rings at z = 5, 10, 15 mm (band half-width 0.1 mm) plus out-of-band
   filler rings at z = 2.5, 7.5, 12.5, 17.5 mm, with planted per-height lobing giving
   known section roundness widths 0.004, 0.007, 0.005 mm (worst = 0.007 mm).

Usage:
  python3 fixtures/gdt/form/make_round_fixtures.py          # write fixtures
  python3 fixtures/gdt/form/make_round_fixtures.py --check  # verify byte-exact match
"""

from __future__ import annotations

import json
import math
import pathlib
import random
import sys

HERE = pathlib.Path(__file__).resolve().parent


def make_lobed_section(
    seed: int,
    n_points: int,
    center: tuple[float, float],
    r_nom: float,
    width_m: float,
    lobes: int,
) -> dict:
    """Build a 2D section whose global Voronoi MZ circle is guaranteed to be centered at
    `center` with exact width `width_m`.

    Guarantee (alternating-contact optimality theorem for Chebyshev circles):
    We place `lobes` (>= 3) exact outer contacts at radius `r_nom + width_m / 2` at angles
    `2*pi*k/lobes` and `lobes` exact inner contacts at radius `r_nom - width_m / 2` at
    interlaced angles `2*pi*(k + 0.5)/lobes`. Every remaining point lies strictly inside the
    open annulus `(r_nom - 0.45 * width_m, r_nom + 0.45 * width_m)`. Any shift of the center
    increases the distance to at least one outer contact or decreases the distance to at least
    one inner contact, so `center` is the unique global MZ center with width `width_m` and
    peak index 0, valley index `lobes`.
    """
    rng = random.Random(seed)
    cx, cy = center
    half = 0.5 * width_m
    pts: list[list[float]] = []

    # Indices 0..lobes-1: exact outer contacts (peak_index == 0 on ties).
    for k in range(lobes):
        th = 2.0 * math.pi * k / lobes
        r = r_nom + half
        pts.append([cx + r * math.cos(th), cy + r * math.sin(th)])

    # Indices lobes..2*lobes-1: exact inner contacts (valley_index == lobes on ties).
    for k in range(lobes):
        th = 2.0 * math.pi * (k + 0.5) / lobes
        r = r_nom - half
        pts.append([cx + r * math.cos(th), cy + r * math.sin(th)])

    # Remaining points strictly inside the annulus.
    rem = n_points - 2 * lobes
    for i in range(rem):
        th = 2.0 * math.pi * (i + 0.37) / rem + rng.uniform(-0.002, 0.002)
        dr = rng.uniform(-0.42 * width_m, 0.42 * width_m)
        r = r_nom + dr
        pts.append([cx + r * math.cos(th), cy + r * math.sin(th)])

    return {
        "center": [cx, cy],
        "expected_mz_width_m": width_m,
        "expected_peak_index": 0,
        "expected_valley_index": lobes,
        "points2d": pts,
    }


def build_round_known_mz() -> dict:
    s0 = make_lobed_section(0xA001, 120, (0.012, -0.007), 0.010, 0.000004, 3)
    s1 = make_lobed_section(0xA002, 120, (-0.005, 0.018), 0.010, 0.000006, 4)
    s2 = make_lobed_section(0xA003, 120, (0.003, 0.004), 0.010, 0.000005, 3)
    s0["position_m"] = 0.005
    s1["position_m"] = 0.010
    s2["position_m"] = 0.015
    return {
        "expected_worst_section_index": 1,
        "expected_worst_width_m": 0.000006,
        "licence": "CC0-1.0",
        "seed": "0xA001",
        "sections": [s0, s1, s2],
    }


def build_round_from_cylinder_cloud() -> dict:
    """Build a 3D cylinder cloud along +Z (nominal radius 10 mm) with exact symmetric lobing
    at z = 5, 10, 15 mm (band half-width 0.1 mm) and symmetric filler rings outside the bands
    so that the least-squares cylinder axis equals +Z to machine precision."""
    r_nom = 0.010
    target_sections = [
        (0.005, 0.000004, 3),
        (0.010, 0.000007, 4),
        (0.015, 0.000005, 3),
    ]
    band_hw = 0.0001
    rng = random.Random(0xC71A)
    pts3: list[list[float]] = []

    # Out-of-band exact nominal circular rings (symmetric in angle and height around z=10 mm)
    # so the LS cylinder axis stays pinned to (0, 0, +1) through (0, 0, 0) while proving that
    # points outside z +/- 0.1 mm are excluded from every section.
    for z_fill in (0.0025, 0.0075, 0.0125, 0.0175):
        n_fill = 48
        for i in range(n_fill):
            th = 2.0 * math.pi * i / n_fill
            # Deliberately larger radial bump (0.025 mm) on filler rings: if band filtering failed,
            # section roundness would blow up from 0.004..0.007 mm to > 0.025 mm!
            r = r_nom + (0.000025 if (i % 4 == 0) else -0.000025 if (i % 4 == 2) else 0.0)
            pts3.append([r * math.cos(th), r * math.sin(th), z_fill])

    # Target rings at z = 5, 10, 15 mm with exact symmetric alternating lobing (zero 1st harmonic
    # so the LS cylinder axis remains exactly +Z through the origin).
    for z0, width_m, lobes in target_sections:
        half = 0.5 * width_m
        n_ring = 96
        for i in range(n_ring):
            th = 2.0 * math.pi * i / n_ring
            # Symmetric cosine lobing of order `lobes`: exact peaks = +half, exact valleys = -half,
            # zero dipole moment (exact LS center = (0,0), exact MZ center = (0,0), exact width = width_m).
            dr = half * math.cos(lobes * th)
            # Small deterministic z jitter strictly within +/- 0.05 mm (< band_half_width = 0.1 mm).
            dz = rng.uniform(-0.00005, 0.00005)
            r = r_nom + dr
            pts3.append([r * math.cos(th), r * math.sin(th), z0 + dz])

    return {
        "axis_origin": [0.0, 0.0, 0.0],
        "axis_direction": [0.0, 0.0, 1.0],
        "band_half_width_m": band_hw,
        "expected_section_widths_m": [w for _, w, _ in target_sections],
        "expected_worst_section_index": 1,
        "expected_worst_width_m": 0.000007,
        "licence": "CC0-1.0",
        "nominal_radius_m": r_nom,
        "points": pts3,
        "positions_m": [z for z, _, _ in target_sections],
        "seed": "0xC71A",
    }


def emit(path: pathlib.Path, doc: dict, check: bool) -> bool:
    payload = json.dumps(doc, sort_keys=True, separators=(",", ":")) + "\n"
    if check:
        if not path.exists() or path.read_text(encoding="utf-8") != payload:
            sys.stderr.write(f"mismatch: {path}\n")
            return False
        print(f"ok: {path}")
        return True
    path.write_text(payload, encoding="utf-8")
    print(f"wrote {path}")
    return True


def main() -> int:
    check = "--check" in sys.argv[1:]
    ok = True
    ok &= emit(HERE / "round_known_mz.json", build_round_known_mz(), check)
    ok &= emit(HERE / "round_from_cylinder_cloud.json", build_round_from_cylinder_cloud(), check)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
