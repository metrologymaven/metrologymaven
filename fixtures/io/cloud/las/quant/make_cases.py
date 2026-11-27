#!/usr/bin/env python3
"""Independent reference for IO.CLOUD.LAS.QUANT (crate fid-io-las, module quant).

Written separately from the Rust code, from the leaf's rules (spec/io/cloud/las/quant/NODE.md).
Every expected value is computed here with exact rational arithmetic (`fractions.Fraction`)
and rounded once to binary64 by CPython's correctly rounded integer division, so nothing
depends on the platform's libm or on any error-free-transformation code. Floats are written
as the 16 hex digits of their IEEE-754 bits; the file uses LF newlines only. No third-party
data; the random cases come from `random.Random(seed).getrandbits`, which is the same on
every platform. Fixtures are CC-BY-4.0 (project-generated).

Rules encoded here (the leaf's Design, plus the module's documented envelope):
- decode: x = offset + scale * X, the exact value rounded once to nearest-even; a zero
  result is +0.0.
- header envelope: each scale must be finite and in [2^-512, 2^512]; each offset finite with
  |offset| <= 2^480; else scale_invalid / offset_invalid naming the first failing axis
  (x, y, z order; for one axis the scale is checked before the offset).
- u_quant = RN(scale / RN(sqrt(12))); sqrt(12) is rounded here with integer square roots.
- export scale: the default 1e-7 per axis, or the caller's; per axis in x, y, z order a value
  that is not finite, or not in [2^-512, 2^512], is scale_invalid, a value above 1e-7 is
  scale_too_coarse.
- export points: first pass, in point order and x, y, z within a point: a NaN or infinity is
  non_finite{index, axis}; |c| > 2^480 is coordinate_outside_envelope{index, axis}. An empty
  set is empty_point_set.
- offset per axis: mid = min * 0.5 + max * 0.5 in binary64 (total order), k = round-half-even
  of RN(mid / scale), offset = RN(k * scale), a zero offset is +0.0.
- integers: X = the integer nearest to the exact (c - offset) / scale, ties to even (each tie
  counted per axis); X outside the i32 range is out_of_range{index, axis}, the first in point
  order and x, y, z order.
- report: max_err per axis = max over points of RN(|c - decode(X)|); bounds = min and max of
  the decoded coordinates per axis.

Run (stdlib only):
  python fixtures/io/cloud/las/quant/make_cases.py           # rewrite cases.txt
  python fixtures/io/cloud/las/quant/make_cases.py --check   # fail if cases.txt is stale
"""

import math
import random
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "cases.txt"

SCALE_MAX = Fraction(2) ** 512
SCALE_MIN = Fraction(1, 2**512)
COORD_MAX = Fraction(2) ** 480
MAX_WRITE_SCALE = 1e-7
I32_MIN = -(2**31)
I32_MAX = 2**31 - 1
AXES = "xyz"


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def h(x):
    return "%016x" % bits(x)


def from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def rn(q):
    """Fraction -> nearest binary64, ties to even (CPython int / int is correctly rounded)."""
    v = q.numerator / q.denominator
    return 0.0 if v == 0 else v


def is_finite(x):
    return not (math.isnan(x) or math.isinf(x))


def sqrt12():
    # sqrt(12) is in [2, 4): mantissa m in [2^52, 2^53), value m * 2^-51.
    n = 12 << 102
    r = math.isqrt(n)
    if n > r * r + r:  # sqrt(n) > r + 1/2 (no tie: 12 is not a square)
        r += 1
    assert 2**52 <= r < 2**53
    return Fraction(r, 2**51)


SQRT12 = sqrt12()


def decode(scale, offset, X):
    return rn(Fraction(offset) + Fraction(scale) * X)


def u_quant(scale):
    return rn(Fraction(scale) / Fraction(rn(SQRT12)))


def header_check(scale, offset):
    for a in range(3):
        s, o = scale[a], offset[a]
        if not is_finite(s) or not (SCALE_MIN <= Fraction(s) <= SCALE_MAX):
            return ("scale_invalid", AXES[a])
        if not is_finite(o) or abs(Fraction(o)) > COORD_MAX:
            return ("offset_invalid", AXES[a])
    return None


