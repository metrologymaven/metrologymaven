#!/usr/bin/env python3
"""Independent reference for KRN.LSQ.INIT (initial estimates for axial primitives) fixtures
(CC-BY-4.0 project data).

Writes fixtures/fit/init-axial-v1/cases.txt. Nothing here runs the Rust code: stdlib only, the
geometry is generated in Decimal at 60 significant digits (sin and cos by Taylor series, pi by
Machin's formula, square roots by the decimal module), never with the platform libm, and every
coordinate is rounded to the nearest binary64 once (float(Fraction) of the Decimal, correctly
rounded). The expected values are the planted geometry itself: the true axis direction (unit)
and a true point on the axis, so the Rust test measures the angle between the start axis and the
truth.

Cases (seeded with random.Random on the case name; units are SI metres):

- cyl / cone / torus, angular coverage 360, 180 and 90 degrees (around the axis for cylinder and
  cone, the major angle for the torus; the torus tube is always fully covered), 3 seeds each.
  Seed 0 puts the axis near the origin (offset up to 0.01 m), seed 1 about 0.3 m and seed 2
  about 1 m from it (far from the origin). The axis direction is a seeded random unit vector.
  - cylinder: radius 0.01 to 0.05 m, height range 0.5 to 3 radii, starting anywhere in
    [-1, 1] radius along the axis (so the mid-height is not the planted axis point);
  - cone: half-angle 10 to 40 degrees, radius at the planted point 0.01 to 0.04 m, heights in a
    range of 0.5 to 2 radii where the radius stays positive;
  - torus: major radius 0.03 to 0.06 m, minor radius 0.005 to 0.015 m.
  200 points per case. Noise: each point moves along the surface normal by a uniform amount in
  [-sqrt(3), sqrt(3)] micrometres (standard deviation 1 micrometre).
- cyl-normals-<seed>: full cylinders with the exact unit surface normal of each point (the
  radial direction, rounded to binary64), for AC-3.
- cyl-ring: a near-degenerate short cylinder (height 0.01 radius, full coverage), 0.3 m from the
  origin: nearly a planar ring; the axis is still determined.
- Error cases (`expect` code and fields): `few-4` (4 points: TooFewPoints n 4 need 5, AC-4),
  `coplanar` (points of a circle in the plane z = 0.125 exactly: rank 2, Coplanar), `collinear`
  (points on the x axis exactly: rank 1, Collinear).

Output lines (hex = 16 lowercase hex digits of an IEEE-754 double):
  case <name> kind <cyl|cone|torus> coverage <degrees>
  points <x y z per point, hex>
  normals <nx ny nz per point, hex>        (normals cases only)
  axis <hex hex hex>                       the true unit axis direction
  point <hex hex hex>                      a true point on the axis
  size <hex>                               radius (cyl), radius at `point` (cone), major radius (torus)
  expect start | <error code> <field=value ...>
  end
"""

import random
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

OUT = Path(__file__).with_name("init-axial-v1") / "cases.txt"
getcontext().prec = 60
N = 200


def dec(x):
    if isinstance(x, Decimal):
        return x
    x = Fraction(x)
    return Decimal(x.numerator) / Decimal(x.denominator)


def fl(x):
    return float(Fraction(x))


def hx(f):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", f))[0]


def machin_pi():
    def atan_inv(k):
        x = Decimal(1) / k
        x2 = x * x
        term, total, n = x, x, 1
        while True:
            term = -term * x2
            n += 2
            t = term / n
            if abs(t) < Decimal(10) ** -65:
                return total
            total += t

    return 16 * atan_inv(5) - 4 * atan_inv(239)


PI = machin_pi()


def sin_cos(t):
    t = dec(t)
    two_pi = 2 * PI
    t = t - two_pi * (t / two_pi).to_integral_value()
    s, c = Decimal(0), Decimal(0)
    term = Decimal(1)
    k = 0
    while True:
        if k % 2 == 0:
            c += term if k % 4 == 0 else -term
        else:
            s += term if k % 4 == 1 else -term
        k += 1
        term = term * t / k
        if abs(term) < Decimal(10) ** -65:
            return s, c


def tan(t):
    s, c = sin_cos(t)
    return s / c


def u01(rng):
    return Decimal(rng.getrandbits(52)) / Decimal(2**52)


def unit(v):
    n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    return [x / n for x in v]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def frame(rng):
    """A seeded random unit axis and two unit vectors completing an orthonormal frame."""
    while True:
        v = [2 * u01(rng) - 1 for _ in range(3)]
        n2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
        if Decimal("0.01") < n2 <= 1:
            break
    d = unit(v)
    k = min(range(3), key=lambda j: abs(d[j]))
    ek = [Decimal(int(j == k)) for j in range(3)]
    e1 = unit(cross(d, ek))
    e2 = cross(d, e1)
    return d, e1, e2


def offset(rng, seed):
    dist = [Decimal("0.01"), Decimal("0.3"), Decimal("1")][seed]
    v = unit([2 * u01(rng) - 1 for _ in range(3)])
    return [x * dist * (Decimal("0.9") + Decimal("0.2") * u01(rng)) for x in v]


def noise(rng):
    return (2 * u01(rng) - 1) * Decimal(3).sqrt() * Decimal("1e-6")


def at(p, d, e1, e2, t, a, b):
    """p + t d + a e1 + b e2."""
    return [p[k] + t * d[k] + a * e1[k] + b * e2[k] for k in range(3)]


