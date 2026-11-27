#!/usr/bin/env python3
"""GEO.CAD.TESS.EDGE fixtures and expected values (independent Python reference, CC-BY-4.0).

Writes, into analytic_set/ next to this script:
  cube.json              cube 20 mm, closed shell, 12 line edges (AC-1, AC-2, AC-4)
  cube_reversed.json     the same entities with every builder list reversed (AC-4)
  washer.json            cylinder r 20 mm, h 10 mm with an 8 mm through hole: two cylinder faces
                         with seam edges, two annular planes, closed shell (AC-1, AC-2, AC-4)
  washer_reversed.json   the washer with every builder list reversed (AC-4)
  sphere.json            whole sphere r 15 mm, one meridian seam edge used twice (AC-1, AC-2)
  torus.json             whole torus R 30 mm, r 8 mm, one u and one v seam circle (AC-1, AC-2)
  plate_offset.json      plane plate 40 x 30 mm whose hole circle lies 1e-6 m above the plane (AC-5)
  expected.json          oracle values for all of them, plus the circle cases of AC-3

Run:  python fixtures/geo/cad/tess/make_tess_fixtures.py          (writes everything)
      python fixtures/geo/cad/tess/make_tess_fixtures.py --check  (fails if anything is stale)

Stdlib only; never runs the Rust code. The oracle is independent of crates/fid-geo-cad: the edge
curves in closed form at 60 digits (`decimal`, sin/cos/atan2 of tools/fixtures/nurbs_oracle.py),
placements orthonormalised exactly from the binary64 inputs, and the pinned bisection of
crates/fid-geo-cad/src/tess/edge.rs (module docs) run on those exact values. Parameters are
binary64 and the bisection midpoints `(a + b) * 0.5` are computed in binary64, exactly as the
Rust code does, so the expected parameter lists are bit-exact. Every accept/split decision is
checked to be at least a relative 1e-6 away from its threshold, so binary64 rounding in the Rust
evaluators cannot flip it; a closer decision stops the script.

Independently of the bisection, every circle case of AC-3 gets the closed-form minimum:
the sagitta of an arc of angle theta on radius r is r (1 - cos(theta / 2)), so the widest arc
within the chordal tolerance t is theta_c = 2 acos(1 - t / r); with the angular tolerance a
(the tangent turns by theta over the arc) the widest arc is min(theta_c, a). The fewest equal
segments is N = ceil(range / theta_max), rounded up to the next power of two (a bisection
level); the samples are N + 1. The script stops if the bisection disagrees.

Vertex indices (REQ-002): edges in ascending label order (bytewise; every edge is labelled), each
edge's samples in order, an end sample takes its body vertex's index (new the first time), an
interior sample a new index. Coedge lists are the edge's, reversed for a reversed coedge.
Planar faces also get the exact uv of every sample: (P - O) . X and (P - O) . Y in the plane's
orthonormalised frame (their chart offsets are zero: planes have no period).
Surface gaps (REQ-003) are exact: the distance of each mesh point (the stored vertex point at the
ends, the curve point inside) to each adjacent analytic surface in closed form.
"""

import importlib.util
import json
import math
import sys
from decimal import Decimal, localcontext
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "analytic_set"
ROOT = HERE.parents[3]
LICENSE = "CC-BY-4.0"


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


OR = _load("nurbs_oracle", "tools/fixtures/nurbs_oracle.py")
D = Decimal

TAU = 6.283185307179586
FRAC_PI_2 = 1.5707963267948966
CHORDAL = 1e-5
ANGULAR = 0.2
MAX_DEPTH = 24
MARGIN = D("1e-6")


def die(msg):
    raise SystemExit("make_tess_fixtures: " + msg)


# ================================================================ builder scripts

def frame(o, z, x):
    return {"origin": list(o), "axis": list(z), "ref_dir": list(x)}


def line(o, direction, normal):
    """GEO.NURBS line O + t X: the direction is the reference direction X, the axis any normal."""
    return dict(type="line", **frame(o, normal, direction))