def export(points, scale_req):
    if scale_req is None:
        scale = [MAX_WRITE_SCALE] * 3
    else:
        scale = list(scale_req)
        for a in range(3):
            s = scale[a]
            if not is_finite(s) or not (SCALE_MIN <= Fraction(s) <= SCALE_MAX):
                return {"error": ("scale_invalid", a)}
            if s > MAX_WRITE_SCALE:
                return {"error": ("scale_too_coarse", a)}
    if not points:
        return {"error": ("empty_point_set",)}
    for i, p in enumerate(points):
        for a in range(3):
            if not is_finite(p[a]):
                return {"error": ("non_finite", i, a)}
            if abs(Fraction(p[a])) > COORD_MAX:
                return {"error": ("coordinate_outside_envelope", i, a)}
    offset = []
    for a in range(3):
        mn = min(p[a] for p in points)
        mx = max(p[a] for p in points)
        mid = mn * 0.5 + mx * 0.5
        k = round(mid / scale[a])
        o = float(k) * scale[a]
        offset.append(0.0 if o == 0 else o)
    xs = []
    ties = [0, 0, 0]
    for i, p in enumerate(points):
        row = []
        for a in range(3):
            q = (Fraction(p[a]) - Fraction(offset[a])) / Fraction(scale[a])
            X = round(q)  # Fraction rounds half to even
            if q - math.floor(q) == Fraction(1, 2):
                ties[a] += 1
            if not (I32_MIN <= X <= I32_MAX):
                return {"error": ("out_of_range", i, a)}
            row.append(X)
        xs.append(row)
    max_err = [0.0, 0.0, 0.0]
    lo = [None, None, None]
    hi = [None, None, None]
    for p, row in zip(points, xs):
        for a in range(3):
            d = decode(scale[a], offset[a], row[a])
            e = rn(abs(Fraction(p[a]) - Fraction(d)))
            if e > max_err[a]:
                max_err[a] = e
            lo[a] = d if lo[a] is None or d < lo[a] else lo[a]
            hi[a] = d if hi[a] is None or d > hi[a] else hi[a]
    return {"scale": scale, "offset": offset, "X": xs, "ties": ties, "max_err": max_err, "min": lo, "max": hi}


def decode_cases(lines):
    rng = random.Random(4207)
    cases = []
    # Ties and near-ties of the final rounding: 1 + k * 2^-53.
    for X in (1, 2, 3, 5, -1, -3, 7, 2**31 - 1, -(2**31)):
        cases.append((2.0**-53, 1.0, X))
    cases.append((2.0**-53 * (1 + 2.0**-52), 1.0, 1))
    cases.append((2.0**-53 * (1 - 2.0**-53), 1.0, 1))
    cases.append((2.0**-53 * (1 + 2.0**-52), 1.0, 3))
    # Cancellation: offset = -RN(scale * X), exact result is the product's rounding error.
    for sc, X in ((0.1, 3), (1e-7, 123456789), (0.001, -987654321), (1.0 / 3.0, 2**31 - 1)):
        cases.append((sc, -(sc * X), X))
    # Exact zero, signed zero offset, X = 0.
    cases += [(0.5, -1.0, 2), (0.001, -0.0, 0), (0.001, 0.0, 0), (1e-7, -0.0, 5)]
    # Envelope edges.
    cases += [(2.0**-512, 2.0**480, 2**31 - 1), (2.0**512, -(2.0**480), -(2**31)), (2.0**-512, 0.0, -1)]
    # Typical LAS scales and offsets.
    for sc in (0.01, 0.001, 0.0001, 1e-7, 0.00025):
        for off in (0.0, 500000.0, 4300000.0, -1234.5678):
            for _ in range(8):
                cases.append((sc, off, rng.getrandbits(32) - 2**31))
    # Random mantissas with offset and product of similar size, where the double rounding of
    # RN(offset + RN(scale * X)) often differs from the correct rounding; keep a mix.
    naive_wrong = 0
    random_cases = []
    while len(random_cases) < 1200:
        sc = from_bits((rng.getrandbits(52)) | ((1023 - 40 + rng.getrandbits(5)) << 52))
        X = rng.getrandbits(32) - 2**31
        if X == 0:
            continue
        p = Fraction(sc) * X
        e = rng.getrandbits(3) - 4  # offset within 2^-4 .. 2^3 of |product|
        off = rn(p * Fraction(2) ** e * (Fraction(rng.getrandbits(52), 2**52) + 1))
        if rng.getrandbits(1):
            off = -off
        exp = decode(sc, off, X)
        naive = off + sc * X
        naive = 0.0 if naive == 0 else naive
        if naive != exp:
            naive_wrong += 1
        random_cases.append((sc, off, X))
    cases += random_cases
    # The fixture must catch a double-rounding implementation.
    assert naive_wrong >= 50, naive_wrong
    for sc, off, X in cases:
        lines.append("D %s %s %d %s" % (h(sc), h(off), X, h(decode(sc, off, X))))
    return naive_wrong


