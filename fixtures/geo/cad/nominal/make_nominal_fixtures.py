#!/usr/bin/env python3
"""GEO.CAD.NOMINAL.DIRECT fixtures and expected values (independent Python reference, CC-BY-4.0).

Writes, next to this script:
  analytic_block.json  a closed block 125 x 62.5 x 31.25 mm whose every face and edge is analytic:
                       six box planes, a through hole (cylinder) with a conical countersink at the
                       top, a boss (cylinder and top disc) with a concave torus fillet at its base,
                       and a spherical dimple. Every value is a binary64 dyadic number except the
                       countersink half-angle pi/4 and the arc ranges (pi, 3 pi / 2, 2 pi as f64).
  expected.json        per face, edge and vertex label the expected nominal element: kind,
                       material side and extent (AC-1, AC-2, AC-4).

Run:  python fixtures/geo/cad/nominal/make_nominal_fixtures.py          (writes both files)
      python fixtures/geo/cad/nominal/make_nominal_fixtures.py --check  (fails if anything is stale)

Stdlib only; never runs the Rust code and shares no code with crates/fid-geo-cad.

How the expected values are found:
- Parameters are the fixture's own definitions (REQ-001/002: copied, never refitted), so the Rust
  test compares the element against the fixture script itself; expected.json repeats only the kind.
- Material side (REQ-003) comes from the design intent of each face (hole, countersink, dimple and
  fillet have material outside them: Inside; boss and the box planes as drawn). The script checks
  that intent against the geometry: at a sample point inside the face it forms the surface's
  natural normal Su x Sv from the written-out partial derivatives (60-digit decimal, sin and cos
  of tools/fixtures/nurbs_oracle.py), turns it by the face sense, and requires that this
  out-of-material normal points to the side the intent says (toward the axis, centre or tube
  circle for Inside; away from it for Outside; along +Z of the placement for an Outside plane).
- Extents (REQ-005) are computed exactly in rational arithmetic: every placement is axis-aligned,
  so its orthonormalised frame is the given one (checked), local coordinates are exact dot
  products, and a full circle in a plane parallel to a local axis reaches centre +- r along it
  (the closed-form maximum of a cos t + b sin t is sqrt(a^2 + b^2); here a^2 + b^2 is 0 or 1,
  checked). Every expected extent is then exactly a binary64 value (checked), so the Rust result
  must match bit for bit on every platform (AC-4).
"""

import importlib.util
import json
import sys
from decimal import Decimal, localcontext
from fractions import Fraction as Q
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
D = Decimal

PI = 3.141592653589793
TAU = 6.283185307179586
PI_3_2 = 4.71238898038469  # 3 * PI / 2 as f64 (both products are exact: 3 PI rounds once, / 2 exact)
FRAC_PI_4 = 0.7853981633974483


def die(msg):
    raise SystemExit("make_nominal_fixtures: " + msg)


# ================================================================ geometry of the block (metres)

L, W, H = 0.125, 0.0625, 0.03125
# Through hole with countersink.
XH, YH, RH, CS = 0.03125, 0.03125, 0.0078125, 0.00390625
# Boss with torus fillet.
XB, YB, RB, HB, RF = 0.09375, 0.03125, 0.0078125, 0.015625, 0.001953125
# Spherical dimple: centre DS above the top face, radius RS, rim radius RR (3-4-5 triangle).
XD, YD, RS, DS, RR = 0.0625, 0.03125, 0.009765625, 0.005859375, 0.0078125

assert Q(RR) ** 2 + Q(DS) ** 2 == Q(RS) ** 2
assert 3 * PI / 2 == PI_3_2

EX, EY, EZ = (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)


def neg(v):
    return tuple(-x if x != 0.0 else 0.0 for x in v)


def frame(o, z, x):
    return {"origin": list(o), "axis": list(z), "ref_dir": list(x)}