def circle(o, z, x, r):
    return dict(type="circle", radius=r, **frame(o, z, x))


def body(desc, bid, vertices, edges, coedges, loops, faces, closed):
    return {
        "description": desc,
        "license": LICENSE,
        "body_id": bid,
        "vertices": [{"point": list(p), "label": lab} for (p, lab) in vertices],
        "edges": [dict(start=s, end=e, range=list(r), curve=c, label=lab) for (s, e, r, c, lab) in edges],
        "coedges": [dict(edge=e, sense=s, pcurve=None, label=lab) for (e, s, lab) in coedges],
        "loops": [dict(coedges=cs, outer=o, label=lab) for (cs, o, lab) in loops],
        "faces": [dict(surface=s, sense=fs, loops=ls, label=lab) for (s, fs, ls, lab) in faces],
        "shells": [{"faces": list(range(len(faces))), "closed": closed}],
    }


def reversed_body(b, desc):
    """Every builder list reversed, references remapped; loops keep their cyclic coedge order."""
    nv, ne, nc, nl, nf = (len(b[k]) for k in ("vertices", "edges", "coedges", "loops", "faces"))
    out = json.loads(json.dumps(b))
    out["description"] = desc
    out["vertices"] = out["vertices"][::-1]
    out["edges"] = [dict(e, start=nv - 1 - e["start"], end=nv - 1 - e["end"]) for e in out["edges"][::-1]]
    out["coedges"] = [dict(c, edge=ne - 1 - c["edge"]) for c in out["coedges"][::-1]]
    out["loops"] = [dict(l, coedges=[nc - 1 - i for i in l["coedges"]]) for l in out["loops"][::-1]]
    out["faces"] = [dict(f, loops=[nl - 1 - i for i in f["loops"]]) for f in out["faces"][::-1]]
    out["shells"] = [{"faces": [nf - 1 - i for i in s["faces"]][::-1], "closed": s["closed"]} for s in out["shells"]]
    return out


Z = (0.0, 0.0, 1.0)
X = (1.0, 0.0, 0.0)
Y = (0.0, 1.0, 0.0)
O = (0.0, 0.0, 0.0)


def cube():
    s = 0.02
    corner = [(x, y, z) for z in (0.0, s) for y in (0.0, s) for x in (0.0, s)]
    verts = [(p, "v%d" % i) for i, p in enumerate(corner)]
    # Faces as vertex cycles, counter-clockwise seen from outside, with an outward normal.
    cycles = [
        ((0, 2, 3, 1), (0.0, 0.0, -1.0), "f-zlo"),
        ((4, 5, 7, 6), Z, "f-zhi"),
        ((0, 1, 5, 4), (0.0, -1.0, 0.0), "f-ylo"),
        ((2, 6, 7, 3), Y, "f-yhi"),
        ((0, 4, 6, 2), (-1.0, 0.0, 0.0), "f-xlo"),
        ((1, 3, 7, 5), X, "f-xhi"),
    ]
    edge_of, edges = {}, []
    for cyc, _, _ in cycles:
        for k in range(4):
            a, b = sorted((cyc[k], cyc[(k + 1) % 4]))
            if (a, b) not in edge_of:
                pa, pb = corner[a], corner[b]
                d = tuple((pb[i] - pa[i]) / s for i in range(3))
                normal = (0.0, 0.0, 1.0) if d[2] == 0.0 else (1.0, 0.0, 0.0)
                edge_of[(a, b)] = len(edges)
                edges.append((a, b, (0.0, s), line(pa, d, normal), "e%d%d" % (a, b)))
    coedges, loops, faces = [], [], []
    for cyc, n, lab in cycles:
        cs = []
        for k in range(4):
            a, b = cyc[k], cyc[(k + 1) % 4]
            e = edge_of[tuple(sorted((a, b)))]
            sense = "same" if edges[e][0] == a else "reversed"
            cs.append(len(coedges))
            coedges.append((e, sense, "c-%s-%d" % (lab[2:], k)))
        loops.append((cs, True, "l-" + lab[2:]))
        o = corner[cyc[0]]
        p1 = corner[cyc[1]]
        xdir = tuple((p1[i] - o[i]) / s for i in range(3))
        faces.append((dict(type="plane", **frame(o, n, xdir)), "same", [len(loops) - 1], lab))
    return body("cube 20 mm, every entity labelled, closed shell of six planes", "c0be0000c0be0000c0be0000c0be0000",
                verts, edges, coedges, loops, faces, True)


