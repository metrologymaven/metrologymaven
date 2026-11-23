#!/usr/bin/env python3
"""Deterministic exact-rational fixture generator for KRN.MZ.LIN.LINE3 (`fixtures/fit/mz-line3-v1`).

License: CC-BY-4.0 (project-authored synthetic geometry).

Each non-degenerate 3D straightness case places 5 exact rational support contacts at
z = (-L, -L/2, 0, +L/2, +L) with exact unit radial normals:
    n_0 = (1, 0), n_1 = (0, 1), n_2 = (-3/5, -4/5), n_3 = (0, 1), n_4 = (1, 0)
which satisfy exact KKT stationarity with strictly positive multipliers (3, 4, 10, 4, 3) / 24
and full-rank 5x5 Jacobian, plus strictly interior bulk points (including points near/on
the axis). Thus the unique minimum-zone 3D line has exact rational diameter D = 2 * R.

Usage:
    python3 fixtures/fit/make_mz_line3_fixtures.py          # write fixtures/fit/mz-line3-v1/cases.txt
    python3 fixtures/fit/make_mz_line3_fixtures.py --check  # verify byte-exact match
"""

from __future__ import annotations

import argparse
from fractions import Fraction
from pathlib import Path
import struct
import sys


OUT_FILE = Path(__file__).resolve().parent / "mz-line3-v1" / "cases.txt"


def f64_hex(x: float) -> str:
    return struct.pack(">d", float(x)).hex()


def lcg(seed: int):
    state = seed & ((1 << 64) - 1)
    while True:
        state = (state * 6364136223846793005 + 1442695040888963407) & ((1 << 64) - 1)
        yield Fraction((state >> 16) & 0xFFFF, 65536)