def u_cases(lines):
    for sc in (0.001, 0.01, 1e-7, 0.0001, 2.0**-24, 1.0, 2.0**-512, 2.0**512):
        lines.append("U %s %s" % (h(sc), h(u_quant(sc))))
    # AC-2 anchor: 0.001 / sqrt(12) to 17 significant digits (printed by repr, exact here).
    lines.append("# u_quant(0.001) = %r" % u_quant(0.001))


def header_cases(lines):
    good = [0.001, 0.001, 0.001]
    off = [0.0, 0.0, 0.0]
    tests = [
        ("ok", good, off),
        ("ok_edges", [2.0**-512, 2.0**512, 1e-7], [2.0**480, -(2.0**480), 0.0]),
        ("scale_zero_y", [0.001, 0.0, 0.001], off),
        ("scale_negative_x", [-0.001, 0.001, 0.001], off),
        ("scale_nan_z", [0.001, 0.001, float("nan")], off),
        ("scale_inf_x", [float("inf"), 0.001, 0.001], off),
        ("scale_tiny_y", [0.001, 2.0**-513, 0.001], off),
        ("scale_huge_z", [0.001, 0.001, 2.0**513], off),
        ("offset_nan_y", good, [0.0, float("nan"), 0.0]),
        ("offset_inf_z", good, [0.0, 0.0, float("-inf")]),
        ("offset_huge_x", good, [-(2.0**481), 0.0, 0.0]),
        ("scale_before_offset_x", [0.0, 0.001, 0.001], [float("nan"), 0.0, 0.0]),
        ("offset_x_before_scale_y", good[:1] + [0.0, 0.001], [float("nan"), 0.0, 0.0]),
    ]
    for name, sc, of in tests:
        r = header_check(sc, of)
        res = "ok" if r is None else "%s %s" % r
        lines.append("H %s %s %s %s" % (name, " ".join(h(v) for v in sc), " ".join(h(v) for v in of), res))


def point_grid(rng, n, lo, span):
    pts = []
    for _ in range(n):
        pts.append(tuple(rn(Fraction(lo[a]) + Fraction(rng.getrandbits(53), 2**53) * span) for a in range(3)))
    return pts


