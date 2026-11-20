#!/usr/bin/env python3
"""Independent reference for GEO.DIST.SIGN; writes the fixtures of the leaf's acceptance criteria.

Run:  python tools/fixtures/geo_dist_sign.py [--check]

Writes (or, with --check, regenerates and compares byte for byte):
  fixtures/geo/dist/sign_cube/expected.txt
      AC-1: 10^5 seeded points around fixtures/geo/types/mesh_cube (inside, outside, near faces,
      edges and corners, exactly on the surface), each labelled by the EXACT inside test of the
      unit cube, plus a bit-exact model result for every 50th point.
  fixtures/geo/dist/sign_open_flipped/expected.txt
      AC-3: 400 seeded points around fixtures/geo/types/mesh_open_flipped (one component with
      boundary and misoriented edges), each with its bit-exact model result; the script asserts
      every one is Undefined (or on the surface).
  fixtures/geo/dist/sphere_concave_convex/mesh.txt
      AC-2: a closed radial mesh of the analytic star-shaped surface |x| = r(x/|x|),
      r(u) = 1 + A (ux^4 + uy^4 + uz^4), A = 3/4 (an octahedron subdivided 16 times; it has
      both convex and concave edges, counted exactly below).
  fixtures/geo/dist/sphere_concave_convex/expected.txt
      AC-2: seeded points near mesh edges (offsets up to 2^-5..2^-7 m), EVERY point labelled
      with the EXACT side of the mesh (integer cone and orientation tests; O / I within 2^-30 m
      of the mesh, by exact distances) and of the analytic surface (rational |q|^2 < r^2). Points
      whose two sides agree are the AC-2 points proper (kept until there are 10 000); the rest lie
      in the mesh-approximation band and are checked against the mesh side only. Plus a
      bit-exact model result for every 16th point. Also a sweep of the fid-num atan2 model
      against a Decimal oracle (error <= 2^-49, the bound sign.rs assumes).

Stdlib only (struct, hashlib, fractions, decimal, math.isqrt, importlib). No platform libm: square roots
are correctly rounded from integer square roots, and atan2 is the bit-exact model of fid-num's
vendored atan2 (fixtures/num/make_fn_fixtures.py). Never runs the Rust code.

Two oracles:
* EXACT: Fraction / integer arithmetic on the binary64 inputs (labels). Shares nothing with the
  crate.
* MODEL: a bit-exact float model of the pinned rules in crates/fid-geo-dist/src/sign.rs (module
  docs): running error arithmetic, pseudo-normals, candidate analysis and verdict, on top of the
  bit-exact GEO.DIST.MESH model in tools/fixtures/geo_dist.py. CPython floats are IEEE binary64
  with round-to-nearest-even and never fuse a multiply-add, so the bytes are the same on every
  platform. The script asserts that every model sign agrees with the exact label (a certified
  sign is never wrong) and that the model gives no Undecidable outside the 2^-30 band.
"""

import argparse
import hashlib
import importlib.util
import struct
import sys
from decimal import localcontext
from fractions import Fraction
from math import isqrt
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "fixtures" / "geo" / "dist"
OUT_CUBE = OUT / "sign_cube" / "expected.txt"
OUT_FLIP = OUT / "sign_open_flipped" / "expected.txt"
OUT_SPH_MESH = OUT / "sphere_concave_convex" / "mesh.txt"
OUT_SPH = OUT / "sphere_concave_convex" / "expected.txt"
CUBE_MESH = ROOT / "fixtures" / "geo" / "types" / "mesh_cube.txt"
FLIP_MESH = ROOT / "fixtures" / "geo" / "types" / "mesh_open_flipped.txt"


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


FN = _load("make_fn_fixtures", "fixtures/num/make_fn_fixtures.py")
GD = _load("geo_dist", "tools/fixtures/geo_dist.py")

N_CUBE = 100_000
CUBE_SAMPLE = 50
SEED_CUBE = 0x6765_6F2E_7369_676E  # "geo.sign"
N_SPH_KEEP = 10_000
SPH_SAMPLE = 16
SEED_SPH = 0x6765_6F2E_7369_676F
N_FLIP = 400
SEED_FLIP = 0x6765_6F2E_7369_6770
SPH_N = 16
SPH_A = 0.75
BAND2 = Fraction(1, 2**60)  # exact squared distance below which a point is "near" (2^-30 m)


def fb(u):
    return struct.unpack("<d", struct.pack("<Q", u))[0]


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return f"{bits(x):016x}"


def pow2(k):
    return fb((k + 1023) << 52)


U52 = pow2(-52)
GROW = fb(0x3FF0_0000_0000_0010)
UP = fb(0x3FF0_0000_0000_0004)
DOWN = fb(0x3FEF_FFFF_FFFF_FFF8)
ETA = pow2(-1022)
ATAN2_ERROR = pow2(-49)
PI = fb(0x400921FB54442D18)
INF = float("inf")


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
        return (self.next() >> 11) * pow2(-53)

    def below(self, n):
        return self.next() % n


