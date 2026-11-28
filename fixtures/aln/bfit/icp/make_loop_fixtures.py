#!/usr/bin/env python3
"""Independent reference for ALN.BFIT.ICP.LOOP; writes loop_cases.json and plate_outliers.json.

Stdlib only. Shares no code with the Rust crates: every expected value is an input of the
synthetic construction (a known rigid pose, a known set of planted outliers), written from
spec/aln/bfit/icp/loop/NODE.md (AC-1 to AC-4) and the exclusion-digest layout of
spec/krn/out/pol/NODE.md (KRN.OUT.POL: SHA-256 of the point count and the ascending excluded
indices, all little-endian u32). All data is invented (CC0-1.0, project-authored); nothing
comes from an external dataset (D-038). The ALN.BFIT.BENCH corpus is not built yet; these are
this leaf's own noise-free cases.

Geometry. A reference box is the closed 12-triangle mesh of an axis-aligned box: vertex index
x + 2 y + 4 z (x, y, z in {0, 1} for the low and high side), two triangles per face, each
face's corners counter-clockwise seen from outside (checked here in exact rational
arithmetic: (b - a) x (c - a) points along the outward axis), triangles (c0, c1, c2) and
(c0, c2, c3). Every vertex coordinate is a dyadic rational, so it is an exact binary64 value.

Points. Surface points are drawn (splitmix64, a fixed seed per file) on a 2^-12 m grid inside
each face, kept 2^-10 m away from the face's edges. A rigid pose P(x) = R x + t (data
coordinates to the reference frame) has a rotation R built from an integer quaternion
(a, b, c, d) as the exact rational matrix

    R = [[a2+b2-c2-d2, 2(bc-ad),    2(bd+ac)],
         [2(bc+ad),    a2-b2+c2-d2, 2(cd-ab)],
         [2(bd-ac),    2(cd+ab),    a2-b2-c2+d2]] / (a2+b2+c2+d2)

and a dyadic translation t. The measured point of a surface point p is x = R^T (p - t),
computed exactly with fractions and rounded once to binary64 (Python's float(Fraction) is
correctly rounded), so P(x) lies on the reference up to that rounding: the cases are
noise-free. The expected pose is R and t, each entry rounded once to binary64. Nothing uses
libm or prints a transcendental value, so the files are the same on every platform.

loop_cases.json (seed 0x4C4F4F50): one box, 0.125 x 0.09375 x 0.046875 m centred at
(0.25, -0.125, 0.0625), 40 points per face, and six poses:

  rot_trans  q = (200, 1, 2, -1), t = (3/2048, -3/4096, 1/1024) m (about 1.4 degrees)
  rot_large  q = (120, -2, 1, 3), t = (-1/512, 1/1024, 3/4096) m (about 3.6 degrees)
  trans_only q = (1, 0, 0, 0),    t = (1/256, 0, -1/1024) m
  rot_z      q = (300, 0, 0, 4),  t = (0, 0, 0)
  locked_tz  q = (150, 1, -1, 2), t = (1/1024, -1/2048, 0) m: tz is 0, for a run with tz locked
  start5     q = (40, 1, 1, 1),   t = (1/1024, 1/2048, -1/4096) m (about 5 degrees, AC-4)

plate_outliers.json (seed 0x504C4154): a plate 0.25 x 0.125 x 0.015625 m centred at the
origin; 300 points on the top face (z = +h), 100 on the bottom, 25 on each side (500 points,
in that order); the top-face points keep 0.02 m from the plate's edges. Ten of the top-face
points (2 %), chosen by the generator, are moved 5 mm (exactly 1/200 m) along +z: the planted
outliers. Pose q = (4000, 1, -2, 3), t = (1/8192, -1/16384, 1/8192) m, so at the start pose
every inlier lies well within 1 mm of the plate and every outlier more than 4 mm from it.
`excluded_digest` is SHA-256 of u32le(500) followed by u32le(i) for each outlier index i,
ascending: what NominalGate{max_distance_m: 1e-3} must exclude (AC-3).

Run:  python fixtures/aln/bfit/icp/make_loop_fixtures.py [--check]
With --check nothing is written; exits non-zero if an output differs.
"""

