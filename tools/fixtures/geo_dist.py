#!/usr/bin/env python3
"""Independent reference for GEO.DIST.MESH; writes the fixtures of the leaf's acceptance criteria.

Run:  python tools/fixtures/geo_dist.py [--check] [--jobs N]

Without --check it rewrites
  fixtures/geo/dist/triangle_regions.json        (AC-2: 50 triangles x 7 Voronoi regions)
  fixtures/geo/dist/shared_vertex_ties/cases.txt (AC-4: queries equidistant from several triangles)
  fixtures/geo/dist/tri_fuzz/cases.txt           (fuzz: general, dyadic on-edge/vertex/plane,
                                                  slivers, needles, repeated and collinear
                                                  corners, large offsets, far queries)
  fixtures/geo/dist/mesh_sphere_1e5/expected.txt (AC-1, AC-6: 10^5 seeded queries on the
                                                  GEO.INDEX sphere mesh, result digest)
With --check nothing is written: the three small files are regenerated and compared byte for
byte, and for the sphere the header (mesh, query and sample lines) is regenerated and every
500th query is searched again and compared with its stored line. The full 10^5-query search
runs when the fixture is written (multiprocessing, --jobs defaults to the CPU count).

Stdlib only (struct, hashlib, math.sqrt, fractions, multiprocessing). It shares no code with
crates/fid-geo-dist. Two oracles:

* EXACT: rational arithmetic (fractions.Fraction) on the exact binary64 inputs. The closest
  point on the closed triangle (plane projection if it lies inside, else the closest point on
  the three closed edges), its squared distance, and the feature: the lowest-coded closed
  feature containing the exact foot (vertex 0 < 1 < 2 < edge 0 < 1 < 2 < interior).
* MODEL: a bit-exact float model of the pinned algorithm in crates/fid-geo-dist/src/mesh.rs
  (module docs) and spec/geo/dist/mesh/NODE.md. CPython floats are IEEE binary64 with
  round-to-nearest-even and never fuse a multiply-add, and only correctly rounded + - * / sqrt
  are used, so the bytes are the same on every platform.

The script asserts, for every case it writes, that the model agrees with the exact oracle:
the feature when every model operation is exact (dyadic inputs; see `exact_ops`), and the
distance within the case's stated tolerance. The Rust tests then check the crate against the
model bit for bit and against the exact values within the tolerance.
"""

import argparse
import hashlib
import json
import math
import os
import struct
import sys
from fractions import Fraction
from multiprocessing import Pool
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "fixtures" / "geo" / "dist"
OUT_REGIONS = OUT / "triangle_regions.json"
OUT_TIES = OUT / "shared_vertex_ties" / "cases.txt"
OUT_FUZZ = OUT / "tri_fuzz" / "cases.txt"
OUT_SPHERE = OUT / "mesh_sphere_1e5" / "expected.txt"
INDEX_SPHERE = ROOT / "fixtures" / "geo" / "index" / "mesh_sphere_1e5" / "expected.txt"

GATE = 2.0**-40  # DEGENERACY_GATE
N_SUB = 71
N_SPHERE_QUERIES = 100_000
SAMPLE_EVERY = 100
CHECK_EVERY = 500
SEED_SPHERE = 0x6765_6F2E_6469_7374  # "geo.dist"
SEED_FUZZ = 0x6765_6F2E_6469_7375
SEED_REGIONS = 0x6765_6F2E_6469_7376
FRAME = bytes(range(16))


def hx(x):
    return f"{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def hxs(xs):
    return " ".join(hx(x) for x in xs)


def le64(v):
    return struct.pack("<Q", v)


def fbits(x):
    return struct.pack("<d", x)


class SplitMix:
    def __init__(self, seed):
        self.s = seed

    def next(self):
        m = (1 << 64) - 1
        self.s = (self.s + 0x9E3779B97F4A7C15) & m
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & m
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & m
        return z ^ (z >> 31)

    def unit(self):
        return (self.next() >> 11) * 2.0**-53

    def uni(self, a, b):
        return a + (b - a) * self.unit()

    def below(self, n):
        return self.next() % n


# ------------------------------------------------------------------ float model (pinned)


def sub(x, y):
    return (x[0] - y[0], x[1] - y[1], x[2] - y[2])


def dot(x, y):
    return (x[0] * y[0] + x[1] * y[1]) + x[2] * y[2]


def cross(x, y):
    return (x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0])


def axpy(s, t, e):
    return (s[0] + t * e[0], s[1] + t * e[1], s[2] + t * e[2])


class Ops:
    """Records whether every division of one model run was exact (only when tracking)."""

    def __init__(self, track):
        self.track = track
        self.exact_div = True

    def div(self, x, y):
        q = x / y
        if self.track and Fraction(x) / Fraction(y) != Fraction(q):
            self.exact_div = False
        return q


def finish(p, a, b, c, foot, code):
    f = list(foot)
    for k in range(3):
        lo = a[k]
        hi = a[k]
        for v in (b[k], c[k]):
            if v < lo:
                lo = v
            if v > hi:
                hi = v
        if f[k] < lo:
            f[k] = lo
        elif f[k] > hi:
            f[k] = hi
    f = tuple(f)
    dp = sub(p, f)
    return (f, dot(dp, dp), code)


