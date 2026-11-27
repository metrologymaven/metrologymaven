#!/usr/bin/env python3
"""Independent reference for SCAN.POLY.SURF.FAN (local tangent-plane Delaunay fans). CC0-1.0 data.

Stdlib only. Writes the vertex sets and expected fans of six cases into this folder as
`<case>.fan.txt`; `--check` regenerates everything in memory and fails on any difference with
the committed files. Nothing here was produced by running the Rust crate `fid-scan-poly`.

Vertex sets (invented data, generated with integer splitmix64 and IEEE `+ - * /` and
`math.sqrt` only; the written decimal values are what the crate reads, so nothing depends on
how a platform evaluates them):

* `hex_grid`: an 11 x 11 triangular lattice at 1 mm on a tilted plane, g = 1.5 mm (AC-1).
* `hex_grid_wide`: the same lattice with g = 2.5 mm (18 neighbours inside, AC-1).
* `square_grid`: a 10 x 10 square lattice at 2^-10 m on the plane z = 2^-5 with normal +z.
  Every coordinate is a dyadic rational, so the projected points are an exact square grid
  and every unit square is exactly cocircular: the index-ordered perturbation decides (AC-2).
* `square_grid_tilted`: a 10 x 10 square lattice at 1 mm on a tilted plane. The squares are
  cocircular only up to rounding, so the exact predicates decide on the rounded projections.
* `two_patches_gap`: two jittered 8 x 8 patches whose closest points are at least 1.5 g
  apart, g = 1.5 mm (AC-3).
* `jitter_dense`: a jittered 12 x 12 patch with g = 3.4 mm (the 32-neighbour cap is active),
  normals rotated by 75 degrees or flipped on some vertices (the 60-degree filter), two
  vertices duplicated exactly and two offset along the normal only.
* `status_mix`: small far-apart groups for every fan status: a collinear row (degenerate
  and no-neighbour ends), an isolated right triangle with legs of exactly g (every edge
  long), and a coincident pair inside a small grid (hidden).

Vertex sources are a seeded permutation, and the vertex-set order is shuffled, so neither
order follows the geometry.

The expected fans follow REQ-SCAN.POLY.SURF.FAN-001..003 and the pinned rules of the crate's
`fan` module documentation, computed here a different way:

* Floating-point steps are repeated with Python floats (IEEE binary64, never fused):
  neighbours by brute force over all vertices, dist2 = (dx*dx + dy*dy) + dz*dz <= g*g and
  normal dot (a0*b0 + a1*b1) + a2*b2 >= 0.5, ordered by (dist2, source), at most 32; the
  basis e1 = normalised n x axis (the axis with the smallest |n_i|, lowest i on ties),
  e2 = n x e1; u = (d0*e1_0 + d1*e1_1) + d2*e1_2 (v likewise) with d = q - centre, and
  |u| < 2^-200 written as 0; an edge is short when dist2 < (g*g)*(1 - 2^-46).
* Predicates are exact: every projected coordinate is turned into an integer by one common
  power of two per neighbourhood, and the orientation and in-circle determinants are
  evaluated as integer determinants. A zero in-circle determinant is resolved by actually
  perturbing the lifted coordinate z_i = x_i^2 + y_i^2 + eps^(r_i), with r_i smaller for a
  larger source (the largest source is lifted most): the sign is that of the first non-zero
  coefficient of eps, each coefficient being the 4 x 4 determinant with the lifted column
  replaced by a unit vector (computed by the generic determinant, not by a sign table).
* The fan of a centre c is every triangle (c, j, k) with orient(c, j, k) > 0, all three
  edges short, and no other neighbourhood point inside its perturbed circumcircle, found by
  brute force over all ordered pairs (j, k). Before the edge filter, the reference checks
  that the Delaunay triangles around c form a proper fan (every j starts and ends at most
  one triangle).
* Neighbours and triangles are ordered by the angle of j from +e1 in [0, 2 pi): the half
  plane (v > 0, or v = 0 and u > 0) first, then by exact cross product.
* Status, first match: hidden (a neighbour with a smaller source projected exactly onto the
  centre), no_neighbours (fewer than 2), degenerate (every exact orientation with the centre
  is 0), ok (a triangle kept), else all_edges_long. The reference also asserts that a hidden
  or degenerate centre has no Delaunay triangle at all, and that any other centre with at
  least 2 neighbours has one before the edge filter.
"""

