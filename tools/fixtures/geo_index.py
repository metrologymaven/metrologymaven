#!/usr/bin/env python3
"""Independent reference for GEO.INDEX.KDTREE; writes the fixtures of the leaf's acceptance criteria.

Run:  python tools/fixtures/geo_index.py [--check] [--jobs N]

Without --check it rewrites
  fixtures/geo/index/uniform_1e5/expected.txt   (AC-1, AC-3: 10^5 points, seed 21)
  fixtures/geo/index/lattice_ties/expected.txt  (AC-2, AC-3: 50^3 lattice at 1 mm, seed 22)
  fixtures/geo/index/kdtree_edge/cases.txt      (edge cases: duplicates, coincident points,
                                                 signed zeros, subnormal and huge coordinates,
                                                 r = 0, leaf-size boundaries, k = 1024, n = 0)
by brute force over every point (multiprocessing, --jobs defaults to the CPU count).
With --check nothing is written: the script regenerates the point clouds, the query streams,
the index digests and the whole edge-case file, compares them with the committed bytes, and
re-runs the brute force for result block 3 of each large fixture, exiting non-zero on any
difference. That keeps the CI step short; the full scan runs when the fixtures are written.

Stdlib only (struct, hashlib, heapq, multiprocessing). It shares no code with
crates/fid-geo-index; it is a bit-exact float model of the expressions pinned in
spec/geo/index/kdtree/NODE.md (Design). CPython floats are IEEE binary64 with
round-to-nearest-even and never fuse a multiply-add, and only correctly rounded + - * are used,
so the bytes are the same on every platform.

The oracle is plain brute force: the pinned squared distance
    d2(p, q) = (dx*dx + dy*dy) + dz*dz,  dx = p.x - q.x (the sign does not change the value)
to EVERY point, then
    knn(q, k)    the min(k, n) smallest (d2, index) pairs, ascending;
    radius(q, r) every point with d2 <= r*r (r*r rounded once), ascending by (d2, index);
    in_box(b)    every point with min_k <= p_k <= max_k on all axes (numeric comparisons, so
                 -0.0 == +0.0), ascending by index.
The index must return exactly these lists. The index digest (AC-3) comes from an independent
model of the pinned build (breadth-first implicit tree, median on (totalOrder coordinate,
index) along the largest-extent axis, every leaf at the same depth with at most 16 points) and
the pinned serialisation `FIDKDT01`; see NODE.md Design.
"""

import argparse
import hashlib
import heapq
import os
import struct
import sys
from multiprocessing import Pool
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "fixtures" / "geo" / "index"
OUT_UNIFORM = OUT / "uniform_1e5" / "expected.txt"
OUT_LATTICE = OUT / "lattice_ties" / "expected.txt"
OUT_EDGE = OUT / "kdtree_edge" / "cases.txt"

BUCKET = 16
MAX_K = 1024
MASK64 = (1 << 64) - 1
BLOCK = 100  # queries per result digest
CHECK_BLOCK = 3  # the block --check recomputes by brute force

U_N = 100_000
U_SEED = 21
U_QUERIES = 1_000
U_KS = (1, 16, 1024)
U_RADIUS = 0.002  # 2 mm
U_HALF = 0.002  # box half-size

L_SIDE = 50
L_SCALE = 0.001  # 1 mm
L_SEED = 22
L_QUERIES = 500
L_K = 26
L_RADIUS = 0.0015


# ------------------------------------------------------------------ bits and order


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def unbits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def hx(x):
    return f"{bits(x):016x}"


def tkey(x):
    """IEEE 754 totalOrder as an integer key (the same map as Rust's f64::total_cmp)."""
    i = struct.unpack("<q", struct.pack("<d", x))[0]
    if i < 0:
        i ^= 0x7FFF_FFFF_FFFF_FFFF
    return i