def segments(p, a, b, c, ops):
    best = None
    for k, (s, e, i_s, i_e) in enumerate(((a, b, 0, 1), (b, c, 1, 2), (c, a, 2, 0))):
        ev = sub(e, s)
        tn = dot(ev, sub(p, s))
        if tn <= 0.0:
            foot, code = s, i_s
        else:
            ee = dot(ev, ev)
            if tn >= ee:
                foot, code = e, i_e
            else:
                foot, code = axpy(s, ops.div(tn, ee), ev), 3 + k
        r = finish(p, a, b, c, foot, code)
        if best is None or r[1] < best[1] or (r[1] == best[1] and r[2] < best[2]):
            best = r
    return best


def voronoi(p, a, b, c, ab, ac, bc, n, ops):
    ap = sub(p, a)
    d1 = dot(ab, ap)
    d2 = dot(ac, ap)
    if d1 <= 0.0 and d2 <= 0.0:
        return a, 0
    bp = sub(p, b)
    d3 = dot(ab, bp)
    d4 = dot(ac, bp)
    if d3 >= 0.0 and d4 <= d3:
        return b, 1
    vc = dot(n, cross(ab, ap))
    if vc <= 0.0 and d1 >= 0.0 and d3 <= 0.0:
        den = d1 - d3
        if den <= 0.0:
            return None
        return axpy(a, ops.div(d1, den), ab), 3
    cp = sub(p, c)
    d5 = dot(ab, cp)
    d6 = dot(ac, cp)
    if d6 >= 0.0 and d5 <= d6:
        return c, 2
    vb = dot(n, cross(ap, ac))
    if vb <= 0.0 and d2 >= 0.0 and d6 <= 0.0:
        den = d2 - d6
        if den <= 0.0:
            return None
        return axpy(a, ops.div(d2, den), ac), 5
    va = dot(n, cross(bc, bp))
    e1 = d4 - d3
    e2 = d5 - d6
    if va <= 0.0 and e1 >= 0.0 and e2 >= 0.0:
        den = e1 + e2
        if den <= 0.0:
            return None
        return axpy(b, ops.div(e1, den), bc), 4
    if va <= 0.0 or vb <= 0.0 or vc <= 0.0:
        return None
    den = (va + vb) + vc
    v = ops.div(vb, den)
    w = ops.div(vc, den)
    return tuple((a[k] + ab[k] * v) + ac[k] * w for k in range(3)), 6


def gate_scale(ab, ac, bc):
    """max(|ab|^2 |ac|^2, |ab|^2 |bc|^2, |ac|^2 |bc|^2), in this order, by '>' (symmetric gate)."""
    la, lc, lb = dot(ab, ab), dot(ac, ac), dot(bc, bc)
    m = la * lc
    for x in (la * lb, lc * lb):
        if x > m:
            m = x
    return m


def model_tri(p, a, b, c, track=False):
    """(foot, dist2, feature code, gate_failed, all divisions exact, took the segment path)."""
    ops = Ops(track)
    ab = sub(b, a)
    ac = sub(c, a)
    bc = sub(c, b)
    n = cross(ab, ac)
    nn = dot(n, n)
    scale = gate_scale(ab, ac, bc)
    if nn <= GATE * scale:
        f, d2, code = segments(p, a, b, c, ops)
        return f, d2, code, True, ops.exact_div, True
    r = voronoi(p, a, b, c, ab, ac, bc, n, ops)
    if r is None:
        f, d2, code = segments(p, a, b, c, ops)
        return f, d2, code, False, ops.exact_div, True
    f, d2, code = finish(p, a, b, c, r[0], r[1])
    return f, d2, code, False, ops.exact_div, False


# ------------------------------------------------------------------ exact oracle


def F3(v):
    return tuple(Fraction(x) for x in v)


def on_segment(x, s, e):
    d = sub(e, s)
    if d == (0, 0, 0):
        return x == s
    w = sub(x, s)
    if cross(d, w) != (0, 0, 0):
        return False
    t = dot(w, d)
    return 0 <= t <= dot(d, d)


def exact_tri(p, a, b, c):
    """Exact (foot, dist2, feature code) of the closest point on the closed triangle."""
    p, a, b, c = F3(p), F3(a), F3(b), F3(c)
    ab, ac = sub(b, a), sub(c, a)
    n = cross(ab, ac)
    nn = dot(n, n)
    foot = None
    inside = False
    if nn != 0:
        t = dot(n, sub(p, a)) / nn
        q = tuple(p[k] - t * n[k] for k in range(3))
        wa = dot(n, cross(sub(c, b), sub(q, b)))
        wb = dot(n, cross(sub(a, c), sub(q, c)))
        wc = dot(n, cross(sub(b, a), sub(q, a)))
        if wa >= 0 and wb >= 0 and wc >= 0:
            foot, inside = q, True
    if foot is None:
        best = None
        for s, e in ((a, b), (b, c), (c, a)):
            d = sub(e, s)
            dd = dot(d, d)
            t = Fraction(0) if dd == 0 else dot(sub(p, s), d) / dd
            t = min(max(t, Fraction(0)), Fraction(1))
            x = tuple(s[k] + t * d[k] for k in range(3))
            dx = sub(p, x)
            d2 = dot(dx, dx)
            if best is None or d2 < best[1]:
                best = (x, d2)
        foot = best[0]
    dp = sub(p, foot)
    d2 = dot(dp, dp)
    corners = (a, b, c)
    code = None
    for i in range(3):
        if foot == corners[i]:
            code = i
            break
    if code is None:
        for k, (s, e) in enumerate(((a, b), (b, c), (c, a))):
            if on_segment(foot, s, e):
                code = 3 + k
                break
    if code is None:
        assert inside
        code = 6
    return foot, d2, code


