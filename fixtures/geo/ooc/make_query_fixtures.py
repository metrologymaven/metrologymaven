#!/usr/bin/env python3
"""Independent reference for GEO.OOC.QUERY (CC-BY-4.0 project data).

Writes
  fixtures/geo/ooc/boundary_ties/cases.json
      AC-2: a small cloud whose points are mirrored across the partition's split planes at
      equal distances, the queries on those planes and their brute-force results.
  fixtures/geo/ooc/query_vectors.json
      AC-1 (pinned subset): 60 seeded queries (k-nearest with k = 1, 16, 1024, radius 1 mm
      and 1 cm, boxes) on the 200 000-point prefix of fixture cloud_5e6 (the size plain `cargo test`
      uses), half of them on planes of the partition grid, with the brute-force result of
      each as a count and a digest.

Run:  python fixtures/geo/ooc/make_query_fixtures.py [--check] [--jobs N]

Without --check it rewrites both files; with --check it regenerates them and compares the
bytes with the committed files (exit 1 on any difference).

Stdlib only. It shares no code with crates/fid-geo-ooc or crates/fid-geo-index. The cloud
generator is the GEO.OOC.LAYOUT reference make_layout_fixtures.py next to this file. The
results are computed by brute force from the rules GEO.INDEX pins (crate fid-geo-index,
spec node GEO.INDEX.KDTREE), which a cross-chunk query must reproduce exactly
(REQ-GEO.OOC.QUERY-001):

* squared distance d2 = (dx*dx + dy*dy) + dz*dz with dx = p.x - q.x, in IEEE binary64
  (CPython floats round to nearest even and never fuse);
* knn(q, k): the min(k, n) smallest pairs (d2, original index), ascending;
* radius(q, r): every point with d2 <= r*r (r*r rounded once), sorted by (d2, index);
* box(lo, hi): every point with lo_k <= p_k <= hi_k on every axis, ascending by index.

Result digest v1 = SHA-256 over the result in order: per neighbour, index u64 | d2 as raw
IEEE 754 f64 bits; per box hit, index u64; all little-endian.

Query generator (pinned, so the queries are reproducible here): SplitMix64 from seed 62;
u() = (next() >> 11) * 2^-53. A query point starts at cloud point j = next() % n and moves
by (u() - 0.5) * 0.002 per axis, so it lies near the scanned surface; every odd query then
moves one axis (next() % 3) onto the grid plane lo + (ext * m) / 16 with m = 1 + next() % 15
(lo, ext of the cloud's bounding box, the partition bounds). Query kinds cycle knn1, knn16,
knn1024, radius (r = 0.001 m, the AC-1 radius, or 0.01 m on every other radius query, since
1 mm spheres around the 200 000-point prefix hold few points), box (half side 0.004 or
0.02 m by next() % 2). The query points themselves are written to the file.
"""

import argparse
import hashlib
import heapq
import json
import os
import struct
import sys
from multiprocessing import Pool
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import make_layout_fixtures as L  # noqa: E402

OUT_TIES = HERE / "boundary_ties" / "cases.json"
OUT_VECTORS = HERE / "query_vectors.json"
SEED = 62
N_QUERIES = 60
MASK64 = (1 << 64) - 1


def bits64(v):
    return struct.unpack("<Q", struct.pack("<d", v))[0]


def hex64(v):
    return f"{bits64(v):016x}"


def d2(p, q):
    dx = p[0] - q[0]
    dy = p[1] - q[1]
    dz = p[2] - q[2]
    return (dx * dx + dy * dy) + dz * dz


def knn(pts, q, k):
    return heapq.nsmallest(k, ((d2(p, q), i) for i, p in enumerate(pts)))


def radius(pts, q, r):
    r2 = r * r
    return sorted((d, i) for d, i in ((d2(p, q), i) for i, p in enumerate(pts)) if d <= r2)


def in_box(pts, lo, hi):
    return [i for i, p in enumerate(pts) if all(lo[k] <= p[k] <= hi[k] for k in range(3))]


def digest_neighbours(res):
    h = hashlib.sha256()
    for d, i in res:
        h.update(struct.pack("<Qd", i, d))
    return h.hexdigest()


def digest_hits(res):
    h = hashlib.sha256()
    for i in res:
        h.update(struct.pack("<Q", i))
    return h.hexdigest()


def dump(obj):
    return json.dumps(obj, indent=1, sort_keys=True) + "\n"


# ------------------------------------------------------------------ AC-2 boundary_ties