class SplitMix:
    def __init__(self, seed):
        self.s = seed & MASK64

    def next(self):
        self.s = (self.s + 0x9E37_79B9_7F4A_7C15) & MASK64
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58_476D_1CE4_E5B9) & MASK64
        z = ((z ^ (z >> 27)) * 0x94D0_49BB_1331_11EB) & MASK64
        return z ^ (z >> 31)

    def unit(self):
        """(next >> 11) * 2^-53, in [0, 1), exact."""
        return (self.next() >> 11) * (2.0**-53)


# ------------------------------------------------------------------ the pinned tree model


def leaf_depth(n):
    """Smallest d >= 0 with ceil(n / 2^d) <= 16."""
    d = 0
    while (n + (1 << d) - 1) >> d > BUCKET:
        d += 1
    return d


def node_range(n, d, j):
    """(start, count) of node j (0-based, left to right) at depth d; left child gets count // 2."""
    s, c = 0, n
    for b in range(d - 1, -1, -1):
        h = c // 2
        if (j >> b) & 1:
            s += h
            c -= h
        else:
            c = h
    return s, c


def build(pts):
    n = len(pts)
    depth = leaf_depth(n)
    keys = [[tkey(p[a]) for p in pts] for a in range(3)]
    perm = list(range(n))
    splits, axes = [], []
    for i in range((1 << depth) - 1):
        d = (i + 1).bit_length() - 1
        s, c = node_range(n, d, i + 1 - (1 << d))
        idx = perm[s : s + c]
        ext = []
        for a in range(3):
            lo = min(idx, key=lambda p: keys[a][p])
            hi = max(idx, key=lambda p: keys[a][p])
            ext.append(pts[hi][a] - pts[lo][a])
        axis = 0
        for a in (1, 2):
            if ext[a] > ext[axis]:
                axis = a
        ka = keys[axis]
        idx.sort(key=lambda p: (ka[p], p))
        perm[s : s + c] = idx
        splits.append(pts[idx[c // 2]][axis])
        axes.append(axis)
    for j in range(1 << depth):
        s, c = node_range(n, depth, j)
        perm[s : s + c] = sorted(perm[s : s + c])
    return depth, splits, axes, perm


def serialise(n, depth, splits, axes, perm):
    out = bytearray(b"FIDKDT01")
    out += struct.pack("<QII", n, depth, BUCKET)
    for s in splits:
        out += struct.pack("<Q", bits(s))
    out += bytes(axes)
    for p in perm:
        out += struct.pack("<I", p)
    return bytes(out)


def index_digest(pts):
    depth, splits, axes, perm = build(pts)
    return depth, hashlib.sha256(serialise(len(pts), depth, splits, axes, perm)).hexdigest()


# ------------------------------------------------------------------ brute force

PTS = None  # worker global


def _init(pts):
    global PTS
    PTS = pts


def d2_all(q):
    qx, qy, qz = q
    return [(((x - qx) * (x - qx)) + ((y - qy) * (y - qy))) + ((z - qz) * (z - qz)) for x, y, z in PTS]


def knn(d2, k):
    return heapq.nsmallest(k, zip(d2, range(len(d2))))


def radius(d2, r):
    r2 = r * r
    return sorted((d, i) for i, d in enumerate(d2) if d <= r2)


def in_box(lo, hi):
    return [i for i, p in enumerate(PTS) if all(lo[a] <= p[a] <= hi[a] for a in range(3))]


def enc_nb(res):
    out = bytearray(struct.pack("<Q", len(res)))
    for d, i in res:
        out += struct.pack("<QQ", i, bits(d))
    return bytes(out)


def enc_idx(res):
    out = bytearray(struct.pack("<Q", len(res)))
    for i in res:
        out += struct.pack("<Q", i)
    return bytes(out)


def _uniform_query(q):
    d2 = d2_all(q)
    lo = tuple(q[a] - U_HALF for a in range(3))
    hi = tuple(q[a] + U_HALF for a in range(3))
    out = {f"knn{k}": enc_nb(knn(d2, k)) for k in U_KS}
    out["radius"] = enc_nb(radius(d2, U_RADIUS))
    out["box"] = enc_idx(in_box(lo, hi))
    return out


def _lattice_query(q):
    d2 = d2_all(q)
    return {f"knn{L_K}": enc_nb(knn(d2, L_K)), "radius": enc_nb(radius(d2, L_RADIUS))}


# ------------------------------------------------------------------ the large fixtures


def uniform_cloud():
    g = SplitMix(U_SEED)
    pts = [(g.unit() * 0.125, g.unit() * 0.125, g.unit() * 0.125) for _ in range(U_N)]
    qs = []
    for i in range(U_QUERIES):
        if i % 10 == 0:
            qs.append(pts[g.next() % U_N])
        else:
            qs.append(tuple(g.unit() * 0.140625 - 0.0078125 for _ in range(3)))
    return pts, qs


def lattice_cloud():
    n = L_SIDE**3
    cells = list(range(n))
    g = SplitMix(L_SEED)
    for i in range(n - 1, 0, -1):
        j = g.next() % (i + 1)
        cells[i], cells[j] = cells[j], cells[i]
    pts = []
    for c in cells:
        a, rest = divmod(c, L_SIDE * L_SIDE)
        b, cc = divmod(rest, L_SIDE)
        pts.append((a * L_SCALE, b * L_SCALE, cc * L_SCALE))
    qs = [pts[g.next() % n] for _ in range(L_QUERIES)]
    return pts, qs


def cloud_digest(pts):
    h = hashlib.sha256(struct.pack("<Q", len(pts)))
    for p in pts:
        h.update(struct.pack("<ddd", *p))
    return h.hexdigest()


def run_queries(pts, qs, fn, jobs, blocks):
    sel = [i for i in range(len(qs)) if i // BLOCK in blocks]
    if jobs > 1:
        with Pool(jobs, initializer=_init, initargs=(pts,)) as pool:
            res = pool.map(fn, [qs[i] for i in sel], chunksize=4)
    else:
        _init(pts)
        res = [fn(qs[i]) for i in sel]
    by_block = {}
    for i, r in zip(sel, res):
        by_block.setdefault(i // BLOCK, []).append(r)
    lines = []
    kinds = list(res[0].keys())
    for kind in kinds:
        for b in sorted(by_block):
            h = hashlib.sha256()
            for r in by_block[b]:
                h.update(r[kind])
            lines.append(f"result {kind} {b} {h.hexdigest()}")
    return lines


def large_header(name, pts, qs):
    depth, dig = index_digest(pts)
    qh = hashlib.sha256()
    for q in qs:
        qh.update(struct.pack("<ddd", *q))
    return [
        f"# GEO.INDEX.KDTREE fixture {name}: written by tools/fixtures/geo_index.py; do not edit.",
        f"# result <kind> <block>: SHA-256 over queries block*{BLOCK} .. block*{BLOCK}+{BLOCK - 1} of the",
        "# per-query encodings (count u64 LE, then per hit index u64 LE and dist2 bits u64 LE; box: index only).",
        f"points {len(pts)} {cloud_digest(pts)}",
        f"queries {len(qs)} {qh.hexdigest()}",
        f"index depth {depth} {dig}",
    ]


# ------------------------------------------------------------------ edge cases


def rand_pts(g, n, scale):
    return [tuple(g.unit() * scale for _ in range(3)) for _ in range(n)]


def wide(g):
    """A random finite double with |x| in [2^-1074, 2^500], random sign."""
    e = g.next() % 1524  # biased exponent 0 (subnormal) .. 1523 (2^500)
    m = g.next() & ((1 << 52) - 1)
    if e == 1523:
        m = 0
    b = (e << 52) | m
    if g.next() & 1:
        b |= 1 << 63
    return unbits(b)


def edge_cases():
    g = SplitMix(23)
    cases = []
    O = (0.0, 0.0, 0.0)

    def knns(q, ks):
        return [("knn", q, k) for k in ks]

    # n = 0 and n = 1
    cases.append(("empty", [], knns(O, (1, 1024)) + [("radius", O, 1.0), ("box", (-1.0,) * 3, (1.0,) * 3)]))
    p = (1.0, 2.0, 3.0)
    cases.append(
        ("single", [p], knns(p, (1, 1024)) + [("radius", p, 0.0), ("radius", O, 0.0), ("radius", O, 4.0), ("box", p, p)])
    )
    # 40 coincident points
    c = (0.5, -0.25, 1.0)
    cases.append(
        (
            "coincident_40",
            [c] * 40,
            knns(c, (1, 16, 40, 1024)) + knns(O, (3, 17)) + [("radius", c, 0.0), ("radius", c, -0.0), ("box", c, c)],
        )
    )
    # 200 points drawn from 6 positions on a 0.25 grid: duplicates and exact distance ties
    pos = [(0.0, 0.0, 0.0), (0.25, 0.0, 0.0), (0.0, 0.25, 0.0), (0.0, 0.0, 0.25), (0.25, 0.25, 0.0), (-0.25, 0.0, 0.0)]
    pts = [pos[g.next() % 6] for _ in range(200)]
    qs = []
    for q in pos + [(0.125, 0.0, 0.0), (0.125, 0.125, 0.125)]:
        qs += knns(q, (1, 7, 16, 100, 1024))
        qs += [("radius", q, 0.0), ("radius", q, 0.25), ("radius", q, 0.125)]
    qs.append(("box", (0.0, 0.0, 0.0), (0.25, 0.0, 0.25)))
    cases.append(("duplicates_200", pts, qs))
    # signed zeros
    pts = []
    for sx in (0.0, -0.0):
        for sy in (0.0, -0.0):
            for sz in (0.0, -0.0):
                pts.append((sx, sy, sz))
    pts += [(1.0, -0.0, 0.0), (-1.0, 0.0, -0.0), (-0.0, 1.0, -0.0), (0.0, -0.0, -1.0)] * 3
    qs = knns((-0.0, -0.0, -0.0), (1, 8, 20)) + knns(O, (8,))
    qs += [("radius", (-0.0, 0.0, -0.0), 0.0), ("radius", O, 1.0)]
    qs += [("box", (-0.0, -0.0, -0.0), (-0.0, -0.0, -0.0)), ("box", (0.0, 0.0, 0.0), (0.0, 0.0, 0.0))]
    qs += [("box", (-0.0, -0.0, -0.0), (1.0, 1.0, 1.0))]
    cases.append(("signed_zero", pts, qs))
    # subnormal coordinates: most squared differences underflow to 0
    t = 2.0**-1074
    pts = []
    for _ in range(60):
        pts.append(tuple(((g.next() % 41) - 20) * t for _ in range(3)))
    pts += [(2.0**-1022, 0.0, 0.0), (0.0, 2.0**-1022, 0.0), (2.0**-537, 0.0, 0.0), (2.0**-536, 2.0**-536, 0.0)]
    qs = knns(O, (1, 10, 64, 1024)) + knns(pts[0], (5,)) + knns((2.0**-537, 0.0, 0.0), (3,))
    qs += [("radius", O, 0.0), ("radius", O, t), ("radius", O, 2.0**-537), ("radius", O, 2.0**-500)]
    qs += [("box", (-5 * t, -5 * t, -5 * t), (5 * t, 5 * t, 5 * t))]
    cases.append(("subnormal", pts, qs))
    # huge coordinates up to the 2^500 limit
    H = 2.0**500
    Hm = unbits(bits(H) - 1)
    vals = [H, -H, Hm, -Hm, 0.0, 1.0, -1.0, 2.0**499]
    pts = [(vals[g.next() % 8], vals[g.next() % 8], vals[g.next() % 8]) for _ in range(120)]
    pts += [(H, H, H), (-H, -H, -H)]
    qs = knns((H, H, H), (1, 3, 1024)) + knns((-H, -H, -H), (16,)) + knns(O, (17,)) + knns((H, -H, 0.0), (5,))
    qs += [("radius", (H, H, H), 0.0), ("radius", O, H), ("radius", (-H, 0.0, H), 2.0**499)]
    qs += [("box", (-H, -H, -H), (H, H, H)), ("box", (Hm, -H, -H), (H, H, H))]
    cases.append(("huge", pts, qs))
    # leaf-size boundaries and k around n
    for n in (15, 16, 17, 32, 33, 34):
        pts = rand_pts(g, n, 1.0)
        qs = []
        for _ in range(3):
            q = tuple(g.unit() for _ in range(3))
            qs += knns(q, (1, 5, 16, 17, 33, 1024)) + [("radius", q, 0.5)]
        qs += knns(pts[n // 2], (1, 2)) + [("radius", pts[n // 2], 0.0)]
        cases.append((f"n_{n}", pts, qs))
    for n in (1024, 1025):
        pts = rand_pts(g, n, 1.0)
        qs = knns((0.5, 0.5, 0.5), (1023, 1024)) + knns((2.0, 2.0, 2.0), (1024,)) + knns(pts[7], (1024,))
        cases.append((f"n_{n}", pts, qs))
    # collinear points with duplicates; queries exactly between grid points give ties
    pts = [((g.next() % 64) * 2.0**-8, 0.0, 0.0) for _ in range(300)]
    qs = []
    for x in (0.0, 0.125, 0.12890625, 0.5, 0.25 - 2.0**-9, 1.0):
        q = (x, 0.0, 0.0)
        qs += knns(q, (1, 16, 64)) + [("radius", q, 2.0**-8)]
    qs += knns((0.125, 0.5, -0.5), (16,)) + [("box", (0.125, 0.0, 0.0), (0.25, 0.0, 0.0))]
    cases.append(("collinear", pts, qs))
    # planar grid with equal x and y extents (axis tie: x before y), z extent 0
    pts = [(a * 0.0625, b * 0.0625, 0.0) for a in range(20) for b in range(20)]
    qs = knns((0.5, 0.5, 0.0), (1, 9, 25)) + knns((0.53125, 0.53125, 1.0), (4, 16))
    qs += [("radius", (0.5, 0.5, 0.0), 0.0625), ("box", (0.25, 0.25, 0.0), (0.5, 0.5, 0.0))]
    cases.append(("plane", pts, qs))
    # exact-tie lattice (2^-10 spacing, every distance exact), shuffled indexes
    side = 8
    cells = list(range(side**3))
    for i in range(len(cells) - 1, 0, -1):
        j = g.next() % (i + 1)
        cells[i], cells[j] = cells[j], cells[i]
    pts = []
    for cc in cells:
        a, rest = divmod(cc, side * side)
        b, z = divmod(rest, side)
        pts.append((a * 2.0**-10, b * 2.0**-10, z * 2.0**-10))
    qs = []
    for q in (pts[0], pts[100], (3.5 * 2.0**-10,) * 3, (0.0, 0.0, 0.0), (4 * 2.0**-10, 4 * 2.0**-10, 4.5 * 2.0**-10)):
        qs += knns(q, (7, 19, 26, 27, 64)) + [("radius", q, 2.0**-10), ("radius", q, 2.0**-9)]
    cases.append(("lattice_exact", pts, qs))
    # magnitudes from 2^-1074 to 2^500 in one cloud
    pts = [(wide(g), wide(g), wide(g)) for _ in range(3000)]
    qs = []
    for i in range(12):
        q = pts[g.next() % 3000] if i % 3 == 0 else (wide(g), wide(g), wide(g))
        qs += knns(q, (1, 16)) + [("radius", q, abs(wide(g)))]
    qs += knns(O, (1024,)) + [("radius", O, 0.0), ("box", (-1.0, -1.0, -1.0), (1.0, 1.0, 1.0))]
    qs += [("box", (-H, 0.0, -H), (H, H, 0.0))]
    cases.append(("wide_magnitude", pts, qs))
    # clustered cloud of 5000 points (many leaves, unbalanced density)
    centres = [tuple(g.unit() * 10.0 for _ in range(3)) for _ in range(5)]
    pts = []
    for _ in range(5000):
        c = centres[g.next() % 5]
        pts.append(tuple(c[a] + (g.unit() - 0.5) * 2.0**-6 for a in range(3)))
    qs = []
    for i in range(10):
        q = centres[i % 5] if i < 5 else tuple(g.unit() * 10.0 for _ in range(3))
        qs += knns(q, (1, 16, 1024)) + [("radius", q, 2.0**-8)]
        qs += [("box", tuple(x - 2.0**-8 for x in q), tuple(x + 2.0**-8 for x in q))]
    cases.append(("clusters_5000", pts, qs))
    return cases


def edge_text():
    global PTS
    lines = [
        "# GEO.INDEX.KDTREE edge cases: written by tools/fixtures/geo_index.py; do not edit.",
        "# case <name> <n>; p <x> <y> <z> (f64 bits, hex); index <depth> <sha256 of FIDKDT01 bytes>;",
        "# knn <qx> <qy> <qz> <k> = <count> [<index>:<dist2 bits>]...; radius <q> <r> = same;",
        "# box <min> <max> = <count> [<index>]...; end",
    ]
    for name, pts, qs in edge_cases():
        PTS = pts
        lines.append(f"case {name} {len(pts)}")
        for p in pts:
            lines.append("p " + " ".join(hx(x) for x in p))
        depth, dig = index_digest(pts)
        lines.append(f"index {depth} {dig}")
        for q in qs:
            kind = q[0]
            if kind == "box":
                res = in_box(q[1], q[2])
                head = " ".join(hx(x) for x in q[1] + q[2])
                lines.append(f"box {head} = {len(res)}" + "".join(f" {i}" for i in res))
                continue
            d2 = d2_all(q[1])
            head = " ".join(hx(x) for x in q[1])
            if kind == "knn":
                res = knn(d2, q[2])
                head += f" {q[2]}"
            else:
                res = radius(d2, q[2])
                head += f" {hx(q[2])}"
            lines.append(f"{kind} {head} = {len(res)}" + "".join(f" {i}:{bits(d):x}" for d, i in res))
        lines.append("end")
    return "\n".join(lines) + "\n"


# ------------------------------------------------------------------ main


def write_or_check(path, text, check, failures):
    if check:
        old = path.read_bytes().decode("utf-8") if path.exists() else None
        if old != text:
            failures.append(str(path))
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))


def large(name, cloud, fn, path, check, jobs, failures):
    pts, qs = cloud()
    head = large_header(name, pts, qs)
    nblocks = (len(qs) + BLOCK - 1) // BLOCK
    blocks = {CHECK_BLOCK} if check else set(range(nblocks))
    res = run_queries(pts, qs, fn, jobs, blocks)
    if check:
        old = path.read_bytes().decode("utf-8").splitlines() if path.exists() else []
        for ln in head:
            if ln not in old:
                failures.append(f"{path}: {ln}")
        for ln in res:
            if ln not in old:
                failures.append(f"{path}: {ln}")
        return
    write_or_check(path, "\n".join(head + res) + "\n", False, failures)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    args = ap.parse_args()
    failures = []
    write_or_check(OUT_EDGE, edge_text(), args.check, failures)
    large("uniform_1e5", uniform_cloud, _uniform_query, OUT_UNIFORM, args.check, args.jobs, failures)
    large("lattice_ties", lattice_cloud, _lattice_query, OUT_LATTICE, args.check, args.jobs, failures)
    if failures:
        for f in failures:
            print("MISMATCH", f, file=sys.stderr)
        return 1
    print("ok" if args.check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