def crsqrt(x):
    """Correctly rounded square root of a finite float x >= 0, from integer square roots."""
    if x == 0.0:
        return x
    n, d = x.as_integer_ratio()  # d is a power of two
    e = -(d.bit_length() - 1)
    if e % 2:
        n <<= 1
        e -= 1
    big = n << 256
    s = isqrt(big)
    sticky = 0 if s * s == big else 1
    # sqrt(x) = sqrt(big) * 2^(e/2 - 128); s has >= 128 bits, the sticky bit breaks false ties.
    num = 2 * s + sticky
    sh = e // 2 - 128 - 1
    return float(Fraction(num) * (Fraction(2) ** sh))


# ------------------------------------------------------------------ running error arithmetic (model)


def grow(t):
    return t * GROW + ETA


def ex(v):
    return (v, 0.0)


def add(a, b):
    v = a[0] + b[0]
    return (v, grow((a[1] + b[1]) + abs(v) * U52))


def sub(a, b):
    v = a[0] - b[0]
    return (v, grow((a[1] + b[1]) + abs(v) * U52))


def mul(a, b):
    v = a[0] * b[0]
    return (v, grow(((abs(a[0]) * b[1] + abs(b[0]) * a[1]) + a[1] * b[1]) + abs(v) * U52))


def div(a, b):
    d = abs(b[0]) - b[1]
    if not d > 0.0:
        return None
    v = a[0] / b[0]
    return (v, grow((a[1] + abs(v) * b[1]) / d + abs(v) * U52))


def esqrt(a):
    if a[0] <= 0.0:
        return (0.0, grow(crsqrt(a[1])))
    r = crsqrt(a[0])
    x = a[1] / r
    y = crsqrt(a[1])
    m = x if x < y else y
    return (r, grow(m + r * U52))


def eabs(a):
    return (abs(a[0]), a[1])


def pos(a):
    return a[0] > a[1]


def neg(a):
    return -a[0] > a[1]


def hi(a):
    s = a[0] + a[1]
    return s * UP + ETA if s >= 0.0 else s * DOWN + ETA


def lo(a):
    s = a[0] - a[1]
    return s * DOWN - ETA if s >= 0.0 else s * UP - ETA


def ex3(p):
    return [ex(p[0]), ex(p[1]), ex(p[2])]


def vsub(x, y):
    return [sub(x[0], y[0]), sub(x[1], y[1]), sub(x[2], y[2])]


def vdot(x, y):
    return add(add(mul(x[0], y[0]), mul(x[1], y[1])), mul(x[2], y[2]))


def vcross(x, y):
    return [
        sub(mul(x[1], y[2]), mul(x[2], y[1])),
        sub(mul(x[2], y[0]), mul(x[0], y[2])),
        sub(mul(x[0], y[1]), mul(x[1], y[0])),
    ]


def norm(r):
    return esqrt(vdot(r, r))


def angle(x, y):
    delta = x[1] + y[1]
    rx = abs(x[0]) - x[1]
    ry = abs(y[0]) - y[1]
    r = rx if rx > ry else ry
    r = r * DOWN - ETA
    if not r > delta * UP:
        return None
    theta = FN.m_atan2(y[0], x[0])
    return (theta, grow((2.0 * delta) / r + ATAN2_ERROR))


# ------------------------------------------------------------------ pseudo-normals (model)

DEGEN, NONMAN, MISOR, NMVERT, BOUND = 0, 1, 2, 3, 4  # severity order; None is OK


def worse(a, b):
    if a is None:
        return b
    if b is None:
        return a
    return b if b < a else a


def tri_geo(c):
    a, b, cc = ex3(c[0]), ex3(c[1]), ex3(c[2])
    n = vcross(vsub(b, a), vsub(cc, a))
    ln = esqrt(vdot(n, n))
    if not pos(ln):
        return None
    nhat = []
    for k in range(3):
        q = div(n[k], ln)
        if q is None:
            return None
        nhat.append(q)
    pts = [a, b, cc]
    ang = []
    for i in range(3):
        q = pts[i]
        e1 = vsub(pts[(i + 1) % 3], q)
        e2 = vsub(pts[(i + 2) % 3], q)
        cr = vcross(e1, e2)
        th = angle(vdot(e1, e2), esqrt(vdot(cr, cr)))
        if th is None:
            return None
        ang.append(th)
    return nhat, ang


