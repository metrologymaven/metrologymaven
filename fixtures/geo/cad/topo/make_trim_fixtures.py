#!/usr/bin/env python3
"""GEO.CAD.TOPO.TRIM fixtures and expected values (independent Python reference, CC-BY-4.0).

Writes, next to this script:
  plate_with_hole.json     plane plate 100 x 80 mm, 10 mm radius hole, 3-D curves only (AC-1, AC-5)
  cylinder_window.json     cylindrical face r 50 mm, u in [-0.5, 0.5] rad, v in [0, 20 mm], with
                           imported line pcurves: a rectangular window across the seam (AC-2)
  trim_polygons.json       five planar faces with imported line pcurves: a square with a hole in
                           both orientations, a comb with horizontal edges and collinear vertices,
                           a triangle with inexact coordinates, a square whose hole is joined to
                           the outside by a bridge edge used twice (fuzz, AC-3)
  cylinder_slit.json       the cylinder window with a slit: a generator edge at u = 0.25 from the
                           bottom to v = 0.01, used once up and once down (boundary, not a seam)
  sphere_seam_closed.json  a whole sphere: one loop, a meridian seam edge used twice (the uses
                           coincide in PCURVE's chart; poles interior)
  sphere_cap.json          the cap v > 0.5 rad of a sphere, bounded by one latitude circle that
                           winds once in u (pole interior)
  trim_expected.json       expected classes and orientation signs

Run:  python fixtures/geo/cad/topo/make_trim_fixtures.py          (writes everything)
      python fixtures/geo/cad/topo/make_trim_fixtures.py --check  (fails if anything is stale)

Stdlib only; never runs the Rust code. Fixture geometry uses only correctly rounded binary64
operations and the bit-exact fid-num sin/cos model (no platform libm), so --check is the same on
every OS.

How the expected values are made
--------------------------------
1. The boundary of record. The derived pcurves come from the bit-exact PCURVE model in
   make_pcurve_fixtures.py (itself checked there against a 60-digit oracle); the polylines are
   assembled by the pinned rules of crates/fid-geo-cad/src/topo/trim.rs (imported line chords,
   continuity shift, chart offsets, connectors, seams), written again here in binary64 Python.
2. Exact classification (planar faces, whose surface is the identity map S(u, v) = (u, v, 0),
   so 3-D distance is uv distance). With `fractions.Fraction` on the binary64 inputs: the least
   squared distance from q to the non-seam segments against tol^2 (band, REQ-002), otherwise
   the even-odd crossing number with the ray +u counted with the strict rule
   "(a.v > q.v) != (b.v > q.v)", a different half-open convention from the Rust code, so the
   two agree only because neither depends on it for a point off the boundary.
3. Analytic answers. AC-1: inside the rectangle and outside the circle, band |r - R| <= tol or
   distance to the rectangle <= tol, all in exact rationals; every point's polyline answer
   must equal the analytic one (the derived circle is within 1e-8 m of the true circle).
   AC-2: the closed-form 3-D distance from S(u, v) to the four boundary curves of the window
   (two arcs, two generators; chord 2 r sin(dphi / 2)) at 60 digits.
4. Margins. The Rust band is closed outward by its resolution: a point at distance d with
   tol < d <= tol (1 + 2^-12) + 1e-12 may be either. No expected case lies in that shell, nor
   within 1e-12 below tol (AC-2: 1e-11 m either side for the 60-digit trigonometry); generated
   near-boundary cases that would are dropped, and the seeded AC-1 points are asserted clear of it.
5. Periodic faces (sphere_seam_closed, sphere_cap, and the PCURVE fixtures cylinder_band,
   cylinder_capped): the analytic face, material on the left of every loop: the whole sphere;
   v > 0.5; 0 < v < 0.02; 0 < v < 1. Inside/outside is an exact rational comparison of the
   binary64 v; the band is the 3-D distance to the boundary circles along the meridian
   (|dv| on a cylinder, exact rationals; 2 r sin(|dv| / 2) on the sphere, 60 digits). Seams and
   poles are not boundary. Cases near the band edge are dropped as in 4.
6. Orientation signs: exact rationals on the binary64 values (and `x + k * TAU` shifts).

Case rows are strings: AC-2 and polygon cases "<u hex> <v hex> <I|O|B>" (hex: the binary64 bit
pattern); orientation cases "<ax> <ay> <bx> <by> <qx> <qy> <sign>", each value "<x hex>:<k>"
meaning x + k * TAU, sign of (b - a) x (q - a) in {-1, 0, 1}.

AC-1 points: splitmix64 with seed 11; `x = (z >> 11) * 2^-53`, `u = lo + (hi - lo) * x` in
binary64 (u first, then v), lo/hi below. The Rust test generates the same 10 000 points.
"""

import importlib.util
import json
import math
import struct
import sys
from decimal import Decimal, localcontext
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
LICENSE = "CC-BY-4.0"


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


MP = _load("make_pcurve_fixtures", "fixtures/geo/cad/topo/make_pcurve_fixtures.py")
OR = MP.OR
AN = MP.AN