def washer():
    ro, ri, h = 0.02, 0.008, 0.01
    verts = [((ro, 0.0, 0.0), "v-ob"), ((ro, 0.0, h), "v-ot"), ((ri, 0.0, 0.0), "v-ib"), ((ri, 0.0, h), "v-it")]
    top = (0.0, 0.0, h)
    edges = [
        (0, 0, (0.0, TAU), circle(O, Z, X, ro), "e-outer-bottom"),
        (1, 1, (0.0, TAU), circle(top, Z, X, ro), "e-outer-top"),
        (2, 2, (0.0, TAU), circle(O, Z, X, ri), "e-inner-bottom"),
        (3, 3, (0.0, TAU), circle(top, Z, X, ri), "e-inner-top"),
        (0, 1, (0.0, h), line((ro, 0.0, 0.0), Z, X), "e-outer-seam"),
        (2, 3, (0.0, h), line((ri, 0.0, 0.0), Z, X), "e-inner-seam"),
    ]
    coedges = [
        (0, "same", "c-oc-bottom"), (4, "same", "c-oc-seam-up"), (1, "reversed", "c-oc-top"),
        (4, "reversed", "c-oc-seam-down"),
        (2, "reversed", "c-ic-bottom"), (5, "same", "c-ic-seam-up"), (3, "same", "c-ic-top"),
        (5, "reversed", "c-ic-seam-down"),
        (0, "reversed", "c-bot-outer"), (2, "same", "c-bot-inner"),
        (1, "same", "c-top-outer"), (3, "reversed", "c-top-inner"),
    ]
    loops = [([0, 1, 2, 3], True, "l-outer-cyl"), ([4, 5, 6, 7], True, "l-inner-cyl"),
             ([8], True, "l-bot-outer"), ([9], False, "l-bot-inner"),
             ([10], True, "l-top-outer"), ([11], False, "l-top-inner")]
    faces = [
        (dict(type="cylinder", radius=ro, **frame(O, Z, X)), "same", [0], "f-outer-cyl"),
        (dict(type="cylinder", radius=ri, **frame(O, Z, X)), "reversed", [1], "f-inner-cyl"),
        (dict(type="plane", **frame(O, (0.0, 0.0, -1.0), X)), "same", [2, 3], "f-bottom"),
        (dict(type="plane", **frame(top, Z, X)), "same", [4, 5], "f-top"),
    ]
    return body("cylinder r 20 mm, height 10 mm, with an 8 mm radius through hole; seam edges on both "
                "cylinder faces, closed shell", "a5e4a5e4a5e4a5e4a5e4a5e4a5e4a5e4", verts, edges, coedges, loops,
                faces, True)


def sphere():
    r = 0.015
    verts = [((0.0, 0.0, -r), "v-south"), ((0.0, 0.0, r), "v-north")]
    # Great half circle in the xz-plane: axis -Y, reference X, so C(t) = r (cos t, 0, sin t).
    edges = [(0, 1, (-FRAC_PI_2, FRAC_PI_2), circle(O, (0.0, -1.0, 0.0), X, r), "e-seam")]
    coedges = [(0, "same", "c-seam-up"), (0, "reversed", "c-seam-down")]
    loops = [([0, 1], True, "l-seam")]
    faces = [(dict(type="sphere", radius=r, **frame(O, Z, X)), "same", [0], "f-sphere")]
    return body("whole sphere r 15 mm: one meridian seam edge from pole to pole used twice, closed shell",
                "5b4e5b4e5b4e5b4e5b4e5b4e5b4e5b4e", verts, edges, coedges, loops, faces, True)