class Script:
    """A BodyBuilder script (the format of fixtures/geo/cad/topo, read by tests/common)."""

    def __init__(self):
        self.vertices, self.edges, self.coedges, self.loops, self.faces = [], [], [], [], []
        self.vix = {}
        self.uses = {}

    def vertex(self, p, label):
        key = tuple(p)
        if key in self.vix:
            return self.vix[key]
        self.vertices.append({"label": label, "point": list(p)})
        self.vix[key] = len(self.vertices) - 1
        return self.vix[key]

    def edge(self, start, end, rng, curve, label):
        self.edges.append({"curve": curve, "end": end, "label": label, "range": list(rng), "start": start})
        return len(self.edges) - 1

    def coedge(self, e, sense):
        self.uses.setdefault(e, []).append(sense)
        self.coedges.append({"edge": e, "label": "c%d" % len(self.coedges), "pcurve": None, "sense": sense})
        return len(self.coedges) - 1

    def loop(self, coedges, outer, label):
        self.loops.append({"coedges": coedges, "label": label, "outer": outer})
        return len(self.loops) - 1

    def face(self, surface, sense, loops, label):
        self.faces.append({"label": label, "loops": loops, "sense": sense, "surface": surface})
        return len(self.faces) - 1

    def ends(self, c):
        co = self.coedges[c]
        e = self.edges[co["edge"]]
        return (e["start"], e["end"]) if co["sense"] == "same" else (e["end"], e["start"])

    def check(self):
        for e, senses in self.uses.items():
            if sorted(senses) != ["reversed", "same"]:
                die("edge %s used %r (closed shell needs one same and one reversed use)" % (e, senses))
        if len(self.uses) != len(self.edges):
            die("unused edge")
        for lp in self.loops:
            cs = lp["coedges"]
            for k, c in enumerate(cs):
                if self.ends(c)[1] != self.ends(cs[(k + 1) % len(cs)])[0]:
                    die("open loop " + lp["label"])

    def json(self):
        return {
            "description": "analytic block: box, through hole with countersink, boss with torus fillet, "
            "spherical dimple; every surface and curve analytic, closed shell",
            "license": LICENSE,
            "body_id": "6e6f6d696e616c2d626c6f636b2d3031",
            "vertices": self.vertices,
            "edges": self.edges,
            "coedges": self.coedges,
            "loops": self.loops,
            "faces": self.faces,
            "shells": [{"closed": True, "faces": list(range(len(self.faces)))}],
        }


def line_curve(a, b):
    """GEO.NURBS line O + t X: X the unit direction, the axis any unit normal (exact here)."""
    d = [b[i] - a[i] for i in range(3)]
    n = sum(Q(x) ** 2 for x in d)
    nz = [i for i in range(3) if d[i] != 0.0]
    if len(nz) == 1:
        length = abs(d[nz[0]])
        x = tuple(d[i] / length for i in range(3))
    elif d[1] == 0.0 and abs(d[0]) == abs(d[2]):
        # The countersink seam runs at 45 degrees in the xz-plane.
        with localcontext() as ctx:
            ctx.prec = 60
            length = float((D(n.numerator) / D(n.denominator)).sqrt())
        s = 0.7071067811865476
        x = (s if d[0] > 0 else -s, 0.0, s if d[2] > 0 else -s)
    else:
        die("unsupported line direction %r" % (d,))
    normal = EY if x[1] == 0.0 else EX
    return dict(type="line", **frame(a, normal, x)), [0.0, length]


def circle_curve(o, z, x, r):
    return dict(type="circle", radius=r, **frame(o, z, x))