def cos(x):
    """fid-num's cos, bit-exact model (no platform libm)."""
    return AN.sin_cos(x)[1]


def sin(x):
    return AN.sin_cos(x)[0]
TAU = MP.TAU
X, Z, O = MP.X, MP.Z, MP.O

RES_REL = 2.0 ** -12
AC1_SEED = 11
AC1_COUNT = 10000
AC1_TOL = 1e-7
AC1_BOX = [-0.01, 0.11, -0.01, 0.09]
AC2_TOL = 1e-7
MASK = (1 << 64) - 1


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return "%016x" % bits(x)


# ================================================================ splitmix64

class SplitMix:
    def __init__(self, seed):
        self.s = seed & MASK

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def unit(self):
        return float(self.next() >> 11) * (2.0 ** -53)

    def between(self, lo, hi):
        return lo + (hi - lo) * self.unit()


# ================================================================ fixtures

def cylinder_window():
    r, h, a = 0.05, 0.02, 0.5
    ca, sa = cos(a), sin(a)
    verts = [(r * ca, -r * sa, 0.0), (r * ca, r * sa, 0.0), (r * ca, r * sa, h), (r * ca, -r * sa, h)]
    edges = [
        (0, 1, (-a, a), MP.circle(O, Z, X, r), "e-bottom"),
        (1, 2, (0.0, h), MP.line(verts[1], Z, (ca, sa, 0.0)), "e-right"),
        (3, 2, (-a, a), MP.circle((0.0, 0.0, h), Z, X, r), "e-top"),
        (0, 3, (0.0, h), MP.line(verts[0], Z, (ca, -sa, 0.0)), "e-left"),
    ]
    l2 = lambda o, d: {"type": "line2", "origin": list(o), "dir": list(d)}  # noqa: E731
    coedges = [(0, "same", l2((0.0, 0.0), (1.0, 0.0)), "c-bottom"),
               (1, "same", l2((a, 0.0), (0.0, 1.0)), "c-right"),
               (2, "reversed", l2((0.0, h), (1.0, 0.0)), "c-top"),
               (3, "reversed", l2((-a, 0.0), (0.0, 1.0)), "c-left")]
    loops = [([0, 1, 2, 3], True, "l-window")]
    faces = [(dict(type="cylinder", radius=r, **MP.frame(O, Z, X)), [0], "f-window")]
    return MP.body("cylindrical face r 0.05 m: the window u in [-0.5, 0.5] rad, v in [0, 0.02] m across the seam, "
                   "imported line pcurves (GEO.CAD.TOPO.TRIM AC-2)", "c71dc71dc71dc71dc71dc71dc71dc71d", verts, edges,
                   coedges, loops, faces)


def cylinder_slit():
    r, h, a, us, hs = 0.05, 0.02, 0.5, 0.25, 0.01
    pt = lambda u, z: (r * cos(u), r * sin(u), z)  # noqa: E731
    verts = [pt(-a, 0.0), pt(us, 0.0), pt(a, 0.0), pt(a, h), pt(-a, h), pt(us, hs)]
    edges = [
        (0, 1, (-a, us), MP.circle(O, Z, X, r), "e-bottom-l"),
        (1, 5, (0.0, hs), MP.line(verts[1], Z, (cos(us), sin(us), 0.0)), "e-slit"),
        (1, 2, (us, a), MP.circle(O, Z, X, r), "e-bottom-r"),
        (2, 3, (0.0, h), MP.line(verts[2], Z, (cos(a), sin(a), 0.0)), "e-right"),
        (4, 3, (-a, a), MP.circle((0.0, 0.0, h), Z, X, r), "e-top"),
        (0, 4, (0.0, h), MP.line(verts[0], Z, (cos(a), -sin(a), 0.0)), "e-left"),
    ]
    l2 = lambda o, d: {"type": "line2", "origin": list(o), "dir": list(d)}  # noqa: E731
    coedges = [(0, "same", l2((0.0, 0.0), (1.0, 0.0)), "c-bottom-l"),
               (1, "same", l2((us, 0.0), (0.0, 1.0)), "c-slit-up"),
               (1, "reversed", l2((us, 0.0), (0.0, 1.0)), "c-slit-down"),
               (2, "same", l2((0.0, 0.0), (1.0, 0.0)), "c-bottom-r"),
               (3, "same", l2((a, 0.0), (0.0, 1.0)), "c-right"),
               (4, "reversed", l2((0.0, h), (1.0, 0.0)), "c-top"),
               (5, "reversed", l2((-a, 0.0), (0.0, 1.0)), "c-left")]
    loops = [([0, 1, 2, 3, 4, 5, 6], True, "l-slit")]
    faces = [(dict(type="cylinder", radius=r, **MP.frame(O, Z, X)), [0], "f-slit")]
    return MP.body("the cylinder window with a slit: the generator edge u = 0.25, v in [0, 0.01] used up and down "
                   "by the loop (GEO.CAD.TOPO.TRIM review)", "511d511d511d511d511d511d511d511d", verts, edges,
                   coedges, loops, faces)


