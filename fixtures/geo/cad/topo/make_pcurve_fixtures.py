#!/usr/bin/env python3
"""GEO.CAD.TOPO.PCURVE fixtures and expected values (independent Python reference, CC-BY-4.0).

Writes, next to this script:
  plate_with_hole_nopcurve.json   planar plate, square outer loop, circular hole, 3-D curves only (AC-1, AC-5)
  plate_with_hole_offset.json     the same with the hole circle 1 mm above the plane (AC-4)
  cylinder_band.json              full cylinder band, two circular loops across the seam (AC-2)
  cylinder_band_pcurves.json      the same with imported 2-D line pcurves (AC-3, REQ-005)
  cylinder_band_pcurve_off.json   imported pcurves, the top one 1 mm off in v (REQ-005)
  sphere_meridian.json            hemisphere bounded by one great circle through both poles (poles)
  sphere_pole_straddle.json       the same circle with its range shifted so no sample hits a pole (REQ-003)
  cone_apex_sector.json           cone sector with two generators through the apex (apex)
  torus_tubes.json                torus sector bounded by two tube circles (periodic v)
  pcurve_expected.json            oracle values and model digests for all of them

Run:  python fixtures/geo/cad/topo/make_pcurve_fixtures.py          (writes everything)
      python fixtures/geo/cad/topo/make_pcurve_fixtures.py --check  (fails if anything is stale)

Stdlib only; never runs the Rust code. Two independent parts:

1. Model (bit-exact). The pinned derivation of crates/fid-geo-cad/src/topo/pcurve.rs (module
   docs) written again in binary64 Python, on top of the bit-exact model of the GEO.NURBS
   analytic types (fixtures/geo/nurbs/analytic/make_analytic_fixtures.py, itself checked
   against fid-num's libm model and the GEO.NURBS.REF oracle). CPython floats are binary64,
   round to nearest even, never fused, so the model's bits are the Rust bits on every target:
   the per-coedge and per-body SHA-256 digests below are AC-5.
2. Oracle (independent, 60 digits). The edge curves and surfaces in closed form (`decimal`,
   sin/cos/atan2 of tools/fixtures/nurbs_oracle.py), with the placements orthonormalised
   exactly from the f64 inputs, exact closed-form inversion (plane: projection; cylinder,
   cone, sphere, torus: azimuth and meridian geometry), the pinned chart rules (singular
   samples by |Su|/|Sv| <= 2^-40 from closed-form derivative lengths, nearest-representative
   unwrapping with exact ties going up, pole duplicates, loop periods and offsets) and the
   left-to-right bisection, each decision checked to be at least a relative 1e-6 (tolerance)
   or 1e-20 (unwrapping) away from its threshold, so binary64 rounding cannot flip it.
   Before anything is written the model must agree with the oracle: same outcome, the same
   parameters t (bit for bit; midpoints of binary64 values are exact), every vertex within
   1e-12 after the loop's chart offset, the same periods, max_dev within 1e-15 m, the failure
   distance within 1e-15 m. The Rust tests compare against the oracle values (charted uv at
   the 17 initial samples, segment counts, max_dev, periods, disagreement) and the model digests.

Digest text (pinned; every f64 is its bit pattern as 16 lowercase hex digits):
  derived coedge:  "D <vertices>\\n", "<t> <u> <v>\\n" per vertex, "m <max_dev_m>\\n"
  imported:        "I\\n"
  failed:          "F <reason> <best_deviation_m>\\n"  (reasons: Tolerance, NoEdgeCurve, CurveEval,
                   SurfaceEval, Inversion:NonUnique, Inversion:Tie, Inversion:Unresolved:<r>,
                   Inversion:Error:<code>)
  face:            "S <period_u|-> <period_v|->\\n", then "L <offset_u> <offset_v> <pu pv|->\\n" per loop
  winding:         "W <loop index> <pu> <pv>\\n"
A coedge's digest is SHA-256 of its text; a body's digest is SHA-256 of all coedge texts in
canonical order, then all face texts in face order, then the winding lines.
"""

import hashlib
import importlib.util
import json
import math
import struct
import sys
from decimal import Decimal, localcontext
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
LICENSE = "CC-BY-4.0"


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


AN = _load("make_analytic_fixtures", "fixtures/geo/nurbs/analytic/make_analytic_fixtures.py")
OR = AN.OR
D = Decimal

TAU = 6.283185307179586
FRAC_PI_2 = 1.5707963267948966
TOL = 1e-8
INITIAL = 17
MAX_DEPTH = 12
SAMPLES_DIS = 33
SING2 = 2.0 ** -80


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return "%016x" % bits(x)


# ================================================================ fixtures (builder scripts)

def frame(o, z, x):
    return {"origin": list(o), "axis": list(z), "ref_dir": list(x)}


def line(o, direction, normal):
    """Line O + t X: the direction is the reference direction X, the axis any normal."""
    return dict(type="line", **frame(o, normal, direction))


def circle(o, z, x, r):
    return dict(type="circle", radius=r, **frame(o, z, x))


def body(desc, bid, vertices, edges, coedges, loops, faces):
    return {
        "description": desc,
        "license": LICENSE,
        "body_id": bid,
        "vertices": [{"point": list(p)} for p in vertices],
        "edges": [dict(start=s, end=e, range=list(r), curve=c, label=lab) for (s, e, r, c, lab) in edges],
        "coedges": [dict(edge=e, sense=s, pcurve=p, label=lab) for (e, s, p, lab) in coedges],
        "loops": [dict(coedges=cs, outer=o, label=lab) for (cs, o, lab) in loops],
        "faces": [dict(surface=s, sense="same", loops=ls, label=lab) for (s, ls, lab) in faces],
        "shells": [{"faces": list(range(len(faces))), "closed": False}],
    }


