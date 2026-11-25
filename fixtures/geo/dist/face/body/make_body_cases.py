#!/usr/bin/env python3
"""Independent oracle for GEO.DIST.FACE.BODY (CC-BY-4.0).

Writes ``cases.json`` next to this script with two invented case sets. Nothing here uses the
Rust crate; the bodies are the invented GEO.DIST.FACE.INCID fixtures (read only for their
labels, which are checked):

- ``lblock``: closest-point distances and inside/outside labels for the concave L-block
  ``fixtures/geo/dist/face/incid/lblock.json`` (L-shaped prism: the union of the boxes
  [0,2]x[0,1]x[0,1] and [0,1]x[0,2]x[0,1], with a concave vertical edge at x = y = 1 and two
  concave corners). 600 points from SplitMix64 seed 23 on a 2^-24 m grid: a third uniform in
  [-0.5, 2.5] x [-0.5, 2.5] x [-0.5, 1.5], a third near the concave edge, a third near the two
  concave corners. The distance is the minimum over the eight faces of the exact squared
  distance to the face (rectangles; the two L-shaped caps as unions of two rectangles), in
  rationals, square-rooted with 60-digit decimals and rounded once. Inside is an exact test.
  Points within 2^-20 m of the boundary are skipped (their sign is legitimately undecidable).
- ``tube_along``: lines ``p + s v`` against the cylinder with a hole
  ``fixtures/geo/dist/face/incid/tube.json`` (outer radius 2^-6 m, hole radius 2^-7 m,
  height 2^-7 m, axis +z from the origin): the intersection of smallest ``|s| |v|`` with the
  four trimmed faces (outer and inner cylinder for 0 <= z <= h, top and bottom annulus for
  r_in <= rho <= r_out), or none. SplitMix64 seed 41, coordinates on a 2^-30 m grid. Kinds:
  ``random`` (point and direction anywhere), ``aimed`` (through a random point of a random
  face), ``wall`` (from inside the hole, roughly across it), ``through_hole`` (inside the hole,
  steep enough to leave through both ends without touching the wall: no hit). Lines whose
  nearest hit lies within 1e-10 m of a trim boundary, whose two nearest hits are within
  1e-10 m of each other in distance, or that graze a cylinder are skipped.

Usage: ``python make_body_cases.py`` (write) or ``--check`` (compare, exit 1 on a difference).
"""

import json
import math
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

getcontext().prec = 60

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
LBLOCK = ROOT / "fixtures" / "geo" / "dist" / "face" / "incid" / "lblock.json"
TUBE = ROOT / "fixtures" / "geo" / "dist" / "face" / "incid" / "tube.json"
OUT = HERE / "cases.json"

MASK = (1 << 64) - 1


class SplitMix:
    def __init__(self, seed):
        self.s = seed & MASK

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n

    def between(self, lo, hi):
        """Integer in [lo, hi]."""
        return lo + self.below(hi - lo + 1)


def to_float(x):
    """Round a Fraction or Decimal once to binary64 (both conversions are correctly rounded)."""
    return float(x)


def dsqrt(fr):
    """60-digit square root of a non-negative Fraction."""
    return (Decimal(fr.numerator) / Decimal(fr.denominator)).sqrt()


def hexf(x):
    return float(x).hex()


# ------------------------------------------------------------------ L-block

LB_GRID = 1 << 24

# Faces as lists of axis-aligned rectangles: (fixed axis, value, (lo, hi) of the two others).
L_FACES = {
    "L-F1": [(2, 0, ((0, 2), (0, 1))), (2, 0, ((0, 1), (1, 2)))],  # bottom z = 0
    "L-F2": [(2, 1, ((0, 2), (0, 1))), (2, 1, ((0, 1), (1, 2)))],  # top z = 1
    "L-F3": [(1, 0, ((0, 2), (0, 1)))],  # y = 0: x, z
    "L-F4": [(0, 2, ((0, 1), (0, 1)))],  # x = 2: y, z
    "L-F5": [(1, 1, ((1, 2), (0, 1)))],  # y = 1: x in [1, 2], z
    "L-F6": [(0, 1, ((1, 2), (0, 1)))],  # x = 1: y in [1, 2], z
    "L-F7": [(1, 2, ((0, 1), (0, 1)))],  # y = 2: x in [0, 1], z
    "L-F8": [(0, 0, ((0, 2), (0, 1)))],  # x = 0: y in [0, 2], z
}


