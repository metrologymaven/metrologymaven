#!/usr/bin/env python3
"""Independent reference for GEO.INDEX.BVH; writes the fixtures of the leaf's acceptance criteria.

Run:  python fixtures/geo/index/make_bvh_fixtures.py [--check] [--jobs N]

Without --check it rewrites
  fixtures/geo/index/mesh_sphere_1e5/expected.txt   (AC-1, AC-3, nearest, AC-4 visit order)
  fixtures/geo/index/grazing_rays/cases.txt         (AC-2)
by brute force over every triangle (multiprocessing, --jobs defaults to the CPU count).
With --check nothing is written: the script regenerates the mesh, the hierarchy, the query
inputs and the grazing file, compares them with the committed bytes, and re-runs a pure
brute-force scan (no slab pre-filter) of queries 100k, 100k + 1 and 100k + 2 (one sphere, box
and ray each) and of every 20th nearest query, exiting non-zero on any difference. That keeps
the CI step short; the full scan runs when the fixture is written.

Stdlib only (struct, hashlib, math.sqrt, fractions, bisect, multiprocessing). It shares no code
with crates/fid-geo-index; it is a bit-exact float model of the expressions pinned in
spec/geo/index/bvh/NODE.md (Design). CPython floats are IEEE binary64 with round-to-nearest-even
and never fuse a multiply-add, and only correctly rounded operations (+ - * / sqrt) are used, so
the bytes are the same on every platform.

Two oracles are used:
* the float model of the pinned per-box tests, evaluated against EVERY triangle's box
  (brute force). The BVH must return exactly this set (AC-1, AC-2);
* exact rational arithmetic (fractions.Fraction) of the true box/query intersection. Every
  triangle that exact arithmetic reports must be in the float set (no false negatives,
  REQ-GEO.INDEX.BVH-001/002). This is asserted here for every grazing case and for a sample of
  the random queries, and the grazing file also stores the exact set so the Rust test checks it.

The hierarchy digest (AC-3) comes from an independent model of the pinned build (median split
on (centroid key, triangle index) along the largest-extent axis, leaves of at most 8,
breadth-first node numbering) and the pinned serialisation; see NODE.md Design.
"""

import argparse
import bisect
import hashlib
import math
import os
import struct
import sys
from fractions import Fraction
from multiprocessing import Pool
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT_SPHERE = HERE / "mesh_sphere_1e5" / "expected.txt"
OUT_GRAZE = HERE / "grazing_rays" / "cases.txt"

C4 = 2.0**-50  # 4 units of f64 roundoff (4 * 2^-52)
INFL = 1.0 + C4
DEFL = 1.0 - C4
TINY = 2.0**-1022  # smallest positive normal: covers underflow in the tests
LEAF = 8
N_SUB = 71  # icosahedron face subdivision: 20 * 71^2 = 100 820 triangles
N_QUERIES = 10_000
N_NEAREST = 1_000
N_VISIT_POINTS = 5
VISIT_LEAVES = 40
SEED = 0x6765_6F2E_6276_6801  # "geo.bvh" + 1


def hx(x):
    return f"{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def hxs(xs):
    return " ".join(hx(x) for x in xs)


def le64(v):
    return struct.pack("<Q", v)


def fbits(x):
    return struct.pack("<d", x)


# ------------------------------------------------------------------ total order (KNP-1)


def tkey(x):
    # IEEE 754 totalOrder for finite values: numeric order with -0.0 < +0.0.
    return (x, math.copysign(1.0, x))


def tmin(a, b):
    return b if tkey(b) < tkey(a) else a


def tmax(a, b):
    return b if tkey(b) > tkey(a) else a


# ------------------------------------------------------------------ pinned per-box tests


def box_hit(lo, hi, qlo, qhi):
    return all(lo[k] <= qhi[k] and qlo[k] <= hi[k] for k in range(3))


def gap(c, lo, hi):
    if c < lo:
        return lo - c
    if c > hi:
        return c - hi
    return 0.0