S = 0.015625  # 1/64 m
LABELS = ["f-square-ccw", "f-square-cw", "f-comb", "f-triangle", "f-bridge"]
POLYGONS = [
    ("f-square-ccw", [[(0, 0), (4, 0), (4, 4), (0, 4)], [(1, 1), (1, 3), (3, 3), (3, 1)]]),
    ("f-square-cw", [[(0, 0), (0, 4), (4, 4), (4, 0)], [(1, 1), (3, 1), (3, 3), (1, 3)]]),
    ("f-comb", [[(0, 0), (2, 0), (4, 0), (4, 1), (3, 1), (3, 3), (2, 3), (2, 1), (1, 1), (1, 3), (0, 3)]]),
    ("f-triangle", [[(0.1, 0.2), (0.7, 0.25), (0.3, 0.9)]]),
]


def polygons():
    verts, edges, coedges, loops, faces = [], [], [], [], []
    for fl, rings in POLYGONS:
        lids = []
        for ri, ring in enumerate(rings):
            base = len(verts)
            pts = [(x * S, y * S) for x, y in ring]
            verts += [(x, y, 0.0) for x, y in pts]
            cs = []
            n = len(pts)
            for k in range(n):
                a, b = pts[k], pts[(k + 1) % n]
                d = (b[0] - a[0], b[1] - a[1])
                lab = "%s-%d-%d" % (fl[2:], ri, k)
                edges.append((base + k, base + (k + 1) % n, (0.0, 1.0), None, "e-" + lab))
                coedges.append((len(edges) - 1, "same", {"type": "line2", "origin": list(a), "dir": list(d)},
                                "c-" + lab))
                cs.append(len(coedges) - 1)
            loops.append((cs, ri == 0, "l-%s-%d" % (fl[2:], ri)))
            lids.append(len(loops) - 1)
        faces.append((dict(type="plane", **MP.frame(O, Z, X)), lids, fl))
    # The bridge: square (0..4)^2 with the hole (1..3)^2 joined to x = 0 by the edge (0,2)-(1,2),
    # used once in each direction by the single loop.
    pts = [(0, 0), (4, 0), (4, 4), (0, 4), (0, 2), (1, 2), (1, 3), (3, 3), (3, 1), (1, 1)]
    base = len(verts)
    verts += [(x * S, y * S, 0.0) for x, y in pts]
    chain = [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7), (7, 8), (8, 9), (9, 5), (4, 0)]
    eix = {}
    for a, b in chain:
        pa, pb = pts[a], pts[b]
        lab = "bridge-%d-%d" % (a, b)
        edges.append((base + a, base + b, (0.0, 1.0), None, "e-" + lab))
        eix[(a, b)] = len(edges) - 1
    cs = []
    for a, b, sense in [(0, 1, "same"), (1, 2, "same"), (2, 3, "same"), (3, 4, "same"), (4, 5, "same"),
                        (5, 6, "same"), (6, 7, "same"), (7, 8, "same"), (8, 9, "same"), (9, 5, "same"),
                        (4, 5, "reversed"), (4, 0, "same")]:
        pa, pb = (pts[a][0] * S, pts[a][1] * S), (pts[b][0] * S, pts[b][1] * S)
        d = (pb[0] - pa[0], pb[1] - pa[1])
        coedges.append((eix[(a, b)], sense, {"type": "line2", "origin": list(pa), "dir": list(d)},
                        "c-bridge-%d-%d-%s" % (a, b, sense)))
        cs.append(len(coedges) - 1)
    loops.append((cs, True, "l-bridge"))
    faces.append((dict(type="plane", **MP.frame(O, Z, X)), [len(loops) - 1], "f-bridge"))
    return MP.body("planar faces with imported line pcurves and no 3-D curves: a square with a hole (both "
                   "orientations), a comb with horizontal edges and collinear vertices, a triangle with inexact "
                   "coordinates (GEO.CAD.TOPO.TRIM fuzz, AC-3)", "7a1a7a1a7a1a7a1a7a1a7a1a7a1a7a1a", verts, edges,
                   coedges, loops, faces)


# ================================================================ the boundary of record (binary64)

