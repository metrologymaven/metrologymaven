#!/usr/bin/env python3
"""Deterministic exact-rational fixture generator for KRN.MZ.LIN.FLAT (`fixtures/fit/mz-flat-v1`).

License: CC-BY-4.0 (project-authored synthetic geometry).

Every non-degenerate test case is built in an exact rational orthonormal frame with
strictly alternating convex-hull support contacts (2-1 / 1-2 for 2D lines; 3-1 / 1-3 / 2-2
for 3D planes) plus strictly interior bulk points, so the unique global minimum-zone width
and mid-zone direction/normal are exact rational numbers verified by convex KKT duality.

Usage:
    python3 fixtures/fit/make_mz_flat_fixtures.py          # write fixtures/fit/mz-flat-v1/cases.txt
    python3 fixtures/fit/make_mz_flat_fixtures.py --check  # verify byte-exact match
"""

from __future__ import annotations

import argparse
from fractions import Fraction
from pathlib import Path
import struct
import sys


OUT_FILE = Path(__file__).resolve().parent / "mz-flat-v1" / "cases.txt"


def f64_hex(x: float) -> str:
    return struct.pack(">d", float(x)).hex()


def frac_f64(q: Fraction) -> float:
    return float(q)


def lcg(seed: int):
    state = seed & ((1 << 64) - 1)
    while True:
        state = (state * 6364136223846793005 + 1442695040888963407) & ((1 << 64) - 1)
        yield Fraction((state >> 16) & 0xFFFF, 65536)


