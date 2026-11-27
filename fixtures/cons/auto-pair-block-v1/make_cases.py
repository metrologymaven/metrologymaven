#!/usr/bin/env python3
"""Independent reference for KRN.CONS.AUTO.PAIR (fixture auto-pair-block-v1, CC-BY-4.0).

Synthetic data, not a measurement. A 100 x 60 x 40 mm block with six planes, two through
holes (cylinders) and one hemispherical pocket (sphere), sampled with 200 000 seeded points
with unit normals and 5 micrometre noise along the normal. The point set is not committed:
the Rust test regenerates it from the generator below, which uses only +, -, *, / and sqrt
(correctly rounded in both languages), so both sides hold bit-identical points.

This script regenerates the points, pairs them with its own implementation of the spec's
rules (spec/krn/cons/auto/pair/NODE.md: distance and normal-angle gates, nearest /
exclude_ambiguous conflict rules, min_points), and writes:

  expected.tsv   per-element counts and canonical-encoding digests of each run
  edge_band.tsv  the points within 0.05 mm of two or more planes, with the element the
                 `nearest` rule gives each when pairing without normals

Run:  python fixtures/cons/auto-pair-block-v1/make_cases.py           (write)
      python fixtures/cons/auto-pair-block-v1/make_cases.py --check   (compare)

Generator (shared with crates/fid-construct/tests/auto_pair.rs):
  rng       splitmix64, seed SEED; u() = (next() >> 11) * 2^-53
  noise     eps = (2u() - 1) * NOISE, drawn last for each accepted point
  planes    face coordinates in the order listed below (first, second), each = extent * u();
            bottom and top reject (x - cx)^2 + (y - cy)^2 <= r^2 for each hole (and the pocket
            for top); point = q + n * eps per component; normal = n
  cylinder  a = 2u() - 1, b = 2u() - 1, s = a*a + b*b, reject s > 1 or s < 1e-4;
            r = sqrt(s), d = (a / r, b / r); z = H * u();
            point = (cx + (R + eps) * dx, cy + (R + eps) * dy, z); normal = (-dx, -dy, 0)
  sphere    a, b, c = 2u() - 1, s = (a*a + b*b) + c*c, reject s > 1 or s < 1e-4;
            r = sqrt(s), d = (a / r, b / r, c / r); dz > 0 -> dz = -dz; dz == 0 -> reject;
            point = centre + (R + eps) * d per component; normal = -d
"""

import hashlib
import math
import struct
import sys
from pathlib import Path

SEED = 20261127
MASK = (1 << 64) - 1
NOISE = 5e-6
L, W, H = 0.1, 0.06, 0.04
GATE = 1e-4
GATE_ANGLE = 10.0 * math.pi / 180.0
EDGE_BAND = 5e-5
MIN_AC1 = 100
MIN_AC3 = 1000

# (cx, cy, R) of the holes and the pocket (pocket centre on the top plane).
CYL1 = (0.03, 0.03, 0.008)
CYL2 = (0.07, 0.02, 0.003)
POCKET = (0.075, 0.042, 0.01)

# Elements in order: kind, data. Plane: origin, normal. Cylinder: axis point, axis, R, sense.
# Sphere: centre, R, sense. sense -1 = inward (normal towards the axis / centre).
ELEMENTS = [
    ("plane", (0.0, 0.0, 0.0), (0.0, 0.0, -1.0)),
    ("plane", (0.0, 0.0, H), (0.0, 0.0, 1.0)),
    ("plane", (0.0, 0.0, 0.0), (-1.0, 0.0, 0.0)),
    ("plane", (L, 0.0, 0.0), (1.0, 0.0, 0.0)),
    ("plane", (0.0, 0.0, 0.0), (0.0, -1.0, 0.0)),
    ("plane", (0.0, W, 0.0), (0.0, 1.0, 0.0)),
    ("cylinder", (CYL1[0], CYL1[1], 0.0), (0.0, 0.0, 1.0), CYL1[2], -1.0),
    ("cylinder", (CYL2[0], CYL2[1], 0.0), (0.0, 0.0, 1.0), CYL2[2], -1.0),
    ("sphere", (POCKET[0], POCKET[1], H), POCKET[2], -1.0),
]
COUNTS = [30000, 30000, 15000, 15000, 20000, 20000, 30000, 400, 39600]