class Normals:
    """Model of PseudoNormals::build."""

    def __init__(self, verts, tris):
        nt = len(tris)
        self.verts, self.tris = verts, tris
        geo, tstat, half, incid = [], [], [], []
        for t, tri in enumerate(tris):
            c = [verts[i] for i in tri]
            if tri[0] == tri[1] or tri[1] == tri[2] or tri[0] == tri[2]:
                geo.append(None)
                tstat.append(DEGEN)
                for k, v in enumerate(tri):
                    if v not in tri[:k]:
                        incid.append((v, t))
                continue
            g = tri_geo(c)
            tstat.append(None if g is not None else DEGEN)
            geo.append(g)
            for a, b in ((tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])):
                half.append((a, b, 2 * t + 1) if a < b else (b, a, 2 * t))
            for v in tri:
                incid.append((v, t))
        half.sort()
        incid.sort()
        parent = list(range(nt))

        def find(x):
            while parent[x] != x:
                parent[x] = parent[parent[x]]
                x = parent[x]
            return x

        def union(a, b):
            ra, rb = find(a), find(b)
            if ra != rb:
                lo_, hi_ = (ra, rb) if ra < rb else (rb, ra)
                parent[hi_] = lo_

        runs = []
        i = 0
        while i < len(half):
            j = i
            while j < len(half) and half[j][:2] == half[i][:2]:
                j += 1
            runs.append(half[i:j])
            i = j
        edges = {}
        order = []
        for run in runs:
            if len(run) == 1:
                st = BOUND
            elif len(run) == 2:
                st = MISOR if (run[0][2] & 1) == (run[1][2] & 1) else None
            else:
                st = NONMAN
            t0 = run[0][2] >> 1
            t1 = run[1][2] >> 1 if len(run) > 1 else t0
            for h in run[1:]:
                union(t0, h[2] >> 1)
            key = run[0][:2]
            edges[key] = {"status": st, "tris": (t0, t1), "run": run}
            order.append(key)
        comp = [None] * nt
        for run in runs:
            if len(run) == 2 and (run[0][2] & 1) == (run[1][2] & 1):
                st = MISOR
            elif len(run) > 2:
                st = NONMAN
            else:
                st = None
            if st is not None:
                root = find(run[0][2] >> 1)
                comp[root] = worse(comp[root], st)
        for t in range(nt):
            if tstat[t] is None:
                tstat[t] = comp[find(t)]
        w = ex(PI)
        for key in order:
            ed = edges[key]
            st = ed["status"]
            for h in ed["run"]:
                st = worse(st, tstat[h[2] >> 1])
            ed["status"] = st
            ed["normal"] = None
            if st is None:
                g0, g1 = geo[ed["tris"][0]], geo[ed["tris"][1]]
                ed["normal"] = [add(mul(w, g0[0][k]), mul(w, g1[0][k])) for k in range(3)]
        self.edges = edges
        self.vstat = [BOUND] * len(verts)
        self.vnormal = [None] * len(verts)
        i = 0
        while i < len(incid):
            j = i
            while j < len(incid) and incid[j][0] == incid[i][0]:
                j += 1
            run = incid[i:j]
            i = j
            v = run[0][0]
            st = None
            for _, t in run:
                st = worse(st, tstat[t])
                tri = tris[t]
                if not (tri[0] == tri[1] or tri[1] == tri[2] or tri[0] == tri[2]):
                    for x in tri:
                        if x != v:
                            st = worse(st, edges[(min(v, x), max(v, x))]["status"])
            if st is None and not self.single_fan(v, run):
                st = NMVERT
            self.vstat[v] = st
            if st is None:
                acc = [ex(0.0)] * 3
                for _, t in run:
                    g = geo[t]
                    th = g[1][tris[t].index(v)]
                    acc = [add(acc[k], mul(th, g[0][k])) for k in range(3)]
                self.vnormal[v] = acc
        self.tstat = tstat

    def single_fan(self, v, run):
        k = len(run)
        t0 = run[0][1]
        cur = t0
        for step in range(1, k + 1):
            tri = self.tris[cur]
            i = tri.index(v)
            x = tri[(i + 1) % 3]
            ed = self.edges[(min(v, x), max(v, x))]
            cur = ed["tris"][1] if ed["tris"][0] == cur else ed["tris"][0]
            if cur == t0:
                return step == k
        return False


# ------------------------------------------------------------------ signed distance (model)


def tri_vals(p, c):
    pts = [ex3(c[0]), ex3(c[1]), ex3(c[2])]
    pe = ex3(p)
    n = vcross(vsub(pts[1], pts[0]), vsub(pts[2], pts[0]))
    w, ev, tn, ee, tm, side = [], [], [], [], [], []
    for k in range(3):
        s, e = pts[k], pts[(k + 1) % 3]
        ev.append(vsub(e, s))
        w.append(vsub(pe, s))
        tn.append(vdot(w[k], ev[k]))
        ee.append(vdot(ev[k], ev[k]))
        tm.append(sub(tn[k], ee[k]))
        side.append(vdot(n, vcross(ev[k], w[k])))
    return pts, n, w, ev, tn, ee, tm, side


def upper_on_triangle(p, c):
    pts, n, w, ev, tn, ee, tm, side = tri_vals(p, c)
    pe = ex3(p)
    best = INF

    def take(d):
        nonlocal best
        if d is not None:
            h = hi(d)
            if h < best:
                best = h

    for q in pts:
        take(norm(vsub(pe, q)))
    for k in range(3):
        if ee[k][0] > 0.0:
            t = tn[k][0] / ee[k][0]
            tau = ex(total_clamp01(t))
            take(norm([sub(w[k][j], mul(tau, ev[k][j])) for j in range(3)]))
    if all(s[0] >= s[1] for s in side):
        s = vdot(w[0], n)
        ln = norm(n)
        if ln is not None:
            take(div(eabs(s), ln))
    assert best != INF
    return best