Z = (0.0, 0.0, 1.0)
X = (1.0, 0.0, 0.0)
O = (0.0, 0.0, 0.0)


def plate(hole_z, name, desc):
    w, h, cx, cy, r = 0.1, 0.08, 0.04, 0.03, 0.01
    verts = [(0.0, 0.0, 0.0), (w, 0.0, 0.0), (w, h, 0.0), (0.0, h, 0.0), (cx + r, cy, hole_z)]
    edges = [
        (0, 1, (0.0, w), line(verts[0], (1.0, 0.0, 0.0), Z), "e-bottom"),
        (1, 2, (0.0, h), line(verts[1], (0.0, 1.0, 0.0), Z), "e-right"),
        (2, 3, (0.0, w), line(verts[2], (-1.0, 0.0, 0.0), Z), "e-top"),
        (3, 0, (0.0, h), line(verts[3], (0.0, -1.0, 0.0), Z), "e-left"),
        (4, 4, (0.0, TAU), circle((cx, cy, hole_z), Z, X, r), "e-hole"),
    ]
    coedges = [(0, "same", None, "c-bottom"), (1, "same", None, "c-right"), (2, "same", None, "c-top"),
               (3, "same", None, "c-left"), (4, "reversed", None, "c-hole")]
    loops = [([0, 1, 2, 3], True, "l-outer"), ([4], False, "l-hole")]
    faces = [(dict(type="plane", **frame(O, Z, X)), [0, 1], "f-plate")]
    return name, body(desc, "9c0e9c0e9c0e9c0e9c0e9c0e9c0e9c0e", verts, edges, coedges, loops, faces)


def band(pcurves, name, desc, v_top=0.02):
    r, h = 0.05, 0.02
    verts = [(r, 0.0, 0.0), (r, 0.0, h)]
    edges = [(0, 0, (0.0, TAU), circle(O, Z, X, r), "e-bottom"),
             (1, 1, (0.0, TAU), circle((0.0, 0.0, h), Z, X, r), "e-top")]
    pc = [None, None]
    if pcurves:
        pc = [{"type": "line2", "origin": [0.0, 0.0], "dir": [1.0, 0.0]},
              {"type": "line2", "origin": [0.0, v_top], "dir": [1.0, 0.0]}]
    coedges = [(0, "same", pc[0], "c-bottom"), (1, "reversed", pc[1], "c-top")]
    loops = [([0], False, "l-bottom"), ([1], False, "l-top")]
    faces = [(dict(type="cylinder", radius=r, **frame(O, Z, X)), [0, 1], "f-band")]
    return name, body(desc, "ba4dba4dba4dba4dba4dba4dba4dba4d", verts, edges, coedges, loops, faces)


def sphere(t0, name, desc):
    r = 0.05
    verts = [(r * math.cos(t0), 0.0, r * math.sin(t0))]
    # Great circle in the xz-plane: axis -Y, reference X, so Y = Z x X = +Z and C(t) = r(cos t, 0, sin t).
    edges = [(0, 0, (t0, t0 + TAU), circle(O, (0.0, -1.0, 0.0), X, r), "e-meridian")]
    coedges = [(0, "same", None, "c-meridian")]
    loops = [([0], True, "l-meridian")]
    faces = [(dict(type="sphere", radius=r, **frame(O, Z, X)), [0], "f-hemisphere")]
    return name, body(desc, "5fe05fe05fe05fe05fe05fe05fe05fe0", verts, edges, coedges, loops, faces)


def cone():
    a = 0.5235987755982988  # pi/6
    sa, ca, ta = math.sin(a), math.cos(a), math.tan(a)
    h = 0.05
    rb = h * ta
    verts = [(0.0, 0.0, 0.0), (rb, 0.0, h), (0.0, rb, h)]
    edges = [(0, 1, (0.0, h / ca), line(O, (sa, 0.0, ca), (ca, 0.0, -sa)), "e-gen0"),
             (1, 2, (0.0, FRAC_PI_2), circle((0.0, 0.0, h), Z, X, rb), "e-arc"),
             (0, 2, (0.0, h / ca), line(O, (0.0, sa, ca), (0.0, ca, -sa)), "e-gen1")]
    coedges = [(0, "same", None, "c-gen0"), (1, "same", None, "c-arc"), (2, "reversed", None, "c-gen1")]
    loops = [([0, 1, 2], True, "l-sector")]
    faces = [(dict(type="cone", radius=0.0, half_angle=a, **frame(O, Z, X)), [0], "f-cone")]
    return "cone_apex_sector", body("cone sector (apex at the origin, half-angle pi/6, height 0.05 m) bounded by "
                                    "two generators through the apex and a quarter arc",
                                    "c0e5c0e5c0e5c0e5c0e5c0e5c0e5c0e5", verts, edges, coedges, loops, faces)