def torus():
    big, small = 0.03, 0.008
    outer = big + small
    verts = [((outer, 0.0, 0.0), "v0")]
    # u seam: the outer equator; v seam: the tube circle at u = 0 (axis -Y, reference X: Y = +Z).
    edges = [(0, 0, (0.0, TAU), circle(O, Z, X, outer), "e-u-seam"),
             (0, 0, (0.0, TAU), circle((big, 0.0, 0.0), (0.0, -1.0, 0.0), X, small), "e-v-seam")]
    coedges = [(0, "same", "c-u"), (1, "same", "c-v"), (0, "reversed", "c-u-back"), (1, "reversed", "c-v-back")]
    loops = [([0, 1, 2, 3], True, "l-torus")]
    faces = [(dict(type="torus", major_radius=big, minor_radius=small, **frame(O, Z, X)), "same", [0], "f-torus")]
    return body("whole torus R 30 mm, r 8 mm: one loop of a u seam circle and a v seam circle, each used twice, "
                "closed shell", "70b570b570b570b570b570b570b570b5", verts, edges, coedges, loops, faces, True)


def plate_offset():
    w, h, cx, cy, r, dz = 0.04, 0.03, 0.02, 0.015, 0.005, 1e-6
    verts = [((0.0, 0.0, 0.0), "v0"), ((w, 0.0, 0.0), "v1"), ((w, h, 0.0), "v2"), ((0.0, h, 0.0), "v3"),
             ((cx + r, cy, dz), "v-hole")]
    edges = [
        (0, 1, (0.0, w), line((0.0, 0.0, 0.0), X, Z), "e-bottom"),
        (1, 2, (0.0, h), line((w, 0.0, 0.0), Y, Z), "e-right"),
        (2, 3, (0.0, w), line((w, h, 0.0), (-1.0, 0.0, 0.0), Z), "e-top"),
        (3, 0, (0.0, h), line((0.0, h, 0.0), (0.0, -1.0, 0.0), Z), "e-left"),
        (4, 4, (0.0, TAU), circle((cx, cy, dz), Z, X, r), "e-hole"),
    ]
    coedges = [(0, "same", "c-bottom"), (1, "same", "c-right"), (2, "same", "c-top"), (3, "same", "c-left"),
               (4, "reversed", "c-hole")]
    loops = [([0, 1, 2, 3], True, "l-outer"), ([4], False, "l-hole")]
    faces = [(dict(type="plane", **frame(O, Z, X)), "same", [0, 1], "f-plate")]
    return body("plane plate 40 x 30 mm with a 5 mm radius hole whose circle lies 1e-6 m above the plane (AC-5)",
                "0ff50ff50ff50ff50ff50ff50ff50ff5", verts, edges, coedges, loops, faces, False)


def fixtures():
    c, w = cube(), washer()
    return [
        ("cube", c),
        ("cube_reversed", reversed_body(c, "the cube with every builder list reversed (AC-4)")),
        ("washer", w),
        ("washer_reversed", reversed_body(w, "the washer with every builder list reversed (AC-4)")),
        ("sphere", sphere()),
        ("torus", torus()),
        ("plate_offset", plate_offset()),
    ]


# ================================================================ oracle geometry (60 digits)

def dv(x):
    return D(x)


