#!/usr/bin/env python3
"""GEO.CAD.TOPO.VALID fixtures and expected reports (independent Python reference, CC-BY-4.0).

Writes, next to this script:
  defects/<kind>.json   one builder script per defect kind, each with one seeded defect (AC-2)
  valid_cap50.json      a sheet body with 50 defects for the cap test (AC-4)
  capped_band.json, capped_band_top_reversed.json, capped_band_flipped.json
                        a closed cylinder whose band face has an undetermined orientation,
                        as built, with its top cap and with its band marked Reversed: never
                        clean, both circle edges unchecked (REQ-004, fail closed)
  valid_expected.json   the expected report of every fixture, of cube_labelled.json (AC-1) and
                        of cylinder_capped.json, as defect lists and as canonical JSON (AC-5)

Run:  python fixtures/geo/cad/topo/make_valid_fixtures.py          (writes everything)
      python fixtures/geo/cad/topo/make_valid_fixtures.py --check  (fails if anything is stale)

Stdlib only; never runs the Rust code. The checker below re-implements the defect rules of
spec/geo/cad/topo/valid/NODE.md (and the module documentation of
crates/fid-geo-cad/src/topo/valid.rs) on the true geometry, not on fid-geo-cad's polylines:

- curves and surfaces in closed form at 60 digits (`decimal`, sin/cos/atan2 of
  tools/fixtures/nurbs_oracle.py), placements orthonormalised exactly from the f64 inputs;
- point-to-surface distances in closed form (plane: |(p - O).Z|; cylinder: |rho - r|;
  torus: |hypot(rho - R, z) - r|), never by iteration;
- trimming loops as exact polygons (`fractions`) in the face chart: line edges on a plane by
  orthogonal projection of their end points, imported line pcurves by their end points, plus
  the connectors between consecutive coedges; a loop of one circle on a plane is a circle;
  intersections, fold-backs and signed areas are decided in exact rational arithmetic;
- PCURVE failures modelled only for the two causes the fixtures use: no edge curve and no
  pcurve (NoEdgeCurve, best deviation infinite), and an initial sample farther than the
  tolerance from the surface (Tolerance, best deviation the largest initial sample distance);
- fail closed: a face whose orientation cannot be told (no loops, a failed pcurve, a winding
  loop, a zero area, no outer loop, loops that disagree with each other) never counts as
  agreeing, and its closed-shell edges are *unchecked* with the lesser reason code; an edge
  without a curve is unchecked for EdgeOffSurface (and for PcurveMismatch on a coedge with an
  imported pcurve). The other fail-closed paths of valid.rs (TRIM refusing a face for another
  reason, an evaluation or inversion fault) do not occur in these fixtures; the Rust tests
  cover them with hand-labelled cases.

Every reported measured value is checked to be exactly representable in binary64, so the
canonical JSON (every f64 as its bit pattern, 16 lowercase hex digits) is the exact expected
output on every platform (AC-5); every threshold decision is checked to be at least a relative
1e-6 away from the tolerance. The expected defects of each AC-2 fixture are also hand-labelled
below (`SEEDED`) and must equal what the checker finds.

Canonical JSON (pinned, no whitespace, keys in ascending order):
  {"defects":[...],"tolerance_m":"<hex>","truncated":<n>,"unchecked":[...]}
  unchecked item          {"check":kind,"entity":id,"reason":code}
  OpenLoop                {"gap_m":h,"kind":"OpenLoop","loop":id}
  SelfIntersectingLoop    {"kind":"SelfIntersectingLoop","loop":id}
  DanglingEdge            {"edge":id,"kind":"DanglingEdge","uses":n}
  NonManifoldEdge         {"edge":id,"kind":"NonManifoldEdge","uses":n}
  InconsistentOrientation {"edge":id,"kind":"InconsistentOrientation"}
  EdgeOffSurface          {"deviation_m":h,"edge":id,"face":id,"kind":"EdgeOffSurface"}
  PcurveMismatch          {"coedge":id,"deviation_m":h,"kind":"PcurveMismatch"}
  PcurveFailed            {"best_deviation_m":h,"coedge":id,"kind":"PcurveFailed","reason":r}
  SeamWinding             {"kind":"SeamWinding","loop":id,"periods":[pu,pv]}
IDs are the imported labels (every fixture entity is labelled) as JSON strings.
"""

import copy
import importlib.util
import json
import math
import struct
import sys
from decimal import Decimal
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
LICENSE = "CC-BY-4.0"


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


OR = _load("nurbs_oracle", "tools/fixtures/nurbs_oracle.py")
TOPO = _load("make_topo_fixtures", "fixtures/geo/cad/topo/make_topo_fixtures.py")
D = Decimal
PI = OR.PI

TOL = 1e-7  # the f64 tolerance of the tests
DTOL = D(TOL)
TAU = 6.283185307179586  # binary64 2 pi, as the fixtures write it
EDGE_SAMPLES = 33
INITIAL = 17
KINDS = [
    "OpenLoop",
    "SelfIntersectingLoop",
    "DanglingEdge",
    "NonManifoldEdge",
    "InconsistentOrientation",
    "EdgeOffSurface",
    "PcurveMismatch",
    "PcurveFailed",
    "SeamWinding",
]
INF = float("inf")


