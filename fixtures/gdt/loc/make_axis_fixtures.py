#!/usr/bin/env python3
"""Independent reference for GDT.LOC.DERIV.AXIS (axis of an associated feature), stdlib only.

Writes two files next to this script:

- `axis_tilted_cyl.json` (AC-1, AC-2): exact points on an invented cylinder whose axis is tilted
  0.01 rad from +z, radius 6 mm, sampled on six rings at the axial positions -15, -9, -2, 4, 11
  and 15 mm from the generating axis point `c0` (so the extent is 30 mm and its midpoint is
  `c0`, while the centroid of the points is not on the extent midpoint), eight points per ring.
  Two cases: the nominal direction +z (AC-1: the direction is the generating `d0`) and -z (AC-2:
  the direction is `-d0`); the point and the extent do not depend on the sign.
- `axis_cone.json` (AC-3): exact points on an invented cone with apex `a0`, axis tilted 0.02 rad
  from +z, half-angle 0.35 rad, sampled on five rings at the distances 12, 16, 21, 27 and 32 mm
  from the apex along the axis (ten points per ring). The extent is 20 mm and its midpoint is
  `a0 + 22 mm * d0`. Cases with the nominal direction +z (direction `d0`) and -z (`-d0`).

The expected values follow from the generating geometry alone (the leaf's rules: the extent is
the range of the projections of the points onto the axis, the point its midpoint, the direction
in the nominal hemisphere); this script never runs or mirrors the Rust code. Each point is
computed once in binary64 from the generating parameters, so the stored points lie on the
generating surface to about 1e-17 m; the check below recomputes the projections of the stored
points onto the generating axis with exact rational arithmetic and asserts that the extent and
midpoint differ from the stated ones by far less than the 1e-12 m tolerance.

With `--check` the files are rebuilt in memory and compared byte for byte with the committed
ones. Invented geometry, no standard's data; all lengths in metres; CC0.
"""

import json
import math
import struct
import sys
from decimal import Decimal, localcontext
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).resolve().parent
TOL_LENGTH_M = 1e-12
TOL_ANGLE_RAD = 1e-12


def hx(x):
    """Bits of a binary64 as 0x + 16 lowercase hex digits (negative zero keeps its sign)."""
    return "0x" + struct.pack(">d", float(x)).hex()


def v3(p):
    return [hx(c) for c in p]


def _sincos(x):
    """sin and cos of the binary64 `x` by Taylor series in 50-digit decimal arithmetic, each
    rounded once to binary64. Platform libm results may differ in the last bit; these do not,
    so `--check` is byte for byte on every host."""
    with localcontext() as ctx:
        ctx.prec = 50
        xd = Decimal(x)
        term, s, k = xd, Decimal(0), 1
        while term != 0 and k < 200:
            s += term
            term = -term * xd * xd / ((k + 1) * (k + 2))
            k += 2
        term, c, k = Decimal(1), Decimal(0), 0
        while term != 0 and k < 200:
            c += term
            term = -term * xd * xd / ((k + 1) * (k + 2))
            k += 2
        return float(s), float(c), float(s / c)


def sin(x):
    return _sincos(x)[0]


def cos(x):
    return _sincos(x)[1]


def tan(x):
    return _sincos(x)[2]


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def unit(a):
    n = math.sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2])
    return (a[0] / n, a[1] / n, a[2] / n)


def tilted(tilt, azimuth):
    """Unit vector tilted `tilt` rad from +z towards the azimuth `azimuth` rad."""
    s = sin(tilt)
    return (s * cos(azimuth), s * sin(azimuth), cos(tilt))


def frame(d):
    """Two unit vectors completing d to an orthonormal frame (any such pair will do)."""
    e1 = unit(cross(d, (1.0, 0.0, 0.0)))
    e2 = unit(cross(d, e1))
    return e1, e2


def ring_points(origin, d, positions, radius_of, per_ring, phase):
    e1, e2 = frame(d)
    pts = []
    for j, t in enumerate(positions):
        r = radius_of(t)
        for k in range(per_ring):
            th = 2.0 * math.pi * k / per_ring + phase * j
            c, s = cos(th), sin(th)
            pts.append(tuple(origin[i] + t * d[i] + r * (c * e1[i] + s * e2[i]) for i in range(3)))
    return pts