import math
import os
import sys
from fractions import Fraction

HERE = os.path.dirname(os.path.abspath(__file__))
M64 = (1 << 64) - 1

MAX_NEIGHBOURS = 32
NORMAL_COS = 0.5
FLUSH = 2.0 ** -200
EDGE_MARGIN = 1.0 - 2.0 ** -46

# A rational orthonormal frame: U x V = N.
U = (2.0 / 3.0, 2.0 / 3.0, -1.0 / 3.0)
V = (-1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0)
N = (2.0 / 3.0, -1.0 / 3.0, 2.0 / 3.0)
ORIGIN = (0.0123, -0.0456, 0.0789)


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

    def unit(self):
        """Uniform in [0, 1) with 53 bits."""
        return (self.next() >> 11) * (2.0 ** -53)

    def below(self, n):
        return self.next() % n


def shuffle(rng, items):
    items = list(items)
    for i in range(len(items) - 1, 0, -1):
        j = rng.below(i + 1)
        items[i], items[j] = items[j], items[i]
    return items


def on_plane(a, b, w=0.0):
    return tuple(((ORIGIN[k] + a * U[k]) + b * V[k]) + w * N[k] for k in range(3))


def normalise(v):
    s = math.sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2])
    return (v[0] / s, v[1] / s, v[2] / s)


def is_unit(n):
    s = (n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]
    return abs(math.sqrt(s) - 1.0) <= 1e-9


# ---------------------------------------------------------------- vertex sets


def assign_sources(rng, verts):
    """verts: list of (position, normal, tag). Returns shuffled (source, p, n, tag)."""
    ids = shuffle(rng, range(len(verts)))
    rows = [(7 * ids[k] + 11, p, n, tag) for k, (p, n, tag) in enumerate(verts)]
    return shuffle(rng, rows)


def hex_vertices():
    s = 1e-3
    h = math.sqrt(3.0) / 2.0
    out = []
    for i in range(11):
        for j in range(11):
            out.append((on_plane((i + 0.5 * j) * s, (j * h) * s), N, "%d:%d" % (i, j)))
    return out


def square_vertices():
    s = 2.0 ** -10
    ox, oy, z = 3 * 2.0 ** -6, -5 * 2.0 ** -7, 2.0 ** -5
    out = []
    for i in range(10):
        for j in range(10):
            out.append(((ox + i * s, oy + j * s, z), (0.0, 0.0, 1.0), "%d:%d" % (i, j)))
    return out


def square_tilted_vertices():
    s = 1e-3
    out = []
    for i in range(10):
        for j in range(10):
            out.append((on_plane(i * s, j * s), N, "%d:%d" % (i, j)))
    return out


def jitter_normal(rng, amount):
    d = [(rng.unit() - 0.5) * amount for _ in range(3)]
    return normalise((N[0] + d[0], N[1] + d[1], N[2] + d[2]))


def jittered_patch(rng, nx, ny, s, a0, tag):
    out = []
    for i in range(nx):
        for j in range(ny):
            a = a0 + (i + (rng.unit() - 0.5) * 0.6) * s
            b = (j + (rng.unit() - 0.5) * 0.6) * s
            w = (rng.unit() - 0.5) * 40e-6
            out.append((on_plane(a, b, w), jitter_normal(rng, 0.1), tag))
    return out


def two_patch_vertices(rng, g):
    s = 1e-3
    a = jittered_patch(rng, 8, 8, s, 0.0, "A")
    b = jittered_patch(rng, 8, 8, s, 7e-3 + 1.5 * g + 1e-3, "B")
    closest = min(math.dist(p, q) for p, _, _ in a for q, _, _ in b)
    assert closest >= 1.5 * g, closest
    return a + b