def total_clamp01(t):
    """fid_num::f::total_clamp(t, 0.0, 1.0): IEEE totalOrder, so -0.0 becomes +0.0."""
    if t < 0.0 or (t == 0.0 and bits(t) >> 63):
        return 0.0
    if t > 1.0:
        return 1.0
    return t


def box_lb(p, c):
    d2 = 0.0
    for k in range(3):
        lo_, hi_ = c[0][k], c[0][k]
        for q in (c[1][k], c[2][k]):
            if q < lo_:
                lo_ = q
            if q > hi_:
                hi_ = q
        if p[k] < lo_:
            g = lo_ - p[k]
        elif p[k] > hi_:
            g = p[k] - hi_
        else:
            g = 0.0
        d2 = g * g if k == 0 else d2 + g * g
    return d2 * DOWN - ETA


def box_d2(p, c):
    d2 = 0.0
    for k in range(3):
        lo_, hi_ = c[0][k], c[0][k]
        for q in (c[1][k], c[2][k]):
            if q < lo_:
                lo_ = q
            if q > hi_:
                hi_ = q
        g = lo_ - p[k] if p[k] < lo_ else (p[k] - hi_ if p[k] > hi_ else 0.0)
        d2 = g * g if k == 0 else d2 + g * g
    return d2


class Verdict:
    def __init__(self):
        self.any = self.pos = self.neg = self.unsure = False
        self.undefined = None
        self.low = INF

    def survivor(self, status, decision, dist_lo):
        self.any = True
        self.undefined = worse(self.undefined, status)
        if decision is not None and pos(decision):
            self.pos = True
        elif decision is not None and neg(decision):
            self.neg = True
        else:
            self.unsure = True
        lo_ = dist_lo if dist_lo > 0.0 else 0.0
        if lo_ < self.low:
            self.low = lo_


def analyse(nm, p, t, upper, vd):
    tri = nm.tris[t]
    c = [nm.verts[i] for i in tri]
    status = nm.tstat[t]
    if status == DEGEN:
        lb = box_lb(p, c)
        lo_ = crsqrt(lb) * DOWN if lb > 0.0 else 0.0
        vd.survivor(status, None, lo_)
        return
    pts, n, w, ev, tn, ee, tm, side = tri_vals(p, c)
    pe = ex3(p)
    for i in range(3):
        prev = (i + 2) % 3
        if pos(tn[i]) or neg(tm[prev]):
            continue
        r = vsub(pe, pts[i])
        vs = nm.vstat[tri[i]]
        dist = norm(r)
        if lo(dist) > upper:
            continue
        decision = vdot(r, nm.vnormal[tri[i]]) if vs is None else None
        vd.survivor(vs, decision, lo(dist))
    for k in range(3):
        if pos(side[k]) or neg(tn[k]) or pos(tm[k]):
            continue
        a_, b_ = tri[k], tri[(k + 1) % 3]
        ed = nm.edges[(min(a_, b_), max(a_, b_))]
        tq = div(tn[k], ee[k])
        r = None if tq is None else [sub(w[k][j], mul(tq, ev[k][j])) for j in range(3)]
        dist = None if r is None else norm(r)
        if dist is not None and lo(dist) > upper:
            continue
        decision = vdot(r, ed["normal"]) if (r is not None and ed["status"] is None) else None
        vd.survivor(ed["status"], decision, 0.0 if dist is None else lo(dist))
    if not any(neg(s) for s in side):
        s = vdot(w[0], n)
        ln = norm(n)
        dist = div(eabs(s), ln)
        if not (dist is not None and lo(dist) > upper):
            vd.survivor(status, s, 0.0 if dist is None else lo(dist))


def round_up(x):
    return x * UP + ETA if x > 0.0 else 0.0


def mesh_hit(verts, tris, p, boxes=None):
    """GEO.DIST.MESH result (triangle, dist) by the bit-exact model of geo_dist.py; triangles in
    ascending order of the pinned box distance, stopping once it exceeds the best dist2 (the
    computed dist2 is never below it, see mesh.rs 'Why pruning loses nothing')."""
    order = sorted(range(len(tris)), key=lambda t: (box_d2(p, [verts[i] for i in tris[t]]), t))
    best = None
    for t in order:
        c = [verts[i] for i in tris[t]]
        if best is not None and box_d2(p, c) > best[0]:
            break
        d2 = GD.model_tri(p, c[0], c[1], c[2])[1]
        if best is None or d2 < best[0] or (d2 == best[0] and t < best[1]):
            best = (d2, t)
    return best[1], crsqrt(best[0])