def dist2(c, lo, hi):
    gx = gap(c[0], lo[0], hi[0])
    gy = gap(c[1], lo[1], hi[1])
    gz = gap(c[2], lo[2], hi[2])
    return (gx * gx + gy * gy) + gz * gz


def sphere_threshold(r):
    return (r * r) * INFL + TINY


def lower_bound(d2):
    lb = d2 * DEFL - TINY
    return 0.0 if lb <= 0.0 else lb


def widen_up(t):
    if not math.isfinite(t):
        return t
    return t + (abs(t) * C4 + TINY)


def widen_down(t):
    if not math.isfinite(t):
        return t
    return t - (abs(t) * C4 + TINY)


def ray_hit(lo, hi, o, d, t0, t1):
    a, b = t0, t1
    for k in range(3):
        if d[k] == 0.0:
            if o[k] < lo[k] or o[k] > hi[k]:
                return False
            continue
        ta = (lo[k] - o[k]) / d[k]
        tb = (hi[k] - o[k]) / d[k]
        if ta <= tb:
            near, far = ta, tb
        else:
            near, far = tb, ta
        a = tmax(a, widen_down(near))
        b = tmin(b, widen_up(far))
    return a <= b


# ------------------------------------------------------------------ exact (rational) tests


def F(x):
    return Fraction(x)


def exact_box(lo, hi, qlo, qhi):
    return box_hit(lo, hi, qlo, qhi)  # comparisons of floats are exact


def exact_sphere(lo, hi, c, r):
    s = Fraction(0)
    for k in range(3):
        cc, l, h = F(c[k]), F(lo[k]), F(hi[k])
        g = l - cc if cc < l else (cc - h if cc > h else Fraction(0))
        s += g * g
    return s <= F(r) * F(r)


def exact_ray(lo, hi, o, d, t0, t1):
    a, b = F(t0), F(t1)
    for k in range(3):
        if d[k] == 0.0:
            if o[k] < lo[k] or o[k] > hi[k]:
                return False
            continue
        ta = (F(lo[k]) - F(o[k])) / F(d[k])
        tb = (F(hi[k]) - F(o[k])) / F(d[k])
        a = max(a, min(ta, tb))
        b = min(b, max(ta, tb))
    return a <= b


# ------------------------------------------------------------------ mesh (pinned generator)

PHI = (1.0 + math.sqrt(5.0)) / 2.0
ICO_V = [
    (-1.0, PHI, 0.0), (1.0, PHI, 0.0), (-1.0, -PHI, 0.0), (1.0, -PHI, 0.0),
    (0.0, -1.0, PHI), (0.0, 1.0, PHI), (0.0, -1.0, -PHI), (0.0, 1.0, -PHI),
    (PHI, 0.0, -1.0), (PHI, 0.0, 1.0), (-PHI, 0.0, -1.0), (-PHI, 0.0, 1.0),
]  # fmt: skip
ICO_F = [
    (0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11),
    (1, 5, 9), (5, 11, 4), (11, 10, 2), (10, 7, 6), (7, 1, 8),
    (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8), (3, 8, 9),
    (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1),
]  # fmt: skip


def sphere_mesh():
    n = N_SUB
    verts, tris = [], []
    fn = float(n)
    for ia, ib, ic in ICO_F:
        A, B, C = ICO_V[ia], ICO_V[ib], ICO_V[ic]
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


def payload_of(t):
    return (t * 2654435761) % (1 << 32)


def mesh_digest(verts, tris):
    h = hashlib.sha256()
    h.update(le64(len(verts)))
    for v in verts:
        h.update(fbits(v[0]) + fbits(v[1]) + fbits(v[2]))
    h.update(le64(len(tris)))
    for t in tris:
        h.update(le64(t[0]) + le64(t[1]) + le64(t[2]))
    return h.hexdigest()


def tri_boxes(verts, tris):
    los, his, keys = [], [], []
    for a, b, c in tris:
        A, B, C = verts[a], verts[b], verts[c]
        los.append(tuple(tmin(tmin(A[k], B[k]), C[k]) for k in range(3)))
        his.append(tuple(tmax(tmax(A[k], B[k]), C[k]) for k in range(3)))
        keys.append(tuple((A[k] + B[k]) + C[k] for k in range(3)))
    return los, his, keys


