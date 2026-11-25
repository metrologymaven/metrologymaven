#!/usr/bin/env python3
"""Independent oracle for GEO.DIST.FACE.INCID AC-1 and AC-2 (CC-BY-4.0).

Writes, next to this script:

- ``tube.json``: an invented B-rep (topology JSON of GEO.CAD.TOPO, lines given by origin,
  direction and a perpendicular normal): a solid cylinder of radius 2^-6 m and height
  2^-7 m with a coaxial through hole of radius 2^-7 m (the "cylinder with hole"; the
  GEO.CAD.TESS ``analytic_set`` fixtures do not exist yet).
- ``cases.txt``: for the pinned point streams below, the exact inside/outside label, the
  closest feature (kind, entity label, face label) and the exact distance to the boundary.

Points lie on a grid of 2^-30 m, so every coordinate is an exact binary64 number and the
inside tests are exact integer tests. Distances are computed with 60-digit decimals and
rounded once to binary64. Nothing here uses the Rust crate.

Point streams (mirrored bit for bit in ``crates/fid-geo-dist/tests/face_incid.rs``): SplitMix64
seeded with 29 (AC-1, the unit cube of ``fixtures/geo/cad/topo/cube_labelled.json``, 10 000
points near its 12 edges and 8 corners) and 31 (AC-2, the tube, 2 000 points near the two
rims of the hole). ``below(n)`` is ``next() % n``.

Usage: ``python make_incid_cases.py`` (write) or ``--check`` (compare, exit 1 on a difference).
"""

import hashlib
import json
import math
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

getcontext().prec = 60

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
CUBE = ROOT / "fixtures" / "geo" / "cad" / "topo" / "cube_labelled.json"
OUT_TUBE = HERE / "tube.json"
OUT_CASES = HERE / "cases.txt"

U = 1 << 30  # grid units per metre
MASK = (1 << 64) - 1

N_CUBE = 10_000
SEED_CUBE = 29
N_TUBE = 2_000
SEED_TUBE = 31

# Tube dimensions in grid units.
R_OUT = U >> 6
R_IN = U >> 7
H = U >> 7
# Smallest distance kept (2^4 units, about 1.5e-8 m): nearer points are skipped, since a
# sign within the foot accuracy of 1e-9 m is legitimately undecidable.
MIN_D = 16


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


def isqrt(n):
    return math.isqrt(n)


def dec_sqrt(n):
    """Square root of a non-negative integer or Decimal at 60 digits."""
    return Decimal(n).sqrt()


def metres(d_units):
    """A distance in grid units (Decimal) as the nearest binary64 in metres."""
    return float(Decimal(d_units) / Decimal(U))


# ---------------------------------------------------------------- the cube (AC-1)


def cube_labels():
    """Face labels by (axis, side), edge labels by sorted end points, vertex labels by point,
    read from the topology fixture (its geometry: vertex points and plane origins/axes)."""
    d = json.loads(CUBE.read_text(encoding="utf-8"))
    pts = [tuple(int(round(c)) for c in v["point"]) for v in d["vertices"]]
    vertex = {pts[i]: v["label"] for i, v in enumerate(d["vertices"])}
    edge = {}
    for e in d["edges"]:
        a, b = pts[e["start"]], pts[e["end"]]
        edge[tuple(sorted((a, b)))] = e["label"]
    face = {}
    for f in d["faces"]:
        s = f["surface"]
        axis = [int(round(c)) for c in s["axis"]]
        k = next(i for i in range(3) if axis[i] != 0)
        side = int(round(s["origin"][k]))
        face[(k, side)] = f["label"]
    return face, edge, vertex


def cube_points():
    g = SplitMix(SEED_CUBE)
    out = []
    for _ in range(N_CUBE):
        while True:
            pick = g.below(20)
            if pick < 12:
                a, bits = pick // 4, pick % 4
                base = [0, 0, 0]
                others = [i for i in range(3) if i != a]
                base[others[0]] = U * (bits & 1)
                base[others[1]] = U * ((bits >> 1) & 1)
                base[a] = g.below(U + 1)
            else:
                c = pick - 12
                base = [U * (c & 1), U * ((c >> 1) & 1), U * ((c >> 2) & 1)]
            e = g.below(14)
            de = 1 << (23 - e)
            p = [base[i] + g.below(2 * de + 1) - de for i in range(3)]
            if cube_distance2(p)[0] >= MIN_D * MIN_D:
                out.append(p)
                break
    return out


