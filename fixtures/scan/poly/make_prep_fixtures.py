#!/usr/bin/env python3
"""Independent reference for SCAN.POLY.SURF.PREP (vertex-set preparation). CC0-1.0 data.

Stdlib only. Writes three seeded synthetic clouds and the expected vertex sets of four
cases into this folder; `--check` regenerates everything in memory and fails on any
difference with the committed files. Nothing here was produced by running the Rust crate
`fid-scan-poly`.

Clouds (invented data, generated with integer splitmix64 and IEEE `+ - * /` only, so the
bytes are the same on every platform; no transcendental function touches a written value):

* `plane_dense`: a 60 x 60 grid at 0.1 mm spacing on the tilted plane
  z = A*x + B*y + C (no noise), points shuffled.
* `plane_noisy`: the same grid with vertical noise of sigma = 20 um (Irwin-Hall: the sum of
  twelve uniforms minus 6, times sigma).
* `cube_edges`: the six faces of a 5.5 mm cube, a 22 x 22 grid per face at 0.25 mm,
  half a step in from every edge, with noise of sigma = 10 um along the face normal.

Every point carries the index of an exact unit normal (the true plane normal, or the outward
face normal). Every point whose index before shuffling is 7 mod 23 has an invalid normal.

The expected vertex sets follow REQ-SCAN.POLY.SURF.PREP-001..003 and the pinned rules of the
crate's `prep` module documentation, computed here a different way:

* Selection (exact, compared bit for bit): cells are a dict keyed by
  (floor((x - ox) / h), floor((y - oy) / h), floor((z - oz) / h)) with o the bounding-box
  minimum of all points; cells are ordered by a Morton key built as a bit string (z, y, x
  bits from bit 31 down); the centroid of a cell is the IEEE sum of its valid points in
  ascending index order divided by their count; the representative is the valid point with
  the smallest (dx*dx + dy*dy) + dz*dz, lowest index on ties.
* Smoothing (compared within 1e-12 m): neighbours are found by brute force, every valid point
  with (dx*dx + dy*dy) + dz*dz <= (2h)*(2h). The least-squares plane is computed in exact
  integer arithmetic (every coordinate scaled by 2^1074) and its normal by inverse
  iteration in 80-digit decimal arithmetic, where the crate uses f64 sums and the fid-num
  cyclic Jacobi eigensolver. The displacement is clamped to (h/2)(1 - 2^-40).
* Normals: every vertex carries its source point's normal unchanged, moved or not.
* Sharp edges: with preservation on, a representative is kept in place when any valid
  neighbour's normal has a dot product with its own normal below cos(30 degrees). The
  fixture dot products are 0, 1 or 1 +- 1e-16, so the decision does not depend on how the
  cosine is evaluated.
"""

import math
import os
import sys
from decimal import Decimal, getcontext

HERE = os.path.dirname(os.path.abspath(__file__))
M64 = (1 << 64) - 1
getcontext().prec = 80

PLANE_A = 0.25
PLANE_B = -0.15
PLANE_C = 0.0075
PLANE_X0 = 0.012
PLANE_Y0 = -0.034
PLANE_N = 60
PLANE_STEP = 1e-4

CUBE_O = (0.021, -0.0045, 0.0132)
CUBE_SIDE = 0.0055
CUBE_N = 22
CUBE_STEP = 0.00025

H = 1e-3
EDGE_ANGLE_RAD = math.pi / 6
CLAMP_FACTOR = 1.0 - 2.0 ** -40
SCALE = 1 << 1074
TOL_MIN_NEIGHBOURS = 3


class Rng:
    """splitmix64 (integer only)."""

    def __init__(self, seed):
        self.s = seed & M64

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & M64
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
        return z ^ (z >> 31)

    def uniform(self):
        return (self.next() >> 11) * (1.0 / 9007199254740992.0)

    def gauss(self):
        s = 0.0
        for _ in range(12):
            s = s + self.uniform()
        return s - 6.0


def shuffle(items, rng):
    for i in range(len(items) - 1, 0, -1):
        j = rng.next() % (i + 1)
        items[i], items[j] = items[j], items[i]


def plane_cloud(sigma, seed):
    rng = Rng(seed)
    ln = math.sqrt(PLANE_A * PLANE_A + PLANE_B * PLANE_B + 1.0)
    normal = (-PLANE_A / ln, -PLANE_B / ln, 1.0 / ln)
    pts = []
    for i in range(PLANE_N):
        for j in range(PLANE_N):
            x = PLANE_X0 + i * PLANE_STEP
            y = PLANE_Y0 + j * PLANE_STEP
            z = PLANE_A * x + PLANE_B * y + PLANE_C
            if sigma:
                z = z + sigma * rng.gauss()
            pts.append([x, y, z, 0])
    for k, p in enumerate(pts):
        p.append(0 if k % 23 == 7 else 1)
    shuffle(pts, rng)
    return [normal], pts