# ------------------------------------------------------------------ pinned build model


def build(los, his, keys, frame, payload):
    n = len(los)
    order = list(range(n))
    ranges = [(0, n)]
    nodes = []  # (lo, hi, left_or_first, count, axis)
    i = 0
    while i < len(ranges):
        s, e = ranges[i]
        lo, hi = list(los[order[s]]), list(his[order[s]])
        for p in order[s + 1 : e]:
            for k in range(3):
                lo[k] = tmin(lo[k], los[p][k])
                hi[k] = tmax(hi[k], his[p][k])
        cnt = e - s
        if cnt <= LEAF:
            order[s:e] = sorted(order[s:e])
            nodes.append((lo, hi, s, cnt, 0))
        else:
            ext = [hi[k] - lo[k] for k in range(3)]
            axis = 0
            for k in (1, 2):
                if ext[k] > ext[axis]:
                    axis = k
            order[s:e] = sorted(order[s:e], key=lambda t: (keys[t][axis], t))
            mid = s + cnt // 2
            left = len(ranges)
            ranges.append((s, mid))
            ranges.append((mid, e))
            nodes.append((lo, hi, left, 0, axis))
        i += 1
    out = bytearray(b"FIDBVH01")
    out += le64(n) + le64(len(nodes)) + bytes(frame)
    for lo, hi, lf, cnt, axis in nodes:
        for x in lo + hi:
            out += fbits(x)
        out += struct.pack("<IHBB", lf, cnt, axis, 0)
    for t in order:
        out += struct.pack("<I", t)
    for t in range(n):
        out += le64(payload[t])
    return nodes, order, bytes(out)


# ------------------------------------------------------------------ query stream (pinned)


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


def queries():
    g = SplitMix(SEED)
    qs = []
    for i in range(N_QUERIES):
        kind = i % 3
        if kind == 0:
            c = tuple(g.uni(-1.5, 1.5) for _ in range(3))
            r = g.uni(0.0, 0.3)
            if i % 30 == 0:
                r = 0.0
            qs.append(("sphere", c, r))
        elif kind == 1:
            c = [g.uni(-1.5, 1.5) for _ in range(3)]
            h = [g.uni(0.0, 0.25) for _ in range(3)]
            qs.append(("box", tuple(c[k] - h[k] for k in range(3)), tuple(c[k] + h[k] for k in range(3))))
        else:
            o = tuple(g.uni(-2.0, 2.0) for _ in range(3))
            d = [g.uni(-1.0, 1.0) for _ in range(3)]
            if i % 7 == 0:
                d[0] = 0.0
            if i % 11 == 0:
                d[1] = 0.0
            a, b = g.uni(-1.0, 3.0), g.uni(-1.0, 3.0)
            t0, t1 = (a, b) if a <= b else (b, a)
            qs.append(("ray", o, tuple(d), t0, t1))
    pts = [tuple(g.uni(-1.5, 1.5) for _ in range(3)) for _ in range(N_NEAREST)]
    return qs, pts


def query_digest(qs, pts):
    h = hashlib.sha256()
    for q in qs:
        h.update(q[0].encode())
        for part in q[1:]:
            for x in part if isinstance(part, tuple) else (part,):
                h.update(fbits(x))
    for p in pts:
        h.update(fbits(p[0]) + fbits(p[1]) + fbits(p[2]))
    return h.hexdigest()


# ------------------------------------------------------------------ brute force workers

_W = {}


def _init(los, his):
    _W["los"], _W["his"] = los, his
    # Slab pre-filter for sphere and box queries only (full regeneration speed): triangles
    # sorted by lo_x; see `_slab` for why it never drops a triangle the tests accept. The
    # --check run re-scans sampled queries with no pre-filter at all.
    order = sorted(range(len(los)), key=lambda t: los[t][0])
    _W["sx_order"] = order
    _W["sx_keys"] = [los[t][0] for t in order]
    _W["wmax"] = max(his[t][0] - los[t][0] for t in range(len(los)))