def cube_distance2(p):
    """(squared distance in units^2, inside flag)."""
    inside = all(0 < k < U for k in p)
    if inside:
        m = min(min(k, U - k) for k in p)
        return m * m, True
    s = 0
    for k in p:
        if k < 0:
            s += k * k
        elif k > U:
            s += (k - U) * (k - U)
    if s == 0:  # on the boundary
        return 0, False
    return s, False


def cube_case(p, labels):
    face, edge, vertex = labels
    d2, inside = cube_distance2(p)
    if inside:
        dists = []
        for k in range(3):
            dists.append((p[k], face[(k, 0)]))
            dists.append((U - p[k], face[(k, 1)]))
        m = min(d for d, _ in dists)
        f = min(lab for d, lab in dists if d == m)
        return "i", "I", "-", f, metres(m)
    clamped = [k for k in range(3) if p[k] < 0 or p[k] > U]
    foot = tuple(min(max(c, 0), U) // U for c in p)  # 0 or 1 on clamped axes
    side = {k: (0 if p[k] < 0 else 1) for k in clamped}
    faces = sorted(face[(k, side[k])] for k in clamped)
    dist = metres(dec_sqrt(d2))
    if len(clamped) == 1:
        return "o", "I", "-", faces[0], dist
    if len(clamped) == 2:
        free = next(k for k in range(3) if k not in clamped)
        a = [0, 0, 0]
        b = [0, 0, 0]
        for k in clamped:
            a[k] = b[k] = side[k]
        a[free], b[free] = 0, 1
        lab = edge[tuple(sorted((tuple(a), tuple(b))))]
        return "o", "E", lab, faces[0], dist
    lab = vertex[foot]
    return "o", "V", lab, faces[0], dist


# ---------------------------------------------------------------- the tube (AC-2)


def tube_body():
    tau = 2.0 * math.pi
    ro, ri, h = R_OUT / U, R_IN / U, H / U

    def circle(z, r):
        return {"type": "circle", "origin": [0.0, 0.0, z], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0],
                "radius": r}

    def seam(r):
        return {"type": "line", "origin": [r, 0.0, 0.0], "direction": [0.0, 0.0, 1.0], "normal": [1.0, 0.0, 0.0]}

    def line2(o, d):
        return {"type": "line2", "origin": o, "dir": d}

    def cyl(r):
        return {"type": "cylinder", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0],
                "radius": r}

    def plane(z, nz):
        return {"type": "plane", "origin": [0.0, 0.0, z], "axis": [0.0, 0.0, nz], "ref_dir": [1.0, 0.0, 0.0]}

    vertices = [
        {"point": [ro, 0.0, 0.0], "label": "T-V1"},
        {"point": [ro, 0.0, h], "label": "T-V2"},
        {"point": [ri, 0.0, 0.0], "label": "T-V3"},
        {"point": [ri, 0.0, h], "label": "T-V4"},
    ]
    full = [0.0, tau]
    edges = [
        {"start": 0, "end": 0, "range": full, "curve": circle(0.0, ro), "label": "T-E1"},  # outer bottom
        {"start": 1, "end": 1, "range": full, "curve": circle(h, ro), "label": "T-E2"},  # outer top
        {"start": 2, "end": 2, "range": full, "curve": circle(0.0, ri), "label": "T-E3"},  # inner bottom
        {"start": 3, "end": 3, "range": full, "curve": circle(h, ri), "label": "T-E4"},  # inner top (rim)
        {"start": 0, "end": 1, "range": [0.0, h], "curve": seam(ro), "label": "T-E5"},  # outer seam
        {"start": 2, "end": 3, "range": [0.0, h], "curve": seam(ri), "label": "T-E6"},  # inner seam
    ]
    coedges = [
        # outer cylinder (sense same, normal outward): CCW in (u, v)
        {"edge": 0, "sense": "same", "pcurve": line2([0.0, 0.0], [1.0, 0.0]), "label": "T-C01"},
        {"edge": 4, "sense": "same", "pcurve": line2([tau, 0.0], [0.0, 1.0]), "label": "T-C02"},
        {"edge": 1, "sense": "reversed", "pcurve": line2([0.0, h], [1.0, 0.0]), "label": "T-C03"},
        {"edge": 4, "sense": "reversed", "pcurve": line2([0.0, 0.0], [0.0, 1.0]), "label": "T-C04"},
        # inner cylinder (sense reversed, outward normal towards the axis)
        {"edge": 5, "sense": "same", "pcurve": line2([0.0, 0.0], [0.0, 1.0]), "label": "T-C05"},
        {"edge": 3, "sense": "same", "pcurve": line2([0.0, h], [1.0, 0.0]), "label": "T-C06"},
        {"edge": 5, "sense": "reversed", "pcurve": line2([tau, 0.0], [0.0, 1.0]), "label": "T-C07"},
        {"edge": 2, "sense": "reversed", "pcurve": line2([0.0, 0.0], [1.0, 0.0]), "label": "T-C08"},
        # top annulus (normal +z): outer circle CCW, hole CW
        {"edge": 1, "sense": "same", "pcurve": None, "label": "T-C09"},
        {"edge": 3, "sense": "reversed", "pcurve": None, "label": "T-C10"},
        # bottom annulus (normal -z): outer circle CW about +z, hole CCW
        {"edge": 0, "sense": "reversed", "pcurve": None, "label": "T-C11"},
        {"edge": 2, "sense": "same", "pcurve": None, "label": "T-C12"},
    ]
    loops = [
        {"coedges": [0, 1, 2, 3], "outer": True, "label": "T-L1"},
        {"coedges": [4, 5, 6, 7], "outer": True, "label": "T-L2"},
        {"coedges": [8], "outer": True, "label": "T-L3"},
        {"coedges": [9], "outer": False, "label": "T-L4"},
        {"coedges": [10], "outer": True, "label": "T-L5"},
        {"coedges": [11], "outer": False, "label": "T-L6"},
    ]
    faces = [
        {"surface": cyl(ro), "sense": "same", "loops": [0], "label": "T-F1"},
        {"surface": cyl(ri), "sense": "reversed", "loops": [1], "label": "T-F2"},
        {"surface": plane(h, 1.0), "sense": "same", "loops": [2, 3], "label": "T-F3"},
        {"surface": plane(0.0, -1.0), "sense": "same", "loops": [4, 5], "label": "T-F4"},
    ]
    return {
        "description": "invented tube: solid cylinder r 2^-6 m, height 2^-7 m, coaxial through hole r 2^-7 m "
        "(GEO.DIST.FACE.INCID AC-2; lines are origin + t direction, normal perpendicular)",
        "license": "CC-BY-4.0",
        "generator": "fixtures/geo/dist/face/incid/make_incid_cases.py",
        "body_id": "54554245494e434944000000000000aa",
        "vertices": vertices,
        "edges": edges,
        "coedges": coedges,
        "loops": loops,
        "faces": faces,
        "shells": [{"faces": [0, 1, 2, 3], "closed": True}],
    }