def model_signed(nm, p):
    """(status code, distance, bound): model of SignTarget::signed_distance."""
    t_hit, d = mesh_hit(nm.verts, nm.tris, p)
    c = [nm.verts[i] for i in nm.tris[t_hit]]
    upper = upper_on_triangle(p, c)
    u2 = (upper * upper) * UP + ETA
    vd = Verdict()
    for t, tri in enumerate(nm.tris):
        if box_lb(p, [nm.verts[i] for i in tri]) > u2:
            continue
        analyse(nm, p, t, upper, vd)
    if d == 0.0:
        status = 10 + vd.undefined if vd.undefined is not None else 2
        d = 0.0
    elif not vd.any:
        status = 3
    elif vd.undefined is not None:
        status = 10 + vd.undefined
    elif vd.pos and not vd.neg and not vd.unsure:
        status = 0
    elif vd.neg and not vd.pos and not vd.unsure:
        status = 1
    else:
        status = 3
    low = vd.low if vd.any else 0.0
    a, b = round_up(upper - d), round_up(d - low)
    bound = a if a > b else b
    return status, (-d if status == 1 else d), bound


# ------------------------------------------------------------------ AC-1 cube


def read_cube(path=CUBE_MESH):
    verts, tris = [], []
    for ln in path.read_text(encoding="utf-8").splitlines():
        f = ln.split()
        if f and f[0] == "v":
            verts.append(tuple(float(x) for x in f[1:4]))
        elif f and f[0] == "t":
            tris.append(tuple(int(x) for x in f[1:4]))
    return verts, tris


def cube_points():
    """The pinned AC-1 point stream (mirrored in crates/fid-geo-dist/tests/sign.rs)."""
    g = SplitMix(SEED_CUBE)
    pts = []
    for _ in range(N_CUBE):
        kind = g.below(8)
        if kind == 0:
            p = [-0.5 + 2.0 * g.unit() for _ in range(3)]
        elif kind == 1:
            f = g.below(6)
            ax, side = f >> 1, float(f & 1)
            h = pow2(-(1 + g.below(40)))
            p = [g.unit() for _ in range(3)]
            p[ax] = side + (2.0 * g.unit() - 1.0) * h
        elif kind in (2, 7):
            e = g.below(12)
            ax, b = e % 3, e // 3
            o1, o2 = (ax + 1) % 3, (ax + 2) % 3
            h = pow2(-(1 + g.below(40 if kind == 2 else 6)))
            p = [0.0, 0.0, 0.0]
            p[ax] = g.unit()
            p[o1] = float(b & 1)
            p[o2] = float((b >> 1) & 1)
            for j in (o1, o2):
                p[j] = p[j] + (2.0 * g.unit() - 1.0) * h
        elif kind == 3:
            c = g.below(8)
            h = pow2(-(1 + g.below(40)))
            p = [float((c >> j) & 1) + (2.0 * g.unit() - 1.0) * h for j in range(3)]
        elif kind == 4:
            f = g.below(6)
            ax, side = f >> 1, float(f & 1)
            p = [(g.next() >> 44) * pow2(-20) for _ in range(3)]
            p[ax] = side
        elif kind == 5:
            p = [-8.0 + 17.0 * g.unit() for _ in range(3)]
        else:
            p = [0.25 + 0.5 * g.unit() for _ in range(3)]
        pts.append(tuple(p))
    return pts


def cube_label(p):
    if all(0.0 < x < 1.0 for x in p):
        m = min(min(Fraction(x), 1 - Fraction(x)) for x in p)
        return "I" if m * m < BAND2 else "i"
    if any(x < 0.0 or x > 1.0 for x in p):
        d2 = Fraction(0)
        for x in p:
            fx = Fraction(x)
            if fx < 0:
                d2 += fx * fx
            elif fx > 1:
                d2 += (fx - 1) * (fx - 1)
        return "O" if d2 < BAND2 else "o"
    return "z"


def points_digest(pts):
    h = hashlib.sha256()
    for p in pts:
        for x in p:
            h.update(struct.pack("<d", x))
    return h.hexdigest()


def check_model(label, res, where):
    status = res[0]
    want = {"o": 0, "O": 0, "+": 0, "i": 1, "I": 1, "-": 1, "z": 2}[label]
    if label in "oi+-":
        assert status == want, f"{where}: model status {status}, exact label {label}"
    elif label in "OI":
        assert status in (want, 3), f"{where}: model status {status}, exact label {label}"
    else:
        assert status in (2, 3) and bits(res[1]) == bits(abs(res[1])), f"{where}: on-surface point gave {res}"


def chunk_lines(labels):
    return [f"l {labels[i:i + 100]}" for i in range(0, len(labels), 100)]


def cube_text():
    verts, tris = read_cube()
    nm = Normals(verts, tris)
    pts = cube_points()
    labels = "".join(cube_label(p) for p in pts)
    counts = {c: labels.count(c) for c in "oOiIz"}
    lines = [
        "# GEO.DIST.SIGN AC-1 (CC-BY-4.0). Written by tools/fixtures/geo_dist_sign.py; do not edit.",
        "# Mesh: fixtures/geo/types/mesh_cube.txt. Points: the pinned stream of cube_points().",
        "# Labels by the exact inside test of the unit cube: o outside, i inside, O/I the same within",
        "# 2^-30 m of the surface (a certified sign or Undecidable), z exactly on the surface.",
        "# s <index> <status code> <signed distance bits> <bound bits>: bit-exact model results.",
        f"seed {SEED_CUBE:#018x}",
        f"count {N_CUBE}",
        f"points_sha256 {points_digest(pts)}",
        "counts " + " ".join(f"{c}={counts[c]}" for c in "oOiIz"),
    ]
    lines += chunk_lines(labels)
    for i in range(0, N_CUBE, CUBE_SAMPLE):
        r = model_signed(nm, pts[i])
        check_model(labels[i], r, f"cube point {i}")
        lines.append(f"s {i} {r[0]} {hx(r[1])} {hx(r[2])}")
    return "\n".join(lines) + "\n", counts