def cube_cloud(sigma, seed):
    rng = Rng(seed)
    normals = []
    pts = []
    for axis in range(3):
        for side in (0, 1):
            n = [0.0, 0.0, 0.0]
            n[axis] = 1.0 if side else -1.0
            normals.append(tuple(n))
            k = len(normals) - 1
            u_axis, v_axis = [a for a in range(3) if a != axis]
            for i in range(CUBE_N):
                for j in range(CUBE_N):
                    p = [0.0, 0.0, 0.0]
                    p[u_axis] = CUBE_O[u_axis] + (i + 0.5) * CUBE_STEP
                    p[v_axis] = CUBE_O[v_axis] + (j + 0.5) * CUBE_STEP
                    p[axis] = CUBE_O[axis] + (CUBE_SIDE if side else 0.0) + sigma * rng.gauss()
                    pts.append(p + [k])
    for k, p in enumerate(pts):
        p.append(0 if k % 23 == 7 else 1)
    shuffle(pts, rng)
    return normals, pts


def cloud_text(name, desc, normals, pts):
    out = ["# SCAN.POLY.SURF.PREP fixture cloud, written by make_prep_fixtures.py (CC0-1.0).",
           f"# {desc}",
           "# Lines after 'points': x y z normal_index valid (metres; valid 1 = normal valid).",
           f"cloud {name}",
           f"normals {len(normals)}"]
    for n in normals:
        out.append(f"{n[0]!r} {n[1]!r} {n[2]!r}")
    out.append(f"points {len(pts)}")
    for p in pts:
        out.append(f"{p[0]!r} {p[1]!r} {p[2]!r} {p[3]} {p[4]}")
    return "\n".join(out) + "\n"


def d2(p, q):
    dx = p[0] - q[0]
    dy = p[1] - q[1]
    dz = p[2] - q[2]
    return (dx * dx + dy * dy) + dz * dz


def morton_key(c):
    bits = []
    for b in range(31, -1, -1):
        bits.append(str((c[2] >> b) & 1))
        bits.append(str((c[1] >> b) & 1))
        bits.append(str((c[0] >> b) & 1))
    return int("".join(bits), 2)


def select(pts, h):
    o = [min(p[a] for p in pts) for a in range(3)]
    cells = {}
    for i, p in enumerate(pts):
        if not p[4]:
            continue
        c = tuple(int(math.floor((p[a] - o[a]) / h)) for a in range(3))
        cells.setdefault(c, []).append(i)
    reps = []
    for c in sorted(cells, key=morton_key):
        members = sorted(cells[c])
        sx = sy = sz = 0.0
        for i in members:
            sx = sx + pts[i][0]
            sy = sy + pts[i][1]
            sz = sz + pts[i][2]
        k = float(len(members))
        cen = (sx / k, sy / k, sz / k)
        best = None
        for i in members:
            dd = d2(pts[i], cen)
            if best is None or dd < best[0]:
                best = (dd, i)
        reps.append((c, best[1]))
    return reps


def to_int(x):
    num, den = x.as_integer_ratio()
    return num * (SCALE // den)


def solve3(m, b):
    """Gaussian elimination with partial pivoting in Decimal."""
    a = [list(m[r]) + [b[r]] for r in range(3)]
    for col in range(3):
        piv = max(range(col, 3), key=lambda r: abs(a[r][col]))
        a[col], a[piv] = a[piv], a[col]
        for r in range(col + 1, 3):
            f = a[r][col] / a[col][col]
            for c in range(col, 4):
                a[r][c] = a[r][c] - f * a[col][c]
    x = [Decimal(0)] * 3
    for r in range(2, -1, -1):
        s = a[r][3]
        for c in range(r + 1, 3):
            s = s - a[r][c] * x[c]
        x[r] = s / a[r][r]
    return x


def unit(v):
    ln = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    return [c / ln for c in v]


def mat_vec(m, v):
    return [m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2] for r in range(3)]


def dot(u, v):
    return u[0] * v[0] + u[1] * v[1] + u[2] * v[2]