def status_mix_vertices():
    s = 2.0 ** -10
    z = (0.0, 0.0, 1.0)
    out = []
    # A collinear row of 5 at spacing s (g = 1.5 s): ends have 1 neighbour, the rest 2.
    for i in range(5):
        out.append(((i * s, 0.0, 0.0), z, "row"))
    # A right triangle with legs exactly g, far away: every edge is at least g long.
    g = 1.5 * s
    ox = 1.0
    for p in ((ox, 0.0, 0.0), (ox + g, 0.0, 0.0), (ox, g, 0.0)):
        out.append((p, z, "long"))
    # A 3 x 3 grid at s with its centre doubled, far away.
    oy = 1.0
    for i in range(3):
        for j in range(3):
            out.append(((i * s, oy + j * s, 0.0), z, "grid"))
    out.append(((s, oy + s, 0.0), z, "twin"))
    return out


def rotate_about_u(n, deg):
    """n rotated about U by deg degrees (Rodrigues), normalised."""
    t = math.radians(deg)
    c, s = math.cos(t), math.sin(t)
    k = U
    kxn = (k[1] * n[2] - k[2] * n[1], k[2] * n[0] - k[0] * n[2], k[0] * n[1] - k[1] * n[0])
    kdn = k[0] * n[0] + k[1] * n[1] + k[2] * n[2]
    return normalise(tuple(n[i] * c + kxn[i] * s + k[i] * kdn * (1 - c) for i in range(3)))


def jitter_dense_vertices(rng):
    s = 1e-3
    base = jittered_patch(rng, 12, 12, s, 0.0, "-")
    out = []
    for k, (p, n, tag) in enumerate(base):
        r = rng.below(20)
        if r == 0:
            n = (-n[0], -n[1], -n[2])
            tag = "flipped"
        elif r == 1:
            n = rotate_about_u(n, 75.0)
            tag = "rotated"
        out.append((p, n, tag))
    # Two exact duplicates (same position and normal, different source).
    for k in (40, 77):
        p, n, _ = out[k]
        out.append((p, n, "dup"))
    # Two vertices offset along the normal only (projected near-duplicates).
    for k in (55, 90):
        p, n, _ = out[k]
        out.append((tuple(p[i] + 1e-4 * N[i] for i in range(3)), n, "stacked"))
    return out


# ---------------------------------------------------------------- float pipeline


def dist2(p, q):
    dx, dy, dz = p[0] - q[0], p[1] - q[1], p[2] - q[2]
    return (dx * dx + dy * dy) + dz * dz


def dot(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def basis(n):
    axis = 0
    for i in (1, 2):
        if abs(n[i]) < abs(n[axis]):
            axis = i
    e = [0.0, 0.0, 0.0]
    e[axis] = 1.0
    t = cross(n, e)
    ln = math.sqrt((t[0] * t[0] + t[1] * t[1]) + t[2] * t[2])
    e1 = (t[0] / ln, t[1] / ln, t[2] / ln)
    return e1, cross(n, e1)


def flush(x):
    return 0.0 if abs(x) < FLUSH else x


def neighbourhood(rows, k, g):
    src, pc, nc, _ = rows[k]
    g2 = g * g
    cands = []
    for m, (s, q, nq, _) in enumerate(rows):
        if m == k:
            continue
        d2 = dist2(q, pc)
        if d2 <= g2 and dot(nc, nq) >= NORMAL_COS:
            cands.append((d2, s, q))
    cands.sort()
    return cands[:MAX_NEIGHBOURS]


# ---------------------------------------------------------------- exact predicates


def det3(m):
    return (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))


def det4(m):
    total = 0
    for c in range(4):
        minor = [[m[r][cc] for cc in range(4) if cc != c] for r in range(1, 4)]
        term = m[0][c] * det3(minor)
        total += term if c % 2 == 0 else -term
    return total


def sign(x):
    return (x > 0) - (x < 0)


def orient(a, b, c):
    return sign(det3([[a[0], a[1], 1], [b[0], b[1], 1], [c[0], c[1], 1]]))