import hashlib
import json
import struct
import sys
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).resolve().parent

MASK = (1 << 64) - 1
GRID = 4096  # surface coordinates are integers / GRID metres
MARGIN = F(1, 1024)


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
        """A Fraction n / GRID with lo <= n / GRID <= hi."""
        a = -((-lo * GRID) // 1)  # ceil
        b = (hi * GRID) // 1  # floor
        a, b = int(a), int(b)
        return F(a + self.next() % (b - a + 1), GRID)

    def below(self, n):
        return self.next() % n


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def box(lo, hi):
    """Vertices and the 12 triangles of the box [lo, hi], outward counter-clockwise."""
    verts = []
    for z in (0, 1):
        for y in (0, 1):
            for x in (0, 1):
                verts.append([(lo[0], hi[0])[x], (lo[1], hi[1])[y], (lo[2], hi[2])[z]])
    tris = []
    for k in range(3):
        i, j = (k + 1) % 3, (k + 2) % 3
        for s in (0, 1):
            corners = []
            for ui, uj in ((0, 0), (1, 0), (1, 1), (0, 1)):
                c = [0, 0, 0]
                c[k], c[i], c[j] = s, ui, uj
                corners.append(c[0] + 2 * c[1] + 4 * c[2])
            a, b, c3 = (verts[corners[0]], verts[corners[1]], verts[corners[2]])
            n = cross([b[m] - a[m] for m in range(3)], [c3[m] - a[m] for m in range(3)])
            outward = 1 if s == 1 else -1
            if n[k] * outward < 0:
                corners = [corners[0], corners[3], corners[2], corners[1]]
            a, b, c3 = (verts[corners[0]], verts[corners[1]], verts[corners[2]])
            n = cross([b[m] - a[m] for m in range(3)], [c3[m] - a[m] for m in range(3)])
            assert n[k] * outward > 0 and n[i] == 0 and n[j] == 0
            tris.append([corners[0], corners[1], corners[2]])
            tris.append([corners[0], corners[2], corners[3]])
    return verts, tris


def face_points(rng, lo, hi, k, side, count, margin):
    """`count` grid points on the face of axis k on `side` (0 low, 1 high)."""
    i, j = (k + 1) % 3, (k + 2) % 3
    out = []
    for _ in range(count):
        p = [None, None, None]
        p[k] = (lo[k], hi[k])[side]
        p[i] = rng.grid(lo[i] + margin, hi[i] - margin)
        p[j] = rng.grid(lo[j] + margin, hi[j] - margin)
        out.append(p)
    return out


def rotation(q):
    a, b, c, d = q
    n = a * a + b * b + c * c + d * d
    r = [
        [a * a + b * b - c * c - d * d, 2 * (b * c - a * d), 2 * (b * d + a * c)],
        [2 * (b * c + a * d), a * a - b * b + c * c - d * d, 2 * (c * d - a * b)],
        [2 * (b * d - a * c), 2 * (c * d + a * b), a * a - b * b - c * c + d * d],
    ]
    return [[F(x, n) for x in row] for row in r]


def measured(r, t, p):
    """R^T (p - t), exact."""
    d = [p[m] - t[m] for m in range(3)]
    return [r[0][k] * d[0] + r[1][k] * d[1] + r[2][k] * d[2] for k in range(3)]


def fl(v):
    return [float(x) for x in v]


def loop_cases():
    rng = SplitMix(0x4C4F4F50)
    centre = [F(1, 4), F(-1, 8), F(1, 16)]
    half = [F(1, 16), F(3, 64), F(3, 128)]
    lo = [centre[m] - half[m] for m in range(3)]
    hi = [centre[m] + half[m] for m in range(3)]
    verts, tris = box(lo, hi)
    surface = []
    for k in range(3):
        for side in (0, 1):
            surface += face_points(rng, lo, hi, k, side, 40, MARGIN)
    poses = [
        ("rot_trans", (200, 1, 2, -1), [F(3, 2048), F(-3, 4096), F(1, 1024)]),
        ("rot_large", (120, -2, 1, 3), [F(-1, 512), F(1, 1024), F(3, 4096)]),
        ("trans_only", (1, 0, 0, 0), [F(1, 256), F(0), F(-1, 1024)]),
        ("rot_z", (300, 0, 0, 4), [F(0), F(0), F(0)]),
        ("locked_tz", (150, 1, -1, 2), [F(1, 1024), F(-1, 2048), F(0)]),
        ("start5", (40, 1, 1, 1), [F(1, 1024), F(1, 2048), F(-1, 4096)]),
    ]
    cases = []
    for name, q, t in poses:
        r = rotation(q)
        # The rotation is exact: R R^T = I.
        for a in range(3):
            for b in range(3):
                assert sum(r[a][m] * r[b][m] for m in range(3)) == (1 if a == b else 0)
        cases.append(
            {
                "name": name,
                "quaternion": list(q),
                "rotation": [fl(row) for row in r],
                "translation": fl(t),
                "measured": [fl(measured(r, t, p)) for p in surface],
            }
        )
    return {
        "about": "ALN.BFIT.ICP.LOOP noise-free cases; see make_loop_fixtures.py",
        "licence": "CC0-1.0",
        "vertices": [fl(v) for v in verts],
        "triangles": tris,
        "surface": [fl(p) for p in surface],
        "cases": cases,
    }


def plate_outliers():
    rng = SplitMix(0x504C4154)
    half = [F(1, 8), F(1, 16), F(1, 128)]
    lo = [-h for h in half]
    hi = list(half)
    verts, tris = box(lo, hi)
    pts = []
    top = face_points(rng, lo, hi, 2, 1, 300, F(1, 50))
    pts += top
    pts += face_points(rng, lo, hi, 2, 0, 100, MARGIN)
    for k in (0, 1):
        for side in (0, 1):
            pts += face_points(rng, lo, hi, k, side, 25, MARGIN)
    assert len(pts) == 500
    outliers = set()
    while len(outliers) < 10:
        outliers.add(rng.below(300))
    outliers = sorted(outliers)
    for i in outliers:
        pts[i] = [pts[i][0], pts[i][1], pts[i][2] + F(1, 200)]
    q = (4000, 1, -2, 3)
    t = [F(1, 8192), F(-1, 16384), F(1, 8192)]
    r = rotation(q)
    digest = hashlib.sha256(struct.pack("<I", len(pts)) + b"".join(struct.pack("<I", i) for i in outliers))
    return {
        "about": "ALN.BFIT.ICP.LOOP AC-3 plate with 2 % outliers at 5 mm; see make_loop_fixtures.py",
        "licence": "CC0-1.0",
        "vertices": [fl(v) for v in verts],
        "triangles": tris,
        "quaternion": list(q),
        "rotation": [fl(row) for row in r],
        "translation": fl(t),
        "gate_max_distance_m": 1e-3,
        "outlier_offset_m": 0.005,
        "outliers": outliers,
        "excluded_digest": "sha256:" + digest.hexdigest(),
        "measured": [fl(measured(r, t, p)) for p in pts],
    }


def dump(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def main():
    check_only = "--check" in sys.argv[1:]
    bad = False
    for fname, build in (("loop_cases.json", loop_cases), ("plate_outliers.json", plate_outliers)):
        text = dump(build())
        target = HERE / fname
        if check_only:
            have = target.read_bytes().decode("utf-8") if target.exists() else None
            if have != text:
                print(f"{fname}: differs from the reference output", file=sys.stderr)
                bad = True
        else:
            target.write_bytes(text.encode("utf-8"))
    if bad:
        sys.exit(1)
    print("make_loop_fixtures: " + ("ok" if check_only else "written"))


if __name__ == "__main__":
    main()
