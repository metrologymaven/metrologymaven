#!/usr/bin/env python3
"""Independent reference for GDT.ZONE.ZTYPE AC-1 to AC-5 (CC0 project data).

Stdlib only. The Rust code never produces any expected value in these files. Writes two
files next to this script:

shapes_analytic.json
    Hand-placed cases for every zone shape: points at known distances (each case has a
    driving point exactly on the boundary of the zone it defines), ties, single points,
    non-dyadic poses and every typed error. Expected sizes are the EXACT mathematical
    value of the leaf's usage formula on the f64 inputs (Fraction arithmetic; square roots
    of exact rationals to 80 significant digits with `decimal`), rounded once to the
    nearest f64. Each success case carries `check`, the class of agreement it asserts:
    `bits` (the exact value is itself an f64 and every intermediate is exact: small dyadic
    coordinates with axis directions; the Rust result must match bit for bit), `ulp1`
    (the listed non-dyadic cases, for which this script verifies that the pinned
    expressions land within 1 ulp; NOT a general property) and `abs_bound` (hard cases:
    near-axis points far along the axis, near-plane points at 1e6 m, distances in the
    underflow range, where only the documented absolute bound holds). Every success case
    also carries `abs_tol` = B + 1 ulp of the expected size, where
    B = 32 eps R + 2^-535 m (R = max |p_i - o|, eps = 2^-53; offset: 2 eps max |d_i - c|)
    is the accuracy bound of the leaf's Design; this script checks |pinned - exact| <= B
    exactly (rational arithmetic) for every success case. The driving index is the exact
    arg-max, lowest index on ties. Expected errors are labelled by the pinned validation
    order of the leaf's Design, re-implemented here.

shapes_random_v1.json
    AC-5: 1 000 000 points from Philox4x32-10 seed 0x5eed. Point i: x, y, z = 2u - 1
    for draws 3i, 3i+1, 3i+2 of stream 0; its signed deviation (offset shape) is 2u - 1
    for draw i of stream 1. For each shape: the usage of the whole set, and the SHA-256
    of the usages of the 1 000 consecutive chunks of 1 000 points (record per chunk: the
    size's f64 bits as u64 little-endian, then the driving index within the chunk as
    u32 little-endian). These are computed by re-implementing the leaf's PINNED binary64
    expressions in Python floats (IEEE 754 binary64, no fused multiply-add,
    correctly-rounded math.sqrt), so they fix the exact output bits every platform must
    produce. As a cross-check, every whole-set size is compared with the EXACT maximum over
    the whole set: by the per-point bound e = 16 eps R + 2^-536 m of the leaf's Design, the
    exact arg-max can only be a point whose pinned value is within 2e of the pinned
    maximum, so the exact values of all points within 8e of it (a 4x margin) are computed
    in rational arithmetic and their maximum is taken. The size must lie within B of
    twice that maximum (checked exactly) and within 4 ulp of its nearest f64.

All f64 values are written as "0x" + 16 hex digits of their bit pattern.

Run:  python fixtures/gdt/zone/make_shapes_fixtures.py [--check]

With --check nothing is written; the script exits non-zero if a file differs.
"""

import decimal
import hashlib
import json
import math
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
CHECK = "--check" in sys.argv[1:]
STALE = []

UNIT_TOL = 1e-12
IN_PLANE_TOL = 1e-12
NAN = float("nan")
INF = float("inf")

decimal.getcontext().prec = 80


def hx(x):
    return "0x%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def ulp_diff(a, b):
    def key(x):
        u = struct.unpack("<q", struct.pack("<d", x))[0]
        return u if u >= 0 else -(u & 0x7FFFFFFFFFFFFFFF)

    return abs(key(a) - key(b))


# ----------------------------------------------------------------- exact oracle


def fr(x):
    return Fraction(x)