def torus():
    big, small = 0.05, 0.01
    verts, edges = [], []
    for i, phi in enumerate((0.5235987755982988, 2.6179938779914944)):  # pi/6, 5 pi/6
        c, s = math.cos(phi), math.sin(phi)
        verts.append(((big + small) * c, (big + small) * s, 0.0))
        # axis (s, -c, 0), reference (c, s, 0): Y = Z x X = +Z, so the circle's t is the torus v.
        edges.append((i, i, (0.0, TAU), circle((big * c, big * s, 0.0), (s, -c, 0.0), (c, s, 0.0), small),
                      "e-tube%d" % i))
    coedges = [(0, "same", None, "c-tube0"), (1, "reversed", None, "c-tube1")]
    loops = [([0], False, "l-tube0"), ([1], False, "l-tube1")]
    faces = [(dict(type="torus", major_radius=big, minor_radius=small, **frame(O, Z, X)), [0, 1], "f-torus")]
    return "torus_tubes", body("torus sector (R 0.05 m, r 0.01 m) between the tube circles at u = pi/6 and 5 pi/6, "
                               "each winding once in v", "70a570a570a570a570a570a570a570a5", verts, edges, coedges,
                               loops, faces)


def fixtures():
    return [
        plate(0.0, "plate_with_hole_nopcurve", "plane plate 100 x 80 mm with a 10 mm radius hole; 3-D curves only"),
        plate(0.001, "plate_with_hole_offset", "the plate with the hole circle 1 mm above the plane (AC-4)"),
        band(False, "cylinder_band", "full cylinder band r 0.05 m, height 0.02 m: two circular loops across the "
                                     "seam, no seam edge, 3-D curves only"),
        band(True, "cylinder_band_pcurves", "the cylinder band with imported 2-D line pcurves u = t"),
        band(True, "cylinder_band_pcurve_off", "the cylinder band with the top pcurve 1 mm off in v", 0.021),
        sphere(0.0, "sphere_meridian", "hemisphere r 0.05 m bounded by one great circle through both poles; "
                                       "the poles are samples 4 and 12"),
        sphere(0.1, "sphere_pole_straddle", "the great circle with range [0.1, 0.1 + 2 pi]: no sample hits a "
                                            "pole, so refinement cannot converge there (REQ-003)"),
        cone(),
        torus(),
    ]