def exact_point_tri_d2(x, a, b, c):
    return exact_tri(x, a, b, c)[1]


def fsqrt(fr):
    """Correctly rounded sqrt of a non-negative Fraction, as a float (for tolerances only)."""
    return math.sqrt(float(fr)) if fr else 0.0


# ------------------------------------------------------------------ helpers


def norm(v):
    return math.sqrt(dot(v, v))


def scale3(v, s):
    return (v[0] * s, v[1] * s, v[2] * s)


def add(x, y):
    return (x[0] + y[0], x[1] + y[1], x[2] + y[2])


def unit(v):
    ln = norm(v)
    return (v[0] / ln, v[1] / ln, v[2] / ln)


def size_of(a, b, c):
    return max(norm(sub(b, a)), norm(sub(c, b)), norm(sub(a, c)))


def sliver_sigma(a, b, c):
    """Exact sigma = |ab x ac|^2 / max(|ab|^2 |ac|^2, |ab|^2 |bc|^2, |ac|^2 |bc|^2): the squared sine
    of the triangle's smallest angle (0 for a degenerate triangle)."""
    A, B, C = F3(a), F3(b), F3(c)
    ab, ac, bc = sub(B, A), sub(C, A), sub(C, B)
    n = cross(ab, ac)
    la, lc, lb = dot(ab, ab), dot(ac, ac), dot(bc, bc)
    m = max(la * lc, la * lb, lc * lb)
    return Fraction(0) if m == 0 else dot(n, n) / m


def tol_for(p, a, b, c):
    """The pinned accuracy bound (mesh.rs module docs, NODE.md Design), in metres. With
    S = the largest |coordinate| of p, a, b, c, L = the longest edge and sigma as above:
      base = 2^-46 * max(S, L)                       (rounding of differences and of the foot)
      sigma == 0:            base                    (exactly degenerate: segment path)
      0 < sigma <= 2^-39:    base + 2^-19 * L        (segment path near the 2^-40 gate: the
                                                      triangle's width is at most 2^-20 L)
      sigma > 2^-39:         base + 2^-46 * L / sqrt(sigma)
                                                     (Voronoi path: the barycentric numerators
                                                      lose about u / sin(smallest angle))"""
    s = max(abs(x) for v in (p, a, b, c) for x in v)
    size = size_of(a, b, c)
    t = 2.0**-46 * max(s, size)
    sg = sliver_sigma(a, b, c)
    if sg == 0:
        return t
    if sg <= Fraction(2) ** -39:
        return t + 2.0**-19 * size
    return t + 2.0**-46 * size / math.sqrt(float(sg))


def check_case(p, a, b, c, dyadic):
    """Runs both oracles, asserts agreement, returns the record fields."""
    f, d2, code, gate_failed, exact_div, used_seg = model_tri(p, a, b, c, track=True)
    ef, ed2, ecode = exact_tri(p, a, b, c)
    tol = tol_for(p, a, b, c)
    dm = math.sqrt(d2)
    de = fsqrt(ed2)
    assert abs(dm - de) <= tol, ("distance", p, a, b, c, dm, de, tol)
    # Corner-order invariance: all six vertex orders stay within the bound of the exact value.
    # They also stay within the bound of each other.
    ds = []
    for q in ((a, b, c), (a, c, b), (b, a, c), (b, c, a), (c, a, b), (c, b, a)):
        dq = math.sqrt(model_tri(p, *q)[1])
        assert abs(dq - de) <= tol, ("corner order", p, q, dq, de, tol)
        ds.append(dq)
    assert max(ds) - min(ds) <= tol, ("corner orders disagree", p, a, b, c, ds, tol)
    # The model's foot lies on the triangle (within the tolerance).
    on = exact_point_tri_d2(f, a, b, c)
    assert on <= Fraction(tol) ** 2, ("foot off triangle", p, a, b, c, f)
    # Dyadic inputs keep + - * exact. The Voronoi path classifies with + - * only, so its
    # feature is exact; the segment path compares rounded distances, so its feature is exact
    # only when every division was exact as well.
    must = dyadic and (exact_div or not used_seg)
    if must:
        assert code == ecode, ("feature", p, a, b, c, code, ecode)
    if dyadic and exact_div:
        assert F3(f) == ef and Fraction(d2) == ed2, ("exact foot", p, a, b, c)
    return f, d2, code, ecode, ed2, tol, must


# ------------------------------------------------------------------ AC-2 regions


def well_shaped(g):
    while True:
        a, b, c = (tuple(g.uni(-1.0, 1.0) for _ in range(3)) for _ in range(3))
        ok = True
        for x, y, z in ((a, b, c), (b, c, a), (c, a, b)):
            u, v = sub(y, x), sub(z, x)
            cosang = dot(u, v) / (norm(u) * norm(v))
            if cosang > math.cos(math.radians(20.0)):
                ok = False
        if ok and size_of(a, b, c) > 0.5:
            return a, b, c