def add3(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def scale3(s, v):
    return (s * v[0], s * v[1], s * v[2])


def canon_dir3(d):
    dx, dy, dz = float(d[0]), float(d[1]), float(d[2])
    if dx < 0.0 or (dx == 0.0 and dy < 0.0) or (dx == 0.0 and dy == 0.0 and dz < 0.0):
        return (-dx, -dy, -dz)
    return (dx, dy, dz)


# Exact rational right-handed orthonormal frame (denom 7, |e_k|^2 = 49/49 = 1).
FRAME_E1 = (Fraction(2, 7), Fraction(3, 7), Fraction(6, 7))
FRAME_E2 = (Fraction(3, 7), Fraction(-6, 7), Fraction(2, 7))
FRAME_A = (Fraction(6, 7), Fraction(2, 7), Fraction(-3, 7))

# Second exact rational orthonormal frame (axis along Z, rotated in XY by 3-4-5).
FRAME_Z_E1 = (Fraction(3, 5), Fraction(4, 5), Fraction(0))
FRAME_Z_E2 = (Fraction(-4, 5), Fraction(3, 5), Fraction(0))
FRAME_Z_A = (Fraction(0), Fraction(0), Fraction(1))


def make_case(
    name: str,
    radius: Fraction,
    half_len: Fraction,
    frame: tuple[
        tuple[Fraction, Fraction, Fraction],
        tuple[Fraction, Fraction, Fraction],
        tuple[Fraction, Fraction, Fraction],
    ],
    origin: tuple[Fraction, Fraction, Fraction],
    n_bulk: int,
    seed: int,
    include_on_axis: bool = False,
    cross_check_mcc: bool = True,
) -> list[str]:
    e1, e2, a = frame

    def to_3d(u: Fraction, v: Fraction, z: Fraction):
        return add3(origin, add3(add3(scale3(u, e1), scale3(v, e2)), scale3(z, a)))

    # 5 exact full-rank KKT contacts (multipliers (3, 4, 10, 4, 3)/24 > 0).
    contacts = [
        (radius, Fraction(0), -half_len),
        (Fraction(0), radius, -half_len / 2),
        (-radius * Fraction(3, 5), -radius * Fraction(4, 5), Fraction(0)),
        (Fraction(0), radius, half_len / 2),
        (radius, Fraction(0), half_len),
    ]
    pts = [to_3d(u, v, z) for (u, v, z) in contacts]

    if include_on_axis:
        # Interior points sitting dead-centre on the axis (rho = 0) to verify no div-by-zero.
        pts.append(to_3d(Fraction(0), Fraction(0), -half_len / 4))
        pts.append(to_3d(Fraction(0), Fraction(0), half_len / 4))

    rng = lcg(seed)
    # Interior bulk points strictly inside cylinder radius < 0.75 * R (or near shell for MCC cross-check).
    for k in range(n_bulk):
        rz = (next(rng) * 2 - 1) * half_len * Fraction(9, 10)
        if cross_check_mcc:
            # Keep bulk points in radial band [0.65 R, 0.85 R] so ls_cyl (used by mcc_cyl) also
            # sees a clean cylindrical shell while strictly interior to the R envelope!
            angle_idx = k % 4
            if angle_idx == 0:
                ux, vx = Fraction(3, 5), Fraction(4, 5)
            elif angle_idx == 1:
                ux, vx = Fraction(-4, 5), Fraction(3, 5)
            elif angle_idx == 2:
                ux, vx = Fraction(-3, 5), Fraction(-4, 5)
            else:
                ux, vx = Fraction(4, 5), Fraction(-3, 5)
            rr = radius * (Fraction(65, 100) + next(rng) * Fraction(20, 100))
            pts.append(to_3d(rr * ux, rr * vx, rz))
        else:
            ru = (next(rng) * 2 - 1) * radius * Fraction(1, 2)
            rv = (next(rng) * 2 - 1) * radius * Fraction(1, 2)
            pts.append(to_3d(ru, rv, rz))

    ca = canon_dir3(a)
    diam = radius * 2
    lines = [
        f"case {name}",
        f"diameter {f64_hex(float(diam))}",
        f"axis {f64_hex(ca[0])} {f64_hex(ca[1])} {f64_hex(ca[2])}",
        f"mcc {'true' if cross_check_mcc else 'false'}",
    ]
    for pt in pts:
        lines.append("p " + " ".join(f64_hex(float(c)) for c in pt))
    lines.append("end")
    return lines


def generate() -> str:
    out = [
        "# Seeded exact-rational minimum-zone 3D line fixtures (KRN.MZ.LIN.LINE3).",
        "# Generated by fixtures/fit/make_mz_line3_fixtures.py (CC-BY-4.0).",
        "# Floats are big-endian IEEE-754 binary64 hex (16 lowercase hex digits).",
        "",
    ]

    cases = [
        # 1. Axis-aligned twisted 5-contact case near origin (MCC cross-check enabled)
        make_case(
            "line3_z_twisted_near",
            radius=Fraction(1, 200),  # R = 5 mm, diameter = 10 mm
            half_len=Fraction(1, 10),  # L = 100 mm (aspect 40)
            frame=(FRAME_Z_E1, FRAME_Z_E2, FRAME_Z_A),
            origin=(Fraction(0), Fraction(0), Fraction(0)),
            n_bulk=32,
            seed=1101,
            include_on_axis=False,
            cross_check_mcc=True,
        ),
        # 2. Tilted 5-contact case at 1 km from origin (MCC cross-check enabled)
        make_case(
            "line3_tilted_1km_mcc",
            radius=Fraction(3, 1000),  # R = 3 mm, diameter = 6 mm
            half_len=Fraction(1, 10),  # L = 100 mm
            frame=(FRAME_E1, FRAME_E2, FRAME_A),
            origin=(Fraction(1000), Fraction(-750), Fraction(400)),
            n_bulk=36,
            seed=1202,
            include_on_axis=False,
            cross_check_mcc=True,
        ),
        # 3. Tilted bent-and-twisted axis with points dead-centre on the axis (rho = 0) at 1 km
        make_case(
            "line3_tilted_1km_on_axis_pts",
            radius=Fraction(1, 20000),  # R = 50 um, diameter = 100 um
            half_len=Fraction(1, 20),  # L = 50 mm
            frame=(FRAME_E1, FRAME_E2, FRAME_A),
            origin=(Fraction(-800), Fraction(600), Fraction(-500)),
            n_bulk=24,
            seed=1303,
            include_on_axis=True,
            cross_check_mcc=False,
        ),
    ]

    for c in cases:
        out.extend(c)
        out.append("")

    return "\n".join(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Verify committed fixture matches generator")
    args = parser.parse_args()

    content = generate()
    if args.check:
        if not OUT_FILE.exists():
            print(f"MISSING: {OUT_FILE}", file=sys.stderr)
            return 1
        actual = OUT_FILE.read_text(encoding="utf-8")
        if actual != content:
            print(f"MISMATCH: {OUT_FILE} differs from generator output", file=sys.stderr)
            return 1
        print(f"OK: {OUT_FILE} matches generator")
        return 0

    OUT_FILE.parent.mkdir(parents=True, exist_ok=True)
    OUT_FILE.write_text(content, encoding="utf-8", newline="\n")
    print(f"Wrote {OUT_FILE} ({len(content)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