def assemble(b, m, fi):
    """Segments (a, b, seam) of face `fi`, by the pinned assembly rules."""
    faces, loops, coedges, res, charts, _wound = m
    f = faces[fi]
    periods, offsets, pers = charts[fi]
    uses = {}
    for li in f["loops"]:
        for ci in loops[li]["coedges"]:
            e = coedges[ci]["edge"]
            uses[e] = uses.get(e, 0) + 1
    segs = []

    def add(a, z, seam):
        if bits(a[0]) == bits(z[0]) and bits(a[1]) == bits(z[1]):
            return
        segs.append((tuple(a), tuple(z), seam))

    for j, li in enumerate(f["loops"]):
        assert pers[j] == [0, 0], "winding loops are undecidable"
        parts, prev_end = [], None
        for ci in loops[li]["coedges"]:
            ce, r = coedges[ci], res[ci]
            if r[0] == "D":
                pts = [list(p) for p in r[1]]
            elif r[0] == "I":
                p = ce["pcurve"]
                assert p["type"] == "line2"
                t0, t1 = b["edges"][ce["edge"]]["range"]
                a, z = MP.pcurve_point(p, t0), MP.pcurve_point(p, t1)
                pts = [z, a] if ce["sense"] == "reversed" else [a, z]
            else:
                raise SystemExit("failed pcurve in a classified face")
            shift = [0.0, 0.0]
            if prev_end is not None:
                for i in range(2):
                    if periods[i] is not None:
                        shift[i] = 0.0 - float(math.floor((pts[0][i] - prev_end[i]) / periods[i] + 0.5)) * periods[i]
            for q in pts:
                for i in range(2):
                    if shift[i] != 0.0:
                        q[i] = q[i] + shift[i]
            prev_end = list(pts[-1])
            for q in pts:
                for i in range(2):
                    if offsets[j][i] != 0.0:
                        q[i] = q[i] + offsets[j][i]
            # Planar faces only: an edge used twice (a bridge) is ordinary boundary.
            assert periods == [None, None]
            parts.append((pts, False))
        n = len(parts)
        for k, (pts, seam) in enumerate(parts):
            for a, z in zip(pts, pts[1:]):
                add(a, z, seam)
            add(pts[-1], parts[(k + 1) % n][0][0], False)
    return segs


# ================================================================ exact planar oracle

def dist2_seg(q, a, b):
    ax, ay, bx, by = F(a[0]), F(a[1]), F(b[0]), F(b[1])
    dx, dy = bx - ax, by - ay
    t = ((q[0] - ax) * dx + (q[1] - ay) * dy) / (dx * dx + dy * dy)
    t = min(max(t, F(0)), F(1))
    px, py = ax + t * dx, ay + t * dy
    return (q[0] - px) ** 2 + (q[1] - py) ** 2


NEAR = 1e-5  # segments whose box is farther than this from q are farther than any tolerance used


class Planar:
    """The segments of a planar face, bucketed by v so a query looks at few of them."""

    BINS = 256

    def __init__(self, segs):
        self.segs = segs
        vs = [c for a, b, _s in segs for c in (a[1], b[1])]
        self.lo, self.hi = min(vs) - 2 * NEAR, max(vs) + 2 * NEAR
        self.step = (self.hi - self.lo) / self.BINS
        self.bins = [[] for _ in range(self.BINS)]
        for sg in segs:
            a, b, _s = sg
            for k in range(self.bin(min(a[1], b[1]) - NEAR), self.bin(max(a[1], b[1]) + NEAR) + 1):
                self.bins[k].append(sg)

    def bin(self, v):
        k = int((v - self.lo) / self.step)
        return min(max(k, 0), self.BINS - 1)

    def candidates(self, v):
        if v < self.lo or v > self.hi:
            return []
        return self.bins[self.bin(v)]

    def near_d2(self, u, v):
        """Exact least squared distance to the non-seam segments within NEAR, or None."""
        q = (F(u), F(v))
        best = None
        for a, b, seam in self.candidates(v):
            if seam:
                continue
            if not (min(a[0], b[0]) - NEAR <= u <= max(a[0], b[0]) + NEAR
                    and min(a[1], b[1]) - NEAR <= v <= max(a[1], b[1]) + NEAR):
                continue
            d2 = dist2_seg(q, a, b)
            if best is None or d2 < best:
                best = d2
        return best

    def parity(self, u, v):
        """Even-odd crossing number of the ray +u, strict rule, exact crossings."""
        q = (F(u), F(v))
        odd = False
        for a, b, _seam in self.candidates(v):
            if (a[1] > v) != (b[1] > v):
                ax, ay, bx, by = F(a[0]), F(a[1]), F(b[0]), F(b[1])
                x = ax + (q[1] - ay) * (bx - ax) / (by - ay)
                if x > q[0]:
                    odd = not odd
        return odd


def shell_hi(tol):
    return F(tol) * (1 + F(RES_REL)) + F(1e-12)


def classify_planar(pl, u, v, tol):
    """('I'|'O'|'B', clear): clear is False when the distance lies in the resolution shell."""
    assert tol * 4 < NEAR
    d2 = pl.near_d2(u, v)
    t = F(tol)
    if d2 is None:
        return ("I" if pl.parity(u, v) else "O"), True
    lo, hi = (t - F(1e-12)) ** 2 if t > F(1e-12) else F(0), shell_hi(tol) ** 2
    clear = not (lo < d2 <= hi)
    if d2 <= t * t:
        return "B", clear
    return ("I" if pl.parity(u, v) else "O"), clear


# ================================================================ AC-1