class Rng:
    def __init__(self, seed):
        self.state = seed & MASK

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def u(self):
        return (self.next() >> 11) * 2.0 ** -53


def in_disc(x, y, disc):
    cx, cy, r = disc
    dx = x - cx
    dy = y - cy
    return dx * dx + dy * dy <= r * r


def generate():
    """Returns (points, normals, labels) as lists of 3-tuples and ints."""
    rng = Rng(SEED)
    pts, nms, labels = [], [], []
    for e, count in enumerate(COUNTS):
        el = ELEMENTS[e]
        k = 0
        while k < count:
            if el[0] == "plane":
                n = el[2]
                if e in (0, 1):
                    x = L * rng.u()
                    y = W * rng.u()
                    discs = [CYL1, CYL2] + ([POCKET] if e == 1 else [])
                    if any(in_disc(x, y, d) for d in discs):
                        continue
                    q = (x, y, el[1][2])
                elif e in (2, 3):
                    y = W * rng.u()
                    z = H * rng.u()
                    q = (el[1][0], y, z)
                else:
                    x = L * rng.u()
                    z = H * rng.u()
                    q = (x, el[1][1], z)
                eps = (2.0 * rng.u() - 1.0) * NOISE
                pts.append((q[0] + n[0] * eps, q[1] + n[1] * eps, q[2] + n[2] * eps))
                nms.append(n)
            elif el[0] == "cylinder":
                a = 2.0 * rng.u() - 1.0
                b = 2.0 * rng.u() - 1.0
                s = a * a + b * b
                if s > 1.0 or s < 1e-4:
                    continue
                r = math.sqrt(s)
                dx, dy = a / r, b / r
                z = H * rng.u()
                eps = (2.0 * rng.u() - 1.0) * NOISE
                o, radius = el[1], el[3]
                pts.append((o[0] + (radius + eps) * dx, o[1] + (radius + eps) * dy, z))
                nms.append((-dx, -dy, 0.0))
            else:
                a = 2.0 * rng.u() - 1.0
                b = 2.0 * rng.u() - 1.0
                c = 2.0 * rng.u() - 1.0
                s = (a * a + b * b) + c * c
                if s > 1.0 or s < 1e-4:
                    continue
                r = math.sqrt(s)
                d = [a / r, b / r, c / r]
                if d[2] > 0.0:
                    d[2] = -d[2]
                if d[2] == 0.0:
                    continue
                eps = (2.0 * rng.u() - 1.0) * NOISE
                cen, radius = el[1], el[2]
                rr = radius + eps
                pts.append((cen[0] + rr * d[0], cen[1] + rr * d[1], cen[2] + rr * d[2]))
                nms.append((-d[0], -d[1], -d[2]))
            labels.append(e)
            k += 1
    return pts, nms, labels


def foot(el, p):
    """(distance, nominal normal or None) of point p to element el."""
    if el[0] == "plane":
        o, n = el[1], el[2]
        v = (p[0] - o[0], p[1] - o[1], p[2] - o[2])
        return abs(v[0] * n[0] + v[1] * n[1] + v[2] * n[2]), n
    if el[0] == "cylinder":
        o, a, radius, sense = el[1], el[2], el[3], el[4]
        v = (p[0] - o[0], p[1] - o[1], p[2] - o[2])
        t = v[0] * a[0] + v[1] * a[1] + v[2] * a[2]
        w = (v[0] - t * a[0], v[1] - t * a[1], v[2] - t * a[2])
        rho = math.sqrt(w[0] * w[0] + w[1] * w[1] + w[2] * w[2])
        nn = None if rho == 0.0 else (sense * w[0] / rho, sense * w[1] / rho, sense * w[2] / rho)
        return abs(rho - radius), nn
    cen, radius, sense = el[1], el[2], el[3]
    v = (p[0] - cen[0], p[1] - cen[1], p[2] - cen[2])
    d = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
    nn = None if d == 0.0 else (sense * v[0] / d, sense * v[1] / d, sense * v[2] / d)
    return abs(d - radius), nn