def hx(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


# ================================================================ fixtures (builder scripts)

Z = [0.0, 0.0, 1.0]
X = [1.0, 0.0, 0.0]
Y = [0.0, 1.0, 0.0]
O = [0.0, 0.0, 0.0]
GAP = 2.0 ** -20  # OpenLoop: the bottom edge stops this short of its end vertex
LIFT = 2.0 ** -12  # EdgeOffSurface: the disc circle above its plane
SHIFT = 2.0 ** -20  # PcurveMismatch: the imported pcurve below its edge in v


def frame(o, z, x):
    return {"origin": list(o), "axis": list(z), "ref_dir": list(x)}


def line(o, direction, normal):
    return dict(type="line", **frame(o, normal, direction))


def circle(o, z, x, r):
    return dict(type="circle", radius=r, **frame(o, z, x))


def plane(o=O, z=Z, x=X):
    return dict(type="plane", **frame(o, z, x))


def script(desc, bid, vertices, edges, coedges, loops, faces, shells):
    return {
        "description": desc,
        "license": LICENSE,
        "body_id": bid,
        "vertices": [{"point": list(p), "label": lab} for (p, lab) in vertices],
        "edges": [dict(start=s, end=e, range=list(r), curve=c, label=lab) for (s, e, r, c, lab) in edges],
        "coedges": [dict(edge=e, sense=s, pcurve=p, label=lab) for (e, s, p, lab) in coedges],
        "loops": [dict(coedges=cs, outer=o, label=lab) for (cs, o, lab) in loops],
        "faces": [dict(surface=s, sense=sn, loops=ls, label=lab) for (s, sn, ls, lab) in faces],
        "shells": [{"faces": fs, "closed": c} for (fs, c) in shells],
    }


class Sheet:
    """Accumulates labelled entities of a sheet (or mixed) body."""

    def __init__(self):
        self.v, self.e, self.c, self.l, self.f = [], [], [], [], []

    def vertex(self, p, lab):
        self.v.append((p, lab))
        return len(self.v) - 1

    def edge(self, s, e, rng, curve, lab):
        self.e.append((s, e, rng, curve, lab))
        return len(self.e) - 1

    def coedge(self, e, sense, pc, lab):
        self.c.append((e, sense, pc, lab))
        return len(self.c) - 1

    def loop(self, cs, lab, outer=True):
        self.l.append((cs, outer, lab))
        return len(self.l) - 1

    def face(self, surf, ls, lab, sense="same"):
        self.f.append((surf, sense, ls, lab))
        return len(self.f) - 1

    def square(self, p, z=0.0, bottom_range=(0.0, 1.0), bottom_curve=True, bottom_pcurve=None):
        """Unit square [0,1]^2 at height z, counter-clockwise about +Z; returns the face index."""
        pts = [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]]
        vs = [self.vertex(q, f"{p}v{i}") for i, q in enumerate(pts)]
        dirs = [X, Y, [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]
        cs = []
        for i, name in enumerate(("bottom", "right", "top", "left")):
            rng, curve, pc = (0.0, 1.0), line(pts[i], dirs[i], Z), None
            if i == 0:
                rng = bottom_range
                curve = curve if bottom_curve else None
                pc = bottom_pcurve
            e = self.edge(vs[i], vs[(i + 1) % 4], rng, curve, f"{p}e-{name}")
            cs.append(self.coedge(e, "same", pc, f"{p}c-{name}"))
        lp = self.loop(cs, f"{p}l")
        return self.face(plane([0.0, 0.0, z]), [lp], f"{p}f")

    def disc(self, p, z_plane, z_circle, curve=True):
        """Disc of radius 0.5 about (0.5, 0.5) on the plane at z_plane, bounded by a circle at
        z_circle (or by an edge without a curve)."""
        v = self.vertex([1.0, 0.5, z_circle], f"{p}v")
        c = circle([0.5, 0.5, z_circle], Z, X, 0.5) if curve else None
        e = self.edge(v, v, (0.0, TAU), c, f"{p}e")
        lp = self.loop([self.coedge(e, "same", None, f"{p}c")], f"{p}l")
        return self.face(plane([0.0, 0.0, z_plane]), [lp], f"{p}f")

    def body(self, desc, bid, shells=None):
        if shells is None:
            shells = [(list(range(len(self.f))), False)]
        return script(desc, bid, self.v, self.e, self.c, self.l, self.f, shells)


def swap_lines(s):
    """The GEO.CAD.TOPO.MODEL scripts (make_topo_fixtures.py) give a line's direction in `axis`
    with a perpendicular `ref_dir`; GEO.NURBS.ANALYTIC's line runs along `ref_dir` (O + t X).
    Swap the two, as the GEO.DIST.FACE tests do when they read cube_labelled.json."""
    s = copy.deepcopy(s)
    for e in s["edges"]:
        c = e["curve"]
        if c is not None and c["type"] == "line":
            c["axis"], c["ref_dir"] = c["ref_dir"], c["axis"]
    return s


def labelled_cylinder():
    s = TOPO.cylinder()
    for kind, short in (("vertices", "v"), ("edges", "e"), ("coedges", "c"), ("loops", "l"), ("faces", "f")):
        for i, ent in enumerate(s[kind]):
            ent["label"] = f"cyl-{short}{i}"
    return s


def defect_fixtures():
    out = []

    s = Sheet()
    s.square("", bottom_range=(0.0, 1.0 - GAP))
    out.append(("open_loop", s.body("unit square sheet whose bottom edge curve stops 2^-20 m short of its end "
                                    "vertex", "0be10be10be10be10be10be10be10be1")))

    s = Sheet()
    a, b, c, d = (s.vertex(p, f"v{i}") for i, p in enumerate([[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0],
                                                             [0.0, 1.0, 0.0]]))
    r2 = math.sqrt(2.0)
    diag = [1.0 / r2, 1.0 / r2, 0.0]
    edges = [(a, b, (0.0, r2), line([0.0, 0.0, 0.0], diag, Z), "e-ab"),
             (b, c, (0.0, 1.0), line([1.0, 1.0, 0.0], [0.0, -1.0, 0.0], Z), "e-bc"),
             (c, d, (0.0, r2), line([1.0, 0.0, 0.0], [-1.0 / r2, 1.0 / r2, 0.0], Z), "e-cd"),
             (d, a, (0.0, 1.0), line([0.0, 1.0, 0.0], [0.0, -1.0, 0.0], Z), "e-da")]
    cs = [s.coedge(s.edge(*e), "same", None, "c" + e[4][1:]) for e in edges]
    s.face(plane(), [s.loop(cs, "l-bowtie")], "f-bowtie")
    out.append(("self_intersecting_loop", s.body("bow-tie sheet: the loop's edges a-b and c-d cross at (0.5, 0.5)",
                                                 "51515151515151515151515151515151")))

    cube = swap_lines(TOPO.cube(True))
    s = Sheet()
    s.v, s.e = [(v["point"], v["label"]) for v in cube["vertices"]], [
        (e["start"], e["end"], e["range"], e["curve"], e["label"]) for e in cube["edges"]]
    s.c = [(c["edge"], c["sense"], c["pcurve"], c["label"]) for c in cube["coedges"]]
    s.l = [(lp["coedges"], lp["outer"], lp["label"]) for lp in cube["loops"]]
    s.f = [(f["surface"], f["sense"], f["loops"], f["label"]) for f in cube["faces"]]
    s.disc("disc-", 3.0, 3.0)
    out.append(("dangling_edge", s.body("labelled unit cube (closed shell) plus a disc sheet (open shell) whose "
                                        "circle edge is used once in the solid body",
                                        "da1da1da1da1da1da1da1da1da1da1da",
                                        shells=[(list(range(6)), True), ([6], False)])))

    cyl = labelled_cylinder()
    cyl["description"] = "labelled capped cylinder whose top cap is marked Reversed while its loop still runs " \
                         "counter-clockwise about the cap's outward axis"
    cyl["license"] = LICENSE
    cyl["body_id"] = "1c0a1c0a1c0a1c0a1c0a1c0a1c0a1c0a"
    cyl["faces"][0]["sense"] = "reversed"
    out.append(("inconsistent_orientation", cyl))

    s = Sheet()
    s.disc("", 0.0, LIFT)
    out.append(("edge_off_surface", s.body("disc sheet on the plane z = 0 whose circle edge lies 2^-12 m above it",
                                           "0ff50ff50ff50ff50ff50ff50ff50ff5")))

    s = Sheet()
    s.square("", bottom_pcurve={"type": "line2", "origin": [0.0, -SHIFT], "dir": [1.0, 0.0]})
    out.append(("pcurve_mismatch", s.body("unit square sheet whose bottom coedge carries an imported line pcurve "
                                          "2^-20 m below its edge in v", "9c9c9c9c9c9c9c9c9c9c9c9c9c9c9c9c")))

    s = Sheet()
    s.square("", bottom_curve=False)
    out.append(("pcurve_failed", s.body("unit square sheet whose bottom edge has neither a 3-D curve nor a pcurve",
                                        "fa11fa11fa11fa11fa11fa11fa11fa11")))

    big, small, phi = 0.05, 0.01, 0.5235987755982988
    c, sn = math.cos(phi), math.sin(phi)
    s = Sheet()
    v = s.vertex([(big + small) * c, (big + small) * sn, 0.0], "v-tube")
    e = s.edge(v, v, (0.0, TAU), circle([big * c, big * sn, 0.0], [sn, -c, 0.0], [c, sn, 0.0], small), "e-tube")
    lp = s.loop([s.coedge(e, "same", None, "c-tube")], "l-tube")
    s.face(dict(type="torus", major_radius=big, minor_radius=small, **frame(O, Z, X)), [lp], "f-torus")
    out.append(("seam_winding", s.body("torus face (R 0.05 m, r 0.01 m) bounded by one tube circle that winds once "
                                       "in v", "70a170a170a170a170a170a170a170a1")))
    return out


def capped_band(top_sense="same", band_sense="same"):
    """Closed cylinder r 0.5, height 1: a seamless band (two circle loops, each winding once in
    u, so its orientation is undetermined) and two planar caps whose loops agree with their
    outward axes. With top_sense "reversed" the top cap disagrees with its sense."""
    s = Sheet()
    v0, v1 = s.vertex([0.5, 0.0, 0.0], "v-bottom"), s.vertex([0.5, 0.0, 1.0], "v-top")
    eb = s.edge(v0, v0, (0.0, TAU), circle([0.0, 0.0, 0.0], Z, X, 0.5), "e-bottom")
    et = s.edge(v1, v1, (0.0, TAU), circle([0.0, 0.0, 1.0], Z, X, 0.5), "e-top")
    lb = s.loop([s.coedge(eb, "same", None, "c-band-bottom")], "l-band-bottom", outer=False)
    lt = s.loop([s.coedge(et, "reversed", None, "c-band-top")], "l-band-top", outer=False)
    s.face(dict(type="cylinder", radius=0.5, **frame(O, Z, X)), [lb, lt], "f-band", sense=band_sense)
    s.face(plane([0.0, 0.0, 1.0]), [s.loop([s.coedge(et, "same", None, "c-cap-top")], "l-cap-top")], "f-cap-top",
           sense=top_sense)
    s.face(plane(O, [0.0, 0.0, -1.0], X), [s.loop([s.coedge(eb, "reversed", None, "c-cap-bottom")], "l-cap-bottom")],
           "f-cap-bottom")
    desc = "closed cylinder of a seamless band and two caps" + (
        "; the top cap is marked Reversed against its loop" if top_sense == "reversed" else "") + (
        "; the band is marked Reversed" if band_sense == "reversed" else "")
    return s.body(desc, "cb0dcb0dcb0dcb0dcb0dcb0dcb0dcb0d", shells=[([0, 1, 2], True)])


def non_manifold():
    """Three square pages sharing one spine edge: the builder refuses it (BadPairing TooMany on
    edge 0), so a NonManifoldEdge can never reach validation."""
    s = Sheet()
    s0, s1 = s.vertex([0.0, 0.0, 0.0], "v-s0"), s.vertex([0.0, 0.0, 1.0], "v-s1")
    spine = s.edge(s0, s1, (0.0, 1.0), line([0.0, 0.0, 0.0], Z, X), "e-spine")
    for k, (dx, dy) in enumerate([(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)]):
        p0 = s.vertex([dx, dy, 0.0], f"v-p{k}0")
        p1 = s.vertex([dx, dy, 1.0], f"v-p{k}1")
        top = s.edge(s1, p1, (0.0, 1.0), line([0.0, 0.0, 1.0], [dx, dy, 0.0], Z), f"e-top{k}")
        out = s.edge(p1, p0, (0.0, 1.0), line([dx, dy, 1.0], [0.0, 0.0, -1.0], [dx, dy, 0.0]), f"e-out{k}")
        bot = s.edge(p0, s0, (0.0, 1.0), line([dx, dy, 0.0], [-dx, -dy, 0.0], Z), f"e-bot{k}")
        cs = [s.coedge(spine, "same" if k != 1 else "reversed", None, f"c-spine{k}")]
        if k == 1:
            cs = [s.coedge(bot, "reversed", None, "c-bot1"), s.coedge(out, "reversed", None, "c-out1"),
                  s.coedge(top, "reversed", None, "c-top1"), cs[0]]
        else:
            cs += [s.coedge(top, "same", None, f"c-top{k}"), s.coedge(out, "same", None, f"c-out{k}"),
                   s.coedge(bot, "same", None, f"c-bot{k}")]
        normal = [-dy, dx, 0.0]
        s.face(plane(O, normal, Z), [s.loop(cs, f"l-page{k}")], f"f-page{k}")
    body = s.body("three square pages sharing the spine edge: refused by BodyBuilder::build", "3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a")
    body["expect"] = {"code": "BadPairing", "edge": 0, "fault": "TooMany"}
    return body


def cap50():
    s = Sheet()
    for i in range(10):
        s.square(f"p{i:02d}-", z=float(i), bottom_range=(0.0, 1.0 - GAP))
    for i in range(10, 50):
        s.disc(f"d{i:02d}-", float(i), float(i), curve=False)
    return s.body("sheet of 10 squares with a short bottom edge (OpenLoop) and 40 discs whose edge has no curve "
                  "(PcurveFailed): 50 defects", "50505050505050505050505050505050")


# hand-labelled expected defects of the AC-2 fixtures (kind, ids..., value)
SEEDED = {
    "open_loop": [("OpenLoop", "l", Fraction(GAP))],
    "self_intersecting_loop": [("SelfIntersectingLoop", "l-bowtie")],
    "dangling_edge": [("DanglingEdge", "disc-e", 1)],
    "inconsistent_orientation": [("InconsistentOrientation", "cyl-e0")],
    "edge_off_surface": [("EdgeOffSurface", "e", "f", Fraction(LIFT)), ("PcurveFailed", "c", "Tolerance", Fraction(LIFT))],
    "pcurve_mismatch": [("PcurveMismatch", "c-bottom", Fraction(SHIFT))],
    "pcurve_failed": [("PcurveFailed", "c-bottom", "NoEdgeCurve", INF)],
    "seam_winding": [("SeamWinding", "l-tube", [0, 1])],
}


# hand-labelled unchecked items of the AC-2 fixtures (check, entity, reason)
SEEDED_UNCHECKED = {
    "pcurve_failed": [("EdgeOffSurface", "e-bottom", "NoEdgeCurve")],
}


# ================================================================ geometry (60 digits)


def dv(v):
    return [D(x) for x in v]


def norm(v):
    return OR.dot(v, v).sqrt()


def unit(v):
    n = norm(v)
    return [x / n for x in v]


def placement(g):
    o, z = dv(g["origin"]), unit(dv(g["axis"]))
    r = dv(g["ref_dir"])
    p = OR.dot(r, z)
    x = unit([r[i] - p * z[i] for i in range(3)])
    return o, x, OR.cross(z, x), z


def curve_point(c, t):
    t = D(t)
    if c["type"] == "line":
        o, x, _, _ = placement(c)
        return [o[i] + t * x[i] for i in range(3)]
    if c["type"] == "circle":
        o, x, y, _ = placement(c)
        s, co = OR.sin_cos(t)
        r = D(c["radius"])
        return [o[i] + r * (co * x[i] + s * y[i]) for i in range(3)]
    if c["type"] == "nurbs":
        assert c["degree"] == 1 and len(c["ctrl"]) == 2 and c["weights"] is None
        k0, k1 = D(c["knots"][1]), D(c["knots"][2])
        w = (t - k0) / (k1 - k0)
        a, b = dv(c["ctrl"][0]), dv(c["ctrl"][1])
        return [a[i] + w * (b[i] - a[i]) for i in range(3)]
    raise ValueError(c["type"])


def local(g, p):
    o, x, y, z = placement(g)
    d = [p[i] - o[i] for i in range(3)]
    return OR.dot(d, x), OR.dot(d, y), OR.dot(d, z)


def surface_distance(s, p):
    lx, ly, lz = local(s, p)
    if s["type"] == "plane":
        return abs(lz)
    rho = (lx * lx + ly * ly).sqrt()
    if s["type"] == "cylinder":
        return abs(rho - D(s["radius"]))
    if s["type"] == "torus":
        a = rho - D(s["major_radius"])
        return abs((a * a + lz * lz).sqrt() - D(s["minor_radius"]))
    raise ValueError(s["type"])


def surface_point(s, u, v):
    o, x, y, z = placement(s)
    u, v = D(u), D(v)
    if s["type"] == "plane":
        return [o[i] + u * x[i] + v * y[i] for i in range(3)]
    if s["type"] == "cylinder":
        sn, co = OR.sin_cos(u)
        r = D(s["radius"])
        return [o[i] + r * (co * x[i] + sn * y[i]) + v * z[i] for i in range(3)]
    raise ValueError(s["type"])


def dist(a, b):
    return sum(((a[i] - b[i]) ** 2 for i in range(3)), D(0)).sqrt()


def grid(t0, t1, k, n):
    """The pinned sample parameters, in binary64 like the spec states them."""
    return t1 if k == n else t0 + ((t1 - t0) * k) / n


def above(x, what):
    """x > TOL, refusing a decision within a relative 1e-6 of the tolerance."""
    if x == INF:
        return True
    assert abs(x - DTOL) > DTOL * D("1e-6"), what
    return x > DTOL


# ================================================================ checker


class Body:
    def __init__(self, s):
        self.s = s
        self.V, self.E, self.C, self.L, self.F, self.S = (s[k] for k in
                                                          ("vertices", "edges", "coedges", "loops", "faces", "shells"))
        self.loop_face = {}
        for fi, f in enumerate(self.F):
            for li in f["loops"]:
                self.loop_face[li] = fi
        self.coedge_loop = {}
        for li, lp in enumerate(self.L):
            for ci in lp["coedges"]:
                self.coedge_loop[ci] = li
        self.face_shell = {}
        for si, sh in enumerate(self.S):
            for fi in sh["faces"]:
                self.face_shell[fi] = si

    def label(self, kind, i):
        lab = getattr(self, kind)[i].get("label")
        assert lab, "every fixture entity is labelled"
        return lab

    def face_of(self, ci):
        return self.loop_face[self.coedge_loop[ci]]

    def uses(self, ei):
        return [ci for ci, c in enumerate(self.C) if c["edge"] == ei]

    def coedge_end(self, ci, end):
        c = self.C[ci]
        e = self.E[c["edge"]]
        at_t1 = end == (c["sense"] == "same")
        if e["curve"] is None:
            return dv(self.V[e["end"] if at_t1 else e["start"]]["point"])
        return curve_point(e["curve"], e["range"][1] if at_t1 else e["range"][0])


def fr(x):
    return Fraction(x) if not isinstance(x, D) else Fraction(x)


def chart_uv(body, fi, p):
    """Exact chart coordinates of a 3-D point on a plane face (orthogonal projection)."""
    surf = body.F[fi]["surface"]
    assert surf["type"] == "plane"
    lx, ly, _ = local(surf, p)
    return (fr(lx), fr(ly))


def pcurve_uv(pc, t):
    assert pc["type"] == "line2"
    o, d = pc["origin"], pc["dir"]
    return (Fraction(o[0]) + Fraction(t) * Fraction(d[0]), Fraction(o[1]) + Fraction(t) * Fraction(d[1]))


def loop_shape(body, li):
    """('polygon', [uv...]) closed polygon in the face chart, ('circle', centre, r, sign) or ('winding',)."""
    fi = body.loop_face[li]
    surf = body.F[fi]["surface"]
    cs = body.L[li]["coedges"]
    if surf["type"] == "torus":
        return ("winding",)
    if surf["type"] == "cylinder" and len(cs) == 1 and body.C[cs[0]]["pcurve"] is None:
        e = body.E[body.C[cs[0]]["edge"]]
        assert e["curve"]["type"] == "circle" and abs(abs(OR.dot(placement(e["curve"])[3], placement(surf)[3])) - 1)             < D("1e-30"), "a coaxial circle winds once around the cylinder"
        return ("winding",)
    if surf["type"] == "plane" and len(cs) == 1:
        e = body.E[body.C[cs[0]]["edge"]]
        if e["curve"] is not None and e["curve"]["type"] == "circle" and body.C[cs[0]]["pcurve"] is None:
            o, x, y, zc = placement(e["curve"])
            _, _, _, zp = placement(surf)
            centre = chart_uv(body, fi, o)
            dot = OR.dot(zc, zp)
            assert abs(abs(dot) - 1) < D("1e-30"), "circle parallel to its plane"
            sign = (1 if dot > 0 else -1) * (1 if body.C[cs[0]]["sense"] == "same" else -1)
            return ("circle", centre, Fraction(e["curve"]["radius"]), sign)
    pts = []
    for ci in cs:
        c = body.C[ci]
        e = body.E[c["edge"]]
        t0, t1 = e["range"]
        if c["pcurve"] is not None:
            a, b = pcurve_uv(c["pcurve"], t0), pcurve_uv(c["pcurve"], t1)
        else:
            assert surf["type"] == "plane" and e["curve"] is not None and e["curve"]["type"] == "line"
            a = chart_uv(body, fi, curve_point(e["curve"], t0))
            b = chart_uv(body, fi, curve_point(e["curve"], t1))
        if c["sense"] == "reversed":
            a, b = b, a
        for q in (a, b):
            if not pts or pts[-1] != q:
                pts.append(q)
    if len(pts) > 1 and pts[0] == pts[-1]:
        pts.pop()
    return ("polygon", pts)


def orient(a, b, q):
    d = (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0])
    return (d > 0) - (d < 0)