def tube_near(p, zc):
    """Exact integer test: within about MIN_D of the rim circle's cylinder or plane (skipped)."""
    x, y, z = p
    return abs(x * x + y * y - R_IN * R_IN) < 2 * R_IN * MIN_D or abs(z - zc) < MIN_D


def tube_points():
    g = SplitMix(SEED_TUBE)
    out = []
    for n in range(N_TUBE):
        zc = H if n % 2 == 0 else 0
        while True:
            x0 = g.below(2 * R_IN + 1) - R_IN
            y0 = isqrt(R_IN * R_IN - x0 * x0)
            if g.below(2) == 1:
                y0 = -y0
            e = g.below(12)
            de = 1 << (21 - e)
            p = [x0 + g.below(2 * de + 1) - de, y0 + g.below(2 * de + 1) - de, zc + g.below(2 * de + 1) - de]
            if not tube_near(p, zc):
                out.append((p, tube_case(p)))
                break
    return out


def tube_case(p):
    """(label, kind, entity, face, distance) of a point kept by ``tube_points``."""
    x, y, z = p
    r2 = x * x + y * y
    rho = dec_sqrt(r2)
    inside = R_IN * R_IN < r2 < R_OUT * R_OUT and 0 < z < H
    cands = []  # (distance in units, rank, face label, kind, entity)
    if 0 < z < H:
        cands.append((abs(rho - R_OUT), 0, "T-F1", "I", "-"))
        cands.append((abs(rho - R_IN), 0, "T-F2", "I", "-"))
    if R_IN * R_IN < r2 < R_OUT * R_OUT:
        cands.append((Decimal(abs(z - H)), 0, "T-F3", "I", "-"))
        cands.append((Decimal(abs(z)), 0, "T-F4", "I", "-"))
    for zc, r, faces, lab in ((0, R_OUT, ("T-F1", "T-F4"), "T-E1"), (H, R_OUT, ("T-F1", "T-F3"), "T-E2"),
                              (0, R_IN, ("T-F2", "T-F4"), "T-E3"), (H, R_IN, ("T-F2", "T-F3"), "T-E4")):
        d = ((rho - r) * (rho - r) + Decimal((z - zc) * (z - zc))).sqrt()
        cands.append((d, 1, min(faces), "E", lab))
    best = min(c[0] for c in cands)
    assert best > MIN_D / 2, p
    tie = Decimal(10) ** -40
    near = [c for c in cands if c[0] - best <= tie]
    near.sort(key=lambda c: (c[1], c[2], c[4]))
    d, _, face, kind, ent = near[0]
    return ("i" if inside else "o", kind, ent, face, metres(d))