def _slab(xlo, xhi):
    # A triangle with lo_x > xhi or hi_x < xlo fails the box test exactly. For the sphere
    # test the caller widens [c - r, c + r] by 1 % of r plus 1e-9 m: a gap g > 1.01 r + 1e-9
    # on one axis gives d2 >= fl(g * g) > thr. hi_x >= xlo implies lo_x >= xlo - 2 * wmax
    # (wmax is the largest rounded width; the factor 2 absorbs its rounding).
    keys, order = _W["sx_keys"], _W["sx_order"]
    a = bisect.bisect_left(keys, xlo - 2.0 * _W["wmax"])
    b = bisect.bisect_right(keys, xhi)
    return order[a:b]


def run_query(args):
    q, prefilter = args
    los, his = _W["los"], _W["his"]
    kind = q[0]
    if kind == "sphere":
        c, r = q[1], q[2]
        thr = sphere_threshold(r)
        cand = _slab(c[0] - (r * 1.01 + 1e-9), c[0] + (r * 1.01 + 1e-9)) if prefilter else range(len(los))
        out = [t for t in cand if dist2(c, los[t], his[t]) <= thr]
    elif kind == "box":
        qlo, qhi = q[1], q[2]
        cand = _slab(qlo[0], qhi[0]) if prefilter else range(len(los))
        out = [t for t in cand if box_hit(los[t], his[t], qlo, qhi)]
    else:
        o, d, t0, t1 = q[1], q[2], q[3], q[4]
        out = [t for t in range(len(los)) if ray_hit(los[t], his[t], o, d, t0, t1)]
    return sorted(out)


def run_nearest(p):
    los, his = _W["los"], _W["his"]
    best, bt = None, None
    for t in range(len(los)):
        d2 = dist2(p, los[t], his[t])
        if best is None or d2 < best:
            best, bt = d2, t
    return bt, best


def result_bytes(ids):
    return le64(len(ids)) + b"".join(le64(t) for t in ids)


# ------------------------------------------------------------------ fixture: mesh_sphere_1e5

FRAME = bytes(range(16))