def add3(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def scale3(s, v):
    return (s * v[0], s * v[1], s * v[2])


def canon_dir2(d):
    dx, dy = float(d[0]), float(d[1])
    if dx < 0.0 or (dx == 0.0 and dy < 0.0):
        return (-dx, -dy)
    return (dx, dy)


def canon_dir3(n):
    nx, ny, nz = float(n[0]), float(n[1]), float(n[2])
    if nx < 0.0 or (nx == 0.0 and ny < 0.0) or (nx == 0.0 and ny == 0.0 and nz < 0.0):
        return (-nx, -ny, -nz)
    return (nx, ny, nz)


# Exact rational right-handed orthonormal frame: columns have denominator 7, norm^2 = 49/49 = 1.
FRAME_E1 = (Fraction(2, 7), Fraction(3, 7), Fraction(6, 7))
FRAME_E2 = (Fraction(3, 7), Fraction(-6, 7), Fraction(2, 7))
FRAME_N = (Fraction(6, 7), Fraction(2, 7), Fraction(-3, 7))


def make_line2_case(
    name: str,
    width: Fraction,
    half_len: Fraction,
    d2: tuple[Fraction, Fraction],
    origin: tuple[Fraction, Fraction, Fraction],
    pattern: str,
    n_bulk: int,
    seed: int,
    proj_plane: tuple[tuple[Fraction, Fraction, Fraction], tuple[Fraction, Fraction, Fraction]] | None = None,
) -> list[str]:
    """Build a 2D line minimum-zone case with exact global optimum."""
    # Exact orthonormal 2D basis in XY (or in proj_plane if given).
    # For XY: e1 = d2, e2 = (-d2[1], d2[0]).
    n2 = (-d2[1], d2[0])
    half_w = width / 2

    if proj_plane is None:
        pe1 = (Fraction(1), Fraction(0), Fraction(0))
        pe2 = (Fraction(0), Fraction(1), Fraction(0))
        pn = (Fraction(0), Fraction(0), Fraction(1))
        p0 = (Fraction(0), Fraction(0), Fraction(0))
        origin_in_plane = origin
    else:
        p0, pn = proj_plane
        # Matches ls_lin::projection_frame for FRAME_N = (6/7, 2/7, -3/7):
        # smallest |n_k| is k=1 (|2/7|), e1 = unit((0,1,0) - (2/7)*n), e2 = n x e1.
        # For simplicity, place points directly in the 3D frame (FRAME_E1, FRAME_E2) with pn = FRAME_N
        # and store expected direction in the projection_frame basis later? Better: use XY or exact
        # coordinate plane (normal = (0, 1, 0) -> e1=(1,0,0), e2=(0,0,-1) exact!).
        assert pn == (Fraction(0), Fraction(1), Fraction(0))
        pe1 = (Fraction(1), Fraction(0), Fraction(0))
        pe2 = (Fraction(0), Fraction(0), Fraction(-1))
        origin_in_plane = origin

    def to_3d(u: Fraction, v: Fraction, z_off: Fraction = Fraction(0)):
        # (x_p, y_p) in projection plane = u * d2 + v * n2
        xp = u * d2[0] + v * n2[0]
        yp = u * d2[1] + v * n2[1]
        pt = add3(origin_in_plane, add3(scale3(xp, pe1), scale3(yp, pe2)))
        return add3(pt, scale3(z_off, pn))

    pts = []
    if pattern == "2-1":
        pts.append(to_3d(-half_len, half_w))
        pts.append(to_3d(half_len, half_w))
        pts.append(to_3d(Fraction(0), -half_w))
    elif pattern == "1-2":
        pts.append(to_3d(Fraction(0), half_w))
        pts.append(to_3d(-half_len, -half_w))
        pts.append(to_3d(half_len, -half_w))
    elif pattern == "zero":
        pts.append(to_3d(-half_len, Fraction(0)))
        pts.append(to_3d(Fraction(0), Fraction(0)))
        pts.append(to_3d(half_len, Fraction(0)))
    else:
        raise ValueError(pattern)

    rng = lcg(seed)
    for k in range(n_bulk):
        ru = (next(rng) * 2 - 1) * half_len * Fraction(17, 20)
        rv = Fraction(0) if pattern == "zero" else (next(rng) * 2 - 1) * half_w * Fraction(17, 20)
        rz = Fraction(0) if proj_plane is None else (next(rng) * 2 - 1) * Fraction(1, 50)
        pts.append(to_3d(ru, rv, rz))

    cd = canon_dir2(d2)
    lines = [
        f"case {name}",
        "kind line2",
        f"pattern {pattern}",
        f"width {f64_hex(frac_f64(width))}",
        f"dir {f64_hex(cd[0])} {f64_hex(cd[1])}",
    ]
    if proj_plane is not None:
        lines.append(
            "proj "
            + " ".join(f64_hex(frac_f64(c)) for c in p0)
            + " "
            + " ".join(f64_hex(frac_f64(c)) for c in pn)
        )
    for pt in pts:
        lines.append("p " + " ".join(f64_hex(frac_f64(c)) for c in pt))
    lines.append("end")
    return lines


def make_plane_case(
    name: str,
    width: Fraction,
    half_len: Fraction,
    frame: tuple[
        tuple[Fraction, Fraction, Fraction],
        tuple[Fraction, Fraction, Fraction],
        tuple[Fraction, Fraction, Fraction],
    ],
    origin: tuple[Fraction, Fraction, Fraction],
    pattern: str,
    n_bulk: int,
    seed: int,
) -> list[str]:
    """Build a 3D plane minimum-zone case with exact global optimum."""
    e1, e2, n = frame
    half_w = width / 2

    def to_3d(u: Fraction, v: Fraction, w: Fraction):
        return add3(origin, add3(add3(scale3(u, e1), scale3(v, e2)), scale3(w, n)))

    pts = []
    if pattern == "3-1":
        pts.append(to_3d(-half_len, -half_len, half_w))
        pts.append(to_3d(half_len, -half_len, half_w))
        pts.append(to_3d(Fraction(0), half_len, half_w))
        pts.append(to_3d(Fraction(0), Fraction(0), -half_w))
    elif pattern == "1-3":
        pts.append(to_3d(Fraction(0), Fraction(0), half_w))
        pts.append(to_3d(-half_len, -half_len, -half_w))
        pts.append(to_3d(half_len, -half_len, -half_w))
        pts.append(to_3d(Fraction(0), half_len, -half_w))
    elif pattern == "2-2":
        pts.append(to_3d(-half_len, Fraction(0), half_w))
        pts.append(to_3d(half_len, Fraction(0), half_w))
        pts.append(to_3d(Fraction(0), -half_len, -half_w))
        pts.append(to_3d(Fraction(0), half_len, -half_w))
    elif pattern == "zero":
        pts.append(to_3d(-half_len, -half_len, Fraction(0)))
        pts.append(to_3d(half_len, -half_len, Fraction(0)))
        pts.append(to_3d(Fraction(0), half_len, Fraction(0)))
    else:
        raise ValueError(pattern)

    rng = lcg(seed)
    for _ in range(n_bulk):
        # Strictly inside triangle/diamond around origin.
        r1 = (next(rng) * 2 - 1) * half_len * Fraction(7, 20)
        r2 = (next(rng) * 2 - 1) * half_len * Fraction(7, 20)
        rw = Fraction(0) if pattern == "zero" else (next(rng) * 2 - 1) * half_w * Fraction(17, 20)
        pts.append(to_3d(r1, r2, rw))

    cn = canon_dir3(n)
    lines = [
        f"case {name}",
        "kind plane",
        f"pattern {pattern}",
        f"width {f64_hex(frac_f64(width))}",
        f"normal {f64_hex(cn[0])} {f64_hex(cn[1])} {f64_hex(cn[2])}",
    ]
    for pt in pts:
        lines.append("p " + " ".join(f64_hex(frac_f64(c)) for c in pt))
    lines.append("end")
    return lines


def generate() -> str:
    id_frame = (
        (Fraction(1), Fraction(0), Fraction(0)),
        (Fraction(0), Fraction(1), Fraction(0)),
        (Fraction(0), Fraction(0), Fraction(1)),
    )
    tilted_frame = (FRAME_E1, FRAME_E2, FRAME_N)

    out = [
        "# Seeded exact-rational minimum-zone 2D line and plane fixtures (KRN.MZ.LIN.FLAT).",
        "# Generated by fixtures/fit/make_mz_flat_fixtures.py (CC-BY-4.0).",
        "# Floats are big-endian IEEE-754 binary64 hex (16 lowercase hex digits).",
        "",
    ]

    cases = [
        # 2D line: 2-1 tilted 3-4-5 near origin
        make_line2_case(
            "line2_tilted_2_1",
            width=Fraction(1, 50000),  # 20 um
            half_len=Fraction(1, 20),  # 50 mm
            d2=(Fraction(3, 5), Fraction(4, 5)),
            origin=(Fraction(0), Fraction(0), Fraction(0)),
            pattern="2-1",
            n_bulk=25,
            seed=101,
        ),
        # 2D line: 1-2 tilted 5-12-13 at 1 km from origin
        make_line2_case(
            "line2_far_1km_1_2",
            width=Fraction(3, 200000),  # 15 um
            half_len=Fraction(1, 10),  # 100 mm
            d2=(Fraction(5, 13), Fraction(12, 13)),
            origin=(Fraction(1000), Fraction(-500), Fraction(250)),
            pattern="1-2",
            n_bulk=30,
            seed=202,
        ),
        # 2D line: projected onto XZ plane (normal = +Y) at 1 km from origin
        make_line2_case(
            "line2_proj_xz_1km",
            width=Fraction(1, 100000),  # 10 um
            half_len=Fraction(3, 50),  # 60 mm
            d2=(Fraction(4, 5), Fraction(-3, 5)),
            origin=(Fraction(600), Fraction(-800), Fraction(100)),
            pattern="2-1",
            n_bulk=20,
            seed=303,
            proj_plane=((Fraction(0), Fraction(5), Fraction(0)), (Fraction(0), Fraction(1), Fraction(0))),
        ),
        # 2D line: exact zero-width collinear (exact binary64 dyadic coordinates at 1 km)
        make_line2_case(
            "line2_exact_zero",
            width=Fraction(0),
            half_len=Fraction(1, 16),
            d2=(Fraction(1), Fraction(0)),
            origin=(Fraction(1000), Fraction(0), Fraction(0)),
            pattern="zero",
            n_bulk=10,
            seed=404,
        ),
        # 3D plane: axis-aligned 3-1 near origin
        make_plane_case(
            "plane_axis_3_1",
            width=Fraction(1, 25000),  # 40 um
            half_len=Fraction(1, 10),  # 100 mm
            frame=id_frame,
            origin=(Fraction(0), Fraction(0), Fraction(0)),
            pattern="3-1",
            n_bulk=35,
            seed=505,
        ),
        # 3D plane: tilted 3-1 at 1 km from origin
        make_plane_case(
            "plane_tilted_far_1km_3_1",
            width=Fraction(1, 50000),  # 20 um
            half_len=Fraction(1, 20),  # 50 mm
            frame=tilted_frame,
            origin=(Fraction(1000), Fraction(-750), Fraction(400)),
            pattern="3-1",
            n_bulk=40,
            seed=606,
        ),
        # 3D plane: tilted 1-3 at 1 km from origin
        make_plane_case(
            "plane_tilted_far_1km_1_3",
            width=Fraction(7, 250000),  # 28 um
            half_len=Fraction(3, 50),  # 60 mm
            frame=tilted_frame,
            origin=(Fraction(-600), Fraction(800), Fraction(-300)),
            pattern="1-3",
            n_bulk=40,
            seed=707,
        ),
        # 3D plane: tilted 2-2 (saddle / edge-edge support!) at 1 km from origin
        make_plane_case(
            "plane_tilted_far_1km_2_2",
            width=Fraction(3, 100000),  # 30 um
            half_len=Fraction(1, 20),  # 50 mm
            frame=tilted_frame,
            origin=(Fraction(-800), Fraction(600), Fraction(-500)),
            pattern="2-2",
            n_bulk=40,
            seed=808,
        ),
        # 3D plane: exact coplanar zero-width at 1 km (exact binary64 dyadic coordinates)
        make_plane_case(
            "plane_exact_zero",
            width=Fraction(0),
            half_len=Fraction(1, 16),
            frame=id_frame,
            origin=(Fraction(1000), Fraction(200), Fraction(-100)),
            pattern="zero",
            n_bulk=15,
            seed=909,
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