def build():
    s = Script()
    # ---- box vertices v(i, j, k) = (i L, j W, k H)
    def v(i, j, k):
        return s.vertex((i * L, j * W, k * H), "v.corner.%d%d%d" % (i, j, k))

    corners = {(i, j, k): v(i, j, k) for i in (0, 1) for j in (0, 1) for k in (0, 1)}
    box_edges = {}

    def box_edge(a, b):
        """Coedge for the traversal a -> b of a box edge (created on first use)."""
        if (b, a) in box_edges:
            return s.coedge(box_edges[(b, a)], "reversed")
        pa, pb = s.vertices[a]["point"], s.vertices[b]["point"]
        curve, rng = line_curve(pa, pb)
        lab = "e.box.%s-%s" % (s.vertices[a]["label"][-3:], s.vertices[b]["label"][-3:])
        box_edges[(a, b)] = s.edge(a, b, rng, curve, lab)
        return s.coedge(box_edges[(a, b)], "same")

    def box_loop(cycle, label):
        ids = [corners[c] for c in cycle]
        cs = [box_edge(ids[k], ids[(k + 1) % 4]) for k in range(4)]
        return s.loop(cs, True, label)

    # ---- circles (closed edges with one vertex at t = 0) and seams
    def circle_edge(o, r, label, vlabel):
        p = (o[0] + r, o[1], o[2])
        vi = s.vertex(p, vlabel)
        return s.edge(vi, vi, [0.0, TAU], circle_curve(o, EZ, EX, r), label)

    def seam(a_pt, b_pt, label):
        a, b = s.vix[tuple(a_pt)], s.vix[tuple(b_pt)]
        curve, rng = line_curve(a_pt, b_pt)
        return s.edge(a, b, rng, curve, label)

    def band(lower, up_seam, upper, label):
        """Loop lower circle (same), seam up (same), upper circle (reversed), seam down (reversed)."""
        return s.loop([s.coedge(lower, "same"), s.coedge(up_seam, "same"), s.coedge(upper, "reversed"),
                       s.coedge(up_seam, "reversed")], True, label)

    # Hole: bottom circle at z = 0, top circle at z = H - CS (radius RH), countersink rim at z = H.
    e_hole_bot = circle_edge((XH, YH, 0.0), RH, "e.hole.bottom", "v.hole.bottom")
    e_hole_top = circle_edge((XH, YH, H - CS), RH, "e.hole.top", "v.hole.top")
    e_cs_rim = circle_edge((XH, YH, H), RH + CS, "e.csink.rim", "v.csink.rim")
    e_hole_seam = seam((XH + RH, YH, 0.0), (XH + RH, YH, H - CS), "e.hole.seam")
    e_cs_seam = seam((XH + RH, YH, H - CS), (XH + RH + CS, YH, H), "e.csink.seam")
    # Boss: fillet outer circle on the top face, fillet/boss circle at z = H + RF, boss top at H + HB.
    e_fil_out = circle_edge((XB, YB, H), RB + RF, "e.fillet.outer", "v.fillet.outer")
    e_boss_bot = circle_edge((XB, YB, H + RF), RB, "e.boss.bottom", "v.boss.bottom")
    e_boss_top = circle_edge((XB, YB, H + HB), RB, "e.boss.top", "v.boss.top")
    e_boss_seam = seam((XB + RB, YB, H + RF), (XB + RB, YB, H + HB), "e.boss.seam")
    # Fillet seam: the tube circle arc v in [pi, 3 pi / 2] in the meridian half-plane u = 0, as a
    # circle centred on the tube centre with axis -Y and reference X (points R + r cos v, r sin v).
    fa = s.vix[(XB + RB, YB, H + RF)]
    fb = s.vix[(XB + RB + RF, YB, H)]
    e_fil_seam = s.edge(fa, fb, [PI, PI_3_2], circle_curve((XB + RB + RF, YB, H + RF), neg(EY), EX, RF),
                        "e.fillet.seam")
    # Dimple rim on the top face.
    e_dim_rim = circle_edge((XD, YD, H), RR, "e.dimple.rim", "v.dimple.rim")

    faces = {}
    # ---- box planes (loops counter-clockwise about the outward normal; checked in expected())
    faces["f.box.bottom"] = s.face(dict(type="plane", **frame((0.0, 0.0, 0.0), neg(EZ), EX)), "same",
                                   [box_loop([(0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)], "l.box.bottom"),
                                    s.loop([s.coedge(e_hole_bot, "reversed")], False, "l.box.bottom.hole")],
                                   "f.box.bottom")
    faces["f.box.top"] = s.face(dict(type="plane", **frame((0.0, 0.0, H), EZ, EX)), "same",
                                [box_loop([(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)], "l.box.top"),
                                 s.loop([s.coedge(e_cs_rim, "same")], False, "l.box.top.csink"),
                                 s.loop([s.coedge(e_fil_out, "reversed")], False, "l.box.top.fillet"),
                                 s.loop([s.coedge(e_dim_rim, "reversed")], False, "l.box.top.dimple")],
                                "f.box.top")
    faces["f.box.x0"] = s.face(dict(type="plane", **frame((0.0, 0.0, 0.0), EX, EY)), "reversed",
                               [box_loop([(0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)], "l.box.x0")], "f.box.x0")
    faces["f.box.x1"] = s.face(dict(type="plane", **frame((L, 0.0, 0.0), EX, EY)), "same",
                               [box_loop([(1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1)], "l.box.x1")], "f.box.x1")
    faces["f.box.y0"] = s.face(dict(type="plane", **frame((0.0, 0.0, 0.0), neg(EY), EX)), "same",
                               [box_loop([(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)], "l.box.y0")], "f.box.y0")
    faces["f.box.y1"] = s.face(dict(type="plane", **frame((0.0, W, 0.0), EY, EZ)), "same",
                               [box_loop([(0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)], "l.box.y1")], "f.box.y1")
    # ---- features
    faces["f.hole"] = s.face(dict(type="cylinder", radius=RH, **frame((XH, YH, 0.0), EZ, EX)), "reversed",
                             [band(e_hole_bot, e_hole_seam, e_hole_top, "l.hole")], "f.hole")
    faces["f.csink"] = s.face(dict(type="cone", radius=RH, half_angle=FRAC_PI_4,
                                   **frame((XH, YH, H - CS), EZ, EX)), "reversed",
                              [band(e_hole_top, e_cs_seam, e_cs_rim, "l.csink")], "f.csink")
    faces["f.fillet"] = s.face(dict(type="torus", major_radius=RB + RF, minor_radius=RF,
                                    **frame((XB, YB, H + RF), EZ, EX)), "reversed",
                               [s.loop([s.coedge(e_fil_out, "same"), s.coedge(e_fil_seam, "reversed"),
                                        s.coedge(e_boss_bot, "reversed"), s.coedge(e_fil_seam, "same")],
                                       True, "l.fillet")], "f.fillet")
    faces["f.boss"] = s.face(dict(type="cylinder", radius=RB, **frame((XB, YB, H + RF), EZ, EX)), "same",
                             [band(e_boss_bot, e_boss_seam, e_boss_top, "l.boss")], "f.boss")
    faces["f.boss.top"] = s.face(dict(type="plane", **frame((XB, YB, H + HB), EZ, EX)), "same",
                                 [s.loop([s.coedge(e_boss_top, "same")], True, "l.boss.top")], "f.boss.top")
    faces["f.dimple"] = s.face(dict(type="sphere", radius=RS, **frame((XD, YD, H + DS), EZ, EX)), "reversed",
                               [s.loop([s.coedge(e_dim_rim, "same")], True, "l.dimple")], "f.dimple")
    s.check()
    return s