def exact_sqrt_to_f64(q):
    """Nearest f64 to sqrt(q) for an exact non-negative rational q."""
    if q == 0:
        return 0.0
    d = decimal.Decimal(q.numerator) / decimal.Decimal(q.denominator)
    return float(d.sqrt())


def exact_dist2(shape, pose, p):
    """Exact distance (planes, lines) or squared distance (round shapes) as a Fraction."""
    o, n = pose
    r = [fr(p[i]) - fr(o[i]) for i in range(3)]
    kind = shape["kind"]
    if kind in ("two_planes", "two_lines"):
        return abs(sum(fr(n[i]) * r[i] for i in range(3)))
    if kind in ("cylinder", "circle"):
        a = n if kind == "cylinder" else shape["plane"]
        a = [fr(x) for x in a]
        h = sum(a[i] * r[i] for i in range(3))
        q = [r[i] - h * a[i] for i in range(3)]
        return sum(x * x for x in q)
    if kind == "sphere":
        return sum(x * x for x in r)
    raise ValueError(kind)


def exact_usage(shape, pose, points, deviations):
    """(size as nearest f64, driving index, size is exactly an f64)."""
    if shape["kind"] == "offset":
        c = fr(shape["shift"])
        vals = [abs(fr(d) - c) for d in deviations]
        best = max(vals)
        idx = vals.index(best)
        size = 2 * best
        f = float(size)
        return f, idx, fr(f) == size
    vals = [exact_dist2(shape, pose, p) for p in points]
    best = max(vals)
    idx = vals.index(best)  # first occurrence: lowest index on ties
    if shape["kind"] in ("two_planes", "two_lines"):
        size = 2 * best
        f = float(size)
        return f, idx, fr(f) == size
    f = 2.0 * exact_sqrt_to_f64(best)  # doubling is exact
    exact = fr(f) * fr(f) == 4 * best
    return f, idx, exact


# ----------------------------------------------------------------- pinned f64 expressions