def ac1(segs):
    pl = Planar(segs)
    w, h, cx, cy, r = F(0.1), F(0.08), F(0.04), F(0.03), F(0.01)
    t = F(AC1_TOL)
    rng = SplitMix(AC1_SEED)
    out = []
    for _ in range(AC1_COUNT):
        u = rng.between(AC1_BOX[0], AC1_BOX[1])
        v = rng.between(AC1_BOX[2], AC1_BOX[3])
        c, clear = classify_planar(pl, u, v, AC1_TOL)
        assert clear, ("AC-1 point in the resolution shell", u, v)
        # Analytic answer.
        U, V = F(u), F(v)
        inside_rect = 0 <= U <= w and 0 <= V <= h
        if inside_rect:
            dr = min(U, w - U, V, h - V)
            d2r = dr * dr
        else:
            dx = max(-U, F(0), U - w)
            dy = max(-V, F(0), V - h)
            d2r = dx * dx + dy * dy
        r2 = (U - cx) ** 2 + (V - cy) ** 2
        band = d2r <= t * t or (r - t) ** 2 <= r2 <= (r + t) ** 2
        # Margin against the 1e-8 m circle approximation and the band edges.
        m = F(2e-8)
        assert not ((r - t - m) ** 2 < r2 < (r - t + m) ** 2 or (r + t - m) ** 2 < r2 < (r + t + m) ** 2)
        assert not ((t - F(1e-12)) ** 2 < d2r <= shell_hi(AC1_TOL) ** 2)
        a = "B" if band else ("I" if inside_rect and r2 > r * r else "O")
        assert a == c, ("AC-1 polyline and analytic answers differ", u, v, a, c)
        out.append(c)
    return "".join(out)


def ac1_band(segs):
    """Seeded points within a few tolerances of the plate's boundary (AC-1 band cases)."""
    pl = Planar(segs)
    w, h, cx, cy, r = 0.1, 0.08, 0.04, 0.03, 0.01
    W, H, CX, CY, R, t = F(w), F(h), F(cx), F(cy), F(r), F(AC1_TOL)
    rng = SplitMix(AC1_SEED + 1000)
    out = []
    while len(out) < 2000:
        kind = rng.next() % 5
        off = rng.between(-3.0, 3.0) * AC1_TOL
        if kind == 4:
            phi = rng.between(0.0, TAU)
            u, v = cx + (r + off) * cos(phi), cy + (r + off) * sin(phi)
        else:
            s_ = rng.between(0.0, 1.0)
            u, v = [(w * s_, off), (w + off, h * s_), (w * s_, h + off), (off, h * s_)][kind]
        c, clear = classify_planar(pl, u, v, AC1_TOL)
        U, V = F(u), F(v)
        inside_rect = 0 <= U <= W and 0 <= V <= H
        if inside_rect:
            dr = min(U, W - U, V, H - V)
            d2r = dr * dr
        else:
            dx, dy = max(-U, F(0), U - W), max(-V, F(0), V - H)
            d2r = dx * dx + dy * dy
        r2 = (U - CX) ** 2 + (V - CY) ** 2
        m = F(2e-8)
        if not clear or (R - t - m) ** 2 < r2 < (R - t + m) ** 2 or (R + t - m) ** 2 < r2 < (R + t + m) ** 2:
            continue
        band = d2r <= t * t or (R - t) ** 2 <= r2 <= (R + t) ** 2
        a = "B" if band else ("I" if inside_rect and r2 > R * R else "O")
        assert a == c, ("AC-1 band case: polyline and analytic answers differ", u, v, a, c)
        out.append("%s %s %s" % (hx(u), hx(v), c))
    return out


# ================================================================ periodic faces

def sphere_seam_closed():
    r = 0.05
    verts = [(0.0, 0.0, -r), (0.0, 0.0, r)]
    # Meridian in the xz-plane, axis -Y: C(t) = r (cos t, 0, sin t), south pole to north pole.
    edges = [(0, 1, (-MP.FRAC_PI_2, MP.FRAC_PI_2), MP.circle(O, (0.0, -1.0, 0.0), X, r), "e-seam")]
    coedges = [(0, "same", None, "c-seam-up"), (0, "reversed", None, "c-seam-down")]
    loops = [([0, 1], True, "l-seam")]
    faces = [(dict(type="sphere", radius=r, **MP.frame(O, Z, X)), [0], "f-sphere")]
    return MP.body("whole sphere r 0.05 m: one loop of a meridian seam edge used twice; no boundary "
                   "(GEO.CAD.TOPO.TRIM)", "5ea55ea55ea55ea55ea55ea55ea55ea5", verts, edges, coedges, loops, faces)


CAP_V0 = 0.5


def sphere_cap():
    r = 0.05
    rc, zc = r * cos(CAP_V0), r * sin(CAP_V0)
    verts = [(rc, 0.0, zc)]
    edges = [(0, 0, (0.0, TAU), MP.circle((0.0, 0.0, zc), Z, X, rc), "e-latitude")]
    coedges = [(0, "same", None, "c-latitude")]
    loops = [([0], True, "l-latitude")]
    faces = [(dict(type="sphere", radius=r, **MP.frame(O, Z, X)), [0], "f-cap")]
    return MP.body("spherical cap v > 0.5 rad of a sphere r 0.05 m: one latitude circle winding once in u, "
                   "material on its left (GEO.CAD.TOPO.TRIM)", "ca95ca95ca95ca95ca95ca95ca95ca95", verts, edges,
                   coedges, loops, faces)


