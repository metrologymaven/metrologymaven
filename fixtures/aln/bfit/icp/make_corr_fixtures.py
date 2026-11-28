#!/usr/bin/env python3
"""Independent reference for ALN.BFIT.ICP.CORR AC-1; writes cube_points.json.

Stdlib only. Shares no code with the Rust crates fid-align-solve or fid-geo-dist: the
expected values below are the analytic closest points of a unit cube, written from
spec/aln/bfit/icp/corr/NODE.md (REQ-001, REQ-004, REQ-005) and the pseudo-normal rule of
spec/geo/dist/sign/NODE.md. All data is invented (CC-BY-4.0, project-authored); nothing comes
from an external dataset (D-038).

The mesh is the unit cube [0, 1]^3: vertex index x + 2 y + 4 z, two triangles per face, each
face's corners listed counter-clockwise seen from outside (checked here in exact integer
arithmetic), triangles (c0, c1, c2) and (c0, c2, c3).

Measured points (splitmix64, seed 0x434F5252, coordinates on a 2^-12 grid, so every input is
an exact binary64 value) in five groups:

  face     outside a face, the in-plane coordinates in [1/16, 15/16]: the foot is the point
           with its face coordinate set to the face's plane; normal the face's outward axis.
  inside   inside the cube within 1/8 of one face, in-plane in [1/4, 3/4], so that face is
           the unique nearest one: foot and normal as for `face`.
  diagonal outside a face, exactly above its triangulation diagonal c0-c2, so the foot lies on
           a mesh edge between two coplanar triangles: the edge pseudo-normal is the face
           normal.
  edge     outside two faces (offsets in [2^-12, 1/4]), the third coordinate in [1/8, 7/8]:
           the foot is on the cube edge; the pseudo-normal (pi n1 + pi n2) normalised is
           (n1 + n2) / sqrt(2).
  vertex   outside three faces: the foot is the corner; the angle-weighted pseudo-normal
           (pi/2 per face, whatever the triangulation) normalised is (n1 + n2 + n3) / sqrt(3).

`dist` is the exact Euclidean distance; `entity` is the lowest index among the triangles
whose closed region contains the exact foot (for a convex cube the foot is unique, so those
are exactly the minimisers, and REQ-004 asks for the lowest index). Irrational values (the
normals and distances) are computed with decimal at 40 digits and rounded once to binary64;
nothing uses libm, so the file is the same on every platform. The Rust test compares within
1e-12 m and 1e-12 (AC-1) and the entity exactly.

Run:  python fixtures/aln/bfit/icp/make_corr_fixtures.py [--check]
With --check nothing is written; exits non-zero if the output differs.
"""

import json
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
getcontext().prec = 40

MASK = (1 << 64) - 1
GRID = 4096  # coordinates are integers / GRID


class SplitMix:
    def __init__(self, seed):
        self.s = seed & MASK

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def grid(self, lo, hi):
        """A Fraction n / GRID with lo <= n / GRID <= hi (lo, hi Fractions on the grid)."""
        a, b = int(lo * GRID), int(hi * GRID)
        return Fraction(a + self.next() % (b - a + 1), GRID)


def vid(x, y, z):
    return x + 2 * y + 4 * z


def faces():
    """(axis k, side s, corners) for the six faces, corners counter-clockwise from outside."""
    out = []
    for k in range(3):
        i, j = (k + 1) % 3, (k + 2) % 3
        for s in (0, 1):
            quad = [(0, 0), (1, 0), (1, 1), (0, 1)]
            if s == 0:
                quad = [(0, 0), (0, 1), (1, 1), (1, 0)]
            corners = []
            for a, b in quad:
                c = [0, 0, 0]
                c[k], c[i], c[j] = s, a, b
                corners.append(vid(*c))
            out.append((k, s, corners))
    return out


def vertex(v):
    return [v & 1, (v >> 1) & 1, (v >> 2) & 1]


def sub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def build_mesh():
    verts = [vertex(v) for v in range(8)]
    tris = []
    for k, s, c in faces():
        for t in ([c[0], c[1], c[2]], [c[0], c[2], c[3]]):
            n = cross(sub(verts[t[1]], verts[t[0]]), sub(verts[t[2]], verts[t[0]]))
            out = [0, 0, 0]
            out[k] = 1 if s == 1 else -1
            assert n == [2 * x for x in out] or n == out, (t, n)
            assert dot(n, out) > 0
            tris.append(t)
    return verts, tris


def in_triangle(q, a, b, c):
    """Closed containment of q (on the triangle's plane) in triangle abc, exact."""
    n = cross(sub(b, a), sub(c, a))
    for u, v in ((a, b), (b, c), (c, a)):
        if dot(cross(sub(v, u), sub(q, u)), n) < 0:
            return False
    # q must also lie in the plane.
    return dot(sub(q, a), n) == 0