def export_cases(lines):
    rng = random.Random(9011)
    q = 2.0**-24
    cases = [
        ("basic", None, [(0.0, 0.0, 0.0), (1.0, 2.0, 3.0), (0.123456789, -4.5, 7.25), (2.5, 1.0e-9, -3.0)]),
        ("ties_pow2", [q, q, q], [(-1.0, 0.0, 0.0), (1.0, 0.0, 0.0), (q / 2, 3 * q / 2, -q / 2),
                                   (3 * q / 2, -q / 2, 5 * q / 2), (-q / 2, -5 * q / 2, q / 2)]),
        ("edge_i32", [q, q, q], [((2**31 - 1) * q, 0.0, 0.0), (-(2**31) * q, 0.0, 0.0)]),
        ("ac5_10m_default", None, point_grid(rng, 300, (0.0, 0.0, 0.0), 10)),
        ("utm_like_default", None, point_grid(rng, 120, (500000.0, 4300000.0, 120.0), 10)),
        ("caller_scales", [1e-7, 5e-8, q], point_grid(rng, 80, (-3.0, 17.0, 0.5), 7)),
        ("negative_zero", None, [(-0.0, 0.0, -0.0), (0.0, -0.0, 0.0)]),
        ("single_point", None, [(123.456, -78.9, 0.001)]),
        # Errors.
        ("ac3_500m", None, [(50.0 * i, 0.0, 0.0) for i in range(11)]),
        ("out_of_range_y_later", None, [(0.0, 0.0, 0.0), (1.0, 1.0, 1.0), (2.0, 2.0, 2.0),
                                         (3.0, 500.0, 3.0), (4.0, 4.0, 900.0)]),
        ("edge_i32_over", [q, q, q], [(2**31 * q, 0.0, 0.0), (-(2**31 + 1) * q, 0.0, 0.0)]),
        ("ac4_coarse_x", [1e-6, 1e-7, 1e-7], [(0.0, 0.0, 0.0)]),
        ("coarse_y", [1e-7, 2e-7, 1e-7], [(0.0, 0.0, 0.0)]),
        ("scale_zero_z", [1e-7, 1e-7, 0.0], [(0.0, 0.0, 0.0)]),
        ("scale_nan_x", [float("nan"), 1e-7, 1e-7], [(0.0, 0.0, 0.0)]),
        ("empty", None, []),
        ("nan_point", None, [(0.0, 0.0, 0.0), (1.0, 1.0, 1.0), (2.0, 2.0, float("nan")), (900.0, 0.0, 0.0)]),
        ("inf_point", None, [(0.0, float("inf"), 0.0)]),
        ("huge_point", None, [(0.0, 0.0, 0.0), (0.0, -(2.0**481), 0.0), (2.0**600, 0.0, 0.0)]),
    ]
    for name, sc, pts in cases:
        r = export(pts, sc)
        mode = "default" if sc is None else " ".join(h(v) for v in sc)
        lines.append("EXPORT %s %d %s" % (name, len(pts), mode))
        for p in pts:
            lines.append("P %s %s %s" % (h(p[0]), h(p[1]), h(p[2])))
        if "error" in r:
            e = r["error"]
            if e[0] in ("scale_invalid", "scale_too_coarse"):
                lines.append("ERROR %s %s" % (e[0], AXES[e[1]]))
            elif e[0] == "empty_point_set":
                lines.append("ERROR empty_point_set")
            else:
                lines.append("ERROR %s %d %s" % (e[0], e[1], AXES[e[2]]))
        else:
            if name.startswith("ac5"):
                assert all(m <= 5e-8 for m in r["max_err"]), r["max_err"]
            lines.append("SCALE %s" % " ".join(h(v) for v in r["scale"]))
            lines.append("OFFSET %s" % " ".join(h(v) for v in r["offset"]))
            for row in r["X"]:
                lines.append("X %d %d %d" % tuple(row))
            lines.append("TIES %d %d %d" % tuple(r["ties"]))
            lines.append("MAXERR %s" % " ".join(h(v) for v in r["max_err"]))
            lines.append("BOUNDS %s %s" % (" ".join(h(v) for v in r["min"]), " ".join(h(v) for v in r["max"])))
        lines.append("END")


def build():
    lines = [
        "# IO.CLOUD.LAS.QUANT reference cases, written by make_cases.py (do not edit).",
        "# D scale offset X expected | U scale u_quant | H name scale*3 offset*3 result",
        "# EXPORT name n default|scale*3, P x y z, then SCALE/OFFSET/X/TIES/MAXERR/BOUNDS or ERROR, END",
        "# Floats are IEEE-754 binary64 bits in hex.",
    ]
    naive_wrong = decode_cases(lines)
    lines.append("# decode cases where RN(offset + RN(scale * X)) is wrong: %d" % naive_wrong)
    u_cases(lines)
    header_cases(lines)
    export_cases(lines)
    return "\n".join(lines) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        old = OUT.read_bytes() if OUT.exists() else b""
        if old != text.encode("ascii"):
            print("cases.txt DIFFERS from make_cases.py output", file=sys.stderr)
            return 1
        print("cases.txt up to date")
        return 0
    OUT.write_bytes(text.encode("ascii"))
    print("wrote", OUT)
    return 0


if __name__ == "__main__":
    sys.exit(main())