def dot3(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def norm(d):
    s = dot3(d, d)
    return math.sqrt(s) if math.isfinite(s) else INF


def f64_dist(kind, n, plane, o, p):
    r = (p[0] - o[0], p[1] - o[1], p[2] - o[2])
    if kind in ("two_planes", "two_lines"):
        return abs(dot3(n, r))
    if kind in ("cylinder", "circle"):
        a = n if kind == "cylinder" else plane
        h = dot3(a, r)
        q = (r[0] - h * a[0], r[1] - h * a[1], r[2] - h * a[2])
        s = dot3(q, q)
        return math.sqrt(s) if math.isfinite(s) else INF
    if kind == "sphere":
        s = dot3(r, r)
        return math.sqrt(s) if math.isfinite(s) else INF
    raise ValueError(kind)


def f64_max(ds):
    best, idx = ds[0], 0
    for i in range(1, len(ds)):
        if ds[i] > best:
            best, idx = ds[i], i
    return best, idx


def f64_usage(shape, pose, points, deviations):
    """Pinned validation order and pinned expressions; returns (size, idx) or error code."""
    kind = shape["kind"]
    o, n = pose
    want_dev = kind == "offset"
    if want_dev != (deviations is not None):
        return "input_kind_mismatch"
    if deviations is not None and len(deviations) != len(points):
        return "deviation_count_mismatch"
    if len(points) == 0:
        return "empty_points"
    scalars = list(o) + list(n)
    if kind in ("two_lines", "circle"):
        scalars += list(shape["plane"])
    if kind == "offset":
        scalars.append(shape["shift"])
    for p in points:
        scalars += list(p)
    if deviations is not None:
        scalars += list(deviations)
    if any(math.isnan(x) for x in scalars):
        return "nan_input"
    if any(math.isinf(x) for x in scalars):
        return "non_finite_input"
    if abs(norm(n) - 1.0) > UNIT_TOL:
        return "non_unit_direction"
    if kind in ("two_lines", "circle") and abs(norm(shape["plane"]) - 1.0) > UNIT_TOL:
        return "non_unit_direction"
    if kind == "two_lines" and abs(dot3(n, shape["plane"])) > IN_PLANE_TOL:
        return "direction_not_in_plane"
    if kind == "offset":
        c = shape["shift"]
        ds = [abs(d - c) for d in deviations]
    else:
        ds = [f64_dist(kind, n, shape.get("plane"), o, p) for p in points]
    for d in ds:
        if not math.isfinite(d):
            return "overflow"
    best, idx = f64_max(ds)
    size = 2.0 * best
    if not math.isfinite(size):
        return "overflow"
    return size, idx


# ----------------------------------------------------------------- analytic cases

Z = (0.0, 0.0, 0.0)
EZ = (0.0, 0.0, 1.0)
EX = (1.0, 0.0, 0.0)
EY = (0.0, 1.0, 0.0)
# Non-dyadic unit directions (each within 1e-12 of unit after rounding; checked below).
D2367 = (2.0 / 7.0, 3.0 / 7.0, 6.0 / 7.0)
P6237 = (6.0 / 7.0, 2.0 / 7.0, -3.0 / 7.0)  # perpendicular to D2367
D68 = (0.6, 0.8, 0.0)

TP = {"kind": "two_planes"}
CY = {"kind": "cylinder"}
SP = {"kind": "sphere"}


def TL(plane):
    return {"kind": "two_lines", "plane": plane}


def CI(plane):
    return {"kind": "circle", "plane": plane}


def OF(shift):
    return {"kind": "offset", "shift": shift}


CASES = []


def case(name, shape, origin, direction, points, deviations=None, note="", hard=False):
    CASES.append(
        {
            "hard": hard,
            "name": name,
            "shape": shape,
            "pose": (tuple(origin), tuple(direction)),
            "points": [tuple(p) for p in points],
            "deviations": None if deviations is None else list(deviations),
            "note": note,
        }
    )


# AC-1, dyadic: every size exact, one point exactly on the zone boundary (the driver).
case("two_planes_z", TP, Z, EZ, [(0.5, 7.0, 0.25), (-3.0, 1.0, -0.5), (2.0, 2.0, 0.125)],
     note="half-widths 0.25, 0.5, 0.125: size 1, driver 1")
case("two_planes_x_offset_origin", TP, (1.0, 2.0, 3.0), EX,
     [(1.75, 0.0, 0.0), (0.5, 9.0, -4.0), (1.0, 2.0, 3.0), (0.0, 2.0, 3.0)],
     note="distances 0.75, 0.5, 0, 1: size 2, driver 3")
case("two_planes_negative_normal", TP, Z, (0.0, -1.0, 0.0), [(0.0, 0.0625, 0.0), (5.0, -0.1875, 1.0)],
     note="size 0.375, driver 1")
case("two_lines_xy_y", TL(EZ), Z, EY, [(4.0, 0.375, 10.0), (-1.0, -0.75, -3.0), (0.0, 0.5, 0.0)],
     note="out-of-plane z ignored; size 1.5, driver 1")
case("two_lines_xz_x", TL(EY), (0.5, 0.0, 0.0), EX, [(0.25, 2.0, 1.0), (1.5, -8.0, 0.0)],
     note="in-plane distances 0.25, 1: size 2, driver 1")
case("cylinder_z", CY, (1.0, 1.0, 0.0), EZ, [(1.75, 2.0, 5.0), (1.0, 1.5, -3.0), (0.5, 1.0, 100.0)],
     note="radii 1.25 (3-4-5), 0.5, 0.5: size 2.5, driver 0")
case("cylinder_x", CY, Z, EX, [(9.0, 0.375, 0.5), (-2.0, 0.0, 0.25)],
     note="radii 0.625, 0.25: size 1.25, driver 0")
case("circle_xy", CI(EZ), Z, EZ, [(0.375, 0.5, 2.0), (0.0, -0.25, -1.0), (0.5, 0.0, 0.0)],
     note="in-plane radii 0.625, 0.25, 0.5: size 1.25, driver 0")
case("circle_yz_pose_direction_unused", CI(EX), (0.0, 1.0, 1.0), EZ,
     [(3.0, 1.0, 1.5), (-7.0, 1.75, 2.0)],
     note="plane normal x; pose direction unused; radii 0.5, 1.25: size 2.5, driver 1")
case("sphere_origin", SP, Z, EZ, [(0.25, 0.375, 0.75), (0.5, 0.0, 0.0), (0.0, -0.5, 0.0)],
     note="radii 0.875 (2-3-6-7), 0.5, 0.5: size 1.75, driver 0")
case("sphere_shifted", SP, (1.0, -1.0, 2.0), EZ, [(1.0, -1.0, 2.0), (4.0, 3.0, 2.0)],
     note="radii 0, 5: size 10, driver 1")
case("offset_shift", OF(0.125), Z, EZ, [Z, Z, Z], [0.25, -0.125, 0.5],
     note="|d - c| 0.125, 0.25, 0.375: size 0.75, driver 2")
case("offset_negative_driver", OF(-0.25), Z, EZ, [Z, Z], [0.5, -1.0],
     note="|d - c| 0.75, 0.75: tie, size 1.5, driver 0")
case("offset_zero_shift", OF(0.0), Z, EZ, [Z], [-0.0625], note="size 0.125")

# Single points and zero size.
case("single_on_datum_planes", TP, Z, EZ, [(1.0, 1.0, 0.0)], note="size 0")
case("single_on_axis_cylinder", CY, Z, EZ, [(0.0, 0.0, 9.0)], note="size 0")
case("single_at_centre_sphere", SP, Z, EZ, [Z], note="size 0")

# AC-2: ties on the maximum, the lowest index wins.
case("tie_two_planes", TP, Z, EZ, [(0.0, 0.0, 0.0), (0.0, 0.0, 0.5), (0.0, 0.0, -0.5), (1.0, 1.0, 0.5)],
     note="indices 1, 2, 3 tie: driver 1")
case("tie_two_lines", TL(EZ), Z, EX, [(-0.25, 0.0, 0.0), (0.25, 3.0, 1.0)], note="driver 0")
case("tie_cylinder", CY, Z, EZ, [(0.0, 0.0, 0.0), (0.6875, 0.0, 0.0), (0.0, -0.6875, 2.0)], note="driver 1")
case("tie_circle", CI(EZ), Z, EY, [(0.375, 0.5, 0.0), (-0.5, -0.375, 1.0), (0.0, 0.625, -1.0)],
     note="all radius 0.625: driver 0")
case("tie_sphere", SP, Z, EZ, [(0.0, 0.0, 0.125), (0.0, 0.875, 0.0), (0.875, 0.0, 0.0), (0.0, 0.0, -0.875)],
     note="driver 1")
case("tie_offset", OF(0.0), Z, EZ, [Z, Z, Z], [0.25, -0.5, 0.5], note="driver 1")

# AC-1, non-dyadic poses (expected within 1 ulp of the exact value).
case("two_planes_d68", TP, (0.1, 0.2, 0.3), D68, [(1.0, 1.0, 0.0), (-0.5, 0.25, 7.0), (0.3, -0.7, 1.0)])
case("two_planes_d2367", TP, Z, D2367, [(1.0, 2.0, 3.0), (-1.0, 0.5, 0.25), (0.75, -0.5, -2.0)])
case("two_lines_d2367_plane_p6237", TL(P6237), (0.1, 0.1, 0.1), D2367,
     [(1.0, 0.0, 0.0), (0.0, -1.0, 0.0), (0.0, 0.0, 1.5)])
case("cylinder_d2367", CY, (0.1, -0.2, 0.3), D2367, [(1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.5, 0.5, 3.0)])
case("circle_p6237", CI(P6237), (0.3, 0.2, 0.1), D2367, [(1.0, 1.0, 1.0), (-0.5, 0.0, 0.25)])
case("sphere_tenths", SP, (0.1, 0.2, 0.3), EZ, [(0.7, 0.0, -0.1), (-0.4, 0.6, 0.3), (0.2, 0.2, 0.2)])
case("offset_tenth", OF(0.1), Z, EZ, [Z, Z, Z], [0.3, -0.4, 0.55])

# Hard cases: only the absolute accuracy bound of the Design holds (no ulp claim).
_O = (0.1, -0.2, 0.3)
case("hard_cylinder_near_axis_1km", CY, _O, D2367,
     [tuple(_O[k] + 1000.0 * D2367[k] + 1e-9 * P6237[k] for k in range(3))],
     note="1e-9 m off an axis, 1 km along it", hard=True)
case("hard_cylinder_near_axis_tiny", CY, Z, EZ, [(1e-163, 0.0, 1.0)],
     note="squared offset flushes to zero: size reads 0", hard=True)
_F = (1e6, -2e6, 5e5)
case("hard_planes_origin_at_1e6", TP, _F, D68,
     [(_F[0] + 3e-3, _F[1] - 1e-3, _F[2] + 7.0), (_F[0] - 2e-3, _F[1] + 4e-3, _F[2])],
     note="origin and points near 1e6 m", hard=True)
case("hard_planes_point_at_1e6_near_plane", TP, Z, D68,
     [tuple(1e6 * t + 2.5e-7 * D68[k] for k, t in enumerate((0.8, -0.6, 0.0))), (0.0, 0.0, 1e6)],
     note="2.5e-7 m off the datum plane, 1e6 m from the origin", hard=True)
case("hard_circle_near_centre_1km", CI(P6237), (1e3, 1e3, 1e3), D2367,
     [tuple(1e3 + 1e3 * P6237[k] + 1e-9 * D2367[k] for k in range(3))],
     note="1e-9 m in-plane from the centre, 1 km out of plane", hard=True)
case("hard_sphere_1e_160", SP, Z, EZ, [(1e-160, 0.0, 0.0)], note="subnormal square", hard=True)
case("hard_sphere_1e_163", SP, Z, EZ, [(0.0, -1e-163, 0.0)], note="square flushes to zero: size 0", hard=True)

# AC-4 and the other pinned errors.
case("err_empty_planes", TP, Z, EZ, [])
case("err_empty_cylinder", CY, Z, EZ, [])
case("err_empty_circle", CI(EZ), Z, EZ, [])
case("err_empty_sphere", SP, Z, EZ, [])
case("err_empty_lines", TL(EZ), Z, EX, [])
case("err_empty_offset", OF(0.0), Z, EZ, [], [])
case("err_empty_before_nan_pose", TP, (NAN, 0.0, 0.0), EZ, [], note="emptiness is checked first")
case("err_dir_len_1_1_planes", TP, Z, (0.0, 0.0, 1.1), [(0.0, 0.0, 1.0)])
case("err_dir_len_1_1_cylinder", CY, Z, (1.1, 0.0, 0.0), [(0.0, 0.0, 1.0)])
case("err_dir_len_1_1_sphere", SP, Z, (0.0, 1.1, 0.0), [(0.0, 0.0, 1.0)], note="pose validated for every shape")
case("err_dir_zero", TP, Z, Z, [(0.0, 0.0, 1.0)])
case("err_plane_len_1_1_circle", CI((0.0, 0.0, 1.1)), Z, EZ, [(1.0, 0.0, 0.0)])
case("err_plane_len_1_1_lines", TL((0.0, 0.0, 1.1)), Z, EX, [(1.0, 0.0, 0.0)])
case("err_dir_just_outside_tol", TP, Z, (0.0, 0.0, 1.0 + 4e-12), [(0.0, 0.0, 1.0)])
case("ok_dir_just_inside_tol", TP, Z, (0.0, 0.0, 1.0 + 5e-13), [(0.0, 0.0, 1.0)],
     note="accepted; size uses the given direction as is")
case("err_nan_point", TP, Z, EZ, [(0.0, 0.0, 1.0), (0.0, NAN, 0.0)])
case("err_nan_point_sphere", SP, Z, EZ, [(NAN, 0.0, 0.0)])
case("err_nan_origin", CY, (0.0, NAN, 0.0), EZ, [(1.0, 0.0, 0.0)])
case("err_nan_direction", TP, Z, (NAN, 0.0, 1.0), [(1.0, 0.0, 0.0)])
case("err_nan_plane", CI((0.0, NAN, 1.0)), Z, EZ, [(1.0, 0.0, 0.0)])
case("err_nan_shift", OF(NAN), Z, EZ, [Z], [0.0])
case("err_nan_deviation", OF(0.0), Z, EZ, [Z, Z], [0.0, NAN])
case("err_nan_before_non_unit", TP, Z, (0.0, 0.0, 2.0), [(NAN, 0.0, 0.0)])
case("err_nan_before_inf", TP, (INF, 0.0, 0.0), EZ, [(NAN, 0.0, 0.0)])
case("err_inf_point", TP, Z, EZ, [(0.0, 0.0, INF)])
case("err_neg_inf_origin", SP, (-INF, 0.0, 0.0), EZ, [Z])
case("err_inf_deviation", OF(0.0), Z, EZ, [Z], [-INF])
case("err_not_in_plane", TL(EZ), Z, (0.0, 0.6, 0.8), [(1.0, 0.0, 0.0)])
case("err_not_in_plane_slight", TL(EZ), Z, (1.0, 0.0, 2e-12), [(1.0, 0.0, 0.0)],
     note="unit within 1e-12, n.m = 2e-12 > 1e-12")
case("err_count_mismatch", OF(0.0), Z, EZ, [Z, Z], [0.0])
case("err_count_mismatch_before_empty", OF(0.0), Z, EZ, [], [0.0])
case("err_kind_points_to_offset", OF(0.0), Z, EZ, [Z])
case("err_kind_deviations_to_planes", TP, Z, EZ, [Z], [0.0])
case("err_overflow_difference", TP, (0.0, 0.0, -1e308), EZ, [(0.0, 0.0, 1e308)])
case("err_overflow_square", SP, Z, EZ, [(1e200, 0.0, 0.0)])
case("err_overflow_doubling", TP, Z, EZ, [(0.0, 0.0, 1e308)])
case("err_overflow_offset", OF(-1e308), Z, EZ, [Z], [1e308])
case("big_but_finite", TP, Z, EZ, [(0.0, 0.0, 8e307)], note="2 * 8e307 is finite")


EPS = Fraction(1, 2**53)


def bound(shape, pose, points, deviations):
    """The Design's accuracy bound B on the size, as an exact Fraction (upper bound)."""
    if shape["kind"] == "offset":
        c = fr(shape["shift"])
        return 2 * EPS * max(abs(fr(d) - c) for d in deviations)
    o = pose[0]
    r2 = max(sum((fr(p[i]) - fr(o[i])) ** 2 for i in range(3)) for p in points)
    r_up = fr(exact_sqrt_to_f64(r2)) * (1 + Fraction(1, 2**40))  # >= sqrt(r2)
    return 32 * EPS * r_up + Fraction(1, 2**535)


def within(shape, pose, points, deviations, got):
    """|got - S| <= B, decided exactly (S = exact usage size, possibly irrational)."""
    b = bound(shape, pose, points, deviations)
    g = fr(got)
    kind = shape["kind"]
    if kind == "offset":
        c = fr(shape["shift"])
        s = 2 * max(abs(fr(d) - c) for d in deviations)
        return abs(g - s) <= b
    best = max(exact_dist2(shape, pose, p) for p in points)
    if kind in ("two_planes", "two_lines"):
        return abs(g - 2 * best) <= b
    lo = g - b
    lo2 = lo * lo if lo > 0 else Fraction(0)
    return lo2 <= 4 * best <= (g + b) * (g + b)


def enc_vec(v):
    return [hx(x) for x in v]


def build_analytic():
    out = []
    for c in CASES:
        shape = c["shape"]
        o, n = c["pose"]
        got = f64_usage(shape, (o, n), c["points"], c["deviations"])
        sj = {"kind": shape["kind"]}
        if "plane" in shape:
            sj["plane"] = enc_vec(shape["plane"])
        if "shift" in shape:
            sj["disposition_shift"] = hx(shape["shift"])
        rec = {
            "name": c["name"],
            "shape": sj,
            "pose": {"origin": enc_vec(o), "direction": enc_vec(n)},
            "points": [enc_vec(p) for p in c["points"]],
        }
        if c["deviations"] is not None:
            rec["deviations"] = [hx(d) for d in c["deviations"]]
        if c["note"]:
            rec["note"] = c["note"]
        if isinstance(got, str):
            rec["expect"] = {"error": got}
        else:
            size, idx, exact = exact_usage(shape, (o, n), c["points"], c["deviations"])
            # Self-checks: the exact arg-max is the f64 arg-max; the pinned f64 expressions
            # satisfy the Design's absolute bound; and, by class, land on the exact value
            # (bits) or within 1 ulp of it (ulp1).
            assert got[1] == idx, (c["name"], got, idx)
            assert within(shape, (o, n), c["points"], c["deviations"], got[0]), c["name"]
            if c["hard"]:
                check = "abs_bound"
            elif exact:
                check = "bits"
                assert got[0] == size, (c["name"], got[0], size)
            else:
                check = "ulp1"
                assert ulp_diff(got[0], size) <= 1, (c["name"], got[0], size)
            b = float(bound(shape, (o, n), c["points"], c["deviations"]))
            tol = b * (1 + 2.0**-40) + math.ulp(size)
            rec["expect"] = {
                "size": hx(size),
                "size_dec": repr(size),
                "got_pinned_dec": repr(got[0]),
                "index": idx,
                "check": check,
                "abs_tol": hx(tol),
            }
        out.append(rec)
    return out


# ----------------------------------------------------------------- AC-5 random set

M0, M1 = 0xD2511F53, 0xCD9E8D57
W0, W1 = 0x9E3779B9, 0xBB67AE85
MASK = 0xFFFFFFFF


def philox(ctr, key):
    c0, c1, c2, c3 = ctr
    k0, k1 = key
    for r in range(10):
        if r:
            k0, k1 = (k0 + W0) & MASK, (k1 + W1) & MASK
        p0, p1 = M0 * c0, M1 * c2
        hi0, lo0, hi1, lo1 = p0 >> 32, p0 & MASK, p1 >> 32, p1 & MASK
        c0, c1, c2, c3 = hi1 ^ c1 ^ k0, lo1, hi0 ^ c3 ^ k1, lo0
    return (c0, c1, c2, c3)


# Published Random123 known answer for philox4x32_10 (kat_vectors), as a self-check.
assert philox((0, 0, 0, 0), (0, 0)) == (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)

SEED = 0x5EED
N_RANDOM = 1_000_000
CHUNK = 1_000


def uniform(seed, stream, index):
    w = philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))
    return (((w[1] << 32) | w[0]) >> 11) * 2.0**-53