class OPlace:
    """Placement orthonormalised exactly from the f64 inputs."""

    def __init__(self, g):
        z, x = [dv(c) for c in g["axis"]], [dv(c) for c in g["ref_dir"]]
        self.o = [dv(c) for c in g["origin"]]
        self.z = OR.vscale(1 / OR.dnorm(z), z)
        xp = OR.vsub(x, OR.vscale(OR.dot(x, self.z), self.z))
        self.x = OR.vscale(1 / OR.dnorm(xp), xp)
        self.y = OR.cross(self.z, self.x)

    def world(self, a, b, c):
        return [self.o[i] + a * self.x[i] + b * self.y[i] + c * self.z[i] for i in range(3)]

    def vec(self, a, b, c):
        return [a * self.x[i] + b * self.y[i] + c * self.z[i] for i in range(3)]

    def local(self, p):
        d = OR.vsub(p, self.o)
        return OR.dot(d, self.x), OR.dot(d, self.y), OR.dot(d, self.z)


class OCurve:
    def __init__(self, g):
        self.kind, self.pl = g["type"], OPlace(g)
        self.r = dv(g.get("radius", 0.0))
        if self.kind not in ("line", "circle"):
            die("oracle curve type " + self.kind)

    def point(self, t):
        t = dv(t)
        if self.kind == "line":
            return self.pl.world(t, 0, 0)
        s, c = OR.sin_cos(t)
        return self.pl.world(self.r * c, self.r * s, 0)

    def tangent(self, t):
        if self.kind == "line":
            return self.pl.vec(1, 0, 0)
        s, c = OR.sin_cos(dv(t))
        return self.pl.vec(-self.r * s, self.r * c, 0)


def d_dist(a, b):
    return OR.dnorm(OR.vsub(a, b))


def seg_dist(p, a, b):
    e = OR.vsub(b, a)
    ee = OR.dot(e, e)
    if ee == 0:
        return d_dist(p, a)
    s = OR.dot(OR.vsub(p, a), e) / ee
    s = D(0) if s < 0 else (D(1) if s > 1 else s)
    return d_dist(p, [a[i] + s * e[i] for i in range(3)])


def d_angle(a, b):
    return OR.atan2(OR.dnorm(OR.cross(a, b)), OR.dot(a, b))


def acos(x):
    return OR.atan2((1 - x * x).sqrt(), x)


def clear_le(value, threshold, what):
    """value <= threshold, refusing decisions within a relative MARGIN of the threshold."""
    t = dv(threshold)
    if abs(value - t) <= MARGIN * t:
        die("%s: %s is within 1e-6 of the threshold %s" % (what, value, t))
    return value <= t


def o_sample(curve, t0, t1, chordal, angular, max_depth, what):
    """The pinned bisection on exact geometry; parameters in binary64 as in the Rust code."""
    ts, stack = [t0], [(t0, t1, 0)]
    worst_m, worst_a, within = D(0), D(0), True
    while stack:
        lt, rt, depth = stack.pop()
        tm = (lt + rt) * 0.5
        checks = ((lt + tm) * 0.5, tm, (tm + rt) * 0.5)
        pl, pr, tl, tr = curve.point(lt), curve.point(rt), curve.tangent(lt), curve.tangent(rt)
        em = max(seg_dist(curve.point(q), pl, pr) for q in checks)
        ea = d_angle(tl, tr)
        for q in checks:
            tq = curve.tangent(q)
            ea = max(ea, d_angle(tl, tq), d_angle(tq, tr))
        if not clear_le(em, chordal, what + " chord at [%r, %r]" % (lt, rt)):
            ok = False
        else:
            ok = clear_le(ea, angular, what + " angle at [%r, %r]" % (lt, rt))
        if ok or depth >= max_depth or not (lt < tm < rt):
            within = within and ok
            worst_m, worst_a = max(worst_m, em), max(worst_a, ea)
            ts.append(rt)
        else:
            stack.append((tm, rt, depth + 1))
            stack.append((lt, tm, depth + 1))
    return ts, within, worst_m, worst_a