def on_box(a, b, p):
    return min(a[0], b[0]) <= p[0] <= max(a[0], b[0]) and min(a[1], b[1]) <= p[1] <= max(a[1], b[1])


def meet(a, b, c, d):
    o1, o2, o3, o4 = orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b)
    if o1 * o2 < 0 and o3 * o4 < 0:
        return True
    return ((o1 == 0 and on_box(a, b, c)) or (o2 == 0 and on_box(a, b, d)) or (o3 == 0 and on_box(c, d, a))
            or (o4 == 0 and on_box(c, d, b)))


def fold(a, b, d):
    """a -> b -> d meets itself beyond b: collinear and d on a's side of b."""
    if orient(a, b, d) != 0:
        return False
    i = 0 if a[0] != b[0] else 1
    return (d[i] - b[i]) * (a[i] - b[i]) > 0


def seg_circle_meet(a, b, centre, r):
    """Closed segment meets the circle (min distance <= r <= max distance), decided with a margin."""
    def d2(p):
        return (p[0] - centre[0]) ** 2 + (p[1] - centre[1]) ** 2
    vx, vy = b[0] - a[0], b[1] - a[1]
    t = ((centre[0] - a[0]) * vx + (centre[1] - a[1]) * vy) / (vx * vx + vy * vy)
    t = min(max(t, Fraction(0)), Fraction(1))
    near = d2((a[0] + t * vx, a[1] + t * vy))
    far = max(d2(a), d2(b))
    r2 = r * r
    assert abs(near - r2) > r2 / 10 ** 6 and abs(far - r2) > r2 / 10 ** 6
    return near <= r2 <= far