# ------------------------------------------------------------------ AC-3 open, flipped mesh


def flip_text():
    verts, tris = read_cube(FLIP_MESH)
    nm = Normals(verts, tris)
    g = SplitMix(SEED_FLIP)
    pts = [tuple(-0.5 + 2.0 * g.unit() for _ in range(3)) for _ in range(N_FLIP)]
    lines = [
        "# GEO.DIST.SIGN AC-3 (CC-BY-4.0). Written by tools/fixtures/geo_dist_sign.py; do not edit.",
        "# Mesh: fixtures/geo/types/mesh_open_flipped.txt. Points: SplitMix(seed), 3 x unit() per point",
        "# mapped to -0.5 + 2 u. s <index> <status code> <distance bits> <bound bits>: bit-exact model.",
        f"seed {SEED_FLIP:#018x}",
        f"count {N_FLIP}",
        f"points_sha256 {points_digest(pts)}",
    ]
    for i, p in enumerate(pts):
        r = model_signed(nm, p)
        assert r[0] >= 10 or r[0] == 2, f"open flipped point {i}: {r}"
        lines.append(f"s {i} {r[0]} {hx(r[1])} {hx(r[2])}")
    return "\n".join(lines) + "\n"


# ------------------------------------------------------------------ AC-2 star-shaped mesh


def det3(a, b, c):
    return (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])) + a[2] * (
        b[0] * c[1] - b[1] * c[0]
    )


def fsub(x, y):
    return (x[0] - y[0], x[1] - y[1], x[2] - y[2])


def sphere_mesh():
    n = SPH_N
    index = {}
    verts = []
    tris = []

    def vid(d):
        if d not in index:
            i2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
            nr = crsqrt(float(i2))
            u = (d[0] / nr, d[1] / nr, d[2] / nr)
            f = ((u[0] * u[0]) * (u[0] * u[0]) + (u[1] * u[1]) * (u[1] * u[1])) + (u[2] * u[2]) * (u[2] * u[2])
            r = 1.0 + SPH_A * f
            index[d] = len(verts)
            verts.append((r * u[0], r * u[1], r * u[2]))
        return index[d]

    for sx in (1, -1):
        for sy in (1, -1):
            for sz in (1, -1):

                def pt(i, j):
                    return vid((sx * i, sy * j, sz * (n - i - j)))

                for i in range(n):
                    for j in range(n - i):
                        faces = [(pt(i, j), pt(i + 1, j), pt(i, j + 1))]
                        if j < n - i - 1:
                            faces.append((pt(i + 1, j), pt(i + 1, j + 1), pt(i, j + 1)))
                        for a, b, c in faces:
                            fa, fb_, fc = (tuple(Fraction(x) for x in verts[k]) for k in (a, b, c))
                            dt = det3(fa, fb_, fc)
                            assert dt != 0
                            tris.append((a, b, c) if dt > 0 else (a, c, b))
    return verts, tris


def mesh_text(verts, tris):
    lines = [
        "# GEO.DIST.SIGN AC-2 mesh (CC-BY-4.0). Written by tools/fixtures/geo_dist_sign.py; do not edit.",
        f"# Radial mesh of |x| = r(x/|x|), r(u) = 1 + {SPH_A} (ux^4 + uy^4 + uz^4): octahedron grid n = {SPH_N},",
        "# directions normalised with a correctly rounded sqrt. Closed, oriented outward (counter-clockwise",
        "# seen from outside). Vertices `v x y z` as binary64 bits (hex), triangles `t a b c`.",
        "frame 0f1e2d3c4b5a69788796a5b4c3d2e1f0",
    ]
    lines += [f"v {hx(v[0])} {hx(v[1])} {hx(v[2])}" for v in verts]
    lines += [f"t {a} {b} {c}" for a, b, c in tris]
    return "\n".join(lines) + "\n"


def sorted_edges(tris):
    es = set()
    for a, b, c in tris:
        for x, y in ((a, b), (b, c), (c, a)):
            es.add((min(x, y), max(x, y)))
    return sorted(es)


SCALE = 1100  # every binary64 value is an integer multiple of 2^-1074


def scaled(x):
    """x * 2^SCALE as an exact integer (orientation signs are invariant under the scaling)."""
    n, d = x.as_integer_ratio()
    return n << (SCALE - (d.bit_length() - 1))


