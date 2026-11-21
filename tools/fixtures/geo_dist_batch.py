#!/usr/bin/env python3
"""Independent reference for GEO.DIST.BATCH; writes the fixtures of the leaf's acceptance criteria.

Run:  python tools/fixtures/geo_dist_batch.py [--check]

Writes (or, with --check, regenerates and compares byte for byte):
  fixtures/geo/dist/scan_1e6_on_mesh/expected.txt  (AC-1, AC-4: 10^6 seeded points near the
                                                    GEO.INDEX sphere mesh; stream digest and
                                                    the closest point of every 10 000th point)
  fixtures/geo/dist/directional_cases.json         (AC-3: lines p + s v against small meshes,
                                                    with exact rational intersections)

Stdlib only (struct, hashlib, math.sqrt, fractions, decimal, json). It shares no code with
crates/fid-geo-dist. The sphere mesh, the SplitMix64 generator and the closest-point model
come from the GEO.DIST.MESH reference tools/fixtures/geo_dist.py, which is itself checked
there against exact rational arithmetic.

Directional oracle (exact): for each triangle with n = (b - a) x (c - a) and n . v != 0, the
line meets the triangle's plane at s = n . (a - p) / (n . v), the point X = p + s v. X counts
when its barycentric coordinates are all >= 0 (closed triangle). A line parallel to a plane
(n . v == 0) never counts. The nearest X has the least s^2 |v|^2. The case records whether
that triangle is unique (several triangles meet at a shared edge or vertex). Random cases
are kept only when they are well conditioned: no triangle has a barycentric coordinate
within 1e-9 of zero, unless the case is one of the designed exact edge or vertex cases.
"""

import argparse
import decimal
import hashlib
import json
import math
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import geo_dist  # noqa: E402  (the GEO.DIST.MESH reference, same directory)

ROOT = HERE.parent.parent
OUT_SCAN = ROOT / "fixtures" / "geo" / "dist" / "scan_1e6_on_mesh" / "expected.txt"
OUT_DIR = ROOT / "fixtures" / "geo" / "dist" / "directional_cases.json"

SEED_SCAN = 51
N_SCAN = 1_000_000
SAMPLE_EVERY = 10_000
SEED_DIR = 0x6765_6F2E_6261_7463  # "geo.batc"


def hx(x):
    return f"{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def fbits(x):
    return struct.pack("<d", x)


# ------------------------------------------------------------------ AC-1 scan


def scan_points():
    """Pinned stream: SplitMix64(51); per point three uni(-1, 1) for a direction d and one
    uni(0.97, 1.03) for a radius r; the point is d * (r / |d|), |d| = sqrt((dx dx + dy dy) + dz dz)."""
    g = geo_dist.SplitMix(SEED_SCAN)
    out = []
    for _ in range(N_SCAN):
        d = (g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0))
        r = g.uni(0.97, 1.03)
        ln = math.sqrt((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2])
        s = r / ln
        out.append((d[0] * s, d[1] * s, d[2] * s))
    return out


def scan_text():
    verts, tris = geo_dist.sphere_mesh()
    pts = scan_points()
    h = hashlib.sha256()
    for p in pts:
        h.update(fbits(p[0]) + fbits(p[1]) + fbits(p[2]))
    geo_dist._init(verts, tris)
    lines = [
        "# GEO.DIST.BATCH fixture scan_1e6_on_mesh (AC-1, AC-4). Written by tools/fixtures/geo_dist_batch.py;",
        "# never edit by hand. CC-BY-4.0. Mesh: the GEO.INDEX sphere (fixtures/geo/index/mesh_sphere_1e5).",
        "# Points: SplitMix64 seed 51; see scan_points() for the pinned stream.",
        "# r <i> <tri> <foot xyz> <dist> : the closest point of every 10 000th point (GEO.DIST.MESH model).",
        f"triangles {len(tris)}",
        f"vertices {len(verts)}",
        f"mesh_digest {geo_dist.mesh_digest(verts, tris)}",
        f"points {N_SCAN}",
        f"point_digest {h.hexdigest()}",
    ]
    for i in range(0, N_SCAN, SAMPLE_EVERY):
        f, d2, _code, t = geo_dist.search(pts[i])
        lines.append(f"r {i} {t} {hx(f[0])} {hx(f[1])} {hx(f[2])} {hx(math.sqrt(d2))}")
    return "\n".join(lines) + "\n"