def face_intersections(body, fi):
    shapes = [(li, loop_shape(body, li)) for li in body.F[fi]["loops"]]
    segs, circles = [], []
    for li, sh in shapes:
        if sh[0] == "polygon":
            pts = sh[1]
            n = len(pts)
            for k in range(n):
                segs.append((li, k, n, pts[k], pts[(k + 1) % n]))
        elif sh[0] == "circle":
            circles.append((li, sh[1], sh[2]))
    hit = set()
    for i in range(len(segs)):
        for j in range(i + 1, len(segs)):
            li, ki, ni, a, b = segs[i]
            lj, kj, _, c, d = segs[j]
            if li == lj and (kj == (ki + 1) % ni):
                m = fold(a, b, d)
            elif li == lj and (ki == (kj + 1) % ni):
                m = fold(c, d, b)
            else:
                m = meet(a, b, c, d)
            if m:
                hit.update((li, lj))
    for li, a, b in [(s[0], s[3], s[4]) for s in segs]:
        for lc, centre, r in circles:
            if seg_circle_meet(a, b, centre, r):
                hit.update((li, lc))
    for i in range(len(circles)):
        for j in range(i + 1, len(circles)):
            (la, ca, ra), (lb, cb, rb) = circles[i], circles[j]
            dd = (ca[0] - cb[0]) ** 2 + (ca[1] - cb[1]) ** 2
            if (ra - rb) ** 2 <= dd <= (ra + rb) ** 2:
                hit.update((la, lb))
    return hit