def region_query(g, a, b, c, region, nhat, size):
    corners = (a, b, c)
    h = g.uni(-1.0, 1.0) * size
    if region < 3:
        v = corners[region]
        o1, o2 = corners[(region + 1) % 3], corners[(region + 2) % 3]
        d = scale3(add(unit(sub(o1, v)), unit(sub(o2, v))), -1.0)
        s = g.uni(0.1, 1.0) * size
        return add(add(v, scale3(d, s)), scale3(nhat, h))
    if region < 6:
        k = region - 3
        s_, e_ = ((a, b), (b, c), (c, a))[k]
        opp = corners[(k + 2) % 3]
        t = g.uni(0.2, 0.8)
        base = add(s_, scale3(sub(e_, s_), t))
        m = unit(cross(sub(e_, s_), nhat))
        if dot(m, sub(opp, base)) > 0:
            m = scale3(m, -1.0)
        s = g.uni(0.1, 1.0) * size
        return add(add(base, scale3(m, s)), scale3(nhat, h))
    u, v = g.uni(0.1, 0.8), g.uni(0.1, 0.8)
    if u + v > 0.9:
        u, v = 0.9 - v * 0.5, v * 0.5
    w = 1.0 - u - v
    q = tuple(a[k] * w + b[k] * u + c[k] * v for k in range(3))
    return add(q, scale3(nhat, h))


def regions_text():
    g = SplitMix(SEED_REGIONS)
    tris = []
    worst = 0.0
    for _ in range(50):
        a, b, c = well_shaped(g)
        nhat = unit(cross(sub(b, a), sub(c, a)))
        size = size_of(a, b, c)
        qs = []
        for region in range(7):
            p = region_query(g, a, b, c, region, nhat, size)
            f, d2, code, gate_failed, _, _ = model_tri(p, a, b, c)
            ef, ed2, ecode = exact_tri(p, a, b, c)
            assert ecode == region, ("region construction", region, ecode)
            assert code == ecode and not gate_failed
            err = dot(sub(F3(f), ef), sub(F3(f), ef))
            lim = Fraction(1e-15 * size) ** 2
            assert err <= lim, ("AC-2 foot tolerance", p, a, b, c)
            worst = max(worst, math.sqrt(float(err)) / size)
            qs.append(
                {
                    "p": [hx(x) for x in p],
                    "feature": ecode,
                    "foot": [hx(float(x)) for x in ef],
                    "dist2": hx(float(ed2)),
                    "model": {"foot": [hx(x) for x in f], "dist2": hx(d2), "feature": code},
                }
            )
        tris.append(
            {"a": [hx(x) for x in a], "b": [hx(x) for x in b], "c": [hx(x) for x in c], "size": hx(size), "queries": qs}
        )
    doc = {
        "about": "GEO.DIST.MESH AC-2. Written by tools/fixtures/geo_dist.py; never edit by hand. CC-BY-4.0. "
        "Numbers are binary64 bit patterns in hex. feature: 0-2 vertex, 3-5 edge (a-b, b-c, c-a), 6 interior. "
        "foot and dist2: the exact rational values rounded to nearest; model: the bit-exact pinned algorithm. "
        f"Worst model foot error / triangle size: {worst:.3e} (limit 1e-15).",
        "triangles": tris,
    }
    return json.dumps(doc, indent=1, sort_keys=True) + "\n"


# ------------------------------------------------------------------ fuzz


def rnd3(g, lo, hi):
    return tuple(g.uni(lo, hi) for _ in range(3))


def dy(g, lo, hi):
    """A small dyadic: an integer in [lo, hi] times 1/2."""
    return float(lo + g.below(hi - lo + 1)) * 0.5


def dy3(g, lo=-8, hi=8):
    return (dy(g, lo, hi), dy(g, lo, hi), dy(g, lo, hi))