def sphere_fixture(jobs, check, committed):
    verts, tris = sphere_mesh()
    los, his, keys = tri_boxes(verts, tris)
    payload = [payload_of(t) for t in range(len(tris))]
    nodes, _order, ser = build(los, his, keys, FRAME, payload)
    qs, pts = queries()
    head = [
        "# GEO.INDEX.BVH fixture mesh_sphere_1e5 (AC-1, AC-3, nearest, AC-4). Written by",
        "# fixtures/geo/index/make_bvh_fixtures.py; never edit by hand. CC-BY-4.0.",
        "# Mesh: icosahedron, each face subdivided 71 x 71 and projected to the unit sphere",
        "# (pinned generator, NODE.md Design); frame bytes 00..0f; payload(t) = t * 2654435761 mod 2^32.",
        "# q <i> <kind> <count> <first 16 hex of SHA-256(u64le count, u64le ids...)>",
        "# result_digest = SHA-256 over the per-query byte strings, in query order.",
        "# nearest <j> <triangle> <d2 bits>; visit <j> <node>:<lower bound bits> ... (first 40 leaves)",
        f"triangles {len(tris)}",
        f"vertices {len(verts)}",
        f"mesh_digest {mesh_digest(verts, tris)}",
        f"nodes {len(nodes)}",
        f"bvh_bytes {len(ser)}",
        f"bvh_digest {hashlib.sha256(ser).hexdigest()}",
        f"query_digest {query_digest(qs, pts)}",
    ]
    if check:
        lines = committed.split("\n")
        if lines[: len(head)] != head:
            for a, b in zip(lines, head):
                if a != b:
                    print(f"mesh_sphere_1e5: header differs:\n  committed {a}\n  model     {b}")
            return False
        stored_q = {}
        stored_n = {}
        for ln in lines:
            f = ln.split(" ")
            if f[0] == "q":
                stored_q[int(f[1])] = (f[2], int(f[3]), f[4])
            elif f[0] == "nearest":
                stored_n[int(f[1])] = (int(f[2]), f[3])
        sample = [i for i in range(N_QUERIES) if i % 100 in (0, 1, 2)]
        sample_n = [j for j in range(N_NEAREST) if j % 20 == 0]
        with Pool(jobs, _init, (los, his)) as pool:
            res = pool.map(run_query, [(qs[i], False) for i in sample])
            near = pool.map(run_nearest, [pts[j] for j in sample_n])
        ok = True
        for i, ids in zip(sample, res):
            want = stored_q.get(i)
            got = (qs[i][0], len(ids), hashlib.sha256(result_bytes(ids)).hexdigest()[:16])
            if want != got:
                print(f"mesh_sphere_1e5: query {i}: committed {want}, brute force {got}")
                ok = False
        for j, (t, d2) in zip(sample_n, near):
            if stored_n.get(j) != (t, hx(d2)):
                print(f"mesh_sphere_1e5: nearest {j}: committed {stored_n.get(j)}, brute force {(t, hx(d2))}")
                ok = False
        print(f"mesh_sphere_1e5: header, {len(sample)} queries and {len(sample_n)} nearest re-scanned")
        return ok
    with Pool(jobs, _init, (los, his)) as pool:
        res = pool.map(run_query, [(q, q[0] != "ray") for q in qs], chunksize=16)
        near = pool.map(run_nearest, pts, chunksize=8)
    body = []
    total = hashlib.sha256()
    for i, (q, ids) in enumerate(zip(qs, res)):
        rb = result_bytes(ids)
        total.update(rb)
        body.append(f"q {i} {q[0]} {len(ids)} {hashlib.sha256(rb).hexdigest()[:16]}")
    # Conservativeness against exact arithmetic on a sample (every 997th query).
    for i in range(0, N_QUERIES, 997):
        q, got = qs[i], set(res[i])
        for t in range(len(los)):
            if q[0] == "sphere":
                ex = exact_sphere(los[t], his[t], q[1], q[2])
            elif q[0] == "box":
                ex = exact_box(los[t], his[t], q[1], q[2])
            else:
                ex = exact_ray(los[t], his[t], *q[1:])
            if ex and t not in got:
                raise SystemExit(f"query {i}: exact hit {t} missing from the float set")
    for j, (t, d2) in enumerate(near):
        body.append(f"nearest {j} {t} {hx(d2)}")
    leaves = [(i, nd) for i, nd in enumerate(nodes) if nd[3] > 0]
    for j in range(N_VISIT_POINTS):
        p = pts[j]
        ordered = sorted(((lower_bound(dist2(p, nd[0], nd[1])), i) for i, nd in leaves), key=lambda x: (x[0], x[1]))
        body.append(f"visit {j} " + " ".join(f"{i}:{hx(lb)}" for lb, i in ordered[:VISIT_LEAVES]))
    text = "\n".join(head + [f"result_digest {total.hexdigest()}"] + body) + "\n"
    return text


# ------------------------------------------------------------------ fixture: grazing_rays


def grazing_mesh():
    verts, tris, notes = [], [], []

    def v(p):
        verts.append(tuple(float(x) for x in p))
        return len(verts) - 1

    # 4 x 4 grid of unit squares in the plane z = 0, two triangles each (flat boxes).
    g = {}
    for i in range(5):
        for j in range(5):
            g[(i, j)] = v((i, j, 0))
    for j in range(4):
        for i in range(4):
            tris.append((g[(i, j)], g[(i + 1, j)], g[(i + 1, j + 1)]))
            tris.append((g[(i, j)], g[(i + 1, j + 1)], g[(i, j + 1)]))
    # A wall in the plane x = 4 from y = 0..2, z = 0..2 (boxes flat in x).
    w = {}
    for j in range(3):
        for k in range(3):
            w[(j, k)] = v((4, j, k))
    for k in range(2):
        for j in range(2):
            tris.append((w[(j, k)], w[(j + 1, k)], w[(j + 1, k + 1)]))
            tris.append((w[(j, k)], w[(j + 1, k + 1)], w[(j, k + 1)]))
    # A slanted triangle with decimal (inexact) coordinates; box [0.1,0.7]x[0.2,0.8]x[0.3,0.9].
    a, b, c = v((0.1, 0.2, 0.3)), v((0.7, 0.2, 0.9)), v((0.1, 0.8, 0.3))
    tris.append((a, b, c))
    # A tiny triangle near 1e-300 (underflow region) and a huge one far away.
    a, b, c = v((1e-300, 1e-300, 0.0)), v((2e-300, 1e-300, 0.0)), v((1e-300, 2e-300, 0.0))
    tris.append((a, b, c))
    a, b, c = v((1e100, 1e100, 1e100)), v((2e100, 1e100, 1e100)), v((1e100, 2e100, 3e100))
    tris.append((a, b, c))
    return verts, tris