# ------------------------------------------------------------------ AC-3 directional

def F3(v):
    return [Fraction(x) for x in v]


def fsub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def fdot(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def fcross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def exact_line(verts, tris, p, v, subset=None):
    """[(s, tri, min_bary)] for every triangle the line meets, plus the least |min_bary| over
    all non-parallel triangles (the conditioning of the case)."""
    P, V = F3(p), F3(v)
    hits = []
    margin = None
    for t, (ia, ib, ic) in enumerate(tris):
        if subset is not None and t not in subset:
            continue
        A, B, C = F3(verts[ia]), F3(verts[ib]), F3(verts[ic])
        n = fcross(fsub(B, A), fsub(C, A))
        den = fdot(n, V)
        if den == 0:
            continue
        s = fdot(n, fsub(A, P)) / den
        X = [P[k] + s * V[k] for k in range(3)]
        nn = fdot(n, n)
        l0 = fdot(fcross(fsub(B, X), fsub(C, X)), n) / nn
        l1 = fdot(fcross(fsub(C, X), fsub(A, X)), n) / nn
        l2 = 1 - l0 - l1
        mb = min(l0, l1, l2)
        m = abs(mb)
        margin = m if margin is None or m < margin else margin
        if mb >= 0:
            hits.append((s, t, X))
    return hits, margin


def dsqrt(fr):
    """sqrt of a non-negative Fraction to 40 significant digits, rounded to the nearest float."""
    with decimal.localcontext() as ctx:
        ctx.prec = 60
        x = decimal.Decimal(fr.numerator) / decimal.Decimal(fr.denominator)
        return float(x.sqrt())


def solve(verts, tris, p, v, max_d, subset=None):
    hits, margin = exact_line(verts, tris, p, v, subset)
    V = F3(v)
    vv = fdot(V, V)
    case = {"p": [hx(x) for x in p], "v": [hx(x) for x in v], "max_distance": None if max_d is None else hx(max_d)}
    if subset is not None:
        case["subset"] = sorted(subset)
    if not hits:
        case["hit"] = False
        return case, margin
    best = min(h[0] * h[0] for h in hits)
    win = sorted(h for h in hits if h[0] * h[0] == best)
    s, t, X = win[0][0], win[0][1], win[0][2]
    d2 = s * s * vv
    if max_d is not None and d2 > Fraction(max_d) ** 2:
        case["hit"] = False
        return case, margin
    dist = dsqrt(d2)
    case["hit"] = True
    case["foot"] = [hx(float(x)) for x in X]
    case["distance"] = hx(dist)
    case["signed"] = hx(dist if s >= 0 else -dist)
    case["tri"] = t if len({h[1] for h in win}) == 1 else None
    return case, margin


def cube():
    vs = [(x, y, z) for x in (-1.0, 1.0) for y in (-1.0, 1.0) for z in (-1.0, 1.0)]
    idx = {v: i for i, v in enumerate(vs)}
    q = lambda a, b, c, d: [(idx[a], idx[b], idx[c]), (idx[a], idx[c], idx[d])]  # noqa: E731
    tris = []
    tris += q((-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1))  # top, outward +z
    tris += q((-1, -1, -1), (-1, 1, -1), (1, 1, -1), (1, -1, -1))  # bottom
    tris += q((1, -1, -1), (1, 1, -1), (1, 1, 1), (1, -1, 1))  # +x
    tris += q((-1, -1, -1), (-1, -1, 1), (-1, 1, 1), (-1, 1, -1))  # -x
    tris += q((-1, 1, -1), (-1, 1, 1), (1, 1, 1), (1, 1, -1))  # +y
    tris += q((-1, -1, -1), (1, -1, -1), (1, -1, 1), (-1, -1, 1))  # -y
    return [list(v) for v in vs], [list(t) for t in tris]


def tetra():
    vs = [[0.3, -0.2, 0.1], [2.1, 0.4, -0.3], [0.7, 1.9, 0.2], [0.9, 0.6, 1.7]]
    tris = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]]
    return vs, tris


BOXES = {"cube": ((-1.2, -1.2, -1.2), (1.2, 1.2, 1.2)), "tetra": ((0.3, -0.2, -0.3), (2.1, 1.9, 1.7))}