PERIODIC = {
    # fixture: (face label or None, kind, parameters)
    "sphere_seam_closed": ("f-sphere", "sphere", {"r": 0.05, "bands": []}),
    "sphere_cap": ("f-cap", "sphere", {"r": 0.05, "bands": [CAP_V0], "inside": (CAP_V0, None)}),
    "cylinder_band": ("f-band", "cylinder", {"bands": [0.0, 0.02], "inside": (0.0, 0.02)}),
    "cylinder_capped": (None, "cylinder", {"bands": [0.0, 1.0], "inside": (0.0, 1.0)}),
}
PER_TOL = 1e-7


def periodic_class(kind, prm, u, v, tol):
    """(class, clear) of the analytic periodic faces."""
    with localcontext() as ctx:
        ctx.prec = 60
        t = dec(tol)
        ds = []
        for vb in prm["bands"]:
            dv = abs(dec(v) - dec(vb))
            if kind == "sphere":
                sn, _c = OR.sin_cos(dv / 2)
                ds.append(2 * dec(prm["r"]) * abs(sn))
            else:
                ds.append(dv)
        margin = dec(1e-12)
        if ds:
            d = min(ds)
            clear = not (t - margin < d <= t * (1 + dec(RES_REL)) + margin)
            if d <= t:
                return "B", clear
        else:
            clear = True
    lo, hi = prm.get("inside", (None, None))
    V = F(v)
    inside = (lo is None or V > F(lo)) and (hi is None or V < F(hi))
    return ("I" if inside else "O"), clear


def periodic_cases():
    out = {}
    rng = SplitMix(55)
    for name, (label, kind, prm) in PERIODIC.items():
        if kind == "sphere":
            vlo, vhi = -MP.FRAC_PI_2, MP.FRAC_PI_2
        else:
            vlo, vhi = prm["bands"][0] - 0.25 * (prm["bands"][1] - prm["bands"][0]), \
                prm["bands"][1] + 0.25 * (prm["bands"][1] - prm["bands"][0])
        pts = []
        for _ in range(300):
            pts.append((rng.between(-8.0, 8.0), rng.between(vlo, vhi)))
        for u in (0.0, 1.0, -1.0, 3.0, TAU, -TAU, 2 * TAU, 0.5 * TAU):
            for vb in prm["bands"]:
                for o in (0.0, 0.5, 0.999, 1.001, 2.0, -0.5, -0.999, -1.001, -2.0, 10.0, -10.0):
                    dv = o * PER_TOL / (prm["r"] if kind == "sphere" else 1.0)
                    pts.append((u, vb + dv))
            if kind == "sphere":
                for pole in (MP.FRAC_PI_2, -MP.FRAC_PI_2):
                    for dlt in (0.0, 1e-15, 1e-12, 1e-9, 1e-6, 1e-3):
                        pts.append((u, pole - dlt if pole > 0 else pole + dlt))
            else:
                mid = 0.5 * (prm["bands"][0] + prm["bands"][1])
                pts.append((u, mid))
        cases = []
        for u, v in pts:
            if kind == "sphere" and not (-MP.FRAC_PI_2 <= v <= MP.FRAC_PI_2):
                continue
            c, clear = periodic_class(kind, prm, u, v, PER_TOL)
            if clear:
                cases.append("%s %s %s" % (hx(u), hx(v), c))
        out[name] = {"face": label, "tol_m": PER_TOL, "cases": cases}
    return out


# ================================================================ AC-2 (60-digit closed form)

def dec(x):
    return OR.dec_of(x)


def ac2_class(u, v):
    """(class, clear) of the cylinder window at 60 digits."""
    with localcontext() as ctx:
        ctx.prec = 60
        r, h, a, t = dec(0.05), dec(0.02), dec(0.5), dec(AC2_TOL)
        pi, two_pi = OR.PI, OR.TWO_PI
        U, V = dec(u), dec(v)
        k = ((U + pi) / two_pi).to_integral_value(rounding="ROUND_FLOOR")
        wv = U - k * two_pi  # in [-pi, pi)

        def ang(x, y):
            d = abs(x - y)
            return two_pi - d if d > pi else d

        def chord(d):
            s, _c = OR.sin_cos(d / 2)
            return 2 * r * abs(s)

        def hyp(x, y):
            return (x * x + y * y).sqrt()

        ds = []
        for z in (dec(0.0), h):
            dz = V - z
            if abs(wv) <= a:
                ds.append(abs(dz))
            else:
                ds.append(hyp(chord(min(ang(wv, a), ang(wv, -a))), dz))
        for phi in (a, -a):
            dz = V - min(max(V, dec(0.0)), h)
            ds.append(hyp(chord(ang(wv, phi)), dz))
        d = min(ds)
        margin = dec(1e-11)
        clear = not (t - margin < d <= t * (1 + dec(RES_REL)) + margin)
        if d <= t:
            return "B", clear
        inside = abs(wv) < a and dec(0.0) < V < h
        return ("I" if inside else "O"), clear


