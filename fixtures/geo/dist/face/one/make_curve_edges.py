#!/usr/bin/env python3
"""Independent oracle for GEO.DIST.FACE.ONE edges given by an ellipse arc or a NURBS curve.

Written for this project from the spec (spec/geo/dist/face/one/NODE.md); it shares no code
with any Rust crate. Python standard library only: `decimal` at 60 digits. Output data is
CC-BY-4.0. Invented geometry (D-038).

Two planar faces in the plane z = 0 (placement at the origin, axis +z, reference +x, so the
surface parameters are (x, y)):

- ellipse_segment.json: the segment of the ellipse x = a cos t, y = b sin t between the arc
  t in [T0, T1] (edge E1, a 3-D ellipse curve, V1 -> V2) and its chord (edge E2, a line,
  V2 -> V1).
- nurbs_arc.json: the quarter disc of radius R: edge E1 is the rational quadratic NURBS arc
  with control points (R, 0, 0), (R, R, 0), (0, R, 0), weights (1, w, 1), w = sqrt(2)/2
  rounded to binary64 (V1 -> V2); E2 the line V2 -> V3 (the origin); E3 the line V3 -> V1.
  The oracle treats E1 as the exact circle: the rounding of w moves the curve by less than
  1e-17 m.

Answer for a query p: the projection (p_x, p_y, 0) when it lies in the closed region
(Decimal tests), else the nearest of the boundary candidates: the ellipse arc by 1000
samples plus Newton on f'(t) = 0 at 60 digits from every local minimum of the samples (the
ends included), the circle arc by its closed form, the segments by clamped projection, the
vertices. A foot at an end of an open edge is reported as that vertex. Fixture format:
fid-face-ref/1 (as tools/fixtures/face_proj_oracle.py), so the same reader and comparison
apply.

Run:   python fixtures/geo/dist/face/one/make_curve_edges.py
Check: python fixtures/geo/dist/face/one/make_curve_edges.py --check
"""

import argparse
import json
import math
import random
import sys
from decimal import Decimal, getcontext
from pathlib import Path

getcontext().prec = 60
D = Decimal
VERSION = "make_curve_edges 1.0.0"
SEED = 4242
HERE = Path(__file__).resolve().parent

A_SEMI, B_SEMI = 0.0859375, 0.046875
T0, T1 = 0.375, 2.875
RADIUS = 0.0625
W_MID = math.sqrt(2.0) / 2.0


def pi():
    getcontext().prec += 5
    # Machin: pi = 16 atan(1/5) - 4 atan(1/239)
    def atan_inv(n):
        x = D(1) / n
        x2 = x * x
        s, t, k = x, x, 1
        while True:
            t = -t * x2
            k += 2
            term = t / k
            if term == 0:
                break
            s += term
        return s
    p = 16 * atan_inv(5) - 4 * atan_inv(239)
    getcontext().prec -= 5
    return +p


PI = pi()