def plane_fit(pts, idx):
    """Exact centroid and covariance; smallest-eigenvalue normal by inverse iteration."""
    k = len(idx)
    ints = [[to_int(pts[i][a]) for a in range(3)] for i in idx]
    s = [sum(q[a] for q in ints) for a in range(3)]
    kc = [[k * sum(q[r] * q[c] for q in ints) - s[r] * s[c] for c in range(3)] for r in range(3)]
    m = [[Decimal(kc[r][c]) for c in range(3)] for r in range(3)]
    # Power iteration for the largest eigenvalue (scale and conditioning checks only).
    v = unit([Decimal(1), Decimal("0.6180339887"), Decimal("0.4142135623")])
    for _ in range(200):
        v = unit(mat_vec(m, v))
    lmax = dot(v, mat_vec(m, v))
    # Inverse iteration for the smallest.
    n = unit([Decimal("0.3"), Decimal("-0.7"), Decimal("1")])
    for _ in range(60):
        n = unit(solve3(m, n))
    lmin = dot(n, mat_vec(m, n))
    lmid = m[0][0] + m[1][1] + m[2][2] - lmax - lmin
    assert lmid > lmax * Decimal("1e-6"), "degenerate neighbourhood in a fixture"
    assert lmin < lmid * Decimal("1e-2"), "poorly separated plane normal in a fixture"
    cen = [Decimal(s[a]) / (Decimal(k) * Decimal(SCALE)) for a in range(3)]
    return cen, n


def smooth(pts, normals, rep, h, sharp):
    p = pts[rep]
    r = 2.0 * h
    r2 = r * r
    nb = [i for i in range(len(pts)) if pts[i][4] and d2(pts[i], p) <= r2]
    if sharp:
        nr = normals[p[3]]
        thr = math.cos(EDGE_ANGLE_RAD)
        for i in nb:
            nq = normals[pts[i][3]]
            dd = (nr[0] * nq[0] + nr[1] * nq[1]) + nr[2] * nq[2]
            assert abs(dd - thr) > 1e-6
            if dd < thr:
                return "kept_edge", (p[0], p[1], p[2])
    if len(nb) < TOL_MIN_NEIGHBOURS:
        return "kept_few", (p[0], p[1], p[2])
    cen, n = plane_fit(pts, nb)
    pd = [Decimal(p[a]) for a in range(3)]
    dist = dot(n, [pd[a] - cen[a] for a in range(3)])
    delta = [-dist * n[a] for a in range(3)]
    ln = abs(dist)
    rho = Decimal(h * 0.5) * Decimal(CLAMP_FACTOR)
    status = "smoothed"
    if ln > rho:
        delta = [d * (rho / ln) for d in delta]
        status = "clamped"
    assert status == "smoothed", "no fixture vertex should need the clamp"
    return status, tuple(float(pd[a] + delta[a]) for a in range(3))


def cube_edge_distance(p):
    """Distance from p to the nearest of the 12 cube edges (segments)."""
    best = None
    lo = CUBE_O
    hi = tuple(CUBE_O[a] + CUBE_SIDE for a in range(3))
    for axis in range(3):
        u, v = [a for a in range(3) if a != axis]
        for cu in (lo[u], hi[u]):
            for cv in (lo[v], hi[v]):
                t = min(max(p[axis], lo[axis]), hi[axis])
                q = [0.0, 0.0, 0.0]
                q[axis] = t
                q[u] = cu
                q[v] = cv
                dd = math.sqrt(sum((p[a] - q[a]) ** 2 for a in range(3)))
                best = dd if best is None else min(best, dd)
    return best


def rms_to_plane(points):
    ln = math.sqrt(PLANE_A * PLANE_A + PLANE_B * PLANE_B + 1.0)
    s = 0.0
    for p in points:
        e = (PLANE_A * p[0] + PLANE_B * p[1] - p[2] + PLANE_C) / ln
        s += e * e
    return math.sqrt(s / len(points))