def ac2_cases():
    rng = SplitMix(22)
    pts = []
    for _ in range(400):
        pts.append((rng.between(-0.7, 0.7), rng.between(-0.004, 0.024)))
    for _ in range(200):
        pts.append((rng.between(-math.pi, math.pi), rng.between(-0.004, 0.024)))
    a, h, r = 0.5, 0.02, 0.05
    offs = [0.0, 0.25, 0.5, 0.99, 1.01, 2.0, 10.0, -0.25, -0.5, -0.99, -1.01, -2.0, -10.0]
    for s in (0.05, 0.3, 0.5, 0.7, 0.95):
        for o in offs:
            dz = o * AC2_TOL
            pts.append((-a + 2 * a * s, 0.0 + dz))       # bottom arc
            pts.append((-a + 2 * a * s, h + dz))         # top arc
            pts.append((a + dz / r, h * s))              # right generator
            pts.append((-a + dz / r, h * s))             # left generator
    for cu in (-a, a):
        for cv in (0.0, h):
            for o in offs:
                pts.append((cu + o * AC2_TOL / r, cv + o * AC2_TOL))
    pts += [(0.0, 0.01), (math.pi, 0.01), (-3.0, 0.01), (a, 0.01), (-a, 0.01), (0.0, 0.0), (0.0, h)]
    cases = []
    for u, v in pts:
        c, clear = ac2_class(u, v)
        if not clear:
            continue
        for w in (u + TAU, u - TAU):
            c2, clear2 = ac2_class(w, v)
            assert clear2 and c2 == c, ("AC-2 shifted point classifies differently", u, v)
        cases.append("%s %s %s" % (hx(u), hx(v), c))
    return cases


# ================================================================ polygons (fuzz)

def polygon_cases(b, m):
    faces, loops, _coedges, _res, _charts, _wound = m
    out = {}
    rng = SplitMix(33)
    labels = LABELS
    for fi, f in enumerate(faces):
        label = labels[fi]
        segs = assemble(b, m, fi)
        pl = Planar(segs)
        pts = []
        verts = sorted({p for a, z, _s in segs for p in (a, z)})
        pts += verts
        for a, z, _s in segs:
            pts.append(((a[0] + z[0]) * 0.5, (a[1] + z[1]) * 0.5))
        vs = sorted({p[1] for p in verts})
        for v in vs:
            for k in range(-4, 21):
                pts.append((k * 0.25 * S, v))
        for _ in range(300):
            pts.append((rng.between(-0.5 * S, 4.5 * S), rng.between(-0.5 * S, 4.5 * S)))
        if label == "f-bridge":
            for x in (0.0, 0.25, 0.5, 0.75, 1.0):
                pts.append((x * S, 2 * S))
        per_tol = {}
        for tol in (1e-6, 1e-9):
            tp = list(pts)
            for a, z, _s in segs:
                mx, my = (a[0] + z[0]) * 0.5, (a[1] + z[1]) * 0.5
                dx, dy = z[0] - a[0], z[1] - a[1]
                ln = math.sqrt(dx * dx + dy * dy)
                nx, ny = -dy / ln, dx / ln
                for o in (0.25, 0.5, 0.999, 1.001, 2.0, 1.0 + 2.0 ** -9):
                    for sg in (1.0, -1.0):
                        tp.append((mx + sg * o * tol * nx, my + sg * o * tol * ny))
                for c in (a, z):
                    for o in (0.5, 2.0):
                        tp.append((c[0] + o * tol, c[1] + o * tol))
                        tp.append((c[0] - o * tol, c[1] + o * tol))
            cases = []
            seen = set()
            for u, v in tp:
                key = (bits(u), bits(v))
                if key in seen:
                    continue
                seen.add(key)
                c, clear = classify_planar(pl, u, v, tol)
                if clear:
                    cases.append("%s %s %s" % (hx(u), hx(v), c))
            per_tol[repr(tol)] = cases
        out[label] = per_tol
    return out


# ================================================================ orientation signs

def sign(x):
    return (x > 0) - (x < 0)


def orient_exact(a, b, q):
    """a, b, q: points of (x, k) pairs, value x + k * TAU."""
    val = lambda c: F(c[0]) + c[1] * F(TAU)  # noqa: E731
    ax, ay, bx, by, qx, qy = (val(c) for c in (a[0], a[1], b[0], b[1], q[0], q[1]))
    return sign((bx - ax) * (qy - ay) - (by - ay) * (qx - ax))