def prev_float(x):
    b = struct.unpack("<q", struct.pack("<d", x))[0]
    return struct.unpack("<d", struct.pack("<q", b - 1 if x > 0 else b + 1))[0]


def grazing_cases():
    s2 = math.sqrt(2.0)
    rays = [
        ("along the grid plane inside row 0", (-1, 0.5, 0), (1, 0, 0), 0, 10),
        ("along grid line y = 1 (two rows touch it)", (-1, 1, 0), (1, 0, 0), 0, 10),
        ("parallel to the plane 1e-300 above it", (-1, 1, 1e-300), (1, 0, 0), 0, 10),
        ("through grid vertices on the diagonal", (-1, -1, 0), (1, 1, 0), 0, 10),
        ("down onto vertex (2, 2)", (2, 2, 5), (0, 0, -1), 0, 10),
        ("down onto (2, 2) ending exactly on the plane", (2, 2, 5), (0, 0, -1), 0, 5),
        ("down onto (2, 2) ending one ulp before the plane", (2, 2, 5), (0, 0, -1), 0, prev_float(5.0)),
        ("along the wall face x = 4", (4, -1, 1), (0, 1, 0), 0, 10),
        ("along the wall's top edge", (4, -1, 2), (0, 1, 0), 0, 10),
        ("along the wall plane but outside it in z", (4, -1, 2.5), (0, 1, 0), 0, 10),
        ("tiny y direction (1e-300)", (-1, 0.5, 0), (1, 1e-300, 0), 0, 10),
        ("subnormal y direction (division overflows to infinity)", (-1, 0.5, 0), (1, 5e-324, 0), 0, 10),
        ("behind the origin (negative t range)", (5, 0.5, 0), (1, 0, 0), -10, -1),
        ("grazing the slanted box's min corner exactly", (0, 0, 0), (0.1, 0.2, 0.3), 0, 10),
        ("near the slanted box's min corner (inexact divisions)", (-0.2, -0.1, 0.0), (0.3, 0.3, 0.3), 0, 10),
        ("ending at the slanted corner, t_max = 1", (0, 0, 0), (0.1, 0.2, 0.3), 0, 1),
        ("ending one ulp before the slanted corner", (0, 0, 0), (0.1, 0.2, 0.3), 0, prev_float(1.0)),
        ("through the tiny triangle's corner", (0, 0, 0), (1, 1, 0), 0, 1),
        ("tiny triangle with t range at its corner", (0, 0, 0), (1, 1, 0), 1e-300, 1e-300),
        ("towards the huge triangle", (0, 0, 0), (1, 1, 1), 0, 1e101),
        ("diagonal with inexact 1/3 slopes through the grid", (0, 0, 0), (1 / 3, 2 / 3, 0), 0, 6),
    ]
    boxes = [
        ("touching the grid's far corner", (4, 4, 0), (5, 5, 1)),
        ("flat box in the plane z = 0", (1, 1, 0), (2, 2, 0)),
        ("just above the plane", (1, 1, 1e-300), (2, 2, 1)),
        ("touching the wall face from x = 4", (4, 0.5, 0.5), (6, 0.6, 0.6)),
    ]
    spheres = [
        ("touching the grid corner (4, 4, 0) at distance sqrt(2)", (5, 5, 0), s2),
        ("point query on vertex (2, 2, 0)", (2, 2, 0), 0.0),
        ("point query 1e-300 above the plane", (2, 2, 1e-300), 0.0),
        ("radius 1 touching the plane exactly", (0.5, 0.5, 1), 1.0),
        ("radius 1 one ulp short of the plane", (0.5, 0.5, 1), prev_float(1.0)),
    ]
    return rays, boxes, spheres