def fuzz_cases():
    g = SplitMix(SEED_FUZZ)
    out = []  # (kind, p, a, b, c, dyadic)

    for _ in range(300):  # general
        out.append(("general", rnd3(g, -2, 2), rnd3(g, -1, 1), rnd3(g, -1, 1), rnd3(g, -1, 1), False))

    for i in range(500):  # dyadic: queries on vertices, edges, plane, above them, anywhere
        a, b, c = dy3(g), dy3(g), dy3(g)
        n = cross(sub(b, a), sub(c, a))
        sel = i % 10
        if sel in (0, 1):
            q = (a, b, c)[g.below(3)]
        elif sel in (2, 3):
            s, e = ((a, b), (b, c), (c, a))[g.below(3)]
            q = tuple((s[k] + e[k]) * 0.5 for k in range(3))
        elif sel in (4, 5):
            q = tuple((a[k] + b[k]) * 0.25 + c[k] * 0.5 for k in range(3))
        elif sel == 6:
            q = tuple((a[k] * 3.0 + b[k] * 3.0 + c[k] * 2.0) * 0.125 for k in range(3))
        else:
            q = dy3(g, -12, 12)
        if sel in (1, 3, 5, 6):
            s = float(g.below(5) - 2) * 0.5
            q = add(q, scale3(n, s))
        out.append(("dyadic", q, a, b, c, True))

    for _ in range(150):  # in-plane: triangle and query in the plane z = z0
        z0 = g.uni(-1, 1)
        a, b, c = (g.uni(-1, 1), g.uni(-1, 1), z0), (g.uni(-1, 1), g.uni(-1, 1), z0), (g.uni(-1, 1), g.uni(-1, 1), z0)
        out.append(("plane", (g.uni(-1.5, 1.5), g.uni(-1.5, 1.5), z0), a, b, c, False))

    for i in range(400):  # slivers: c near the line a-b
        a, b = rnd3(g, -1, 1), rnd3(g, -1, 1)
        eps = (0.0, 1e-17, 1e-14, 1e-11, 1e-9, 1e-7, 2e-6, 1e-5, 1e-4, 1e-2)[i % 10]
        t = g.uni(-0.5, 1.5)
        perp = unit(cross(sub(b, a), rnd3(g, -1, 1)))
        c = add(add(a, scale3(sub(b, a), t)), scale3(perp, eps * norm(sub(b, a))))
        corners = [a, b, c]
        r = g.below(3)
        corners = corners[r:] + corners[:r]
        p = rnd3(g, -2, 2) if i % 3 else add(add(a, scale3(sub(b, a), g.uni(-0.2, 1.2))), scale3(perp, g.uni(-1, 1)))
        out.append(("sliver", p, corners[0], corners[1], corners[2], False))

    for i in range(600):  # thin triangles, shuffled corners (GEO.DIST.MESH review of #287)
        L = 10.0 ** g.uni(-3.0, 3.0)
        lo, hi = (-13.0, -10.0) if i < 400 else (-10.0, -2.0)
        eps = 10.0 ** g.uni(lo, hi)
        th = g.uni(0.0, 6.283185307179586)
        ct, st = math.cos(th), math.sin(th)
        off = tuple(g.uni(-1.0, 1.0) * 10.0 ** g.uni(0.0, 3.0) for _ in range(3))

        def rot(q):
            return (ct * q[0] - st * q[1] + off[0], st * q[0] + ct * q[1] + off[1], q[2] + off[2])

        tri = [(0.0, 0.0, 0.0), (L, 0.0, 0.0), (g.uni(-0.5, 1.5) * L, L * eps, 0.0)]
        k = g.below(6)
        tri = [tri[j] for j in ((0, 1, 2), (0, 2, 1), (1, 0, 2), (1, 2, 0), (2, 0, 1), (2, 1, 0))[k]]
        a, b, c = (rot(q) for q in tri)
        sel = i % 4
        if sel == 0:  # inside, in the plane
            u = g.uni(0.0, 1.0)
            v = g.uni(0.0, 1.0 - u)
            q = (tri[0][0] * (1 - u - v) + tri[1][0] * u + tri[2][0] * v, tri[0][1] * (1 - u - v) + tri[1][1] * u + tri[2][1] * v, 0.0)
        elif sel == 1:  # on an edge, in the plane
            s_, e_ = tri[g.below(3)], tri[g.below(3)]
            t = g.uni(0.0, 1.0)
            q = (s_[0] + t * (e_[0] - s_[0]), s_[1] + t * (e_[1] - s_[1]), 0.0)
        elif sel == 2:  # 1 m above a point of the triangle
            u = g.uni(0.0, 1.0)
            q = (tri[0][0] * (1 - u) + tri[2][0] * u, tri[0][1] * (1 - u) + tri[2][1] * u, 1.0)
        else:  # near the triangle, in or off the plane
            q = (g.uni(-0.2, 1.2) * L, g.uni(-3.0, 3.0) * L * eps, (0.0, 1.0, g.uni(-1.0, 1.0) * L)[g.below(3)])
        out.append(("thin", rot(q), a, b, c, False))

    # The reproduction from the review of #287: thin at b, exact distance 0.
    a = (3.7, 2.3, 0.7)
    u = (0.6, 0.8, 0.0)
    v = (-0.8, 0.6, 0.0)
    w = 1e-12
    b = tuple(a[j] + 100 * u[j] for j in range(3))
    c = tuple(a[j] + w * v[j] for j in range(3))
    p = tuple(a[j] + 40 * u[j] + 0.2 * w * v[j] for j in range(3))
    for q in ((a, b, c), (b, c, a), (c, a, b)):
        out.append(("repro287", p, q[0], q[1], q[2], False))

    for i in range(150):  # needles: two corners very close
        a, c = rnd3(g, -1, 1), rnd3(g, -1, 1)
        b = add(a, scale3(rnd3(g, -1, 1), (1e-9, 1e-13, 1e-16)[i % 3]))
        out.append(("needle", rnd3(g, -2, 2), a, b, c, False))

    for i in range(150):  # repeated corners
        a, b = rnd3(g, -1, 1), rnd3(g, -1, 1)
        tri = ((a, a, b), (a, b, a), (b, a, a), (a, a, a))[i % 4]
        out.append(("repeated", rnd3(g, -2, 2), tri[0], tri[1], tri[2], False))

    for i in range(150):  # exactly collinear, dyadic
        a = dy3(g)
        d = dy3(g, -3, 3)
        ks = [float(g.below(7) - 3) for _ in range(2)]
        b, c = add(a, scale3(d, ks[0])), add(a, scale3(d, ks[1]))
        q = dy3(g, -12, 12) if i % 2 else add(a, scale3(d, float(g.below(9) - 4) * 0.5))
        out.append(("collinear", q, a, b, c, True))

    for _ in range(150):  # small triangle far from the origin
        o = rnd3(g, -1e3, 1e3)
        a, b, c = (add(o, rnd3(g, -1e-3, 1e-3)) for _ in range(3))
        out.append(("offset", add(o, rnd3(g, -2e-3, 2e-3)), a, b, c, False))

    for i in range(150):  # far queries
        a, b, c = rnd3(g, -1, 1), rnd3(g, -1, 1), rnd3(g, -1, 1)
        r = (1e3, 1e5, 1e7)[i % 3]
        out.append(("far", rnd3(g, -r, r), a, b, c, False))
    return out