def perturbed_incircle(pts, ids):
    """Sign of the lifted 4 x 4 determinant with z_i + eps^(r_i); larger id lifted most."""
    m = [[x, y, x * x + y * y, 1] for (x, y) in pts]
    d = det4(m)
    if d:
        return sign(d)
    for r in sorted(range(4), key=lambda r: -ids[r]):
        mm = [row[:] for row in m]
        for rr in range(4):
            mm[rr][2] = 1 if rr == r else 0
        c = det4(mm)
        if c:
            return sign(c)
    return 0


def to_integers(coords):
    """Exact integers sharing one power-of-two scale."""
    fr = [(Fraction(u), Fraction(v)) for u, v in coords]
    k = 0
    for u, v in fr:
        for x in (u, v):
            den = x.denominator
            k = max(k, den.bit_length() - 1)
    scale = 1 << k
    out = []
    for u, v in fr:
        iu, iv = u * scale, v * scale
        assert iu.denominator == 1 and iv.denominator == 1
        out.append((iu.numerator, iv.numerator))
    return out


def half(p):
    return 0 if (p[1] > 0 or (p[1] == 0 and p[0] > 0)) else 1


def angular_sort(items, pt):
    """Insertion sort of ids by angle from +e1 (exact)."""
    out = []
    for i in items:
        pos = len(out)
        for idx, j in enumerate(out):
            a, b = pt[i], pt[j]
            if half(a) != half(b):
                before = half(a) < half(b)
            else:
                o = orient((0, 0), a, b)
                assert o != 0, "angular tie"
                before = o > 0
            if before:
                pos = idx
                break
        out.insert(pos, i)
    return out


def fan_of(rows, k, g):
    src, pc, nc, _ = rows[k]
    nb = neighbourhood(rows, k, g)
    e1, e2 = basis(nc)
    coords = [(0.0, 0.0)]
    ids = [src]
    pos3 = [pc]
    for _, s, q in nb:
        d = (q[0] - pc[0], q[1] - pc[1], q[2] - pc[2])
        coords.append((flush(dot(d, e1)), flush(dot(d, e2))))
        ids.append(s)
        pos3.append(q)
    ints = to_integers(coords)
    n = len(ids)
    g2m = (g * g) * EDGE_MARGIN

    def short(a, b):
        return dist2(pos3[a], pos3[b]) < g2m

    delaunay = []
    for j in range(1, n):
        for kk in range(1, n):
            if j == kk or orient(ints[0], ints[j], ints[kk]) <= 0:
                continue
            ok = True
            for l in range(1, n):
                if l in (j, kk):
                    continue
                four = [ints[0], ints[j], ints[kk], ints[l]]
                if perturbed_incircle(four, [ids[0], ids[j], ids[kk], ids[l]]) >= 0:
                    ok = False
                    break
            if ok:
                delaunay.append((j, kk))
    starts = [j for j, _ in delaunay]
    ends = [kk for _, kk in delaunay]
    assert len(set(starts)) == len(starts) and len(set(ends)) == len(ends), "not a fan"
    kept = [(j, kk) for j, kk in delaunay if short(0, j) and short(0, kk) and short(j, kk)]
    twins = [ids[i] for i in range(1, n) if ints[i] == (0, 0) and ids[i] < src]
    collinear = all(orient(ints[0], ints[a], ints[b]) == 0 for a in range(1, n) for b in range(1, n))
    if twins:
        assert not delaunay
        status = "hidden:%d" % min(twins)
    elif n - 1 < 2:
        assert not delaunay
        status = "no_neighbours"
    elif collinear:
        assert not delaunay
        status = "degenerate"
    else:
        assert delaunay, "a visible, non-collinear centre has a Delaunay triangle"
        status = "ok" if kept else "all_edges_long"
    pt = {i: ints[i] for i in range(n)}
    order = angular_sort(sorted({x for t in kept for x in t}), pt)
    rank = {i: r for r, i in enumerate(order)}
    kept.sort(key=lambda t: rank[t[0]])
    return (src, status, [ids[i] for i in order], [(ids[j], ids[kk]) for j, kk in kept], len(nb))