def fl3(p):
    return tuple(float(x) for x in p)


def grazing_fixture():
    verts, tris = grazing_mesh()
    los, his, _ = tri_boxes(verts, tris)
    n = len(tris)
    rays, boxes, spheres = grazing_cases()
    out = [
        "# GEO.INDEX.BVH fixture grazing_rays (AC-2). Written by fixtures/geo/index/make_bvh_fixtures.py;",
        "# never edit by hand. CC-BY-4.0. Hand-constructed degenerate queries along triangle edges and",
        "# box faces. `expect` is the float model of the pinned tests over every triangle (the BVH must",
        "# return exactly it); `exact` is the rational-arithmetic set (it must be contained in expect).",
        "# payload(t) = 1000 + t. Values are f64 bits in hex.",
        f"frame {FRAME.hex()}",
    ]
    for i, p in enumerate(verts):
        out.append(f"vertex {i} {hxs(p)}")
    for t, (a, b, c) in enumerate(tris):
        out.append(f"tri {t} {a} {b} {c} {1000 + t}")

    def ids(xs):
        return ",".join(str(x) for x in xs) if xs else "-"

    def emit(kind, name, fields, fl, ex):
        flt = [t for t in range(n) if fl(t)]
        exa = [t for t in range(n) if ex(t)]
        missing = set(exa) - set(flt)
        if missing:
            raise SystemExit(f"grazing {name}: exact hits {sorted(missing)} missing from the float set")
        out.append(f"# {name}")
        out.append(f"{kind} {fields} expect={ids(flt)} exact={ids(exa)}")

    for name, o, d, t0, t1 in rays:
        o, d, t0, t1 = fl3(o), fl3(d), float(t0), float(t1)
        emit(
            "ray",
            name,
            f"{hxs(o)} {hxs(d)} {hx(t0)} {hx(t1)}",
            lambda t: ray_hit(los[t], his[t], o, d, t0, t1),
            lambda t: exact_ray(los[t], his[t], o, d, t0, t1),
        )
    for name, qlo, qhi in boxes:
        qlo, qhi = fl3(qlo), fl3(qhi)
        emit(
            "box",
            name,
            f"{hxs(qlo)} {hxs(qhi)}",
            lambda t: box_hit(los[t], his[t], qlo, qhi),
            lambda t: exact_box(los[t], his[t], qlo, qhi),
        )
    for name, c, r in spheres:
        c, r = fl3(c), float(r)
        thr = sphere_threshold(r)
        emit(
            "sphere",
            name,
            f"{hxs(c)} {hx(r)}",
            lambda t: dist2(c, los[t], his[t]) <= thr,
            lambda t: exact_sphere(los[t], his[t], c, r),
        )
    return "\n".join(out) + "\n"


# ------------------------------------------------------------------ main


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    args = ap.parse_args()
    ok = True
    graze = grazing_fixture()
    if args.check:
        if not OUT_GRAZE.exists() or OUT_GRAZE.read_bytes() != graze.encode():
            print(f"{OUT_GRAZE} differs from the reference")
            ok = False
        committed = OUT_SPHERE.read_bytes().decode() if OUT_SPHERE.exists() else ""
        ok = sphere_fixture(args.jobs, True, committed) and ok
        if not ok:
            sys.exit(1)
        print("bvh fixtures OK")
        return
    OUT_GRAZE.parent.mkdir(parents=True, exist_ok=True)
    OUT_GRAZE.write_bytes(graze.encode())
    text = sphere_fixture(args.jobs, False, None)
    OUT_SPHERE.parent.mkdir(parents=True, exist_ok=True)
    OUT_SPHERE.write_bytes(text.encode())
    print(f"wrote {OUT_GRAZE} and {OUT_SPHERE}")


if __name__ == "__main__":
    main()