# ---------------------------------------------------------------- the L-block (concave edges and corners)

N_LBLOCK = 4_000
SEED_LBLOCK = 37
# L cross-section in grid units (counter-clockwise), extruded over z in [0, U].
L_POLY = [(0, 0), (2 * U, 0), (2 * U, U), (U, U), (U, 2 * U), (0, 2 * U)]


def lblock_model():
    """Vertices (ints), faces [(label, axis, side, polygon vertex indices CCW about the outward
    normal)], edges [(label, a, b)] in label order, each edge's faces."""
    n = len(L_POLY)
    verts = [(x, y, 0) for x, y in L_POLY] + [(x, y, U) for x, y in L_POLY]
    faces = [("L-F1", (0, 0, -1), list(reversed(range(n)))), ("L-F2", (0, 0, 1), list(range(n, 2 * n)))]
    for i in range(n):
        j = (i + 1) % n
        (x0, y0), (x1, y1) = L_POLY[i], L_POLY[j]
        d = (x1 - x0, y1 - y0)
        normal = (int(math.copysign(1, d[1])) if d[1] else 0, int(math.copysign(1, -d[0])) if d[0] else 0, 0)
        faces.append((f"L-F{3 + i}", normal, [i, j, n + j, n + i]))
    pairs = []
    for _, _, poly in faces:
        for k in range(len(poly)):
            a, b = poly[k], poly[(k + 1) % len(poly)]
            key = (min(a, b), max(a, b))
            if key not in pairs:
                pairs.append(key)
    pairs.sort()
    edges = [(f"L-E{k + 1:02d}", a, b) for k, (a, b) in enumerate(pairs)]
    return verts, faces, edges