class Exact:
    """Exact inside tests of the radial mesh (integer arithmetic) and of the analytic surface."""

    def __init__(self, verts, tris):
        self.fv = [tuple(scaled(x) for x in v) for v in verts]
        self.tris = tris
        self.inc = {}
        for t, tri in enumerate(tris):
            for v in tri:
                self.inc.setdefault(v, []).append(t)
        for a, b, c in tris:
            assert det3(self.fv[a], self.fv[b], self.fv[c]) > 0

    def cone(self, t, q):
        a, b, c = (self.fv[i] for i in self.tris[t])
        return det3(a, b, q) >= 0 and det3(b, c, q) >= 0 and det3(c, a, q) >= 0

    def mesh_side(self, q, near):
        fq = tuple(scaled(x) for x in q)
        cand = []
        for v in near:
            cand += self.inc[v]
        for t in sorted(set(cand)) + list(range(len(self.tris))):
            if self.cone(t, fq):
                a, b, c = (self.fv[i] for i in self.tris[t])
                s = det3(fsub(a, fq), fsub(b, fq), fsub(c, fq))
                return 0 if s == 0 else (-1 if s > 0 else 1)
        raise AssertionError("no cone contains the query")

    @staticmethod
    def analytic_side(q):
        fq = [Fraction(x) for x in q]
        s2 = sum(x * x for x in fq)
        s4 = sum(x * x * x * x for x in fq)
        r = 1 + Fraction(SPH_A) * s4 / (s2 * s2)
        d = s2 - r * r
        return 0 if d == 0 else (1 if d > 0 else -1)

    def convex(self, lo_, hi_):
        ts = [t for t in self.inc[lo_] if hi_ in self.tris[t]]
        assert len(ts) == 2
        a, b, c = (self.fv[i] for i in self.tris[ts[0]])
        d = next(self.fv[i] for i in self.tris[ts[1]] if i not in (lo_, hi_))
        n = det3(fsub(b, a), fsub(c, a), fsub(d, a))
        assert n != 0
        return n < 0