def closed_form_samples(radius, span, chordal, angular):
    """AC-3: fewest equal segments within both tolerances, rounded up to a bisection level."""
    r, t = dv(radius), dv(chordal)
    theta_c = 2 * acos(1 - t / r)
    theta = min(theta_c, dv(angular))
    n = (dv(span) / theta).to_integral_value(rounding="ROUND_CEILING")
    if abs(dv(span) / theta - n) < D("1e-6"):
        die("closed form too close to an integer")
    level = 0
    while 2 ** level < n:
        level += 1
    return int(n), 2 ** level + 1


def surface_dist(g, p):
    """Exact distance from p to an analytic surface."""
    pl = OPlace(g)
    a, b, c = pl.local(p)
    rho = (a * a + b * b).sqrt()
    k = g["type"]
    if k == "plane":
        return abs(c)
    if k == "cylinder":
        return abs(rho - dv(g["radius"]))
    if k == "sphere":
        return abs((rho * rho + c * c).sqrt() - dv(g["radius"]))
    if k == "torus":
        q = rho - dv(g["major_radius"])
        return abs((q * q + c * c).sqrt() - dv(g["minor_radius"]))
    die("surface type " + k)


# ================================================================ expected values

def fnum(x):
    """A Decimal as the nearest binary64 (JSON number)."""
    return float(x)


def body_expected(name, b):
    edges = b["edges"]
    order = sorted(range(len(edges)), key=lambda i: edges[i]["label"].encode("utf-8"))
    vertex_ix, n_points = {}, 0
    per_edge = {}
    coedges_of = {i: [] for i in range(len(edges))}
    for ci, c in enumerate(b["coedges"]):
        coedges_of[c["edge"]].append(ci)
    face_of_loop = {}
    for fi, f in enumerate(b["faces"]):
        for li in f["loops"]:
            face_of_loop[li] = fi
    face_of_coedge = {}
    for li, l in enumerate(b["loops"]):
        for ci in l["coedges"]:
            face_of_coedge[ci] = face_of_loop[li]
    for e in order:
        ed = edges[e]
        cur = OCurve(ed["curve"])
        t0, t1 = ed["range"]
        what = "%s/%s" % (name, ed["label"])
        ts, within, wm, wa = o_sample(cur, t0, t1, CHORDAL, ANGULAR, MAX_DEPTH, what)
        if not within:
            die(what + " does not meet the tolerances")
        ix, pts = [], []
        for k, t in enumerate(ts):
            if k == 0 or k == len(ts) - 1:
                v = ed["start"] if k == 0 else ed["end"]
                if v not in vertex_ix:
                    vertex_ix[v] = n_points
                    n_points += 1
                ix.append(vertex_ix[v])
                pts.append([dv(c) for c in b["vertices"][v]["point"]])
            else:
                ix.append(n_points)
                n_points += 1
                pts.append(cur.point(t))
        end_gap = max(d_dist(cur.point(t0), pts[0]), d_dist(cur.point(t1), pts[-1]))
        if end_gap > D("1e-12"):
            die(what + " end vertex off its curve")
        faces = sorted({face_of_coedge[c] for c in coedges_of[e]})
        gap = max(surface_dist(b["faces"][f]["surface"], p) for f in faces for p in pts)
        per_edge[e] = dict(t=ts, vertex_ix=ix, pts=pts, kind=ed["curve"]["type"])
        per_edge[e]["out"] = {
            "kind": ed["curve"]["type"],
            "samples": len(ts),
            "t": ts,
            "vertex_ix": ix,
            "max_surface_gap_m": fnum(gap),
            "achieved_m": fnum(wm),
            "achieved_rad": fnum(wa),
        }
    coedges = {}
    for ci, c in enumerate(b["coedges"]):
        pe = per_edge[c["edge"]]
        rev = c["sense"] == "reversed"
        ix = pe["vertex_ix"][::-1] if rev else list(pe["vertex_ix"])
        entry = {"edge": edges[c["edge"]]["label"], "vertex_ix": ix}
        surf = b["faces"][face_of_coedge[ci]]["surface"]
        if surf["type"] == "plane":
            pl = OPlace(surf)
            pts = pe["pts"][::-1] if rev else pe["pts"]
            entry["uv"] = [[fnum(pl.local(p)[0]), fnum(pl.local(p)[1])] for p in pts]
        coedges[c["label"]] = entry
    return {
        "n_points": n_points,
        "edge_order": [edges[e]["label"] for e in order],
        "edges": {edges[e]["label"]: per_edge[e]["out"] for e in sorted(per_edge, key=lambda i: edges[i]["label"])},
        "coedges": {k: coedges[k] for k in sorted(coedges)},
    }