def loop_area(sh):
    """Signed area of a loop shape in the face chart (exact for polygons; circles by pi r^2)."""
    if sh[0] == "circle":
        return sh[3] * OR.dec_of(sh[2] * sh[2]) * PI
    pts = sh[1]
    a = sum(pts[k][0] * pts[(k + 1) % len(pts)][1] - pts[(k + 1) % len(pts)][0] * pts[k][1] for k in range(len(pts)))
    return OR.dec_of(a / 2)


def face_flipped(body, fi, failed):
    """True/False when the loops disagree/agree with the face sense, or the reason code why it
    cannot be told (the rules of the module documentation of valid.rs)."""
    f = body.F[fi]
    loops = f["loops"]
    if not loops:
        return "NoLoops"
    if body.F[fi]["surface"]["type"] == "torus" or any(ci in failed for li in loops for ci in body.L[li]["coedges"]):
        return "NoBoundary"
    shapes = [loop_shape(body, li) for li in loops]
    if any(sh[0] == "winding" for sh in shapes):
        return "LoopWinds"
    areas = [loop_area(sh) for sh in shapes]
    if any(a == 0 for a in areas):
        return "AreaUndecided"
    marked = [i for i, li in enumerate(loops) if body.L[li]["outer"]]
    if len(marked) == 1:
        outer = marked[0]
    elif len(loops) == 1:
        outer = 0
    else:
        big = max(abs(a) for a in areas)
        top = [i for i, a in enumerate(areas) if abs(a) == big]
        if len(top) != 1:
            return "OuterUnknown"
        outer = top[0]
    same = f["sense"] == "same"
    ccw = [(a > 0) == same for a in areas]
    if all(ccw[i] != ccw[outer] for i in range(len(loops)) if i != outer):
        return not ccw[outer]
    return "LoopsDisagree"