# ---------------------------------------------------------------- cases


def cases():
    out = []
    hexes = hex_vertices()
    out.append(("hex_grid", 1.5e-3, assign_sources(Rng(101), hexes),
                "11 x 11 triangular lattice, 1 mm, tilted plane, g = 1.5 mm; tag = i:j"))
    out.append(("hex_grid_wide", 2.5e-3, assign_sources(Rng(102), hexes),
                "11 x 11 triangular lattice, 1 mm, tilted plane, g = 2.5 mm; tag = i:j"))
    out.append(("square_grid", 1.5 * 2.0 ** -10, assign_sources(Rng(103), square_vertices()),
                "10 x 10 square lattice, 2^-10 m, plane z = 2^-5, normal +z, g = 1.5 * 2^-10 m; tag = i:j"))
    out.append(("square_grid_tilted", 1.5e-3, assign_sources(Rng(104), square_tilted_vertices()),
                "10 x 10 square lattice, 1 mm, tilted plane, g = 1.5 mm; tag = i:j"))
    g = 1.5e-3
    out.append(("two_patches_gap", g, assign_sources(Rng(105), two_patch_vertices(Rng(205), g)),
                "two jittered 8 x 8 patches, 1 mm, at least 1.5 g apart, g = 1.5 mm; tag = patch"))
    out.append(("jitter_dense", 3.4e-3, assign_sources(Rng(106), jitter_dense_vertices(Rng(206))),
                "jittered 12 x 12 patch, 1 mm, g = 3.4 mm, rotated/flipped normals, duplicates; tag = kind"))
    out.append(("status_mix", 1.5 * 2.0 ** -10, assign_sources(Rng(107), status_mix_vertices()),
                "status groups: collinear row, right triangle with legs g, doubled grid centre"))
    return out


def render(name, g, rows, note):
    for _, p, n, _ in rows:
        assert all(math.isfinite(x) for x in p) and is_unit(n)
    lines = [
        "# SCAN.POLY.SURF.FAN fixture, written by make_fan_fixtures.py (CC0-1.0).",
        "# " + note,
        "# Lines after 'vertices' (vertex-set order): source x y z nx ny nz tag.",
        "# Lines after 'fans' (same order): centre ; status ; neighbours in angular order ;",
        "# triangles j:k (status hidden:<by> names the twin's source).",
        "# (each triangle is centre, j, k, counter-clockwise about the centre's normal).",
        "case " + name,
        "g " + repr(g),
        "vertices %d" % len(rows),
    ]
    for s, p, n, tag in rows:
        lines.append(" ".join([str(s)] + [repr(x) for x in p] + [repr(x) for x in n] + [tag]))
    fans = [fan_of(rows, k, g) for k in range(len(rows))]
    capped = sum(1 for f in fans if f[4] == MAX_NEIGHBOURS)
    counts = {}
    for f in fans:
        key = f[1].split(":")[0]
        counts[key] = counts.get(key, 0) + 1
    lines.insert(2, "# %d fans, %d triangles, %d neighbourhoods at the 32 cap; status %s." % (
        len(fans), sum(len(f[3]) for f in fans), capped,
        ", ".join("%s %d" % kv for kv in sorted(counts.items()))))
    lines.append("fans %d" % len(fans))
    for c, status, nbs, tris in (f[:4] for f in fans):
        lines.append("%d ; %s ; %s ; %s" % (c, status, " ".join(map(str, nbs)), " ".join("%d:%d" % t for t in tris)))
    return "\n".join(lines) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, g, rows, note in cases():
        text = render(name, g, rows, note)
        target = os.path.join(HERE, name + ".fan.txt")
        if check:
            try:
                with open(target, encoding="utf-8", newline="") as f:
                    old = f.read()
            except OSError:
                old = None
            if old != text:
                print("MISMATCH " + target)
                bad += 1
        else:
            with open(target, "w", encoding="utf-8", newline="\n") as f:
                f.write(text)
            print("wrote " + target)
    if bad:
        sys.exit(1)
    if check:
        print("fan fixtures OK")


if __name__ == "__main__":
    main()