def render(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def canonical(b):
    """Coedges, loops and faces in the canonical traversal order of GEO.CAD.TOPO.MODEL."""
    faces, loops, coedges = [], [], []
    for sh in b["shells"]:
        for fi in sh["faces"]:
            f = b["faces"][fi]
            fl = []
            for li in f["loops"]:
                lp = b["loops"][li]
                cs = []
                for ci in lp["coedges"]:
                    cs.append(len(coedges))
                    coedges.append(b["coedges"][ci])
                fl.append(len(loops))
                loops.append({"label": lp["label"], "outer": lp["outer"], "coedges": cs})
            faces.append({"surface": f["surface"], "loops": fl})
    return faces, loops, coedges


# ================================================================ part 1: bit-exact model

class Fail(Exception):
    def __init__(self, reason, best):
        super().__init__(reason)
        self.reason = reason
        self.best = best


def analytic_params(g):
    t = g["type"]
    if t in ("circle", "cylinder", "sphere"):
        return {"radius": g["radius"]}
    if t == "cone":
        return {"radius": g["radius"], "semi_angle": g["half_angle"]}
    if t == "torus":
        return {"major_radius": g["major_radius"], "minor_radius": g["minor_radius"]}
    return {}


def model_of(g):
    return AN.Model(g["type"], list(g["origin"]), list(g["axis"]), list(g["ref_dir"]), analytic_params(g))


def larger(a, b):
    return b if b > a else a


def grid(t0, t1, k, n):
    return t1 if k == n else t0 + ((t1 - t0) * float(k)) / float(n)


def nearest(c, r, p):
    k = math.floor((r - c) / p + 0.5)
    return c if k == 0 else c + float(k) * p


def reduce(j, p):
    return j - float(math.floor(j / p + 0.5)) * p


def hyp(x, y):
    return AN.hyp(x, y)


def dist3(a, b):
    e = [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    try:
        d = hyp(hyp(e[0], e[1]), e[2])
    except AN.Err:
        return None
    return d if math.isfinite(d) else None


class MSurf:
    def __init__(self, g):
        self.m = model_of(g)
        self.periods = {"plane": [None, None], "cylinder": [TAU, None], "cone": [TAU, None],
                        "sphere": [TAU, None], "torus": [TAU, TAU]}[g["type"]]
        self.lower = [0.0, 0.0]

    def point(self, uv):
        return self.m.surf_eval(uv[0], uv[1], 0)[1][(0, 0)]

    def singular(self, uv):
        S = self.m.surf_eval(uv[0], uv[1], 1)[1]
        a2, b2 = AN.dot(S[(1, 0)], S[(1, 0)]), AN.dot(S[(0, 1)], S[(0, 1)])
        return [a2 <= SING2 * b2, b2 <= SING2 * a2]

    def invert(self, q):
        try:
            params, _foot, dist, nu = self.m.invert(q)
        except AN.Err as e:
            raise Fail("Inversion:Error:" + e.code, None)
        if nu:
            raise Fail("Inversion:NonUnique", dist)
        return list(params), dist


class MCurve:
    def __init__(self, g):
        self.m = model_of(g)

    def point(self, t):
        return self.m.curve_eval(t, 0)[1]


class Raw:
    def __init__(self, t, q, uv, dist, sing, depth):
        self.t, self.q, self.uv, self.dist, self.sing, self.depth = t, q, uv, dist, sing, depth


def m_sample(S, C, t, depth):
    try:
        q = C.point(t)
    except (AN.Err, AN.EvalErr):
        raise Fail("CurveEval", None)
    uv, dist = S.invert(q)
    try:
        sing = S.singular(uv)
    except (AN.Err, AN.EvalErr):
        raise Fail("SurfaceEval", dist)
    return Raw(t, q, uv, dist, sing, depth)


def m_deviation(S, uv, q):
    try:
        p = S.point(uv)
    except (AN.Err, AN.EvalErr):
        raise Fail("SurfaceEval", None)
    d = dist3(p, q)
    if d is None:
        raise Fail("SurfaceEval", None)
    return d


class MOut:
    def __init__(self):
        self.uv, self.t, self.max_dev = [], [], 0.0

    def emit(self, S, uv, t, q):
        try:
            d = m_deviation(S, uv, q)
        except Fail as f:
            raise Fail(f.reason, self.max_dev)
        self.max_dev = larger(self.max_dev, d)
        if d > TOL:
            raise Fail("Tolerance", self.max_dev)
        self.uv.append(list(uv))
        self.t.append(t)


def m_derive(S, C, rng, reversed_, ref):
    """Mirror of `derive` in pcurve.rs; returns (uv, t, max_dev) or raises Fail."""
    periods = S.periods
    init, worst, unknown, fault = [], 0.0, False, None
    for k in range(INITIAL):
        j = INITIAL - 1 - k if reversed_ else k
        try:
            r = m_sample(S, C, grid(rng[0], rng[1], j, INITIAL - 1), 0)
            worst = larger(worst, r.dist)
            init.append(r)
        except Fail as f:
            if f.best is None:
                unknown = True
            else:
                worst = larger(worst, f.best)
            if fault is None:
                fault = f.reason
    if fault is not None:
        raise Fail(fault, math.inf if unknown else worst)
    if worst > TOL:
        raise Fail("Tolerance", worst)
    f0 = init[0]
    first = [0.0, 0.0]
    for i in range(2):
        v = f0.uv[i]
        if f0.sing[i]:
            for r in init:
                if not r.sing[i]:
                    v = r.uv[i]
                    break
        if periods[i] is not None and ref is not None:
            v = nearest(v, ref[i], periods[i])
        first[i] = v
    anchor = list(first)
    out = MOut()
    out.emit(S, first, f0.t, f0.q)
    lt, lq, luv, lsing = f0.t, f0.q, list(first), f0.sing
    stack = list(reversed(init[1:]))
    while stack:
        r = stack.pop()
        ruv = list(r.uv)
        for i in range(2):
            if r.sing[i]:
                ruv[i] = luv[i]
            elif periods[i] is not None:
                rf = anchor[i] if lsing[i] else luv[i]
                ruv[i] = nearest(r.uv[i], rf, periods[i])
        a = list(luv)
        for i in range(2):
            if lsing[i]:
                a[i] = ruv[i]
        mid = [(a[0] + ruv[0]) * 0.5, (a[1] + ruv[1]) * 0.5]
        tm = (lt + r.t) * 0.5
        try:
            qm = C.point(tm)
        except (AN.Err, AN.EvalErr):
            raise Fail("CurveEval", out.max_dev)
        try:
            dm = m_deviation(S, mid, qm)
        except Fail as f:
            raise Fail(f.reason, out.max_dev)
        if dm <= TOL:
            out.max_dev = larger(out.max_dev, dm)
            if bits(a[0]) != bits(luv[0]) or bits(a[1]) != bits(luv[1]):
                out.emit(S, a, lt, lq)
            out.emit(S, ruv, r.t, r.q)
            lt, lq, luv, lsing = r.t, r.q, ruv, r.sing
            continue
        best = larger(out.max_dev, dm)
        if r.depth >= MAX_DEPTH or tm == lt or tm == r.t:
            raise Fail("Tolerance", best)
        try:
            m = m_sample(S, C, tm, r.depth + 1)
        except Fail as f:
            raise Fail(f.reason, math.inf if f.best is None else larger(best, f.best))
        if m.dist > TOL:
            raise Fail("Tolerance", larger(best, m.dist))
        stack.append(Raw(r.t, r.q, r.uv, r.dist, r.sing, r.depth + 1))
        stack.append(m)
    return out.uv, out.t, out.max_dev


def pcurve_point(p, t):
    return [p["origin"][0] + t * p["dir"][0], p["origin"][1] + t * p["dir"][1]]


class Range:
    def __init__(self):
        self.lo, self.hi, self.empty = [0.0, 0.0], [0.0, 0.0], True

    def add(self, p, lt=lambda a, b: a < b):
        for i in range(2):
            if self.empty or lt(p[i], self.lo[i]):
                self.lo[i] = p[i]
            if self.empty or lt(self.hi[i], p[i]):
                self.hi[i] = p[i]
        self.empty = False


def m_body(b):
    """The whole of `derive_pcurves` on one fixture: per-coedge results, charts, windings."""
    faces, loops, coedges = canonical(b)
    res = [None] * len(coedges)
    charts, wound = [], []
    for f in faces:
        S = MSurf(f["surface"])
        results = []
        for li in f["loops"]:
            lp = loops[li]
            cs = lp["coedges"]
            n = len(cs)
            start = next((k for k, ci in enumerate(cs) if coedges[ci]["pcurve"] is not None), 0)
            ends = [None] * n
            rg = Range()
            prev_end = None
            for i in range(n):
                k = (start + i) % n
                ce = coedges[cs[k]]
                e = b["edges"][ce["edge"]]
                rev = ce["sense"] == "reversed"
                if ce["pcurve"] is not None:
                    p = ce["pcurve"]
                    t0, t1 = e["range"]
                    for j in range(SAMPLES_DIS):
                        rg.add(pcurve_point(p, grid(t0, t1, j, SAMPLES_DIS - 1)))
                    a, z = pcurve_point(p, t0), pcurve_point(p, t1)
                    ends[k] = (z, a) if rev else (a, z)
                    res[cs[k]] = ("I",)
                    prev_end = ends[k][1]
                else:
                    try:
                        uv, ts, md = m_derive(S, MCurve(e["curve"]), e["range"], rev, prev_end)
                        for q in uv:
                            rg.add(q)
                        ends[k] = (uv[0], uv[-1])
                        res[cs[k]] = ("D", uv, ts, md)
                        prev_end = uv[-1]
                    except Fail as fl:
                        res[cs[k]] = ("F", fl.reason, fl.best)
                        prev_end = None
            periods = None
            if all(x is not None for x in ends) and not rg.empty:
                periods = [0, 0]
                for i in range(2):
                    p = S.periods[i]
                    if p is None:
                        continue
                    total = 0.0
                    for k in range(n):
                        a, z = ends[k]
                        nxt = ends[(k + 1) % n][0]
                        total += z[i] - a[i]
                        total += reduce(nxt[i] - z[i], p)
                    periods[i] = math.floor(total / p + 0.5)
                if periods != [0, 0]:
                    wound.append((li, periods))
            centre = [(rg.lo[0] + rg.hi[0]) * 0.5, (rg.lo[1] + rg.hi[1]) * 0.5]
            results.append((periods, centre))
        anchor = next((j for j, li in enumerate(f["loops"]) if loops[li]["outer"]), 0)
        offsets = [[0.0, 0.0] for _ in results]
        if results and results[anchor][0] is not None:
            ca = results[anchor][1]
            off_a, c0 = [0.0, 0.0], [0.0, 0.0]
            for i in range(2):
                p = S.periods[i]
                if p is not None:
                    off_a[i] = 0.0 - float(math.floor((ca[i] - S.lower[i]) / p)) * p
                    c0[i] = ca[i] + off_a[i]
            for j, (per, cen) in enumerate(results):
                if j == anchor:
                    offsets[j] = off_a
                    continue
                if per is None:
                    continue
                for i in range(2):
                    p = S.periods[i]
                    if p is not None:
                        offsets[j][i] = 0.0 - float(math.floor((cen[i] - c0[i]) / p + 0.5)) * p
        charts.append((S.periods, offsets, [r[0] for r in results]))
    wound.sort(key=lambda w: w[0])
    return faces, loops, coedges, res, charts, wound


def coedge_text(r):
    if r[0] == "I":
        return "I\n"
    if r[0] == "F":
        return "F %s %s\n" % (r[1], hx(r[2]))
    _, uv, ts, md = r
    lines = ["D %d\n" % len(uv)]
    lines += ["%s %s %s\n" % (hx(t), hx(p[0]), hx(p[1])) for t, p in zip(ts, uv)]
    lines.append("m %s\n" % hx(md))
    return "".join(lines)


def chart_text(c):
    periods, offsets, pers = c
    s = "S %s %s\n" % tuple("-" if p is None else hx(p) for p in periods)
    for off, per in zip(offsets, pers):
        s += "L %s %s %s\n" % (hx(off[0]), hx(off[1]), "-" if per is None else "%d %d" % tuple(per))
    return s


def digests(m):
    faces, loops, coedges, res, charts, wound = m
    per = {}
    total = hashlib.sha256()
    for ce, r in zip(coedges, res):
        t = coedge_text(r).encode()
        per[ce["label"]] = hashlib.sha256(t).hexdigest()
        total.update(t)
    for c in charts:
        total.update(chart_text(c).encode())
    for li, p in wound:
        total.update(("W %d %d %d\n" % (li, p[0], p[1])).encode())
    return per, total.hexdigest()


# ================================================================ part 2: 60-digit oracle

def dv(x):
    return OR.dec_of(x)


def dvec(v):
    return [dv(c) for c in v]


class OPlace:
    """Placement orthonormalised exactly (60 digits) from the f64 inputs."""

    def __init__(self, g):
        z, x = dvec(g["axis"]), dvec(g["ref_dir"])
        self.o = dvec(g["origin"])
        self.z = OR.vscale(1 / OR.dnorm(z), z)
        xp = OR.vsub(x, OR.vscale(OR.dot(x, self.z), self.z))
        self.x = OR.vscale(1 / OR.dnorm(xp), xp)
        self.y = OR.cross(self.z, self.x)

    def world(self, a, b, c):
        return [self.o[i] + a * self.x[i] + b * self.y[i] + c * self.z[i] for i in range(3)]

    def local(self, p):
        d = OR.vsub(p, self.o)
        return OR.dot(d, self.x), OR.dot(d, self.y), OR.dot(d, self.z)


class OCurve:
    def __init__(self, g):
        self.t, self.pl = g["type"], OPlace(g)
        self.r = dv(g.get("radius", 0.0))

    def point(self, t):
        t = dv(t)
        if self.t == "line":
            return self.pl.world(t, 0, 0)
        s, c = OR.sin_cos(t)
        return self.pl.world(self.r * c, self.r * s, 0)


def dhyp(a, b):
    return (a * a + b * b).sqrt()


SING_REL = D(2) ** -40


class OSurf:
    def __init__(self, g):
        self.type, self.pl = g["type"], OPlace(g)
        T = self.type
        self.periods = {"plane": [None, None], "torus": [OR.TWO_PI, OR.TWO_PI]}.get(T, [OR.TWO_PI, None])
        if T in ("cylinder", "sphere"):
            self.r = dv(g["radius"])
        if T == "cone":
            self.r0 = dv(g["radius"])
            self.sa, self.ca = OR.sin_cos(dv(g["half_angle"]))
            self.ta = self.sa / self.ca
        if T == "torus":
            self.R, self.rr = dv(g["major_radius"]), dv(g["minor_radius"])

    def point(self, uv):
        u, v = uv
        T = self.type
        if T == "plane":
            return self.pl.world(u, v, 0)
        su, cu = OR.sin_cos(u)
        if T == "cylinder":
            return self.pl.world(self.r * cu, self.r * su, v)
        if T == "cone":
            rho = self.r0 + v * self.ta
            return self.pl.world(rho * cu, rho * su, v)
        sv, cv = OR.sin_cos(v)
        if T == "sphere":
            return self.pl.world(self.r * cv * cu, self.r * cv * su, self.r * sv)
        rho = self.R + self.rr * cv
        return self.pl.world(rho * cu, rho * su, self.rr * sv)

    def invert(self, p):
        """Closed-form closest point: (uv, distance, |Su|/|Sv| at the foot)."""
        x, y, z = self.pl.local(p)
        T = self.type
        if T == "plane":
            return [x, y], abs(z), D(1)
        rho = dhyp(x, y)
        u = OR.wrap_2pi(OR.atan2(y, x))
        if T == "cylinder":
            return [u, z], abs(rho - self.r), self.r
        if T == "sphere":
            v = OR.atan2(z, rho)
            _, cv = OR.sin_cos(v)
            return [u, v], abs(dhyp(rho, z) - self.r), abs(cv)
        if T == "cone":
            w0 = rho - self.r0
            g = z * self.sa + self.r0 * self.ca
            assert g > 0 or (rho == 0 and z == 0), "oracle cone: query below the apex plane"
            s = w0 * self.sa + z * self.ca
            v = s * self.ca
            ratio = abs(self.r0 + v * self.ta) / (self.ta * self.ta + 1).sqrt()
            return [u, v], abs(w0 * self.ca - z * self.sa), ratio
        mx = rho - self.R
        v = OR.wrap_2pi(OR.atan2(z, mx))
        _, cv = OR.sin_cos(v)
        return [u, v], abs(dhyp(mx, z) - self.rr), (self.R + self.rr * cv) / self.rr


def d_dist(a, b):
    return dhyp(dhyp(a[0] - b[0], a[1] - b[1]), a[2] - b[2])


def o_floor(x, what):
    """floor with exact ties: a value within 1e-30 of an integer is that integer (ties go up);
    anything between 1e-30 and 1e-20 of an integer is too close to call."""
    n = x.to_integral_value()
    gap = abs(x - n)
    assert not (D("1e-30") < gap < D("1e-20")), f"oracle: {what} too close to an integer ({x})"
    if gap <= D("1e-30"):
        return int(n)
    return int(x.to_integral_value(rounding="ROUND_FLOOR"))


def o_nearest(c, r, p):
    return c + o_floor((r - c) / p + D("0.5"), "unwrap") * p


def o_reduce(j, p):
    return j - o_floor(j / p + D("0.5"), "reduce") * p


def clear(value, threshold, what):
    """value <= threshold, decided with a relative margin of 1e-6."""
    assert abs(value - threshold) > threshold * D("1e-6"), f"oracle: {what} too close to its threshold"
    return value <= threshold


class OFail(Exception):
    def __init__(self, reason, best):
        super().__init__(reason)
        self.reason, self.best = reason, best


def o_derive(S, C, rng, reversed_, ref):
    """The pinned derivation at 60 digits (t values are the binary64 parameters, exactly)."""
    tol = dv(TOL)
    periods = S.periods

    def sample(t):
        q = C.point(t)
        uv, d, ratio = S.invert(q)
        assert ratio < SING_REL / 100 or ratio > SING_REL * 100, "oracle: singularity too close to call"
        return {"t": t, "q": q, "uv": uv, "d": d, "sing": [ratio <= SING_REL, False]}

    init = []
    for k in range(INITIAL):
        j = INITIAL - 1 - k if reversed_ else k
        init.append(sample(grid(rng[0], rng[1], j, INITIAL - 1)))
    worst = max(s["d"] for s in init)
    if not clear(worst, tol, "sample distance"):
        raise OFail("Tolerance", worst)
    f0 = init[0]
    first = list(f0["uv"])
    for i in range(2):
        if f0["sing"][i]:
            nxt = next((s for s in init if not s["sing"][i]), None)
            if nxt is not None:
                first[i] = nxt["uv"][i]
        if periods[i] is not None and ref is not None:
            first[i] = o_nearest(first[i], ref[i], periods[i])
    anchor = list(first)
    verts, ts, max_dev = [list(first)], [f0["t"]], max(D(0), d_dist(S.point(first), f0["q"]))
    L = {"t": f0["t"], "q": f0["q"], "uv": list(first), "sing": f0["sing"]}
    stack = [dict(s, depth=0) for s in reversed(init[1:])]
    while stack:
        r = stack.pop()
        ruv = list(r["uv"])
        for i in range(2):
            if r["sing"][i]:
                ruv[i] = L["uv"][i]
            elif periods[i] is not None:
                rf = anchor[i] if L["sing"][i] else L["uv"][i]
                ruv[i] = o_nearest(r["uv"][i], rf, periods[i])
        a = list(L["uv"])
        for i in range(2):
            if L["sing"][i]:
                a[i] = ruv[i]
        mid = [(a[0] + ruv[0]) / 2, (a[1] + ruv[1]) / 2]
        tm = (L["t"] + r["t"]) * 0.5
        dm = d_dist(S.point(mid), C.point(tm))
        if clear(dm, tol, "chord midpoint deviation"):
            max_dev = max(max_dev, dm)
            if abs(a[0] - L["uv"][0]) > D("1e-40") or abs(a[1] - L["uv"][1]) > D("1e-40"):
                verts.append(a)
                ts.append(L["t"])
                max_dev = max(max_dev, d_dist(S.point(a), L["q"]))
            verts.append(ruv)
            ts.append(r["t"])
            max_dev = max(max_dev, d_dist(S.point(ruv), r["q"]))
            L = {"t": r["t"], "q": r["q"], "uv": ruv, "sing": r["sing"]}
            continue
        if r["depth"] >= MAX_DEPTH or tm == L["t"] or tm == r["t"]:
            raise OFail("Tolerance", max(max_dev, dm))
        m = sample(tm)
        assert clear(m["d"], tol, "midpoint sample distance")
        stack.append(dict(r, depth=r["depth"] + 1))
        stack.append(dict(m, depth=r["depth"] + 1))
    return verts, ts, max_dev


def o_body(b):
    """Oracle results: per coedge ("D", verts, ts, max_dev) / ("F", reason, best) / ("I", disagreement),
    per face the charted offsets and periods per loop."""
    faces, loops, coedges = canonical(b)
    res = [None] * len(coedges)
    charts = []
    for f in faces:
        S = OSurf(f["surface"])
        results = []
        for li in f["loops"]:
            cs = loops[li]["coedges"]
            n = len(cs)
            start = next((k for k, ci in enumerate(cs) if coedges[ci]["pcurve"] is not None), 0)
            ends, pts, prev_end = [None] * n, [], None
            for i in range(n):
                k = (start + i) % n
                ce = coedges[cs[k]]
                e = b["edges"][ce["edge"]]
                rev = ce["sense"] == "reversed"
                C = OCurve(e["curve"])
                if ce["pcurve"] is not None:
                    p = ce["pcurve"]
                    t0, t1 = e["range"]
                    worst = D(0)
                    for j in range(SAMPLES_DIS):
                        t = grid(t0, t1, j, SAMPLES_DIS - 1)
                        uv = [dv(p["origin"][0]) + dv(t) * dv(p["dir"][0]), dv(p["origin"][1]) + dv(t) * dv(p["dir"][1])]
                        pts.append(uv)
                        worst = max(worst, d_dist(S.point(uv), C.point(t)))
                    a = [dv(p["origin"][c]) + dv(t0) * dv(p["dir"][c]) for c in range(2)]
                    z = [dv(p["origin"][c]) + dv(t1) * dv(p["dir"][c]) for c in range(2)]
                    ends[k] = (z, a) if rev else (a, z)
                    res[cs[k]] = ("I", worst)
                    prev_end = ends[k][1]
                else:
                    try:
                        verts, ts, md = o_derive(S, C, e["range"], rev, prev_end)
                        pts += verts
                        ends[k] = (verts[0], verts[-1])
                        res[cs[k]] = ("D", verts, ts, md)
                        prev_end = verts[-1]
                    except OFail as fl:
                        res[cs[k]] = ("F", fl.reason, fl.best)
                        prev_end = None
            periods = None
            if all(x is not None for x in ends):
                periods = [0, 0]
                for i in range(2):
                    p = S.periods[i]
                    if p is None:
                        continue
                    total = D(0)
                    for k in range(n):
                        a, z = ends[k]
                        total += z[i] - a[i] + o_reduce(ends[(k + 1) % n][0][i] - z[i], p)
                    periods[i] = o_floor(total / p + D("0.5"), "periods")
            centre = [(min(q[i] for q in pts) + max(q[i] for q in pts)) / 2 for i in range(2)] if pts else [D(0)] * 2
            results.append((periods, centre))
        anchor = next((j for j, li in enumerate(f["loops"]) if loops[li]["outer"]), 0)
        offsets = [[D(0), D(0)] for _ in results]
        if results and results[anchor][0] is not None:
            ca = results[anchor][1]
            c0 = [D(0), D(0)]
            for i in range(2):
                p = S.periods[i]
                if p is not None:
                    offsets[anchor][i] = -o_floor(ca[i] / p, "anchor") * p
                    c0[i] = ca[i] + offsets[anchor][i]
            for j, (per, cen) in enumerate(results):
                if j == anchor or per is None:
                    continue
                for i in range(2):
                    p = S.periods[i]
                    if p is not None:
                        offsets[j][i] = -o_floor((cen[i] - c0[i]) / p + D("0.5"), "offset") * p
        charts.append((offsets, [r[0] for r in results]))
    return faces, loops, coedges, res, charts


# ================================================================ comparison and output

def compare(name, m, o):
    """The model must agree with the oracle (see the module docstring)."""
    mfaces, mloops, mco, mres, mcharts, _ = m
    ofaces, oloops, oco, ores, ocharts = o
    loop_of = {}
    for fi, f in enumerate(ofaces):
        for j, li in enumerate(f["loops"]):
            for ci in oloops[li]["coedges"]:
                loop_of[ci] = (fi, j)
    for ci, (mr, orr) in enumerate(zip(mres, ores)):
        lab = mco[ci]["label"]
        assert mr[0] == orr[0], f"{name} {lab}: model {mr[0]} vs oracle {orr[0]}"
        if mr[0] == "F":
            assert mr[1] == orr[1], f"{name} {lab}: reason {mr[1]} vs {orr[1]}"
            if mr[1] == "Tolerance" and orr[1] == "Tolerance" and orr[2] > 0:
                assert abs(dv(mr[2]) - orr[2]) <= D("1e-15") or (orr[2] > dv(TOL) and mr[2] > TOL), \
                    f"{name} {lab}: best deviation {mr[2]} vs {orr[2]}"
            continue
        if mr[0] == "I":
            continue
        _, muv, mts, mmd = mr
        _, ouv, ots, omd = orr
        assert len(muv) == len(ouv), f"{name} {lab}: {len(muv)} vertices vs oracle {len(ouv)}"
        assert all(bits(a) == bits(float(b)) for a, b in zip(mts, ots)), f"{name} {lab}: parameters differ"
        assert abs(dv(mmd) - omd) <= D("1e-15"), f"{name} {lab}: max_dev {mmd} vs {omd}"
        fi, j = loop_of[ci]
        moff, ooff = mcharts[fi][1][j], ocharts[fi][0][j]
        for a, b in zip(muv, ouv):
            for i in range(2):
                diff = (dv(a[i]) + dv(moff[i])) - (b[i] + ooff[i])
                assert abs(diff) <= D("1e-12"), f"{name} {lab}: charted vertex {a} vs {b}"
    for fi in range(len(mcharts)):
        mp, op = mcharts[fi][2], ocharts[fi][1]
        assert mp == op, f"{name} face {fi}: periods {mp} vs oracle {op}"


def fnum(x):
    return float(x)


def expected_entry(o, digest_per, body_digest):
    ofaces, oloops, oco, ores, ocharts = o
    loop_of = {}
    for fi, f in enumerate(ofaces):
        for j, li in enumerate(f["loops"]):
            for ci in oloops[li]["coedges"]:
                loop_of[ci] = (fi, j)
    co = {}
    for ci, orr in enumerate(ores):
        lab = oco[ci]["label"]
        e = {"digest": digest_per[lab]}
        if orr[0] == "I":
            e.update(status="imported", disagreement_m=fnum(orr[1]))
        elif orr[0] == "F":
            e.update(status="failed", reason=orr[1], best_deviation_m=fnum(orr[2]))
        else:
            _, verts, ts, md = orr
            fi, j = loop_of[ci]
            off = ocharts[fi][0][j]
            e.update(status="derived", vertices=len(verts), max_dev_m=fnum(md))
            e["_ts"] = ts
            e["_verts"] = [[v[0] + off[0], v[1] + off[1]] for v in verts]
        co[lab] = e
    lp = {}
    for fi, f in enumerate(ofaces):
        for j, li in enumerate(f["loops"]):
            per = ocharts[fi][1][j]
            lp[oloops[li]["label"]] = {"periods": per}
    return {"body_digest": body_digest, "coedges": co, "loops": lp}


def finish_samples(b, entry):
    """Keeps, per derived coedge, the charted oracle vertices at the 17 initial parameters."""
    _, _, coedges = canonical(b)
    for ce in coedges:
        e = entry["coedges"][ce["label"]]
        ts = e.pop("_ts", None)
        verts = e.pop("_verts", None)
        if ts is None:
            continue
        rng = b["edges"][ce["edge"]]["range"]
        want = {bits(grid(rng[0], rng[1], k, INITIAL - 1)) for k in range(INITIAL)}
        e["samples"] = [{"t": hx(t), "uv": [fnum(v[0]), fnum(v[1])]} for t, v in zip(ts, verts) if bits(t) in want]
        e["segments"] = sum(1 for a, z in zip(ts, ts[1:]) if bits(a) != bits(z))
        assert len({s["t"] for s in e["samples"]}) == INITIAL


def build():
    files = {}
    expected = {
        "description": "GEO.CAD.TOPO.PCURVE expected values: 60-digit closed-form oracle (charted uv at the 17 "
                       "initial samples, vertex and segment counts, max_dev, failures, periods, disagreement) and "
                       "SHA-256 digests of the bit-exact model (AC-5). Written by make_pcurve_fixtures.py; do not edit.",
        "license": LICENSE,
        "tolerance_m": TOL,
        "fixtures": {},
    }
    for name, b in fixtures():
        files[name + ".json"] = render(b)
        m = m_body(b)
        o = o_body(b)
        compare(name, m, o)
        per, total = digests(m)
        entry = expected_entry(o, per, total)
        finish_samples(b, entry)
        expected["fixtures"][name] = entry
        print(f"{name}: model agrees with the oracle; body digest {total[:16]}")
    files["pcurve_expected.json"] = render(expected)
    return files


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    if check:
        stale = [n for n, text in files.items() if (HERE / n).read_text(encoding="utf-8") != text]
        if stale:
            raise SystemExit(f"stale: {stale}")
        print(f"{len(files)} files up to date")
    else:
        for n, text in files.items():
            (HERE / n).write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {len(files)} files")


if __name__ == "__main__":
    main()