def winding(body, li):
    """Periods (u, v) a loop of circle coedges winds on a torus or a cylinder, by unwrapping 64
    samples per coedge."""
    surf = body.F[body.loop_face[li]]["surface"]
    total = [D(0), D(0)]
    for ci in body.L[li]["coedges"]:
        c = body.C[ci]
        e = body.E[c["edge"]]
        t0, t1 = D(e["range"][0]), D(e["range"][1])
        prev = None
        n = 64
        ks = range(n + 1) if c["sense"] == "same" else range(n, -1, -1)
        for k in ks:
            lx, ly, lz = local(surf, curve_point(e["curve"], t0 + (t1 - t0) * k / n))
            rho = (lx * lx + ly * ly).sqrt()
            if surf["type"] == "torus":
                uv = (OR.atan2(ly, lx), OR.atan2(lz, rho - D(surf["major_radius"])))
            else:
                uv = (OR.atan2(ly, lx), D(0))
            if prev is not None:
                for i in range(2):
                    j = uv[i] - prev[i]
                    j -= 2 * PI * (j / (2 * PI)).to_integral_value()
                    assert abs(j) < PI / 4
                    total[i] += j
            prev = uv
    return [int((t / (2 * PI)).to_integral_value()) for t in total]


def check(s):
    """(defects, unchecked) of a script, each in REQ-007 order."""
    body = Body(s)
    out, skipped = [], set()
    # OpenLoop
    for li, lp in enumerate(body.L):
        cs = lp["coedges"]
        gap = max(dist(body.coedge_end(cs[k], True), body.coedge_end(cs[(k + 1) % len(cs)], False))
                  for k in range(len(cs)))
        if above(gap, ("gap", li)):
            out.append(("OpenLoop", body.label("L", li), gap))
    # SelfIntersectingLoop (faces whose boundary is known: no failed pcurve)
    failed = {ci for ci, _, _ in pcurve_failures(body)}
    for fi, f in enumerate(body.F):
        if any(ci in failed for li in f["loops"] for ci in body.L[li]["coedges"]):
            continue
        for li in sorted(face_intersections(body, fi)):
            out.append(("SelfIntersectingLoop", body.label("L", li)))
    # DanglingEdge, NonManifoldEdge
    if any(sh["closed"] for sh in body.S):
        for ei in range(len(body.E)):
            u = body.uses(ei)
            faces = {body.face_of(ci) for ci in u}
            if len(u) < 2:
                out.append(("DanglingEdge", body.label("E", ei), len(u)))
            elif len(faces) > 2:
                out.append(("NonManifoldEdge", body.label("E", ei), len(faces)))
    # InconsistentOrientation
    for ei in range(len(body.E)):
        u = body.uses(ei)
        if len(u) != 2:
            continue
        fa, fb = body.face_of(u[0]), body.face_of(u[1])
        if fa == fb or not body.S[body.face_shell[fa]]["closed"]:
            continue
        xa, xb = face_flipped(body, fa, failed), face_flipped(body, fb, failed)
        reasons = [x for x in (xa, xb) if isinstance(x, str)]
        if reasons:
            skipped.add(("InconsistentOrientation", body.label("E", ei), min(reasons)))
        elif xa != xb:
            out.append(("InconsistentOrientation", body.label("E", ei)))
    # EdgeOffSurface
    for ei, e in enumerate(body.E):
        if e["curve"] is None:
            skipped.add(("EdgeOffSurface", body.label("E", ei), "NoEdgeCurve"))
            continue
        t0, t1 = e["range"]
        for fi in sorted({body.face_of(ci) for ci in body.uses(ei)}):
            surf = body.F[fi]["surface"]
            dev = max(surface_distance(surf, curve_point(e["curve"], grid(t0, t1, k, EDGE_SAMPLES - 1)))
                      for k in range(EDGE_SAMPLES))
            if above(dev, ("off", ei, fi)):
                out.append(("EdgeOffSurface", body.label("E", ei), body.label("F", fi), dev))
    # PcurveMismatch
    for ci, c in enumerate(body.C):
        if c["pcurve"] is None:
            continue
        e = body.E[c["edge"]]
        if e["curve"] is None:
            skipped.add(("PcurveMismatch", body.label("C", ci), "NoEdgeCurve"))
            continue
        surf = body.F[body.face_of(ci)]["surface"]
        t0, t1 = e["range"]
        dev = D(0)
        for k in range(EDGE_SAMPLES):
            t = grid(t0, t1, k, EDGE_SAMPLES - 1)
            u, v = pcurve_uv(c["pcurve"], t)
            sp = surface_point(surf, OR.dec_of(u), OR.dec_of(v))
            dev = max(dev, dist(sp, curve_point(e["curve"], t)))
        if above(dev, ("mismatch", ci)):
            out.append(("PcurveMismatch", body.label("C", ci), dev))
    # PcurveFailed
    for ci, reason, best in pcurve_failures(body):
        out.append(("PcurveFailed", body.label("C", ci), reason, best))
    # SeamWinding: doubly periodic surfaces, or more than one period on a cylinder
    for li in range(len(body.L)):
        kind = body.F[body.loop_face[li]]["surface"]["type"]
        if kind == "torus" or (kind == "cylinder" and all(body.C[ci]["pcurve"] is None for ci in body.L[li]["coedges"])
                               and all(body.E[body.C[ci]["edge"]]["curve"]["type"] == "circle"
                                       for ci in body.L[li]["coedges"])):
            w = winding(body, li)
            if (kind == "torus" and w != [0, 0]) or any(abs(x) > 1 for x in w):
                out.append(("SeamWinding", body.label("L", li), w))
    out.sort(key=lambda d: (KINDS.index(d[0]), d[1].encode("utf-8"), d[2].encode("utf-8")
                            if d[0] == "EdgeOffSurface" else b""))
    unchecked = sorted(skipped, key=lambda u: (KINDS.index(u[0]), u[1].encode("utf-8"), u[2]))
    return out, unchecked