def check_extent(pts, origin, d, lo, hi):
    """Exact projections of the stored points onto the generating axis (origin, d)."""
    dd = [F(c) for c in d]
    norm2 = sum(c * c for c in dd)
    # |d| differs from 1 by at most a few ulps; divide the rational dot by the float norm.
    norm = F(math.sqrt(float(norm2)))
    ts = [sum((F(p[i]) - F(origin[i])) * dd[i] for i in range(3)) / norm for p in pts]
    tmin, tmax = min(ts), max(ts)
    assert abs(float(tmin - F(lo))) < 1e-15, ("tmin", float(tmin), lo)
    assert abs(float(tmax - F(hi))) < 1e-15, ("tmax", float(tmax), hi)
    return float(tmax - tmin)


def neg(d):
    return (-d[0], -d[1], -d[2])


def case(name, nominal, point, direction, extent):
    return {
        "expected": {"direction": v3(direction), "extent": hx(extent), "point": v3(point)},
        "name": name,
        "nominal_direction": v3(nominal),
    }


def build_cylinder():
    c0 = (0.012, -0.007, 0.05)
    d0 = tilted(0.01, 0.7)
    radius = 0.006
    positions = [-0.015, -0.009, -0.002, 0.004, 0.011, 0.015]
    pts = ring_points(c0, d0, positions, lambda t: radius, 8, 0.3)
    extent = check_extent(pts, c0, d0, -0.015, 0.015)
    assert abs(extent - 0.03) < 1e-15
    return {
        "about": "GDT.LOC.DERIV.AXIS AC-1 and AC-2: exact points on an invented cylinder, axis tilted 0.01 rad, "
        "length 30 mm, and the expected axis per nominal direction (see make_axis_fixtures.py). CC0.",
        "cases": [
            case("ac1_nominal_plus_z", (0.0, 0.0, 1.0), c0, d0, 0.03),
            case("ac2_nominal_minus_z", (0.0, 0.0, -1.0), c0, neg(d0), 0.03),
        ],
        "feature_kind": "cylinder",
        "generating": {"direction": v3(d0), "point": v3(c0), "radius": hx(radius), "tilt_rad": hx(0.01)},
        "points": [v3(p) for p in pts],
        "tolerance": {"angle_rad": hx(TOL_ANGLE_RAD), "length_m": hx(TOL_LENGTH_M)},
    }


def build_cone():
    a0 = (-0.004, 0.009, 0.02)
    d0 = tilted(0.02, -1.1)
    half = 0.35
    tan_half = tan(half)
    positions = [0.012, 0.016, 0.021, 0.027, 0.032]
    pts = ring_points(a0, d0, positions, lambda t: t * tan_half, 10, 0.45)
    extent = check_extent(pts, a0, d0, 0.012, 0.032)
    assert abs(extent - 0.02) < 1e-15
    mid = 0.022
    point = tuple(a0[i] + mid * d0[i] for i in range(3))
    return {
        "about": "GDT.LOC.DERIV.AXIS AC-3: exact points on an invented cone, axis tilted 0.02 rad, half-angle "
        "0.35 rad, 20 mm long, and the expected axis per nominal direction (see make_axis_fixtures.py). CC0.",
        "cases": [
            case("ac3_nominal_plus_z", (0.0, 0.0, 1.0), point, d0, 0.02),
            case("ac3_nominal_minus_z", (0.0, 0.0, -1.0), point, neg(d0), 0.02),
        ],
        "feature_kind": "cone",
        "generating": {"apex": v3(a0), "direction": v3(d0), "half_angle_rad": hx(half), "tilt_rad": hx(0.02)},
        "points": [v3(p) for p in pts],
        "tolerance": {"angle_rad": hx(TOL_ANGLE_RAD), "length_m": hx(TOL_LENGTH_M)},
    }


def render(obj):
    return json.dumps(obj, indent=1, sort_keys=True, ensure_ascii=True) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    outputs = {"axis_tilted_cyl.json": build_cylinder(), "axis_cone.json": build_cone()}
    bad = False
    for name, obj in outputs.items():
        text = render(obj)
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"{name}: out of date; run make_axis_fixtures.py", file=sys.stderr)
                bad = True
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
            print(f"wrote {name}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