# ================================================================ the oracle

def q3(v):
    return tuple(Q(x) for x in v)


def qdot(a, b):
    return sum(x * y for x, y in zip(a, b))


def qcross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def exact_frame(fr):
    """(O, X, Y, Z) exactly; the given axis and reference must be exact orthonormal (checked)."""
    o, z, x = q3(fr["origin"]), q3(fr["axis"]), q3(fr["ref_dir"])
    if qdot(z, z) != 1 or qdot(x, x) != 1 or qdot(z, x) != 0:
        die("placement is not exactly orthonormal: %r" % fr)
    return o, x, qcross(z, x), z


def local(fr, p):
    o, x, y, z = exact_frame(fr)
    d = tuple(Q(p[i]) - o[i] for i in range(3))
    return (qdot(d, x), qdot(d, y), qdot(d, z))


def as_f64(q, what):
    f = float(q)
    if Q(f) != q:
        die("%s = %s is not a binary64 value" % (what, q))
    return f


def boundary_ranges(sc, face):
    """Exact [min, max] of the three local coordinates over the face's boundary vertices and the
    closed-form extremes of its circle edges (lines reach their extremes at their vertices)."""
    fr = face["surface"]
    lo, hi = [None] * 3, [None] * 3

    def take(k, val):
        lo[k] = val if lo[k] is None or val < lo[k] else lo[k]
        hi[k] = val if hi[k] is None or val > hi[k] else hi[k]

    _, fx, fy, fz = exact_frame(fr)
    axes = (fx, fy, fz)
    for li in face["loops"]:
        for c in sc["loops"][li]["coedges"]:
            e = sc["edges"][sc["coedges"][c]["edge"]]
            for vi in (e["start"], e["end"]):
                loc = local(fr, sc["vertices"][vi]["point"])
                for k in range(3):
                    take(k, loc[k])
            cv = e["curve"]
            if cv["type"] == "line":
                continue
            if cv["type"] != "circle" or e["range"][1] - e["range"][0] < TAU:
                die("only full circles and lines bound the faces with extents")
            _, cx, cy, _ = exact_frame(cv)
            centre = local(fr, cv["origin"])
            r = Q(cv["radius"])
            for k in range(3):
                a, b = qdot(cx, axes[k]), qdot(cy, axes[k])
                h2 = a * a + b * b
                if h2 not in (0, 1):
                    die("circle not parallel or perpendicular to a local axis")
                take(k, centre[k] + r * h2)
                take(k, centre[k] - r * h2)
    return lo, hi