def pcurve_failures(body):
    out = []
    for ci, c in enumerate(body.C):
        if c["pcurve"] is not None:
            continue
        e = body.E[c["edge"]]
        if e["curve"] is None:
            out.append((ci, "NoEdgeCurve", INF))
            continue
        surf = body.F[body.face_of(ci)]["surface"]
        t0, t1 = e["range"]
        ds = [surface_distance(surf, curve_point(e["curve"], grid(t0, t1, k, INITIAL - 1))) for k in range(INITIAL)]
        if any(above(d, ("sample", ci)) for d in ds):
            out.append((ci, "Tolerance", max(ds)))
    return out


# ================================================================ output


def exact_f64(x):
    """The binary64 value of an exactly representable measured value."""
    if x == INF:
        return INF
    f = float(x)
    assert Fraction(f) == Fraction(x), f"measured value {x} is not exactly representable"
    return f


def defect_json(d):
    k = d[0]
    q = json.dumps
    if k == "OpenLoop":
        return '{"gap_m":%s,"kind":"OpenLoop","loop":%s}' % (q(hx(exact_f64(d[2]))), q(d[1]))
    if k == "SelfIntersectingLoop":
        return '{"kind":"SelfIntersectingLoop","loop":%s}' % q(d[1])
    if k in ("DanglingEdge", "NonManifoldEdge"):
        return '{"edge":%s,"kind":"%s","uses":%d}' % (q(d[1]), k, d[2])
    if k == "InconsistentOrientation":
        return '{"edge":%s,"kind":"InconsistentOrientation"}' % q(d[1])
    if k == "EdgeOffSurface":
        return '{"deviation_m":%s,"edge":%s,"face":%s,"kind":"EdgeOffSurface"}' % (q(hx(exact_f64(d[3]))), q(d[1]),
                                                                                   q(d[2]))
    if k == "PcurveMismatch":
        return '{"coedge":%s,"deviation_m":%s,"kind":"PcurveMismatch"}' % (q(d[1]), q(hx(exact_f64(d[2]))))
    if k == "PcurveFailed":
        return '{"best_deviation_m":%s,"coedge":%s,"kind":"PcurveFailed","reason":%s}' % (
            q(hx(exact_f64(d[3]))), q(d[1]), q(d[2]))
    if k == "SeamWinding":
        return '{"kind":"SeamWinding","loop":%s,"periods":[%d,%d]}' % (q(d[1]), d[2][0], d[2][1])
    raise ValueError(k)