def lblock_body():
    verts, faces, edges = lblock_model()
    f = lambda k: k / U  # noqa: E731
    vjson = [{"point": [f(c) for c in v], "label": f"L-V{i + 1:02d}"} for i, v in enumerate(verts)]
    ejson = []
    index = {}
    for k, (lab, a, b) in enumerate(edges):
        va, vb = verts[a], verts[b]
        d = [vb[i] - va[i] for i in range(3)]
        length = max(abs(c) for c in d)
        direction = [c // length for c in d]
        normal = [0, 0, 1] if direction[2] == 0 else [1, 0, 0]
        ejson.append({"start": a, "end": b, "range": [0.0, f(length)],
                      "curve": {"type": "line", "origin": [f(c) for c in va], "direction": [float(c) for c in direction],
                                "normal": [float(c) for c in normal]}, "label": lab})
        index[(a, b)] = k
    cjson, ljson, fjson = [], [], []
    for lab, normal, poly in faces:
        cos = []
        for k in range(len(poly)):
            a, b = poly[k], poly[(k + 1) % len(poly)]
            e = index[(min(a, b), max(a, b))]
            cos.append(len(cjson))
            cjson.append({"edge": e, "sense": "same" if a < b else "reversed", "pcurve": None})
        ljson.append({"coedges": cos, "outer": True})
        o = verts[poly[0]]
        d = [verts[poly[1]][i] - o[i] for i in range(3)]
        m = max(abs(c) for c in d)
        fjson.append({"surface": {"type": "plane", "origin": [f(c) for c in o], "axis": [float(c) for c in normal],
                                  "ref_dir": [float(c // m) for c in d]},
                      "sense": "same", "loops": [len(ljson) - 1], "label": lab})
    return {
        "description": "invented L-block: L-shaped prism (legs 2 m x 1 m, height 1 m) with a concave vertical edge "
        "and two concave corners (GEO.DIST.FACE.INCID review); lines are origin + t direction",
        "license": "CC-BY-4.0",
        "generator": "fixtures/geo/dist/face/incid/make_incid_cases.py",
        "body_id": "4c424c4f434b494e4349440000000cc0",
        "vertices": vjson, "edges": ejson, "coedges": cjson, "loops": ljson, "faces": fjson,
        "shells": [{"faces": list(range(len(fjson))), "closed": True}],
    }


def in_poly2(x, y, poly):
    """Strictly inside a simple polygon (exact integers; on an edge is not inside)."""
    n = len(poly)
    for k in range(n):
        (x0, y0), (x1, y1) = poly[k], poly[(k + 1) % n]
        cross = (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0)
        if cross == 0 and min(x0, x1) <= x <= max(x0, x1) and min(y0, y1) <= y <= max(y0, y1):
            return False
    inside = False
    for k in range(n):
        (x0, y0), (x1, y1) = poly[k], poly[(k + 1) % n]
        if (y0 > y) != (y1 > y):
            xi = Fraction(x1 - x0) * (y - y0) / (y1 - y0) + x0
            if x < xi:
                inside = not inside
    return inside


def lblock_points():
    verts, _, edges = lblock_model()
    g = SplitMix(SEED_LBLOCK)
    out = []
    for _ in range(N_LBLOCK):
        pick = g.below(len(edges) + len(verts))
        if pick < len(edges):
            _, a, b = edges[pick]
            va, vb = verts[a], verts[b]
            d = [vb[i] - va[i] for i in range(3)]
            length = max(abs(c) for c in d)
            k = g.below(length + 1)
            base = [va[i] + (d[i] // length) * k for i in range(3)]
        else:
            base = list(verts[pick - len(edges)])
        e = g.below(14)
        de = 1 << (23 - e)
        out.append([base[i] + g.below(2 * de + 1) - de for i in range(3)])
    return out


def lblock_case(p):
    verts, faces, edges = lblock_model()
    x, y, z = p
    inside = 0 < z < U and in_poly2(x, y, L_POLY)
    cands = []  # (d2 Fraction, rank, face label, kind, entity)
    edge_faces = {}
    vertex_faces = {}
    for lab, normal, poly in faces:
        for k in range(len(poly)):
            a, b = poly[k], poly[(k + 1) % len(poly)]
            edge_faces.setdefault((min(a, b), max(a, b)), []).append(lab)
            vertex_faces.setdefault(a, []).append(lab)
        k = next(i for i in range(3) if normal[i] != 0)
        plane = verts[poly[0]][k]
        others = [i for i in range(3) if i != k]
        poly2 = [(verts[v][others[0]], verts[v][others[1]]) for v in poly]
        if in_poly2(p[others[0]], p[others[1]], poly2):
            cands.append((Fraction((p[k] - plane) ** 2), 0, lab, "I", "-"))
    for lab, a, b in edges:
        va, vb = verts[a], verts[b]
        d = [vb[i] - va[i] for i in range(3)]
        w = [p[i] - va[i] for i in range(3)]
        t = Fraction(sum(w[i] * d[i] for i in range(3)), sum(c * c for c in d))
        if 0 < t < 1:
            r = [w[i] - t * d[i] for i in range(3)]
            cands.append((sum(c * c for c in r), 1, min(edge_faces[(a, b)]), "E", lab))
    for i, v in enumerate(verts):
        cands.append((Fraction(sum((p[k] - v[k]) ** 2 for k in range(3))), 2, min(vertex_faces[i]), "V", f"L-V{i + 1:02d}"))
    best = min(c[0] for c in cands)
    if best < MIN_D * MIN_D:
        return None
    near = sorted((c for c in cands if c[0] == best), key=lambda c: (c[1], c[2], c[4]))
    d2, _, face, kind, ent = near[0]
    dist = metres((Decimal(d2.numerator) / Decimal(d2.denominator)).sqrt())
    return ("i" if inside else "o", kind, ent, face, dist)


# ---------------------------------------------------------------- cone apexes


def dec_sin_cos(x):
    """sin and cos of a Decimal by their Taylor series (|x| < 1, 60 digits)."""
    s, c = Decimal(0), Decimal(0)
    term = Decimal(1)  # x^n / n!
    for n in range(0, 80):
        if n > 0:
            term = term * x / n
        r = n % 4
        if r == 0:
            c += term
        elif r == 1:
            s += term
        elif r == 2:
            c -= term
        else:
            s -= term
    return s, c


class Cone:
    """A solid cone: apex `apex`, unit axis `w` from the apex into the solid, half-angle `a`
    (the binary64 value, taken exactly), height `hc` from the apex to the base plane."""

    def __init__(self, name, r0, half_angle, z_origin, axis_z, hc_override=None):
        self.name = name
        self.r0 = r0
        self.a = half_angle
        s, c = dec_sin_cos(Decimal(half_angle))
        self.sin, self.cos, self.tan = s, c, s / c
        self.axis_z = axis_z  # +1 or -1: the placement axis Z along +z or -z
        # Placement origin O = (0, 0, z_origin); apex at v_a = -r0 / tan a along Z.
        va = -Decimal(r0) / self.tan
        self.apex_z = Decimal(z_origin) + axis_z * va
        self.w = axis_z  # v increases from the apex into the solid along Z
        self.hc = Decimal(hc_override) if hc_override is not None else -va
        self.rb = self.hc * self.tan
        self.z_origin = z_origin

    def case(self, p):
        """(label, kind, entity, face, distance, foot) of a query p (binary64 floats)."""
        px, py, pz = (Decimal(c) for c in p)
        s = (pz - self.apex_z) * self.w
        rho = (px * px + py * py).sqrt()
        inside = 0 < s < self.hc and rho < s * self.tan
        cands = []
        t = rho * self.sin + s * self.cos
        if 0 < t < self.hc / self.cos:
            cands.append((abs(rho * self.cos - s * self.sin), 0, f"{self.name}-F1", "I", "side"))
        cands.append(((rho * rho + s * s).sqrt(), 0, f"{self.name}-F1", "A", "apex"))
        if rho < self.rb:
            cands.append((abs(s - self.hc), 0, f"{self.name}-F2", "I", "base"))
        cands.append((((rho - self.rb) ** 2 + (s - self.hc) ** 2).sqrt(), 1, f"{self.name}-F1", "E", "circle"))
        best = min(c[0] for c in cands)
        if best * U < MIN_D:
            return None
        tie = Decimal(10) ** -40
        near = sorted((c for c in cands if c[0] - best <= tie), key=lambda c: (c[1], c[2], c[3]))
        d, _, face, kind, what = near[0]
        rx, ry = (px / rho, py / rho) if rho > 0 else (Decimal(1), Decimal(0))
        if what == "side":
            radial, axial = t * self.sin, t * self.cos
        elif what == "apex":
            radial, axial = Decimal(0), Decimal(0)
        elif what == "base":
            radial, axial = rho, self.hc
        else:
            radial, axial = self.rb, self.hc
        foot = (float(radial * rx), float(radial * ry), float(self.apex_z + self.w * axial))
        ent = f"{self.name}-E1" if kind == "E" else "-"
        return ("i" if inside else "o", kind, ent, face, float(d), foot)

    def body(self):
        tau = 2.0 * math.pi
        a = self.a
        apex = [0.0, 0.0, float(self.apex_z)]
        base_z = float(self.apex_z + self.w * self.hc)
        rb = float(self.rb)
        vb = [rb, 0.0, base_z]
        slant = float(self.hc / self.cos)
        cos_a = float(self.cos)
        # v of the base circle in the placement (O at z_origin along Z).
        v_base = (base_z - self.z_origin) * self.axis_z
        down = [apex[k] - vb[k] for k in range(3)]
        norm = math.sqrt(sum(c * c for c in down))
        direction = [c / norm for c in down]
        axis = [0.0, 0.0, float(self.axis_z)]
        return {
            "description": f"invented solid cone {self.name}: r0 {self.r0}, half-angle {a!r} rad, apex at the top of "
            "the placement's nappe, planar base (GEO.DIST.FACE.INCID review: signs at a cone apex)",
            "license": "CC-BY-4.0",
            "generator": "fixtures/geo/dist/face/incid/make_incid_cases.py",
            "body_id": "434f4e45" + self.name.encode().hex().ljust(24, "0"),
            "vertices": [{"point": apex, "label": f"{self.name}-V1"}, {"point": vb, "label": f"{self.name}-V2"}],
            "edges": [
                {"start": 1, "end": 1, "range": [0.0, tau], "curve": {"type": "circle", "origin": [0.0, 0.0, base_z],
                 "axis": axis, "ref_dir": [1.0, 0.0, 0.0], "radius": rb}, "label": f"{self.name}-E1"},
                {"start": 1, "end": 0, "range": [0.0, slant], "curve": {"type": "line", "origin": vb,
                 "direction": direction, "normal": [0.0, 1.0, 0.0]}, "label": f"{self.name}-E2"},
            ],
            "coedges": [
                {"edge": 1, "sense": "reversed", "pcurve": {"type": "line2", "origin": [tau, v_base],
                                                            "dir": [0.0, -cos_a]}},
                {"edge": 0, "sense": "reversed", "pcurve": {"type": "line2", "origin": [0.0, v_base],
                                                            "dir": [1.0, 0.0]}},
                {"edge": 1, "sense": "same", "pcurve": {"type": "line2", "origin": [0.0, v_base],
                                                        "dir": [0.0, -cos_a]}},
                {"edge": 0, "sense": "same", "pcurve": None},
            ],
            "loops": [{"coedges": [0, 1, 2], "outer": True}, {"coedges": [3], "outer": True}],
            "faces": [
                {"surface": {"type": "cone", "origin": [0.0, 0.0, float(self.z_origin)], "axis": axis,
                             "ref_dir": [1.0, 0.0, 0.0], "radius": float(self.r0), "half_angle": a},
                 "sense": "same", "loops": [0], "label": f"{self.name}-F1"},
                {"surface": {"type": "plane", "origin": [0.0, 0.0, base_z], "axis": axis, "ref_dir": [1.0, 0.0, 0.0]},
                 "sense": "same", "loops": [1], "label": f"{self.name}-F2"},
            ],
            "shells": [{"faces": [0, 1], "closed": True}],
        }


# Cone C30: apex at the origin, r0 = 0, half-angle 30 degrees, solid below the apex (axis -z),
# height 2^-5 m. Cone C20: r0 = 3 m at z = 0, half-angle 20 degrees, axis +z, apex below.
CONES = [
    (Cone("C30", 0.0, math.radians(30.0), 0.0, -1, hc_override=Decimal("0.03125")), 41, 400, 23),
    (Cone("C20", 3.0, math.radians(20.0), 0.0, 1), 43, 400, 27),
]


def cone_points(cone, seed, n, top):
    """Points around the apex: every eighth straight beyond the tip on the axis, the others
    offset from the apex by up to 2^(top - e) grid units."""
    g = SplitMix(seed)
    az = float(cone.apex_z)
    out = []
    for i in range(n):
        if i % 8 == 0:
            k = 3 + g.below(18)
            out.append([0.0, 0.0, az - cone.w * 2.0 ** -k])
            continue
        e = g.below(14)
        de = 1 << (top - e)
        o = [g.below(2 * de + 1) - de for _ in range(3)]
        out.append([o[0] / U, o[1] / U, az + o[2] / U])
    return out


# ---------------------------------------------------------------- output


def digest(points, scale=U):
    h = hashlib.sha256()
    for p in points:
        for k in p:
            h.update(struct.pack("<d", k / scale))
    return h.hexdigest()


def render():
    labels = cube_labels()
    cube = cube_points()
    tube = tube_points()
    lines = [
        "# GEO.DIST.FACE.INCID AC-1 and AC-2 (CC-BY-4.0). Written by make_incid_cases.py; do not edit.",
        "# Points: the pinned SplitMix64 streams of the script (grid 2^-30 m), mirrored in",
        "# crates/fid-geo-dist/tests/face_incid.rs. Line: label (o outside, i inside the solid),",
        "# closest feature kind (I interior, E edge, V vertex), feature label (- for interior),",
        "# face label (lowest of the faces at an edge or vertex), exact distance in metres",
        "# rounded once to binary64. `z`: a point within 2^-26 m of the surface, skipped. Cone lines",
        "# add the foot (kind A: the apex, where the sign is undefined by rule).",
        f"cube seed {SEED_CUBE} count {N_CUBE} points_sha256 {digest(cube)}",
    ]
    for p in cube:
        lab, kind, ent, face, d = cube_case(p, labels)
        lines.append(f"{lab} {kind} {ent} {face} {d!r}")
    lines.append(f"tube seed {SEED_TUBE} count {N_TUBE} points_sha256 {digest([p for p, _ in tube])}")
    for _, (lab, kind, ent, face, d) in tube:
        lines.append(f"{lab} {kind} {ent} {face} {d!r}")
    lpts = lblock_points()
    lines.append(f"lblock seed {SEED_LBLOCK} count {N_LBLOCK} points_sha256 {digest(lpts)}")
    for p in lpts:
        c = lblock_case(p)
        lines.append("z" if c is None else "{} {} {} {} {!r}".format(*c))
    out = {}
    for cone, seed, n, top in CONES:
        pts = cone_points(cone, seed, n, top)
        az = float(cone.apex_z)
        lines.append(f"cone {cone.name} seed {seed} count {n} top {top} apex_z {az!r} points_sha256 {digest(pts, 1)}")
        for p in pts:
            c = cone.case(p)
            if c is None:
                lines.append("z")
            else:
                lab, kind, ent, face, d, foot = c
                lines.append(f"{lab} {kind} {ent} {face} {d!r} {foot[0]!r} {foot[1]!r} {foot[2]!r}")
        out[HERE / f"cone_{cone.name.lower()}.json"] = json.dumps(cone.body(), indent=1) + "\n"
    out[OUT_CASES] = "\n".join(lines) + "\n"
    out[OUT_TUBE] = json.dumps(tube_body(), indent=1) + "\n"
    out[HERE / "lblock.json"] = json.dumps(lblock_body(), indent=1) + "\n"
    return out


def main():
    check = "--check" in sys.argv[1:]
    bad = False
    for out, text in render().items():
        if check:
            old = out.read_text(encoding="utf-8") if out.exists() else None
            if old != text:
                print(f"{out.relative_to(ROOT)} is stale; run {Path(__file__).name}")
                bad = True
        else:
            out.write_text(text, encoding="utf-8", newline="\n")
            print(f"wrote {out.relative_to(ROOT)}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