RANDOM_POSE = ((0.1, -0.2, 0.3), D2367)
RANDOM_SHAPES = [TP, TL(P6237), CY, CI(P6237), SP, OF(0.1)]


def build_random():
    pts = []
    for i in range(N_RANDOM):
        pts.append(tuple(2.0 * uniform(SEED, 0, 3 * i + k) - 1.0 for k in range(3)))
    devs = [2.0 * uniform(SEED, 1, i) - 1.0 for i in range(N_RANDOM)]
    o, n = RANDOM_POSE
    shapes_out = []
    for shape in RANDOM_SHAPES:
        kind = shape["kind"]
        if kind == "offset":
            c = shape["shift"]
            ds = [abs(d - c) for d in devs]
        else:
            ds = [f64_dist(kind, n, shape.get("plane"), o, p) for p in pts]
        best, idx = f64_max(ds)
        size = 2.0 * best
        # Cross-check the pinned binary64 result with the exact maximum over the whole set.
        sub_d = None
        if kind == "offset":
            e = 2.0**-53 * best * 2
        else:
            r_max = max(math.sqrt(dot3(*(2 * [(p[0] - o[0], p[1] - o[1], p[2] - o[2])]))) for p in pts)
            e = 16 * 2.0**-53 * r_max * 1.01 + 2.0**-536
        cand = [i for i in range(N_RANDOM) if ds[i] >= best - 8 * e]
        assert idx in cand
        c_pts = [pts[i] for i in cand]
        if kind == "offset":
            sub_d = [devs[i] for i in cand]
        ex_size, ex_idx, _ = exact_usage(shape, (o, n), c_pts, sub_d)
        assert within(shape, (o, n), c_pts, sub_d, size), kind
        assert ulp_diff(size, ex_size) <= 4, (kind, size, ex_size)
        print(f"{kind}: {len(cand)} candidates, exact arg-max {cand[ex_idx]}, pinned {idx}, "
              f"{ulp_diff(size, ex_size)} ulp", file=sys.stderr)
        # And with the full pinned path (validation included).
        full = f64_usage(shape, (o, n), pts, devs if kind == "offset" else None)
        assert full == (size, idx), (kind, full)
        h = hashlib.sha256()
        for k in range(0, N_RANDOM, CHUNK):
            b, j = f64_max(ds[k : k + CHUNK])
            h.update(struct.pack("<QI", struct.unpack("<Q", struct.pack("<d", 2.0 * b))[0], j))
        sj = {"kind": kind}
        if "plane" in shape:
            sj["plane"] = enc_vec(shape["plane"])
        if "shift" in shape:
            sj["disposition_shift"] = hx(shape["shift"])
        shapes_out.append(
            {"shape": sj, "size": hx(size), "size_dec": repr(size), "index": idx, "chunk_sha256": h.hexdigest()}
        )
    first = [enc_vec(p) for p in pts[:2]]
    return {
        "seed": "0x%x" % SEED,
        "count": N_RANDOM,
        "chunk": CHUNK,
        "pose": {"origin": enc_vec(o), "direction": enc_vec(n)},
        "first_points": first,
        "first_deviations": [hx(d) for d in devs[:2]],
        "shapes": shapes_out,
    }


