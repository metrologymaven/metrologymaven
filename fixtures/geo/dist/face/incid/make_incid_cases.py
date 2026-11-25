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


# ---------------------------------------------------------------- output


def digest(points):
    h = hashlib.sha256()
    for p in points:
        for k in p:
            h.update(struct.pack("<d", k / U))
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
        "# rounded once to binary64.",
        f"cube seed {SEED_CUBE} count {N_CUBE} points_sha256 {digest(cube)}",
    ]
    for p in cube:
        lab, kind, ent, face, d = cube_case(p, labels)
        lines.append(f"{lab} {kind} {ent} {face} {d!r}")
    lines.append(f"tube seed {SEED_TUBE} count {N_TUBE} points_sha256 {digest([p for p, _ in tube])}")
    for _, (lab, kind, ent, face, d) in tube:
        lines.append(f"{lab} {kind} {ent} {face} {d!r}")
    cases = "\n".join(lines) + "\n"
    tube_json = json.dumps(tube_body(), indent=1) + "\n"
    return {OUT_CASES: cases, OUT_TUBE: tube_json}


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