def orient_cases():
    out = []

    def add(a, b, q):
        a, b, q = [tuple(c if isinstance(c, tuple) else (c, 0) for c in p) for p in (a, b, q)]
        for x, y in ((a, b), (b, a)):
            s = orient_exact(x, y, q)
            out.append(" ".join(["%s:%d" % (hx(c[0]), c[1]) for p in (x, y, q) for c in p] + ["%d" % s]))

    # Kettner et al. style: points near the line through (12, 12) and (24, 24).
    ulp = 2.0 ** -53
    for i in range(12):
        for j in range(12):
            add((12.0, 12.0), (24.0, 24.0), (0.5 + i * ulp, 0.5 + j * ulp))
    # Collinear points with inexact values, exactly on a dyadic line.
    rng = SplitMix(44)
    for _ in range(60):
        x0, y0 = rng.between(-1, 1), rng.between(-1, 1)
        add((x0, y0), (x0 + 0.5, y0 + 0.25), (x0 + 1.0, y0 + 0.5))
        add((x0, y0), (x0 + 0.5, y0 + 0.25), (x0 + 1.0, y0 + 0.5 + 2.0 ** -40))
        add((rng.between(-1, 1), rng.between(-1, 1)), (rng.between(-1, 1), rng.between(-1, 1)),
            (rng.between(-1, 1), rng.between(-1, 1)))
    # Huge, tiny and subnormal coordinates.
    for s in (1e-300, 1e-200, 5e-324 * 2 ** 20, 1e200, 1e300):
        add((0.0, 0.0), (s, s), (2 * s, 2 * s))
        add((0.0, 0.0), (s, 0.0), (0.0, s))
        add((s, 3 * s), (2 * s, s), (5 * s, 7 * s))
    add((5e-324, 0.0), (0.0, 5e-324), (5e-324, 5e-324))
    # Shifted queries: x + k * TAU.
    for k in (-2, -1, 1, 2):
        for x in (0.5, -0.5, 0.0, 2.0 ** -60, -(2.0 ** -60)):
            add((0.0, 0.0), (0.0, 1.0), ((x, k), (0.5, 0)))
            add((TAU * k, 0.0), (TAU * k, 1.0), ((x, k), (0.25, 0)))
            add((0.5, 0.0), (-0.5, 0.02), ((x, k), (0.01, 0)))
    return out


# ================================================================ build

def render(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def build():
    files = {}
    _, plate = MP.plate(0.0, "plate_with_hole", "plane plate 100 x 80 mm with a 10 mm radius hole, 3-D curves only "
                                                "(GEO.CAD.TOPO.TRIM AC-1)")
    window = cylinder_window()
    poly = polygons()
    files["plate_with_hole.json"] = MP.render(plate)
    files["cylinder_window.json"] = MP.render(window)
    files["trim_polygons.json"] = MP.render(poly)
    files["cylinder_slit.json"] = MP.render(cylinder_slit())
    files["sphere_seam_closed.json"] = MP.render(sphere_seam_closed())
    files["sphere_cap.json"] = MP.render(sphere_cap())

    mp = MP.m_body(plate)
    segs = assemble(plate, mp, 0)
    classes = ac1(segs)
    band_cases = ac1_band(segs)
    mw = MP.m_body(window)
    assert mw[4][0][1] == [[0.0, 0.0]], mw[4][0]
    ac2 = ac2_cases()
    mpoly = MP.m_body(poly)
    expected = {
        "description": "GEO.CAD.TOPO.TRIM expected values: exact rational classification of the pinned boundary "
                       "polylines and analytic answers (AC-1, AC-2), polygon fuzz cases (AC-3) and exact "
                       "orientation signs. Written by make_trim_fixtures.py; do not edit.",
        "license": LICENSE,
        "ac1": {
            "fixture": "plate_with_hole",
            "seed": AC1_SEED,
            "count": AC1_COUNT,
            "tol_m": AC1_TOL,
            "box": AC1_BOX,
            "classes": classes,
            "counts": {c: classes.count(c) for c in "IOB"},
            "segments": len(segs),
            "band_cases": band_cases,
            "band_counts": {c: sum(1 for x in band_cases if x.endswith(c)) for c in "IOB"},
        },
        "ac2": {"fixture": "cylinder_window", "tol_m": AC2_TOL, "cases": ac2},
        "polygons": polygon_cases(poly, mpoly),
        "periodic": periodic_cases(),
        "orient": orient_cases(),
    }
    files["trim_expected.json"] = render(expected)
    n_poly = sum(len(c) for f in expected["polygons"].values() for c in f.values())
    print("AC-1: %s seeded, %d band cases %s; AC-2: %d cases; polygons: %d cases; periodic: %d cases; "
          "orient: %d cases"
          % (expected["ac1"]["counts"], len(band_cases), expected["ac1"]["band_counts"], len(ac2), n_poly,
             sum(len(f["cases"]) for f in expected["periodic"].values()), len(expected["orient"])))
    return files


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    if check:
        stale = [n for n, text in files.items()
                 if not (HERE / n).exists() or (HERE / n).read_text(encoding="utf-8") != text]
        if stale:
            raise SystemExit(f"stale: {stale}")
        print(f"{len(files)} files up to date")
    else:
        for n, text in files.items():
            (HERE / n).write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {len(files)} files")


if __name__ == "__main__":
    main()