def fuzz_text():
    lines = [
        "# GEO.DIST.MESH point-to-triangle fuzz. Written by tools/fixtures/geo_dist.py; never edit by hand. CC-BY-4.0.",
        "# case <kind> <p xyz> <a xyz> <b xyz> <c xyz> <model foot xyz> <model dist2> <model feature>",
        "#      <exact feature> <exact dist2 rounded> <distance tolerance> <feature_must_match 0|1>",
        "# Numbers are binary64 bit patterns in hex. feature: 0-2 vertex, 3-5 edge, 6 interior.",
    ]
    for kind, p, a, b, c, dyadic in fuzz_cases():
        f, d2, code, ecode, ed2, tol, must = check_case(p, a, b, c, dyadic)
        lines.append(
            f"case {kind} {hxs(p)} {hxs(a)} {hxs(b)} {hxs(c)} {hxs(f)} {hx(d2)} {code} "
            f"{ecode} {hx(float(ed2))} {hx(tol)} {1 if must else 0}"
        )
    return "\n".join(lines) + "\n"


# ------------------------------------------------------------------ AC-4 ties


def ties_mesh():
    ring = [(2.0, 0.0, 0.0), (1.0, 2.0, 0.0), (-1.0, 2.0, 0.0), (-2.0, 0.0, 0.0), (-1.0, -2.0, 0.0), (1.0, -2.0, 0.0)]
    verts = [(0.0, 0.0, 0.0)] + ring
    tris = []
    for i in range(6):
        r0, r1 = 1 + i, 1 + (i + 1) % 6
        # Vary which corner holds the shared centre.
        tris.append(((0, r0, r1), (r0, r1, 0), (r1, 0, r0))[i % 3])
    # Stacked parallel triangles at z = +1 and z = -1 over x in [8, 12].
    base = len(verts)
    for z in (1.0, -1.0):
        verts += [(8.0, -2.0, z), (12.0, -2.0, z), (10.0, 2.0, z)]
    tris.append((base, base + 1, base + 2))
    tris.append((base + 3, base + 5, base + 4))
    # A duplicate of fan triangle 2 and a flipped copy of fan triangle 4.
    tris.append(tris[2])
    t4 = tris[4]
    tris.append((t4[0], t4[2], t4[1]))
    # Two coplanar squares split along shared diagonals at z = 0, x in [20, 24].
    b2 = len(verts)
    verts += [(20.0, 0.0, 0.0), (22.0, 0.0, 0.0), (24.0, 0.0, 0.0), (20.0, 2.0, 0.0), (22.0, 2.0, 0.0), (24.0, 2.0, 0.0)]
    tris += [(b2, b2 + 1, b2 + 4), (b2, b2 + 4, b2 + 3), (b2 + 1, b2 + 2, b2 + 5), (b2 + 1, b2 + 5, b2 + 4)]
    # Pinned permutation so that the lowest tied index is rarely the first one the
    # hierarchy visits.
    order = [11, 3, 13, 0, 9, 6, 12, 1, 10, 4, 7, 2, 8, 5]
    assert sorted(order) == list(range(len(tris)))
    return verts, [tris[i] for i in order]


def ties_queries():
    qs = []
    for h in (0.0, 0.5, -1.0, 3.0):
        qs.append((0.0, 0.0, h))  # above the shared centre vertex
        qs.append((1.0, 0.0, h))  # above the midpoint of the edge centre-(2,0,0)
        qs.append((0.5, 1.0, h))  # above the midpoint of the edge centre-(1,2,0)
        qs.append((10.0, 0.0, h * 0.25))  # between the stacked triangles
        qs.append((22.0, 1.0, h))  # shared vertex of the squares' diagonals
        qs.append((22.0, 0.5, h))  # on the shared edge x = 22
        qs.append((-1.0, -1.0, h))  # inside the duplicated/flipped region
    qs.append((3.0, 0.0, 0.0))  # outside the fan beyond the ring vertex (2,0,0)
    qs.append((-3.0, 0.0, 1.0))
    qs.append((0.0, 3.0, -2.0))  # outside, beyond the ring edge (1,2)-(-1,2)
    qs.append((22.0, -1.0, 0.0))  # below the squares' shared vertex (22,0)
    qs.append((26.0, 1.0, 0.5))
    return qs


def ties_text():
    verts, tris = ties_mesh()
    lines = [
        "# GEO.DIST.MESH AC-4 tie fixture. Written by tools/fixtures/geo_dist.py; never edit by hand. CC-BY-4.0.",
        "# All coordinates are small dyadics, so the pinned float arithmetic is exact and equal exact",
        "# distances stay equal. Expected: the lowest triangle index among the exact minimisers.",
        "# v <x y z>; t <i j k>; q <p xyz> <tri> <feature> <foot xyz> <dist2> <number of tied triangles>",
        "# Numbers are binary64 bit patterns in hex.",
    ]
    for v in verts:
        lines.append(f"v {hxs(v)}")
    for t in tris:
        lines.append(f"t {t[0]} {t[1]} {t[2]}")
    for p in ties_queries():
        ex = []
        for i, t in enumerate(tris):
            ex.append((exact_tri(p, verts[t[0]], verts[t[1]], verts[t[2]])[1], i))
        m = min(d for d, _ in ex)
        tied = [i for d, i in ex if d == m]
        best = None
        for i, t in enumerate(tris):
            f, d2, code, _, exact_div, _ = model_tri(p, verts[t[0]], verts[t[1]], verts[t[2]], track=True)
            if i in tied:
                # Every tied triangle's model run is exact, so the tie survives in floats.
                assert exact_div and Fraction(d2) == m, ("inexact tie", p, i)
            if best is None or d2 < best[1]:
                best = (f, d2, code, i)
        assert best[3] == tied[0], ("model breaks the exact tie", p)
        ef, ed2, ecode = exact_tri(p, *(verts[j] for j in tris[tied[0]]))
        assert ecode == best[2] and F3(best[0]) == ef and Fraction(best[1]) == ed2
        lines.append(f"q {hxs(p)} {tied[0]} {best[2]} {hxs(best[0])} {hx(best[1])} {len(tied)}")
    ntied = sum(1 for ln in lines if ln.startswith("q ") and int(ln.split(" ")[-1]) > 1)
    assert ntied >= 20, ntied
    return "\n".join(lines) + "\n"