def angle(pn, nn):
    cx = pn[1] * nn[2] - pn[2] * nn[1]
    cy = pn[2] * nn[0] - pn[0] * nn[2]
    cz = pn[0] * nn[1] - pn[1] * nn[0]
    return math.atan2(math.sqrt(cx * cx + cy * cy + cz * cz), pn[0] * nn[0] + pn[1] * nn[1] + pn[2] * nn[2])


def claims(pts, nms):
    """Per point: (list of (dist, angle, elem)), undefined flag."""
    out = []
    for i, p in enumerate(pts):
        cl, undefined = [], False
        for e, el in enumerate(ELEMENTS):
            dist, nn = foot(el, p)
            if dist > GATE:
                continue
            if nms is None:
                ang = 0.0
            elif nn is None:
                undefined = True
                continue
            else:
                ang = angle(nms[i], nn)
                if ang > GATE_ANGLE:
                    continue
            cl.append((dist, ang, e))
        out.append((cl, undefined))
    return out


def resolve(table, rule, min_points):
    """The pairing as a dict mirroring the spec's Pairing."""
    won = [[] for _ in ELEMENTS]
    ambiguous, contested, tie_broken, undefined, ungated = [], [], [], [], 0
    for i, (cl, und) in enumerate(table):
        if und:
            undefined.append(i)
        elif not cl:
            ungated += 1
        elif len(cl) >= 2 and rule == "exclude_ambiguous":
            ambiguous.append(i)
        else:
            best = min(cl)  # (dist, angle, elem): the spec's lexicographic order
            won[best[2]].append(i)
            if len(cl) >= 2:
                contested.append(i)
                if sum(1 for c in cl if (c[0], c[1]) == (best[0], best[1])) >= 2:
                    tie_broken.append(i)
    elements = []
    for lst in won:
        if len(lst) < min_points:
            elements.append({"count": len(lst), "indices": [], "released": lst, "status": "unpaired"})
        else:
            elements.append({"count": len(lst), "indices": lst, "released": [], "status": "paired"})
    return {"points": len(table), "elements": elements, "ambiguous": ambiguous, "contested": contested,
            "tie_broken": tie_broken, "normal_undefined": undefined, "ungated": ungated}


def hexf(x):
    return struct.pack(">d", x).hex()


def params_digest(rule, min_points):
    rule_v = 0.0 if rule == "nearest" else 1.0
    s = ('{"angular_threshold":"%s","distance_threshold":"%s","format":"construct.params@1",'
         '"values":[{"kind":"count","name":"conflict_rule","unit":"1","value":"%s"},'
         '{"kind":"count","name":"min_points","unit":"1","value":"%s"}]}'
         % (hexf(GATE_ANGLE), hexf(GATE), hexf(rule_v), hexf(float(min_points))))
    return "sha256:" + hashlib.sha256(s.encode("ascii")).hexdigest()


def ints(xs):
    return "[" + ",".join('"%d"' % x for x in xs) + "]"


def pairing_digest(pr, rule, min_points):
    els = ",".join('{"count":"%d","indices":%s,"released":%s,"status":"%s"}'
                   % (e["count"], ints(e["indices"]), ints(e["released"]), e["status"])
                   for e in pr["elements"])
    s = ('{"ambiguous":%s,"contested":%s,"elements":[%s],"format":"construct.auto_pair.pairing@1",'
         '"normal_undefined":%s,"params_digest":"%s","points":"%d","tie_broken":%s,"ungated":"%d"}'
         % (ints(pr["ambiguous"]), ints(pr["contested"]), els, ints(pr["normal_undefined"]),
            params_digest(rule, min_points), pr["points"], ints(pr["tie_broken"]), pr["ungated"]))
    return "sha256:" + hashlib.sha256(s.encode("ascii")).hexdigest()