def entity(verts, tris, foot):
    best = None
    for t, (ia, ib, ic) in enumerate(tris):
        a, b, c = ([Fraction(x) for x in verts[i]] for i in (ia, ib, ic))
        if in_triangle(foot, a, b, c):
            best = t if best is None else min(best, t)
    assert best is not None, foot
    return best


def dsqrt(q):
    """sqrt of a non-negative Fraction, 40 decimal digits, rounded once to binary64."""
    return float(Decimal(q.numerator).sqrt() / Decimal(q.denominator).sqrt()) if q else 0.0


def unit(axes):
    """The unit vector sum of signed axes [(k, sign)], rounded once per component."""
    v = [0, 0, 0]
    for k, sg in axes:
        v[k] = sg
    m = Decimal(len(axes)).sqrt()
    return [float(Decimal(x) / m) for x in v]


def case(group, p, foot, axes, verts, tris):
    d2 = sum((a - b) * (a - b) for a, b in zip(p, foot))
    return {
        "group": group,
        "p": [float(x) for x in p],
        "foot": [float(x) for x in foot],
        "normal": unit(axes),
        "dist": dsqrt(d2),
        "entity": entity(verts, tris, foot),
    }


def build():
    verts, tris = build_mesh()
    rng = SplitMix(0x434F5252)
    F = Fraction
    g = F(1, GRID)
    cases = []
    for k, s, corners in faces():
        i, j = (k + 1) % 3, (k + 2) % 3
        out = 1 if s == 1 else -1
        for group, lo, hi, sign in (("face", F(1, 16), F(15, 16), 1), ("inside", F(1, 4), F(3, 4), -1)):
            for _ in range(4):
                p = [F(0)] * 3
                p[i], p[j] = rng.grid(lo, hi), rng.grid(lo, hi)
                d = rng.grid(g, F(1, 4) if sign > 0 else F(1, 8))
                p[k] = F(s) + out * sign * d
                foot = list(p)
                foot[k] = F(s)
                cases.append(case(group, p, foot, [(k, out)], verts, tris))
        # Exactly above the diagonal c0-c2: both in-plane coordinates equal.
        for _ in range(3):
            t = rng.grid(F(1, 16), F(15, 16))
            p = [F(0)] * 3
            p[i], p[j] = t, t
            p[k] = F(s) + out * rng.grid(g, F(1, 4))
            foot = list(p)
            foot[k] = F(s)
            cases.append(case("diagonal", p, foot, [(k, out)], verts, tris))
    # Edges: axes a < b outside at sides sa, sb; the third axis c inside.
    for a in range(3):
        for b in range(a + 1, 3):
            c = 3 - a - b
            for sa in (0, 1):
                for sb in (0, 1):
                    for _ in range(3):
                        p = [F(0)] * 3
                        p[c] = rng.grid(F(1, 8), F(7, 8))
                        oa, ob = (1 if sa else -1), (1 if sb else -1)
                        p[a] = F(sa) + oa * rng.grid(g, F(1, 4))
                        p[b] = F(sb) + ob * rng.grid(g, F(1, 4))
                        foot = list(p)
                        foot[a], foot[b] = F(sa), F(sb)
                        cases.append(case("edge", p, foot, [(a, oa), (b, ob)], verts, tris))
    for v in range(8):
        corner = vertex(v)
        for _ in range(3):
            p = [F(corner[m]) + (1 if corner[m] else -1) * rng.grid(g, F(1, 4)) for m in range(3)]
            foot = [F(x) for x in corner]
            axes = [(m, 1 if corner[m] else -1) for m in range(3)]
            cases.append(case("vertex", p, foot, axes, verts, tris))
    return {
        "about": "ALN.BFIT.ICP.CORR AC-1: points at known offsets from a unit-cube mesh with the "
        "analytic foot, outward unit (pseudo-)normal, distance and lowest containing triangle. "
        "Generated by make_corr_fixtures.py (independent reference); invented data. CC-BY-4.0.",
        "seed": 0x434F5252,
        "vertices": [[float(x) for x in v] for v in verts],
        "triangles": tris,
        "cases": cases,
    }


def dump(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def main():
    check_only = "--check" in sys.argv[1:]
    text = dump(build())
    path = HERE / "cube_points.json"
    if check_only:
        have = path.read_bytes().decode("utf-8") if path.exists() else None
        if have != text:
            print("cube_points.json: differs from the reference output", file=sys.stderr)
            sys.exit(1)
        print("make_corr_fixtures: ok")
    else:
        path.write_bytes(text.encode("utf-8"))
        print("make_corr_fixtures: written")


if __name__ == "__main__":
    main()