CIRCLE_CASES = [
    # name, radius, range, chordal, angular, max_depth
    ("r10mm-full", 0.01, (0.0, TAU), 1e-5, 0.2, MAX_DEPTH),
    ("r50mm-full", 0.05, (0.0, TAU), 1e-5, 0.2, MAX_DEPTH),
    ("r2mm-full-fine", 0.002, (0.0, TAU), 1e-6, 0.2, MAX_DEPTH),
    ("r30mm-arc", 0.03, (0.3, 2.1), 1e-5, 0.2, MAX_DEPTH),
    ("r100mm-angular", 0.1, (0.0, TAU), 1e-3, 0.05, MAX_DEPTH),
]

TOLERANCE_CASES = [
    # REQ-005: a depth limit too shallow for the tolerance
    ("r10mm-depth3", 0.01, (0.0, TAU), 1e-6, 0.2, 3),
]


def circle_case(name, radius, rng, chordal, angular, depth, expect_within):
    g = circle(O, Z, X, radius)
    ts, within, wm, wa = o_sample(OCurve(g), rng[0], rng[1], chordal, angular, depth, name)
    if within != expect_within:
        die(name + " tolerance outcome")
    entry = {
        "name": name,
        "radius": radius,
        "range": list(rng),
        "chordal_m": chordal,
        "angular_rad": angular,
        "max_depth": depth,
        "samples": len(ts),
        "t": ts,
        "within_tolerance": within,
        "achieved_m": fnum(wm),
        "achieved_rad": fnum(wa),
    }
    if expect_within:
        n, closed = closed_form_samples(radius, rng[1] - rng[0], chordal, angular)
        if closed != len(ts):
            die("%s: closed form %d samples, bisection %d" % (name, closed, len(ts)))
        entry["closed_form_min_segments"] = n
        entry["closed_form_samples"] = closed
    return entry


def render(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def build():
    files = {}
    expected = {
        "description": "GEO.CAD.TESS.EDGE expected values: 60-digit closed-form oracle of the pinned bisection "
                       "(bit-exact binary64 parameters), vertex index lists, planar uv, surface gaps and the AC-3 "
                       "closed-form circle sample counts. Written by make_tess_fixtures.py; do not edit.",
        "license": LICENSE,
        "chordal_m": CHORDAL,
        "angular_rad": ANGULAR,
        "max_depth": MAX_DEPTH,
        "bodies": {},
    }
    for name, b in fixtures():
        files[name + ".json"] = render(b)
        expected["bodies"][name] = body_expected(name, b)
        print("%s: %d mesh vertices" % (name, expected["bodies"][name]["n_points"]))
    expected["circle_cases"] = [circle_case(*c, True) for c in CIRCLE_CASES]
    expected["tolerance_cases"] = [circle_case(*c, False) for c in TOLERANCE_CASES]
    files["expected.json"] = render(expected)
    return files


def main():
    check = "--check" in sys.argv[1:]
    with localcontext() as ctx:
        ctx.prec = OR.PREC
        files = build()
    if check:
        stale = [n for n, text in files.items()
                 if not (OUT / n).exists() or (OUT / n).read_text(encoding="utf-8") != text]
        if stale:
            raise SystemExit(f"stale: {stale}")
        print(f"{len(files)} files up to date")
    else:
        OUT.mkdir(exist_ok=True)
        for n, text in files.items():
            (OUT / n).write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {len(files)} files")


if __name__ == "__main__":
    main()