def directional_cases():
    meshes = {"cube": cube(), "tetra": tetra()}
    cases = []

    def add(mesh, p, v, max_d=None, subset=None, designed=False):
        vs, ts = meshes[mesh]
        c, margin = solve(vs, ts, p, v, max_d, subset)
        if not designed and margin is not None and margin < Fraction(1, 10**9):
            return False
        c["mesh"] = mesh
        c["designed"] = designed
        cases.append(c)
        return True

    # Designed cases on the cube (exact in binary64).
    add("cube", (0.5, 0.25, 3.0), (0.0, 0.0, -1.0), designed=True)  # top interior, s = 2
    add("cube", (0.5, 0.25, 3.0), (0.0, 0.0, -1.0), max_d=1.5, designed=True)  # beyond: no hit
    add("cube", (0.5, 0.25, 3.0), (0.0, 0.0, -1.0), max_d=2.0, designed=True)  # exactly at max
    add("cube", (0.5, 0.25, 3.0), (0.0, 0.0, 1.0), designed=True)  # negative s
    add("cube", (0.5, 0.5, 3.0), (0.0, 0.0, -1.0), designed=True)  # top diagonal, shared edge
    add("cube", (1.0, 1.0, 3.0), (0.0, 0.0, -1.0), designed=True)  # corner vertex
    add("cube", (0.1, 0.2, 0.3), (0.0, 0.0, 1.0), designed=True)  # inside: top at +0.7
    add("cube", (0.1, 0.2, 0.3), (0.0, 0.0, -1.0), designed=True)  # inside: top at -0.7
    add("cube", (0.1, 0.2, -0.3), (0.0, 0.0, 1.0), designed=True)  # inside: bottom at -0.7
    add("cube", (0.25, 0.5, 5.0), (0.0, 0.0, -1e-300), designed=True)  # tiny direction
    add("cube", (0.25, 0.5, 5.0), (0.0, 0.0, -3.0e5), designed=True)  # large direction
    add("cube", (3.0, 3.0, 3.0), (1.0, 0.0, 0.0), designed=True)  # misses
    add("cube", (0.5, 0.25, 3.0), (0.0, 0.0, -1.0), subset={2, 3}, designed=True)  # only bottom
    add("cube", (0.5, 0.25, 3.0), (0.0, 0.0, -1.0), subset={4, 5}, designed=True)  # +x only: parallel
    add("cube", (2.0, 0.5, 0.25), (-1.0, 0.0, 0.0), designed=True)  # +x face from outside

    g = geo_dist.SplitMix(SEED_DIR)
    want = {"cube": 60, "tetra": 60}
    for mesh in ("cube", "tetra"):
        got = 0
        while got < want[mesh]:
            p = (g.uni(-3.0, 3.0), g.uni(-3.0, 3.0), g.uni(-3.0, 3.0))
            # Aim at a point of the mesh's box most of the time, so most lines hit; the
            # direction is the float difference, scaled by a random factor (sign included).
            lo, hi = BOXES[mesh]
            q = tuple(g.uni(lo[k], hi[k]) for k in range(3))
            k = g.uni(-2.0, 2.0)
            v = tuple((q[j] - p[j]) * k for j in range(3))
            max_d = None
            if g.below(4) == 0:
                max_d = g.uni(0.0, 4.0)
            if add(mesh, p, v, max_d):
                got += 1
    about = (
        "GEO.DIST.BATCH AC-3. Written by tools/fixtures/geo_dist_batch.py; never edit by hand. CC-BY-4.0. "
        "Numbers are binary64 bit patterns in hex (u64 of to_bits). Lines p + s v, s of either sign. "
        "foot: the exact intersection rounded per axis; distance: |s| |v| and signed: s |v| (sign of s), "
        "both rounded from 60-digit decimals; tri: the winning triangle, null when several share the nearest point."
    )
    out = {"about": about, "meshes": {}, "cases": cases}
    for name, (vs, ts) in meshes.items():
        out["meshes"][name] = {"vertices": [[hx(x) for x in v] for v in vs], "triangles": ts}
    return json.dumps(out, indent=1, sort_keys=True) + "\n"


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
    args = ap.parse_args()
    failures = []
    write_or_check(OUT_DIR, directional_cases(), args.check, failures)
    write_or_check(OUT_SCAN, scan_text(), args.check, failures)
    if failures:
        for f in failures:
            print("MISMATCH", f, file=sys.stderr)
        return 1
    print("ok" if args.check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