# ------------------------------------------------------------------ AC-1 / AC-6 sphere

ICO = None


def sphere_mesh():
    phi = (1.0 + math.sqrt(5.0)) / 2.0
    v = [
        (-1.0, phi, 0.0), (1.0, phi, 0.0), (-1.0, -phi, 0.0), (1.0, -phi, 0.0),
        (0.0, -1.0, phi), (0.0, 1.0, phi), (0.0, -1.0, -phi), (0.0, 1.0, -phi),
        (phi, 0.0, -1.0), (phi, 0.0, 1.0), (-phi, 0.0, -1.0), (-phi, 0.0, 1.0),
    ]
    f = [
        (0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4),
        (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8),
        (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1),
    ]
    n = N_SUB
    fn = float(n)
    verts, tris = [], []
    for ia, ib, ic in f:
        A, B, C = v[ia], v[ib], v[ic]
        base = len(verts)
        for i in range(n + 1):
            for j in range(n + 1 - i):
                wa, wb, wc = float(n - i - j), float(i), float(j)
                p = [((A[k] * wa + B[k] * wb) + C[k] * wc) / fn for k in range(3)]
                ln = math.sqrt((p[0] * p[0] + p[1] * p[1]) + p[2] * p[2])
                verts.append((p[0] / ln, p[1] / ln, p[2] / ln))

        def idx(i, j):
            return base + i * (n + 1) - (i * (i - 1)) // 2 + j

        for i in range(n):
            for j in range(n - i):
                tris.append((idx(i, j), idx(i + 1, j), idx(i, j + 1)))
                if j < n - i - 1:
                    tris.append((idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)))
    return verts, tris


def mesh_digest(verts, tris):
    h = hashlib.sha256()
    h.update(le64(len(verts)))
    for v in verts:
        h.update(fbits(v[0]) + fbits(v[1]) + fbits(v[2]))
    h.update(le64(len(tris)))
    for t in tris:
        h.update(le64(t[0]) + le64(t[1]) + le64(t[2]))
    return h.hexdigest()


def sphere_queries(verts, tris):
    """Pinned stream: i % 50 == 0 a mesh vertex; i % 50 == 1 the float midpoint of a triangle
    edge; else a point at radius r in [0.95, 1.05] along a uniform cube direction."""
    g = SplitMix(SEED_SPHERE)
    qs = []
    for i in range(N_SPHERE_QUERIES):
        if i % 50 == 0:
            qs.append(verts[g.below(len(verts))])
        elif i % 50 == 1:
            t = tris[g.below(len(tris))]
            k = g.below(3)
            s, e = verts[t[k]], verts[t[(k + 1) % 3]]
            qs.append(tuple((s[j] + e[j]) * 0.5 for j in range(3)))
        else:
            d = (g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0))
            r = g.uni(0.95, 1.05)
            ln = math.sqrt((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2])
            s = r / ln
            qs.append((d[0] * s, d[1] * s, d[2] * s))
    return qs


def query_digest(qs):
    h = hashlib.sha256()
    for q in qs:
        h.update(fbits(q[0]) + fbits(q[1]) + fbits(q[2]))
    return h.hexdigest()


# Uniform grid over the triangle boxes, used only to find candidates faster than a scan. It
# is exact with respect to brute force: a triangle not yet seen after ring k lies outside the
# cube of cells within Chebyshev distance k of the query cell (registration is padded by one
# cell), so its true distance is at least (k - 2) * H; its computed dist2 is at least the
# rounded squared box distance, so the search stops only when ((k - 2) * H)^2 * (1 - 2^-30)
# exceeds the best dist2 found. Ties are resolved by triangle index over all candidates seen.
H = 0.02
X0 = -1.0 - 4 * H
NC = int(math.ceil((2.0 + 8 * H) / H)) + 1
_G = {}


def _cell(x):
    return int(math.floor((x - X0) / H))


def build_grid(verts, tris):
    grid = {}
    for t, (ia, ib, ic) in enumerate(tris):
        A, B, C = verts[ia], verts[ib], verts[ic]
        lo = [min(A[k], B[k], C[k]) for k in range(3)]
        hi = [max(A[k], B[k], C[k]) for k in range(3)]
        r = [range(max(0, _cell(lo[k]) - 1), min(NC - 1, _cell(hi[k]) + 1) + 1) for k in range(3)]
        for i in r[0]:
            for j in r[1]:
                for k in r[2]:
                    grid.setdefault((i, j, k), []).append(t)
    return grid


def _init(verts, tris):
    _G["v"] = verts
    _G["t"] = tris
    _G["g"] = build_grid(verts, tris)