def build():
    pts, nms, labels = generate()
    assert len(pts) == sum(COUNTS) == 200000

    # AC-1: with normals, every point goes to its generating element.
    table = claims(pts, nms)
    ac1 = resolve(table, "nearest", MIN_AC1)
    for e, el in enumerate(ac1["elements"]):
        assert el["status"] == "paired" and el["indices"] == [i for i, lb in enumerate(labels) if lb == e], e
    assert not ac1["ambiguous"] and not ac1["normal_undefined"] and ac1["ungated"] == 0
    # AC-3: min_points 1000 leaves the small hole unpaired with its 400 points.
    ac3 = resolve(table, "nearest", MIN_AC3)
    assert ac3["elements"][7]["status"] == "unpaired" and ac3["elements"][7]["count"] == 400
    assert all(el["status"] == "paired" for e, el in enumerate(ac3["elements"]) if e != 7)
    ac1_excl = resolve(table, "exclude_ambiguous", MIN_AC1)

    # AC-2: the edge band, paired without normals.
    band = []
    for i, p in enumerate(pts):
        near = sum(1 for e in range(6) if foot(ELEMENTS[e], p)[0] <= EDGE_BAND)
        if near >= 2:
            band.append(i)
    sub = [pts[i] for i in band]
    sub_table = claims(sub, None)
    edge_near = resolve(sub_table, "nearest", 1)
    edge_excl = resolve(sub_table, "exclude_ambiguous", 1)
    assert len(edge_excl["ambiguous"]) == len(band) and edge_excl["ungated"] == 0
    nearest_of = {}
    for e, el in enumerate(edge_near["elements"]):
        for j in el["indices"] + el["released"]:
            nearest_of[j] = e
    assert sorted(nearest_of) == list(range(len(band)))

    lines = [
        "# KRN.CONS.AUTO.PAIR fixture auto-pair-block-v1 (synthetic, CC-BY-4.0).",
        "# Written by make_cases.py from its own generator and pairing; do not edit.",
        "# key\tvalue",
        "seed\t%d" % SEED,
        "points\t%d" % len(pts),
        "gate_m\t%s" % hexf(GATE),
        "gate_rad\t%s" % hexf(GATE_ANGLE),
    ]
    for e, el in enumerate(ac1["elements"]):
        lines.append("ac1.count.%d\t%d" % (e, el["count"]))
    lines += [
        "ac1.params_digest\t%s" % params_digest("nearest", MIN_AC1),
        "ac1.contested\t%d" % len(ac1["contested"]),
        "ac1.digest\t%s" % pairing_digest(ac1, "nearest", MIN_AC1),
        "ac1_exclude.ambiguous\t%d" % len(ac1_excl["ambiguous"]),
        "ac1_exclude.digest\t%s" % pairing_digest(ac1_excl, "exclude_ambiguous", MIN_AC1),
        "ac3.unpaired.7\t%d" % ac3["elements"][7]["count"],
        "ac3.digest\t%s" % pairing_digest(ac3, "nearest", MIN_AC3),
        "edge.points\t%d" % len(band),
        "edge.nearest.contested\t%d" % len(edge_near["contested"]),
        "edge.nearest.tie_broken\t%d" % len(edge_near["tie_broken"]),
        "edge.nearest.digest\t%s" % pairing_digest(edge_near, "nearest", 1),
        "edge.exclude.digest\t%s" % pairing_digest(edge_excl, "exclude_ambiguous", 1),
    ]
    expected = "\n".join(lines) + "\n"
    edge = ["# KRN.CONS.AUTO.PAIR auto-pair-block-v1 edge band (points within 0.05 mm of two planes).",
            "# Written by make_cases.py; do not edit.",
            "# point_index\tnearest_element (pairing without normals, gate 0.1 mm)"]
    edge += ["%d\t%d" % (band[j], nearest_of[j]) for j in range(len(band))]
    return {"expected.tsv": expected, "edge_band.tsv": "\n".join(edge) + "\n"}


def main():
    here = Path(__file__).parent
    files = build()
    if "--check" in sys.argv[1:]:
        bad = [name for name, text in files.items()
               if not (here / name).exists() or (here / name).read_text(encoding="utf-8") != text]
        if bad:
            print("auto-pair-block-v1: out of date: " + ", ".join(bad))
            sys.exit(1)
        print("auto-pair-block-v1: ok")
        return
    for name, text in files.items():
        (here / name).write_text(text, encoding="utf-8", newline="\n")
        print("wrote", name)


if __name__ == "__main__":
    main()