def canonical(defects, truncated, unchecked):
    items = ",".join('{"check":"%s","entity":%s,"reason":"%s"}' % (u[0], json.dumps(u[1]), u[2]) for u in unchecked)
    return '{"defects":[%s],"tolerance_m":"%s","truncated":%d,"unchecked":[%s]}' % (
        ",".join(defect_json(d) for d in defects), hx(TOL), truncated, items)


def report(defects, unchecked, cap=None):
    kept = defects if cap is None else defects[:cap]
    return {"cap": cap, "defects": listed(kept), "truncated": len(defects) - len(kept),
            "unchecked": [{"check": u[0], "entity": u[1], "reason": u[2]} for u in unchecked],
            "canonical": canonical(kept, len(defects) - len(kept), unchecked)}


def value_text(x):
    if x == INF:
        return "inf"
    return format(D(x) if not isinstance(x, D) else x, "e")


def listed(defects):
    out = []
    for d in defects:
        row = {"kind": d[0], "entity": d[1]}
        rest = list(d[2:])
        if d[0] == "EdgeOffSurface":
            row["face"] = rest.pop(0)
        if d[0] == "PcurveFailed":
            row["reason"] = rest.pop(0)
        if rest:
            v = rest[0]
            if isinstance(v, list):
                row["periods"] = v
            elif isinstance(v, int):
                row["uses"] = v
            else:
                row["value"] = value_text(v)
        out.append(row)
    return out


def dump(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def no_dup_keys(pairs):
    d = {}
    for k, v in pairs:
        if k in d:
            raise ValueError(f"duplicate key {k!r}")
        d[k] = v
    return d


def same_seeded(found, seeded):
    """The checker's defects equal the hand-labelled ones (values exactly)."""
    if len(found) != len(seeded):
        return False
    for f, s in zip(found, seeded):
        if f[0] in ("SelfIntersectingLoop", "InconsistentOrientation"):
            if f != s:
                return False
            continue
        if f[:-1] != s[:-1]:
            return False
        fv, sv = f[-1], s[-1]
        if isinstance(sv, Fraction):
            if fv == INF or Fraction(fv) != sv:
                return False
        elif fv != sv:
            return False
    return True


def build_all():
    files, reports = {}, {}
    for name, s in defect_fixtures():
        files[f"defects/{name}.json"] = dump(s)
        parsed = json.loads(files[f"defects/{name}.json"], object_pairs_hook=no_dup_keys)
        found, unchecked = check(parsed)
        assert same_seeded(found, SEEDED[name]), (name, found)
        assert unchecked == SEEDED_UNCHECKED.get(name, []), (name, unchecked)
        reports[f"defects/{name}"] = report(found, unchecked)
    nm = non_manifold()
    files["defects/non_manifold_edge.json"] = dump(nm)
    # AC-1: the labelled cube is clean (its lines read with swap_lines, as the Rust test does). cylinder_capped.json is unlabelled (derived IDs), so the
    # checker runs on the same script with labels added; an empty report has no IDs to differ.
    cube = swap_lines(json.loads((HERE / "cube_labelled.json").read_text(encoding="utf-8"),
                                 object_pairs_hook=no_dup_keys))
    plain = json.loads((HERE / "cylinder_capped.json").read_text(encoding="utf-8"), object_pairs_hook=no_dup_keys)
    labelled = labelled_cylinder()
    for kind in ("vertices", "edges", "coedges", "loops", "faces"):
        for a, b in zip(plain[kind], labelled[kind]):
            assert {k: v for k, v in b.items() if k != "label"} == a, ("cylinder_capped drift", kind)
    for name, parsed in (("cube_labelled", cube), ("cylinder_capped", labelled)):
        found, unchecked = check(parsed)
        assert found == [] and unchecked == [], (name, found, unchecked)
        reports[name] = report(found, unchecked)
    # The band winds, so its orientation is undetermined: both circle edges are unchecked,
    # however the caps or the band are marked (hand-labelled, and found by the checker).
    winds = [("InconsistentOrientation", "e-bottom", "LoopWinds"), ("InconsistentOrientation", "e-top", "LoopWinds")]
    for name, top, band in (("capped_band", "same", "same"), ("capped_band_top_reversed", "reversed", "same"),
                            ("capped_band_flipped", "same", "reversed")):
        files[f"{name}.json"] = dump(capped_band(top, band))
        found, unchecked = check(json.loads(files[f"{name}.json"], object_pairs_hook=no_dup_keys))
        assert found == [] and unchecked == winds, (name, found, unchecked)
        reports[name] = report(found, unchecked)
    c50 = cap50()
    files["valid_cap50.json"] = dump(c50)
    found, unchecked = check(json.loads(files["valid_cap50.json"], object_pairs_hook=no_dup_keys))
    assert len(found) == 50 and [d[0] for d in found] == ["OpenLoop"] * 10 + ["PcurveFailed"] * 40
    # the 40 curveless disc edges cannot be measured against their planes (never capped)
    assert unchecked == [("EdgeOffSurface", f"d{i:02d}-e", "NoEdgeCurve") for i in range(10, 50)], unchecked
    for cap in (None, 20):
        reports["valid_cap50" if cap is None else "valid_cap50@20"] = report(found, unchecked, cap)
    expected = {
        "description": "GEO.CAD.TOPO.VALID expected reports at tolerance 1e-7 m; generated by make_valid_fixtures.py "
                       "(stdlib decimal and fractions on the closed-form geometry; independent of fid-geo-cad)",
        "license": LICENSE,
        "tolerance_m": hx(TOL),
        "non_manifold_edge": nm["expect"],
        "reports": reports,
    }
    files["valid_expected.json"] = dump(expected)
    return files


def main():
    check_only = "--check" in sys.argv[1:]
    drift = []
    for rel, text in build_all().items():
        path = HERE / rel
        if check_only:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                drift.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
    if drift:
        print("out of date: " + ", ".join(drift), file=sys.stderr)
        return 1
    print("ok" if check_only else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