def search(p):
    verts, tris, grid = _G["v"], _G["t"], _G["g"]
    c = [_cell(p[k]) for k in range(3)]
    seen = set()
    best = None
    k = 0
    while True:
        for i in range(c[0] - k, c[0] + k + 1):
            if i < 0 or i >= NC:
                continue
            for j in range(c[1] - k, c[1] + k + 1):
                if j < 0 or j >= NC:
                    continue
                for m in range(c[2] - k, c[2] + k + 1):
                    if m < 0 or m >= NC:
                        continue
                    if max(abs(i - c[0]), abs(j - c[1]), abs(m - c[2])) != k:
                        continue
                    for t in grid.get((i, j, m), ()):
                        if t in seen:
                            continue
                        seen.add(t)
                        ia, ib, ic = tris[t]
                        f, d2, code, _, _, _ = model_tri(p, verts[ia], verts[ib], verts[ic])
                        if best is None or d2 < best[1] or (d2 == best[1] and t < best[3]):
                            best = (f, d2, code, t)
        if best is not None and k >= 2:
            lb = ((k - 2) * H) ** 2 * (1.0 - 2.0**-30)
            if lb > best[1]:
                break
        if k > 3 * NC:
            break
        k += 1
    return best


def result_bytes(r):
    f, d2, code, t = r
    return le64(t) + bytes([code]) + fbits(f[0]) + fbits(f[1]) + fbits(f[2]) + fbits(d2) + fbits(math.sqrt(d2))


def result_line(i, r):
    f, d2, code, t = r
    return f"r {i} {t} {code} {hxs(f)} {hx(d2)} {hx(math.sqrt(d2))}"


def sphere_text(check, committed, jobs):
    verts, tris = sphere_mesh()
    md = mesh_digest(verts, tris)
    index_head = INDEX_SPHERE.read_text(encoding="utf-8")
    assert f"mesh_digest {md}" in index_head, "sphere mesh differs from GEO.INDEX's mesh_sphere_1e5"
    qs = sphere_queries(verts, tris)
    head = [
        "# GEO.DIST.MESH fixture mesh_sphere_1e5 (AC-1, AC-6). Written by tools/fixtures/geo_dist.py;",
        "# never edit by hand. CC-BY-4.0. Mesh: the GEO.INDEX sphere (fixtures/geo/index/mesh_sphere_1e5).",
        "# Queries: SplitMix64 seed 0x6765_6f2e_6469_7374; see sphere_queries() for the pinned stream.",
        "# Per query bytes: u64le tri, u8 feature, f64le foot xyz, f64le dist2, f64le dist.",
        "# result_digest = SHA-256 over the per-query bytes in query order (all 10^5 queries).",
        "# r <i> <tri> <feature> <foot xyz> <dist2> <dist> for every 100th query.",
        f"triangles {len(tris)}",
        f"vertices {len(verts)}",
        f"mesh_digest {md}",
        f"queries {len(qs)}",
        f"query_digest {query_digest(qs)}",
    ]
    if check:
        lines = committed.split("\n")
        if lines[: len(head)] != head:
            print("mesh_sphere_1e5: header differs", file=sys.stderr)
            return None
        stored = {}
        for ln in lines:
            f = ln.split(" ")
            if f[0] == "r":
                stored[int(f[1])] = ln
        sample = [i for i in range(0, N_SPHERE_QUERIES, CHECK_EVERY)]
        with Pool(jobs, _init, (verts, tris)) as pool:
            res = pool.map(search, [qs[i] for i in sample])
        for i, r in zip(sample, res):
            if stored.get(i) != result_line(i, r):
                print(f"mesh_sphere_1e5: query {i} differs", file=sys.stderr)
                return None
        return committed
    with Pool(jobs, _init, (verts, tris)) as pool:
        res = pool.map(search, qs, chunksize=200)
    h = hashlib.sha256()
    for r in res:
        h.update(result_bytes(r))
    body = [f"result_digest {h.hexdigest()}"]
    for i in range(0, N_SPHERE_QUERIES, SAMPLE_EVERY):
        body.append(result_line(i, res[i]))
    # Spot-check the model against the exact oracle on the sample.
    for i in range(0, N_SPHERE_QUERIES, SAMPLE_EVERY * 10):
        f, d2, code, t = res[i]
        a, b, c = (verts[j] for j in tris[t])
        de = fsqrt(exact_tri(qs[i], a, b, c)[1])
        assert abs(math.sqrt(d2) - de) <= tol_for(qs[i], a, b, c)
    return "\n".join(head + body) + "\n"


# ------------------------------------------------------------------ driver


def write_or_check(path, text, check, failures):
    if check:
        old = path.read_text(encoding="utf-8") if path.exists() else None
        if old != text:
            failures.append(str(path))
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    ap.add_argument("--skip-sphere", action="store_true", help="development aid: leave the sphere fixture alone")
    args = ap.parse_args()
    failures = []
    write_or_check(OUT_REGIONS, regions_text(), args.check, failures)
    write_or_check(OUT_TIES, ties_text(), args.check, failures)
    write_or_check(OUT_FUZZ, fuzz_text(), args.check, failures)
    if not args.skip_sphere:
        committed = OUT_SPHERE.read_text(encoding="utf-8") if args.check and OUT_SPHERE.exists() else ""
        text = sphere_text(args.check, committed, args.jobs)
        if text is None:
            failures.append(str(OUT_SPHERE))
        elif not args.check:
            write_or_check(OUT_SPHERE, text, False, failures)
    if failures:
        for f in failures:
            print("MISMATCH", f, file=sys.stderr)
        return 1
    print("ok" if args.check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