# ----------------------------------------------------------------- output


def emit(name, obj):
    text = json.dumps(obj, indent=1, sort_keys=True) + "\n"
    path = HERE / name
    if CHECK:
        if not path.exists() or path.read_text(encoding="utf-8") != text:
            STALE.append(name)
    else:
        path.write_text(text, encoding="utf-8", newline="\n")


def main():
    for d in (D2367, P6237, D68):
        assert abs(norm(d) - 1.0) <= UNIT_TOL
    assert abs(dot3(D2367, P6237)) <= IN_PLANE_TOL
    emit(
        "shapes_analytic.json",
        {
            "leaf": "GDT.ZONE.ZTYPE",
            "generator": "fixtures/gdt/zone/make_shapes_fixtures.py",
            "licence": "CC0-1.0",
            "cases": build_analytic(),
        },
    )
    if "--analytic-only" not in sys.argv[1:]:
        rnd = build_random()
        rnd.update({"leaf": "GDT.ZONE.ZTYPE", "generator": "fixtures/gdt/zone/make_shapes_fixtures.py",
                    "licence": "CC0-1.0"})
        emit("shapes_random_v1.json", rnd)
    if STALE:
        print("stale:", ", ".join(STALE))
        sys.exit(1)


if __name__ == "__main__":
    main()