def rect_d2(p, axis, value, ranges):
    others = [k for k in range(3) if k != axis]
    d2 = (p[axis] - value) ** 2
    for k, (lo, hi) in zip(others, ranges):
        if p[k] < lo:
            d2 += (lo - p[k]) ** 2
        elif p[k] > hi:
            d2 += (p[k] - hi) ** 2
    return d2


def lblock_inside(p):
    x, y, z = p
    if not (0 < z < 1):
        return False
    return (0 < x < 2 and 0 < y < 1) or (0 < x < 1 and 0 < y < 2)


def lblock_cases():
    body = json.loads(LBLOCK.read_text())
    labels = sorted(f["label"] for f in body["faces"])
    assert labels == sorted(L_FACES), labels
    g = SplitMix(23)
    out = []
    n = 0
    while len(out) < 600:
        kind = ("uniform", "concave_edge", "concave_corner")[n % 3]
        n += 1
        if kind == "uniform":
            q = [g.between(-LB_GRID // 2, 5 * LB_GRID // 2), g.between(-LB_GRID // 2, 5 * LB_GRID // 2),
                 g.between(-LB_GRID // 2, 3 * LB_GRID // 2)]
        elif kind == "concave_edge":
            w = LB_GRID // 20
            q = [LB_GRID + g.between(-w, w), LB_GRID + g.between(-w, w), g.between(1, LB_GRID - 1)]
        else:
            w = LB_GRID // 20
            zc = LB_GRID * g.below(2)
            q = [LB_GRID + g.between(-w, w), LB_GRID + g.between(-w, w), zc + g.between(-w, w)]
        p = [Fraction(c, LB_GRID) for c in q]
        d2 = min(rect_d2(p, a, v, r) for rs in L_FACES.values() for (a, v, r) in rs)
        if d2 < Fraction(1, 1 << 40):
            continue
        inside = lblock_inside(p)
        out.append({
            "kind": kind,
            "p": [hexf(to_float(c)) for c in p],
            "distance": hexf(to_float(dsqrt(d2))),
            "side": "inside" if inside else "outside",
        })
    return out


# ------------------------------------------------------------------ tube, along a direction

U = 1 << 30
R_OUT = Fraction(1, 64)
R_IN = Fraction(1, 128)
H = Fraction(1, 128)
SKIP = Decimal("1e-10")


def tube_hits(p, v):
    """All hits (face, s as Decimal, foot as Decimals, margin to the trim) of the line."""
    pd = [Decimal(c.numerator) / Decimal(c.denominator) for c in p]
    vd = [Decimal(c.numerator) / Decimal(c.denominator) for c in v]
    vlen = (vd[0] * vd[0] + vd[1] * vd[1] + vd[2] * vd[2]).sqrt()
    hits = []
    graze = False
    for face, r in (("T-F1", R_OUT), ("T-F2", R_IN)):
        a = v[0] * v[0] + v[1] * v[1]
        if a == 0:
            continue
        hh = p[0] * v[0] + p[1] * v[1]
        c = p[0] * p[0] + p[1] * p[1] - r * r
        disc = hh * hh - a * c
        if disc < 0:
            continue
        sq = dsqrt(disc)
        rd = Decimal(r.numerator) / Decimal(r.denominator)
        if sq / (Decimal(a.numerator) / Decimal(a.denominator)).sqrt() < Decimal("1e-7") * rd:
            graze = True
        for sign in (-1, 1):
            s = (-(Decimal(hh.numerator) / Decimal(hh.denominator)) + sign * sq) / (Decimal(a.numerator) / Decimal(a.denominator))
            z = pd[2] + s * vd[2]
            hd = Decimal(H.numerator) / Decimal(H.denominator)
            margin = min(z, hd - z) * 1  # metres along the generator
            if z < 0 or z > hd:
                hits.append((face, s, None, -margin))
                continue
            foot = [pd[k] + s * vd[k] for k in range(3)]
            hits.append((face, s, foot, margin))
    for face, zval in (("T-F3", H), ("T-F4", Fraction(0))):
        if v[2] == 0:
            continue
        s = (zval - p[2]) / v[2]
        x = p[0] + s * v[0]
        y = p[1] + s * v[1]
        rho2 = x * x + y * y
        rho = dsqrt(rho2)
        rin = Decimal(R_IN.numerator) / Decimal(R_IN.denominator)
        rout = Decimal(R_OUT.numerator) / Decimal(R_OUT.denominator)
        margin = min(rho - rin, rout - rho)
        sd = Decimal(s.numerator) / Decimal(s.denominator)
        foot = [Decimal(x.numerator) / Decimal(x.denominator), Decimal(y.numerator) / Decimal(y.denominator),
                Decimal(zval.numerator) / Decimal(zval.denominator)]
        hits.append((face, sd, foot if R_IN * R_IN <= rho2 <= R_OUT * R_OUT else None, margin if margin >= 0 else margin))
    return hits, vlen, graze


def tube_case(p, v):
    hits, vlen, graze = tube_hits(p, v)
    if graze:
        return None
    for (_, _, _, margin) in hits:
        if abs(margin) <= SKIP:
            return None
    real = sorted(((abs(s) * vlen, face, s, foot) for (face, s, foot, _) in hits if foot is not None),
                  key=lambda t: (t[0], t[1]))
    if not real:
        return {"hit": None}
    if len(real) >= 2 and real[1][0] - real[0][0] <= SKIP and real[1][1] != real[0][1]:
        return None
    d, face, s, foot = real[0]
    return {"hit": {"face": face, "s_sign": 1 if s >= 0 else -1, "distance": hexf(float(d)),
                    "foot": [hexf(float(c)) for c in foot]}}


def tube_along_cases():
    body = json.loads(TUBE.read_text())
    labels = sorted(f["label"] for f in body["faces"])
    assert labels == ["T-F1", "T-F2", "T-F3", "T-F4"], labels
    g = SplitMix(41)
    box = U >> 5  # 2^-5 m
    out = []
    n = 0
    want = {"random": 120, "aimed": 160, "wall": 60, "through_hole": 60}
    count = {k: 0 for k in want}
    while any(count[k] < want[k] for k in want):
        kind = ("random", "aimed", "wall", "through_hole")[n % 4]
        n += 1
        if count[kind] >= want[kind]:
            continue
        if kind == "random":
            pi = [g.between(-box, box) for _ in range(3)]
            vi = [g.between(-1024, 1024) for _ in range(3)]
        elif kind == "aimed":
            pi = [g.between(-box, box) for _ in range(3)]
            face = g.below(4)
            # A target point on (or next to) the face, in integers: it only aims the line.
            if face < 2:
                r = (U >> 6) if face == 0 else (U >> 7)
                tx = g.between(-r, r)
                ty = math.isqrt(r * r - tx * tx) * (1 if g.below(2) else -1)
                tp = [tx, ty, g.between(1, (U >> 7) - 1)]
            else:
                rr = g.between((U >> 7) + 1, (U >> 6) - 1)
                tx = g.between(-rr, rr)
                ty = math.isqrt(rr * rr - tx * tx) * (1 if g.below(2) else -1)
                tp = [tx, ty, (U >> 7) if face == 2 else 0]
            vi = [tp[k] - pi[k] for k in range(3)]
            if vi == [0, 0, 0]:
                continue
        elif kind == "wall":
            q = U >> 9
            pi = [g.between(-q, q), g.between(-q, q), g.between(U >> 9, (U >> 7) - (U >> 9))]
            vi = [g.between(-1024, 1024), g.between(-1024, 1024), g.between(-64, 64)]
        else:
            q = U >> 10
            pi = [g.between(-q, q), g.between(-q, q), g.between(1, (U >> 7) - 1)]
            vi = [g.between(-64, 64), g.between(-64, 64), 1024 if g.below(2) else -1024]
        if vi == [0, 0, 0]:
            continue
        p = [Fraction(c, U) for c in pi]
        v = [Fraction(c, 1) for c in vi]
        case = tube_case(p, v)
        if case is None:
            continue
        if kind == "through_hole":
            assert case["hit"] is None, (pi, vi, case)
        count[kind] += 1
        case.update({"kind": kind, "p": [hexf(to_float(c)) for c in p], "v": [hexf(float(c)) for c in vi]})
        out.append(case)
    return out


def build():
    doc = {
        "format": "fid-face-body-cases/1",
        "license": "CC-BY-4.0",
        "generator": "fixtures/geo/dist/face/body/make_body_cases.py",
        "unit": "m",
        "lblock_body": "fixtures/geo/dist/face/incid/lblock.json",
        "tube_body": "fixtures/geo/dist/face/incid/tube.json",
        "lblock": lblock_cases(),
        "tube_along": tube_along_cases(),
    }
    return json.dumps(doc, indent=1, sort_keys=True) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        if OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT} differs from the oracle output", file=sys.stderr)
            return 1
        print("cases.json matches")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