def make(kind, cov, seed):
    name = f"{kind}-{cov}-{seed}"
    rng = random.Random(f"init-axial-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, seed)
    arc = dec(cov) * PI / 180
    phase = 2 * PI * u01(rng)
    pts = []
    if kind == "cyl":
        r = Decimal("0.01") + Decimal("0.04") * u01(rng)
        length = r * (Decimal("0.5") + Decimal("2.5") * u01(rng))
        t0 = r * (2 * u01(rng) - 1)
        size = r
        for _ in range(N):
            s, c = sin_cos(phase + arc * u01(rng))
            t = t0 + length * u01(rng)
            rr = r + noise(rng)
            pts.append(at(p0, d, e1, e2, t, rr * c, rr * s))
    elif kind == "cone":
        alpha = (Decimal(10) + Decimal(30) * u01(rng)) * PI / 180
        ta = tan(alpha)
        r0 = Decimal("0.01") + Decimal("0.03") * u01(rng)
        length = r0 * (Decimal("0.5") + Decimal("1.5") * u01(rng))
        # heights where the radius r0 + t tan(alpha) stays above r0 / 4
        lo = -(r0 * Decimal("0.75")) / ta
        t0 = lo + (-lo) * u01(rng) * Decimal("0.5")
        size = r0
        sa, ca = sin_cos(alpha)
        for _ in range(N):
            s, c = sin_cos(phase + arc * u01(rng))
            t = t0 + length * u01(rng)
            rad = r0 + t * ta
            # normal noise: move along the surface normal (ca * radial - sa * axial)
            e = noise(rng)
            pts.append(at(p0, d, e1, e2, t - e * sa, (rad + e * ca) * c, (rad + e * ca) * s))
    else:
        big = Decimal("0.03") + Decimal("0.03") * u01(rng)
        small = Decimal("0.005") + Decimal("0.01") * u01(rng)
        size = big
        for _ in range(N):
            s, c = sin_cos(phase + arc * u01(rng))
            ps, pc = sin_cos(2 * PI * u01(rng))
            rr = small + noise(rng)
            rad = big + rr * pc
            pts.append(at(p0, d, e1, e2, rr * ps, rad * c, rad * s))
    return name, kind, cov, pts, None, d, p0, size


def make_normals(seed):
    name = f"cyl-normals-{seed}"
    rng = random.Random(f"init-axial-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, seed)
    r = Decimal("0.01") + Decimal("0.04") * u01(rng)
    length = r * (Decimal("0.5") + Decimal("2.5") * u01(rng))
    pts, nrm = [], []
    for _ in range(N):
        s, c = sin_cos(2 * PI * u01(rng))
        t = length * u01(rng)
        rr = r + noise(rng)
        pts.append(at(p0, d, e1, e2, t, rr * c, rr * s))
        nrm.append([c * e1[k] + s * e2[k] for k in range(3)])
    return name, "cyl", 360, pts, nrm, d, p0, r


def make_ring():
    name = "cyl-ring"
    rng = random.Random(f"init-axial-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, 1)
    r = Decimal("0.03")
    pts = []
    for _ in range(N):
        s, c = sin_cos(2 * PI * u01(rng))
        t = r * Decimal("0.01") * u01(rng)
        rr = r + noise(rng)
        pts.append(at(p0, d, e1, e2, t, rr * c, rr * s))
    return name, "cyl", 360, pts, None, d, p0, r


def emit(out, name, kind, cov, pts, nrm, d, p0, size, expect="start"):
    out.append(f"case {name} kind {kind} coverage {cov}")
    out.append("points " + " ".join(hx(fl(x)) for p in pts for x in p))
    if nrm is not None:
        rounded = [[fl(x) for x in n] for n in nrm]
        for n in rounded:
            norm = sum(Fraction(x) ** 2 for x in n)
            assert abs(norm - 1) < Fraction(1, 10**12), norm
        out.append("normals " + " ".join(hx(x) for n in rounded for x in n))
    out.append("axis " + " ".join(hx(fl(x)) for x in d))
    out.append("point " + " ".join(hx(fl(x)) for x in p0))
    out.append(f"size {hx(fl(size))}")
    out.append(f"expect {expect}")
    out.append("end")


def render():
    out = [
        "# KRN.LSQ.INIT fixture init-axial-v1 (CC-BY-4.0). Generated by fixtures/fit/make_init_fixtures.py; do not edit.",
        "# Seeded partial cylinders, cones and tori (planted axes), normals cases, a near-planar ring and error cases.",
    ]
    for kind in ("cyl", "cone", "torus"):
        for cov in (360, 180, 90):
            for seed in range(3):
                emit(out, *make(kind, cov, seed))
    for seed in range(3):
        emit(out, *make_normals(seed))
    emit(out, *make_ring())
    zero3 = [Decimal(0)] * 3
    z = [Decimal(0), Decimal(0), Decimal(1)]
    # few-4: four points of a cylinder
    rng = random.Random("init-axial-v1/few-4")
    few = [[Decimal(i) / 8, u01(rng), u01(rng)] for i in range(4)]
    emit(out, "few-4", "cyl", 360, few, None, z, zero3, Decimal(1), "too_few_points n=4 need=5")
    # coplanar: a circle in z = 0.125
    cop = []
    for i in range(24):
        s, c = sin_cos(2 * PI * i / 24)
        cop.append([Decimal("0.02") * c, Decimal("0.02") * s, Decimal("0.125")])
    emit(out, "coplanar", "cyl", 360, cop, None, z, zero3, Decimal("0.02"), "coplanar n=24 rank=2")
    col = [[Decimal(i) / 64, Decimal(0), Decimal(0)] for i in range(24)]
    emit(out, "collinear", "cyl", 360, col, None, [Decimal(1), Decimal(0), Decimal(0)], zero3, Decimal(1),
         "collinear n=24 rank=1")
    return "\n".join(out) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        if not (OUT.exists() and OUT.read_text(encoding="utf-8") == text):
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT} is up to date")
        return 0
    OUT.parent.mkdir(exist_ok=True)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