def sin_cos(x):
    x = D(x)
    k = (x / (2 * PI)).to_integral_value()
    x -= k * 2 * PI
    getcontext().prec += 5
    s, c, term, n = D(0), D(0), D(1), 0
    while True:
        if n % 2 == 0:
            c += term if (n // 2) % 2 == 0 else -term
        else:
            s += term if (n // 2) % 2 == 0 else -term
        n += 1
        term = term * x / n
        if abs(term) < D("1e-70"):
            break
    getcontext().prec -= 5
    return +s, +c


def sqrt(x):
    return D(x).sqrt()


def dist(a, b):
    return sqrt(sum((D(x) - D(y)) ** 2 for x, y in zip(a, b)))


def ellipse_point(t):
    s, c = sin_cos(t)
    return (D(A_SEMI) * c, D(B_SEMI) * s, D(0))


def f3(v):
    return [float(x) for x in v]


def seg_closest(p, a, b):
    """Closest point of segment a -> b; t in [0, 1]."""
    p, a, b = [tuple(D(x) for x in v) for v in (p, a, b)]
    ab = [b[i] - a[i] for i in range(3)]
    ap = [p[i] - a[i] for i in range(3)]
    t = sum(x * y for x, y in zip(ab, ap)) / sum(x * x for x in ab)
    if t <= 0:
        return D(0), a
    if t >= 1:
        return D(1), b
    return t, tuple(a[i] + t * ab[i] for i in range(3))


def ellipse_arc_closest(p):
    a, b = D(A_SEMI), D(B_SEMI)
    px, py = D(p[0]), D(p[1])
    t0, t1 = D(T0), D(T1)

    def f(t):
        x, y, _ = ellipse_point(t)
        return (x - px) ** 2 + (y - py) ** 2

    def df(t):
        s, c = sin_cos(t)
        x, y = a * c, b * s
        return 2 * (-(x - px) * a * s + (y - py) * b * c), 2 * (
            a * a * s * s + b * b * c * c - (x - px) * a * c - (y - py) * b * s
        )

    n = 1000
    ts = [t0 + (t1 - t0) * k / n for k in range(n + 1)]
    vals = [f(t) for t in ts]
    cands = [t0, t1]
    for k in range(0, n + 1):
        left = vals[k - 1] if k > 0 else None
        right = vals[k + 1] if k < n else None
        if (left is None or vals[k] <= left) and (right is None or vals[k] <= right):
            t = ts[k]
            for _ in range(100):
                g, h = df(t)
                if h <= 0:
                    break
                step = g / h
                t -= step
                if abs(step) < D("1e-55"):
                    break
            if t0 <= t <= t1:
                cands.append(t)
    best = min(cands, key=lambda t: (f(t), t))
    return best, ellipse_point(best)


def circle_arc_closest(p):
    px, py = D(p[0]), D(p[1])
    r = D(RADIUS)
    rho = sqrt(px * px + py * py)
    if rho > 0 and px >= 0 and py >= 0:
        return "mid", (r * px / rho, r * py / rho, D(0))
    v1, v2 = (r, D(0), D(0)), (D(0), r, D(0))
    return ("start", v1) if dist(p, v1) <= dist(p, v2) else ("end", v2)


def ellipse_face():
    v1, v2 = ellipse_point(T0), ellipse_point(T1)
    fv1, fv2 = f3(v1), f3(v2)
    chord = [fv1[i] - fv2[i] for i in range(3)]
    length = math.sqrt(sum(x * x for x in chord))
    direction = [x / length for x in chord]
    body = {
        "format": "fid-brep-builder/0",
        "body_id": "ellipse_segment",
        "unit": "m",
        "description": "Planar segment of an ellipse between an elliptical arc edge (3-D ellipse curve) and its chord.",
        "vertices": [{"label": "V1", "point": fv1}, {"label": "V2", "point": fv2}],
        "edges": [
            {
                "label": "E1", "start": "V1", "end": "V2",
                "curve": {"type": "ellipse",
                          "placement": {"origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0]},
                          "semi_axis_1": A_SEMI, "semi_axis_2": B_SEMI},
                "param_range": [T0, T1],
            },
            {
                "label": "E2", "start": "V2", "end": "V1",
                "curve": {"type": "line", "origin": fv2, "direction": direction},
                "param_range": [0.0, length],
            },
        ],
        "faces": [plane_face(["E1", "E2"])],
        "shells": [{"label": "S1", "faces": ["F1"]}],
    }
    mid = ellipse_point((D(T0) + D(T1)) / 2)

    def side(q):
        a, b = [D(x) for x in fv2], [D(x) for x in fv1]
        return (b[0] - a[0]) * (D(q[1]) - a[1]) - (b[1] - a[1]) * (D(q[0]) - a[0])

    arc_side = side(mid) > 0

    def inside(q):
        x, y = D(q[0]), D(q[1])
        in_ell = (x / D(A_SEMI)) ** 2 + (y / D(B_SEMI)) ** 2 <= 1
        s = side(q)
        return in_ell and (s == 0 or (s > 0) == arc_side)

    def boundary(q):
        out = []
        t, foot = ellipse_arc_closest(q)
        feat = ("vertex", "V1") if t == D(T0) else ("vertex", "V2") if t == D(T1) else ("edge", "E1")
        out.append((dist(q, foot), foot, feat))
        t, foot = seg_closest(q, fv2, fv1)
        feat = ("vertex", "V2") if t == 0 else ("vertex", "V1") if t == 1 else ("edge", "E2")
        out.append((dist(q, foot), foot, feat))
        return out

    def on_boundary(t_edge):
        k, t = t_edge
        if k == 0:
            return f3(ellipse_point(D(T0) + (D(T1) - D(T0)) * D(t)))
        return [fv2[i] + (fv1[i] - fv2[i]) * t for i in range(3)]

    return body, inside, boundary, on_boundary, [(-0.11, 0.11), (-0.02, 0.07)], [fv1, fv2]


def plane_face(edges):
    return {
        "label": "F1",
        "surface": {"type": "plane",
                    "placement": {"origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "ref_dir": [1.0, 0.0, 0.0]}},
        "sense": "same",
        "loops": [{"label": "F1L1", "outer": True,
                   "coedges": [{"label": f"F1L1C{k + 1}", "edge": e, "sense": "same", "pcurve": None}
                               for k, e in enumerate(edges)]}],
    }


def nurbs_face():
    r = RADIUS
    body = {
        "format": "fid-brep-builder/0",
        "body_id": "nurbs_arc",
        "unit": "m",
        "description": "Planar quarter disc bounded by a rational quadratic NURBS arc and two line edges.",
        "vertices": [{"label": "V1", "point": [r, 0.0, 0.0]}, {"label": "V2", "point": [0.0, r, 0.0]},
                     {"label": "V3", "point": [0.0, 0.0, 0.0]}],
        "edges": [
            {"label": "E1", "start": "V1", "end": "V2",
             "curve": {"type": "nurbs", "degree": 2, "knots": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                       "ctrl": [[r, 0.0, 0.0], [r, r, 0.0], [0.0, r, 0.0]], "weights": [1.0, W_MID, 1.0]},
             "param_range": [0.0, 1.0]},
            {"label": "E2", "start": "V2", "end": "V3",
             "curve": {"type": "line", "origin": [0.0, r, 0.0], "direction": [0.0, -1.0, 0.0]},
             "param_range": [0.0, r]},
            {"label": "E3", "start": "V3", "end": "V1",
             "curve": {"type": "line", "origin": [0.0, 0.0, 0.0], "direction": [1.0, 0.0, 0.0]},
             "param_range": [0.0, r]},
        ],
        "faces": [plane_face(["E1", "E2", "E3"])],
        "shells": [{"label": "S1", "faces": ["F1"]}],
    }
    v1, v2, v3 = [r, 0.0, 0.0], [0.0, r, 0.0], [0.0, 0.0, 0.0]

    def inside(q):
        x, y = D(q[0]), D(q[1])
        return x >= 0 and y >= 0 and x * x + y * y <= D(r) ** 2

    def boundary(q):
        out = []
        kind, foot = circle_arc_closest(q)
        feat = {"mid": ("edge", "E1"), "start": ("vertex", "V1"), "end": ("vertex", "V2")}[kind]
        out.append((dist(q, foot), foot, feat))
        for a, b, la, lb, le in ((v2, v3, "V2", "V3", "E2"), (v3, v1, "V3", "V1", "E3")):
            t, foot = seg_closest(q, a, b)
            feat = ("vertex", la) if t == 0 else ("vertex", lb) if t == 1 else ("edge", le)
            out.append((dist(q, foot), foot, feat))
        return out

    def on_boundary(t_edge):
        k, t = t_edge
        if k == 0:
            s, c = sin_cos(D(t) * PI / 2)
            return [float(D(r) * c), float(D(r) * s), 0.0]
        a, b = (v2, v3) if k == 1 else (v3, v1)
        return [a[i] + (b[i] - a[i]) * t for i in range(3)]

    return body, inside, boundary, on_boundary, [(-0.03, 0.09), (-0.03, 0.09)], [v1, v2, v3]


def hexv(v):
    return {"hex": [float(x).hex() for x in v], "dec": [float(x) for x in v]}


def hexs(x):
    return {"hex": float(x).hex(), "dec": float(x)}


def solve(q, inside, boundary):
    proj = (D(q[0]), D(q[1]), D(0))
    untrimmed = (dist(q, proj), proj)
    cands = boundary(q)
    edge_d = min(c[0] for c in cands)
    if inside(q):
        ans = (abs(D(q[2])), proj, ("interior", None))
    else:
        rank = {"vertex": 0, "edge": 1}
        ans = min(cands, key=lambda c: (c[0], rank[c[2][0]]))
    return ans, untrimmed, edge_d


def offset(rng, lo, hi):
    """A seeded offset m * 10^-k (k in [lo, hi]) and a direction (sin, cos), computed in
    Decimal so that no platform libm enters the fixture bytes."""
    m = rng.randint(100, 999)
    k = rng.randint(lo, hi)
    off = float(D(m) / D(10) ** (k + 2))
    s, c = sin_cos(D(rng.randint(0, 999_999)) * 2 * PI / 1_000_000)
    return off, (float(s), float(c))


def make(name, builder, rng):
    body, inside, boundary, on_boundary, box, verts = builder()
    queries = []
    pts = []
    for _ in range(60):
        pts.append(("random", [rng.uniform(*box[0]), rng.uniform(*box[1]), rng.uniform(-0.03, 0.03)]))
    n_edges = len(body["edges"])
    for k in range(40):
        e = k % n_edges
        b = on_boundary((e, rng.uniform(0.05, 0.95)))
        off, (s, c) = offset(rng, 3, 7)
        pts.append(("near_edge", [b[0] + off * c, b[1] + off * s, rng.uniform(-1e-3, 1e-3)]))
    for k in range(12):
        v = verts[k % len(verts)]
        off, (s, c) = offset(rng, 2, 6)
        pts.append(("near_vertex", [v[0] + off * c, v[1] + off * s, rng.uniform(-0.01, 0.01)]))
    for i, (gen, p) in enumerate(pts):
        (d, foot, feat), (ud, ufoot), edge_d = solve(p, inside, boundary)
        foot_f = f3(foot)
        fe = min(c[0] for c in boundary(foot_f))
        tags = ["trimmed_away"] if ud < d - D("1e-9") else []
        queries.append({
            "id": f"q{i + 1:04d}", "gen": gen, "tags": tags, "p": hexv(p), "face": "F1",
            "feature": {"kind": feat[0]} if feat[1] is None else {"kind": feat[0], "id": feat[1]},
            "foot": hexv(foot_f), "distance": hexs(d), "alternatives": [],
            "edge_distance": hexs(edge_d), "foot_edge_distance": hexs(fe),
            "untrimmed": {"face": "F1", "foot": hexv(f3(ufoot)), "distance": hexs(ud)},
        })
    doc = {
        "format": "fid-face-ref/1", "oracle_version": VERSION, "seed": SEED, "body_seed": SEED,
        "license": "CC-BY-4.0", "unit": "m", "foot_tolerance_m": 1e-9, "analytic": True,
        "body": body, "queries": queries,
    }
    return json.dumps(doc, indent=1, sort_keys=False) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()
    rng = random.Random(SEED)
    bad = 0
    for name, builder in (("ellipse_segment.json", ellipse_face), ("nurbs_arc.json", nurbs_face)):
        text = make(name, builder, rng)
        target = HERE / name
        if args.check:
            if not target.exists() or target.read_text(encoding="utf-8") != text:
                print(f"{target}: differs from the oracle output", file=sys.stderr)
                bad += 1
        else:
            target.write_text(text, encoding="utf-8", newline="\n")
            print(f"wrote {target}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