def natural_normal_side(surface, sense, u, v):
    """+1 when the out-of-material normal (Su x Sv turned by the sense) points to the outer side
    (+Z for a plane; away from the axis, centre or tube circle otherwise), -1 when inward."""
    with localcontext() as ctx:
        ctx.prec = 60
        typ = surface["type"]
        su_, cu = OR.sin_cos(D(u))
        sv_, cv = OR.sin_cos(D(v))
        ru = (cu, su_, D(0))          # cos u X + sin u Y in local coordinates
        du = (-su_, cu, D(0))
        z = (D(0), D(0), D(1))
        if typ == "plane":
            Su, Sv, outer = (D(1), D(0), D(0)), (D(0), D(1), D(0)), z
        elif typ == "cylinder":
            r = D(surface["radius"])
            Su, Sv, outer = tuple(r * c for c in du), z, ru
        elif typ == "cone":
            r0 = D(surface["radius"])
            sa, ca = OR.sin_cos(D(surface["half_angle"]))
            t = sa / ca
            rho = r0 + D(v) * t
            Su = tuple(rho * c for c in du)
            Sv = tuple(t * ru[i] + z[i] for i in range(3))
            outer = tuple(rho * c for c in ru)  # from the axis to the point
        elif typ == "sphere":
            r = D(surface["radius"])
            Su = tuple(r * cv * c for c in du)
            Sv = tuple(-r * sv_ * ru[i] + r * cv * z[i] for i in range(3))
            outer = tuple(cv * ru[i] + sv_ * z[i] for i in range(3))  # from the centre
        elif typ == "torus":
            R, r = D(surface["major_radius"]), D(surface["minor_radius"])
            Su = tuple((R + r * cv) * c for c in du)
            Sv = tuple(-r * sv_ * ru[i] + r * cv * z[i] for i in range(3))
            outer = tuple(cv * ru[i] + sv_ * z[i] for i in range(3))  # from the tube circle
        else:
            die("surface type " + typ)
        n = (Su[1] * Sv[2] - Su[2] * Sv[1], Su[2] * Sv[0] - Su[0] * Sv[2], Su[0] * Sv[1] - Su[1] * Sv[0])
        side = sum(n[i] * outer[i] for i in range(3))
        if side == 0:
            die("degenerate sample point")
        nat = 1 if side > 0 else -1
        return nat if sense == "same" else -nat