def case_text(case, cloud, pts, normals, h, smoothing, sharp, plane):
    reps = select(pts, h)
    rows = []
    for c, i in reps:
        p = pts[i]
        if smoothing:
            status, pos = smooth(pts, normals, i, h, sharp)
        else:
            status, pos = "unmoved", (p[0], p[1], p[2])
        if cloud == "cube_edges":
            de = cube_edge_distance(p)
            assert abs(de - h) > 1e-9
            near = "1" if de <= h else "0"
        else:
            near = "-"
        rows.append((i, c, status, pos, near))
    out = ["# SCAN.POLY.SURF.PREP expected vertex set, written by make_prep_fixtures.py (CC0-1.0).",
           "# Lines after 'vertices': source cx cy cz status x y z nx ny nz near_edge (n: the source",
           "# point's normal, kept by every vertex; near_edge: within h of a cube edge, '-' when not",
           "# a cube). Smoothed positions are within 1e-12 m; everything else is exact.",
           f"case {case}",
           f"cloud {cloud}",
           f"h {h!r}",
           f"smoothing {int(smoothing)}",
           f"sharp_edges {int(sharp)}",
           f"edge_angle_rad {EDGE_ANGLE_RAD!r}",
           f"vertices {len(rows)}"]
    for i, c, status, pos, near in rows:
        n = normals[pts[i][3]]
        out.append(f"{i} {c[0]} {c[1]} {c[2]} {status} {pos[0]!r} {pos[1]!r} {pos[2]!r} "
                   f"{n[0]!r} {n[1]!r} {n[2]!r} {near}")
    if plane:
        before = rms_to_plane([pts[i] for i, *_ in rows])
        after = rms_to_plane([r[3] for r in rows])
        out.append(f"# rms to the true plane: before {before:.3e} m, after {after:.3e} m")
        if smoothing:
            assert after <= 0.5 * before, "AC-2 precondition"
    moved = sum(1 for r in rows if r[2] in ("smoothed", "clamped"))
    edge = sum(1 for r in rows if r[2] == "kept_edge")
    out.append(f"# moved {moved}, kept at edges {edge}")
    if cloud == "cube_edges":
        assert all(r[2] == "kept_edge" for r in rows if r[4] == "1"), "AC-3 precondition"
        assert moved > 0 and any(r[4] == "1" for r in rows)
    return "\n".join(out) + "\n"


def self_check():
    assert morton_key((1, 0, 0)) == 1 and morton_key((0, 1, 0)) == 2 and morton_key((0, 0, 1)) == 4
    assert morton_key((2, 0, 0)) == 8 and morton_key((3, 3, 3)) == 63
    assert d2((1.0, 2.0, 3.0), (1.0, 2.0, 3.0)) == 0.0
    # Inverse iteration on a known matrix: diag(1, 4, 9) -> normal e_x.
    m = [[Decimal(9), Decimal(0), Decimal(0)], [Decimal(0), Decimal(4), Decimal(0)], [Decimal(0), Decimal(0), Decimal(1)]]
    n = unit([Decimal("0.3"), Decimal("-0.7"), Decimal("1")])
    for _ in range(60):
        n = unit(solve3(m, n))
    assert abs(abs(n[2]) - 1) < Decimal("1e-20")


def main():
    check = "--check" in sys.argv[1:]
    self_check()
    clouds = {
        "plane_dense": ("60 x 60 grid, 0.1 mm spacing, on z = 0.25 x - 0.15 y + 0.0075, no noise",)
        + plane_cloud(0.0, 0x5EED0001),
        "plane_noisy": ("the plane_dense grid with vertical noise sigma = 20 um",) + plane_cloud(20e-6, 0x5EED0002),
        "cube_edges": ("5.5 mm cube, 22 x 22 points per face at 0.25 mm, normal noise sigma = 10 um",)
        + cube_cloud(10e-6, 0x5EED0003),
    }
    files = {}
    for name, (desc, normals, pts) in clouds.items():
        files[f"{name}.txt"] = cloud_text(name, desc, normals, pts)
    cases = [
        ("plane_dense_h1mm", "plane_dense", False, False, True),
        ("plane_noisy_smooth", "plane_noisy", True, False, True),
        ("plane_noisy_smooth_sharp", "plane_noisy", True, True, True),
        ("cube_edges_smooth_sharp", "cube_edges", True, True, False),
    ]
    for case, cloud, smoothing, sharp, plane in cases:
        _, normals, pts = clouds[cloud]
        files[f"{case}.prep.txt"] = case_text(case, cloud, pts, normals, H, smoothing, sharp, plane)
    bad = 0
    for fname, text in sorted(files.items()):
        path = os.path.join(HERE, fname)
        if check:
            try:
                with open(path, encoding="utf-8", newline="") as fh:
                    if fh.read() != text:
                        print(f"differs: {fname}")
                        bad += 1
            except FileNotFoundError:
                print(f"missing: {fname}")
                bad += 1
        else:
            with open(path, "w", encoding="utf-8", newline="\n") as fh:
                fh.write(text)
    if bad:
        sys.exit(1)
    print(("checked " if check else "wrote ") + f"{len(files)} files")


if __name__ == "__main__":
    main()