class BandIndex:
    """Triangles by grid cell (cell 1/16 m), each registered in every cell its box, widened by
    2^-29, overlaps: every triangle within 2^-30 m of a point is listed in the point's cell."""

    C = 0.0625

    def __init__(self, verts, tris):
        self.verts, self.tris, self.cells = verts, tris, {}
        w = pow2(-29)
        for t, tri in enumerate(tris):
            c = [verts[i] for i in tri]
            lo_ = [min(v[k] for v in c) - w for k in range(3)]
            hi_ = [max(v[k] for v in c) + w for k in range(3)]
            rng = [range(int(lo_[k] // self.C), int(hi_[k] // self.C) + 1) for k in range(3)]
            for x in rng[0]:
                for y in rng[1]:
                    for z in rng[2]:
                        self.cells.setdefault((x, y, z), []).append(t)

    def near(self, q):
        """True when the exact distance from q to the mesh is below 2^-30 m."""
        cand = self.cells.get(tuple(int(q[k] // self.C) for k in range(3)), [])
        fl = [GD.model_tri(q, *[self.verts[i] for i in self.tris[t]])[1] for t in cand]
        if not fl or min(fl) > pow2(-58):  # float model error is far below the 2^-30 margin
            return False
        return min(GD.exact_tri(q, *[self.verts[i] for i in self.tris[t]])[1] for t in cand) < BAND2


def sphere_queries(verts, tris, edges, ex_):
    """The pinned AC-2 stream: (points, side labels, analytic labels, convex, concave).

    Side labels (exact mesh test): o outside, i inside, O / I the same within 2^-30 m of the
    mesh, z on the mesh. Analytic labels: + outside, - inside, 0 on the analytic surface. A point
    is kept (AC-2 proper) when its side is o or i and the analytic side agrees; the stream runs
    until 10 000 points are kept."""
    g = SplitMix(SEED_SPH)
    band = BandIndex(verts, tris)
    pts, side, ana = [], [], []
    kept = convex = concave = 0
    while kept < N_SPH_KEEP:
        e = g.below(len(edges))
        lo_, hi_ = edges[e]
        a, b = verts[lo_], verts[hi_]
        t = g.unit()
        h = pow2(-(5 + g.below(3)))
        q = tuple((a[j] + t * (b[j] - a[j])) + (2.0 * g.unit() - 1.0) * h for j in range(3))
        pts.append(q)
        ms = ex_.mesh_side(q, (lo_, hi_))
        an = ex_.analytic_side(q)
        nb = ms != 0 and band.near(q)
        side.append("z" if ms == 0 else {1: "o", -1: "i"}[ms].upper() if nb else {1: "o", -1: "i"}[ms])
        ana.append({1: "+", -1: "-", 0: "0"}[an])
        if ms != 0 and not nb and ms == an:
            kept += 1
            if ex_.convex(lo_, hi_):
                convex += 1
            else:
                concave += 1
    return pts, "".join(side), "".join(ana), convex, concave


def sphere_text():
    verts, tris = sphere_mesh()
    ex_ = Exact(verts, tris)
    edges = sorted_edges(tris)
    n_convex_edges = sum(1 for lo_, hi_ in edges if ex_.convex(lo_, hi_))
    n_concave_edges = len(edges) - n_convex_edges
    pts, side, ana, convex, concave = sphere_queries(verts, tris, edges, ex_)
    assert n_concave_edges > 0 and n_convex_edges > 0
    assert concave >= 1000 and convex >= 1000, (convex, concave)
    counts = {c: side.count(c) for c in "oOiIz"}
    nm = Normals(verts, tris)
    lines = [
        "# GEO.DIST.SIGN AC-2 (CC-BY-4.0). Written by tools/fixtures/geo_dist_sign.py; do not edit.",
        "# Mesh: mesh.txt. Points: the pinned stream of sphere_queries() near its sorted edges.",
        "# l: exact mesh side of EVERY point: o outside, i inside, O / I the same within 2^-30 m of",
        "# the mesh (the sign or Undecidable), z on the mesh.",
        "# a: exact side of the analytic surface: + outside, - inside, 0 on it. Kept points (o / i",
        "# with the same analytic side) are the AC-2 points proper.",
        "# s <index> <status code> <signed distance bits> <bound bits>: bit-exact model results.",
        f"seed {SEED_SPH:#018x}",
        f"generated {len(pts)}",
        f"kept {N_SPH_KEEP}",
        "side_counts " + " ".join(f"{c}={counts[c]}" for c in "oOiIz"),
        f"edges convex={n_convex_edges} concave={n_concave_edges}",
        f"kept_near convex={convex} concave={concave}",
        f"points_sha256 {points_digest(pts)}",
    ]
    lines += chunk_lines(side)
    lines += [f"a {ana[i:i + 100]}" for i in range(0, len(ana), 100)]
    for i in range(0, len(pts), SPH_SAMPLE):
        r = model_signed(nm, pts[i])
        check_model(side[i], r, f"sphere point {i}")
        lines.append(f"s {i} {r[0]} {hx(r[1])} {hx(r[2])}")
    print("sphere sides:", counts, "kept convex/concave:", convex, concave)
    return mesh_text(verts, tris), "\n".join(lines) + "\n"


# ------------------------------------------------------------------ fid-num atan2 accuracy


def atan2_sweep():
    """Asserts |m_atan2(y, x) - atan2(y, x)| <= 2^-49 (ATAN2_ERROR) for y >= 0, the domain the
    angle rule uses, against the 90-digit Decimal oracle of fixtures/num/make_fn_fixtures.py.
    m_atan2 is the bit-exact model of fid-num's atan2, which changes musl's (VENDOR.md item 4:
    division residual inside atan, one final rounding of pi - atan for x < 0)."""
    g = SplitMix(0x6174_616E_3273_7770)
    cases = []
    for _ in range(3000):
        x = (2.0 * g.unit() - 1.0) * pow2(g.below(40) - 20)
        y = g.unit() * pow2(g.below(40) - 20)
        cases.append((y, x))
    for k in range(1, 60):
        e = pow2(-k)
        cases += [(e, 1.0), (e, -1.0), (1.0, e), (1.0, -e), (1.0, 1.0 + e), (1.0, -1.0 - e)]
    cases += [(1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (1.0, 1.0), (1.0, -1.0)]
    worst = Fraction(0)
    lim = Fraction(1, 2**49)
    with localcontext() as c:
        c.prec = 80
        for y, x in cases:
            got = Fraction(FN.m_atan2(y, x))
            ref = FN.oracle("atan2", y, x)
            err = abs(got - Fraction(ref))
            assert err <= lim, (y, x, float(err))
            worst = err if err > worst else worst
    return len(cases), float(worst / lim)


# ------------------------------------------------------------------ main


def self_test():
    for x in (2.0, 3.0, 0.5, 1e-300, 1.7976931348623157e308, 5e-324, 1.0 + 2.0**-52):
        r = crsqrt(x)
        fr = Fraction(r)
        up = Fraction(fb(bits(r) + 1))
        dn = Fraction(fb(bits(r) - 1))
        fx = Fraction(x)
        # r is the nearest float: the midpoints around r bracket sqrt(x).
        assert ((fr + dn) / 2) ** 2 <= fx <= ((fr + up) / 2) ** 2, x


def write_or_check(path, text, check, failures):
    if check:
        old = path.read_text(encoding="utf-8") if path.exists() else None
        if old != text:
            failures.append(str(path.relative_to(ROOT)))
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8", newline="\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--check", action="store_true", help="compare with the committed fixtures")
    args = ap.parse_args()
    self_test()
    n_at, worst = atan2_sweep()
    print(f"atan2 sweep: {n_at} cases, worst error {worst:.3f} x 2^-49")
    failures = []
    cube, counts = cube_text()
    write_or_check(OUT_CUBE, cube, args.check, failures)
    print("cube labels:", counts)
    write_or_check(OUT_FLIP, flip_text(), args.check, failures)
    mesh, sph = sphere_text()
    write_or_check(OUT_SPH_MESH, mesh, args.check, failures)
    write_or_check(OUT_SPH, sph, args.check, failures)
    if failures:
        print("stale fixtures:", ", ".join(failures))
        return 1
    print("ok" if args.check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