def boundary_ties():
    """Bounds [0, 8]^3 (fixed by two corner points), max_chunk_points 4, so the root splits
    at x = y = z = 4 exactly (s = (4 / 8) * 2^21 = 2^20, the upper cells' first plane) and
    deeper cells split on the planes 2 and 6. Pairs are mirrored across x = 4, y = 4 or
    z = 4 at equal distance from the plane; within every other pair the point in the upper
    cell gets the lower original index, so a correct merge must order ties by original
    index, not by chunk."""
    pts = []
    queries = []
    mirrored = [
        (0, 0.5, (1.0, 1.0)),
        (0, 1.0, (1.0, 3.0)),
        (0, 1.5, (3.0, 1.0)),
        (0, 0.25, (3.0, 3.0)),
        (0, 2.0, (5.0, 5.0)),
        (0, 0.75, (6.0, 2.0)),
        (1, 0.5, (1.0, 6.0)),
        (1, 1.25, (7.0, 1.0)),
        (2, 0.5, (2.0, 2.0)),
        (2, 1.0, (6.0, 7.0)),
    ]
    for n, (axis, off, (a, b)) in enumerate(mirrored):
        others = [k for k in range(3) if k != axis]

        def at(v):
            p = [0.0, 0.0, 0.0]
            p[axis] = v
            p[others[0]] = a
            p[others[1]] = b
            return tuple(p)

        lower, upper = at(4.0 - off), at(4.0 + off)
        pts.extend([upper, lower] if n % 2 == 0 else [lower, upper])
        queries.append({"kind": "knn", "k": 2, "point": at(4.0)})
        queries.append({"kind": "radius", "r": off, "point": at(4.0)})
    # An octet mirrored across all three planes around the centre.
    for o in (7, 0, 5, 2, 3, 4, 1, 6):
        pts.append(tuple(4.0 + (0.25 if (o >> k) & 1 else -0.25) for k in range(3)))
    centre = (4.0, 4.0, 4.0)
    for k in (2, 8, 9):
        queries.append({"kind": "knn", "k": k, "point": centre})
    queries.append({"kind": "box", "lo": (3.0, 0.0, 0.0), "hi": (5.0, 8.0, 8.0)})
    queries.append({"kind": "box", "lo": (4.0, 4.0, 4.0), "hi": (4.25, 4.25, 4.25)})
    pts.append((0.0, 0.0, 0.0))
    pts.append((8.0, 8.0, 8.0))

    cases = []
    for q in queries:
        if q["kind"] == "knn":
            res = knn(pts, q["point"], q["k"])
        elif q["kind"] == "radius":
            res = radius(pts, q["point"], q["r"])
        else:
            res = None
            hits = in_box(pts, q["lo"], q["hi"])
        entry = {"kind": q["kind"]}
        if q["kind"] == "box":
            entry["lo"] = [hex64(v) for v in q["lo"]]
            entry["hi"] = [hex64(v) for v in q["hi"]]
            entry["expected"] = hits
        else:
            entry["point"] = [hex64(v) for v in q["point"]]
            if q["kind"] == "knn":
                entry["k"] = q["k"]
            else:
                entry["r"] = hex64(q["r"])
            entry["expected"] = [[i, hex64(d)] for d, i in res]
        cases.append(entry)
    return {
        "comment": "GEO.OOC.QUERY AC-2 fixture boundary_ties from make_query_fixtures.py (independent brute-force "
        "reference, seed 62 family). CC-BY-4.0. Points and floats as raw IEEE 754 bits (hex); expected knn/radius "
        "results as [original index, d2 bits] in result order, box results as original indexes.",
        "max_chunk_points": 4,
        "points": [[hex64(v) for v in p] for p in pts],
        "cases": cases,
    }


# ------------------------------------------------------------------ AC-1 pinned subset


def splitmix64(seed):
    state = seed & MASK64
    while True:
        state = (state + 0x9E3779B97F4A7C15) & MASK64
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
        yield z ^ (z >> 31)


def prefix_points(jobs, n):
    step = 50_000
    blocks = [(s, min(s + step, n)) for s in range(0, n, step)]
    with Pool(jobs) as pool:
        parts = pool.map(L.scan_block, blocks)
    return [r[0] for part in parts for r in part]


def query_vectors(jobs):
    n = L.N_SMALL
    pts = prefix_points(jobs, n)
    lo, hi = L.tight_box(pts)
    ext = tuple(hi[k] - lo[k] for k in range(3))
    rng = splitmix64(SEED)
    kinds = ["knn1", "knn16", "knn1024", "radius", "box"]
    out = []
    for i in range(N_QUERIES):
        base = pts[next(rng) % n]
        q = [base[k] + (((next(rng) >> 11) * 2.0**-53) - 0.5) * 0.002 for k in range(3)]
        if i % 2 == 1:
            axis = next(rng) % 3
            m = 1 + next(rng) % 15
            q[axis] = lo[axis] + (ext[axis] * m) / 16.0
        kind = kinds[i % 5]
        entry = {"kind": kind, "point": [hex64(v) for v in q]}
        if kind.startswith("knn"):
            k = int(kind[3:])
            res = knn(pts, q, k)
            entry["k"] = k
            entry["count"] = len(res)
            entry["digest"] = digest_neighbours(res)
            entry["first"] = [res[0][1], hex64(res[0][0])]
        elif kind == "radius":
            r = 0.001 if (i // 5) % 2 == 0 else 0.01
            res = radius(pts, q, r)
            entry["r"] = hex64(r)
            entry["count"] = len(res)
            entry["digest"] = digest_neighbours(res)
        else:
            h = 0.004 if next(rng) % 2 == 0 else 0.02
            blo = [v - h for v in q]
            bhi = [v + h for v in q]
            res = in_box(pts, blo, bhi)
            entry["lo"] = [hex64(v) for v in blo]
            entry["hi"] = [hex64(v) for v in bhi]
            entry["count"] = len(res)
            entry["digest"] = digest_hits(res)
        out.append(entry)
    return {
        "comment": "GEO.OOC.QUERY AC-1 pinned subset from make_query_fixtures.py (independent brute-force reference). "
        "CC-BY-4.0. Cloud: the first 200 000 points of fixture cloud_5e6 (seed 61). Queries: SplitMix64 seed 62 (see "
        "the script). Result digest v1: SHA-256 over per neighbour index u64 | d2 raw f64 bits, per box hit index "
        "u64, little-endian.",
        "n_points": n,
        "aabb_min": [hex64(v) for v in lo],
        "aabb_max": [hex64(v) for v in hi],
        "queries": out,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    args = ap.parse_args()
    files = [(OUT_TIES, dump(boundary_ties())), (OUT_VECTORS, dump(query_vectors(args.jobs)))]
    if args.check:
        bad = [p.name for p, text in files if not p.exists() or p.read_text(encoding="utf-8") != text]
        if bad:
            print("differs from the reference: " + ", ".join(bad), file=sys.stderr)
        sys.exit(1 if bad else 0)
    for p, text in files:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