# Design intent: which side the material lies on, and a parameter sample inside each face.
INTENT = {
    "f.box.bottom": ("Outside", 0.0, 0.0),   # placement axis -Z points out of the material
    "f.box.top": ("Outside", 0.0, 0.0),
    "f.box.x0": ("Inside", 0.0, 0.0),        # axis +X points into the material
    "f.box.x1": ("Outside", 0.0, 0.0),
    "f.box.y0": ("Outside", 0.0, 0.0),
    "f.box.y1": ("Outside", 0.0, 0.0),
    "f.hole": ("Inside", 1.0, 0.01),         # material around the hole
    "f.csink": ("Inside", 1.0, 0.002),
    "f.fillet": ("Inside", 1.0, 3.9269908169872414),  # concave: material away from the tube
    "f.boss": ("Outside", 1.0, 0.005),
    "f.boss.top": ("Outside", 0.0, 0.0),
    "f.dimple": ("Inside", 1.0, -1.2),       # material outside the sphere
}


# A point inside the material behind each plane face (the out-of-material normal points away).
MATERIAL_POINT = {"f.boss.top": (XB, YB, H + HB / 2)}
BOX_CENTRE = (L / 2, W / 2, H / 2)


def ccw_ok(sc, face):
    """Box planes: the outer loop turns counter-clockwise about the out-of-material normal."""
    fr = face["surface"]
    lp = sc["loops"][face["loops"][0]]
    pts = []
    for c in lp["coedges"]:
        co = sc["coedges"][c]
        e = sc["edges"][co["edge"]]
        start = e["start"] if co["sense"] == "same" else e["end"]
        pts.append(local(fr, sc["vertices"][start]["point"]))
    area2 = sum(pts[k][0] * pts[(k + 1) % len(pts)][1] - pts[(k + 1) % len(pts)][0] * pts[k][1]
                for k in range(len(pts)))
    sign = 1 if face["sense"] == "same" else -1
    return area2 * sign > 0


def expected(sc):
    out_faces = {}
    for face in sc["faces"]:
        lab = face["label"]
        mat, u, v = INTENT[lab]
        side = natural_normal_side(face["surface"], face["sense"], u, v)
        if (side > 0) != (mat == "Outside"):
            die("%s: sense contradicts the intended material side %s" % (lab, mat))
        typ = face["surface"]["type"]
        if typ == "plane":
            inner = local(face["surface"], MATERIAL_POINT.get(lab, BOX_CENTRE))[2]
            out_is_plus_z = (mat == "Outside")
            if (inner < 0) != out_is_plus_z:
                die("%s: material point lies on the wrong side for %s" % (lab, mat))
        if typ == "plane" and lab.startswith("f.box.") and not ccw_ok(sc, face):
            die("%s: outer loop is not counter-clockwise about the out-of-material normal" % lab)
        rec = {"kind": typ, "material": mat}
        if typ in ("plane", "cylinder", "cone"):
            lo, hi = boundary_ranges(sc, face)
            if typ == "plane":
                rec["extent"] = {"planar": {"u": [as_f64(lo[0], lab), as_f64(hi[0], lab)],
                                            "v": [as_f64(lo[1], lab), as_f64(hi[1], lab)]}}
            else:
                rec["extent"] = {"axial": [as_f64(lo[2], lab), as_f64(hi[2], lab)]}
        else:
            rec["extent"] = "not_defined"
        out_faces[lab] = rec
    out_edges = {}
    for e in sc["edges"]:
        typ = e["curve"]["type"]
        if typ == "line":
            rec = {"kind": "line", "extent": {"axial": e["range"]}}
        else:
            rec = {"kind": "circle", "extent": {"angular": e["range"]}}
        out_edges[e["label"]] = rec
    out_vertices = {vx["label"]: {"kind": "point", "point": vx["point"]} for vx in sc["vertices"]}
    return {
        "description": "GEO.CAD.NOMINAL.DIRECT expected nominal elements of analytic_block.json "
        "(make_nominal_fixtures.py; material from design intent checked against Su x Sv, extents exact)",
        "license": LICENSE,
        "faces": out_faces,
        "edges": out_edges,
        "vertices": out_vertices,
    }


def dumps(obj):
    return json.dumps(obj, indent=1, sort_keys=True) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    sc = build().json()
    files = {"analytic_block.json": dumps(sc), "expected.json": dumps(expected(sc))}
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
