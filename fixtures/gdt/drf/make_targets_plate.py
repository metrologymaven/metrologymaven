#!/usr/bin/env python3
"""Independent reference for GDT.DRF.COMMON.COVER (datum target coverage), stdlib only.

Writes `targets_plate.json`: a measured plate (an 11 x 11 grid at 1 mm spacing in the plane
z = 0, x and y from 0 to 10 mm) and datum targets on and off it (point, line, parallelogram
and disc targets), with the expected coverage distance of every target and the expected
outcome of each case (a report, or `target_not_covered` naming the first target in
definition order whose sampled distance plus sampling bound `h` exceeds the threshold).

Two independent expectations per target:

- `sampled`: the leaf's pinned sample set (NODE.md Design and the module documentation of
  `fid_gps::drf::targets::coverage`) rebuilt here from the rule in binary64 (Python floats
  round like the Rust code; no fused multiply-add), then the exact nearest-point distance of
  every sample over all measured points by brute force in rational arithmetic (`Fraction`),
  the largest one, and its square root at 80 digits (`decimal`). Disc rim samples use
  `math.cos`/`math.sin` (not guaranteed correctly rounded; a 1-ulp difference moves a
  distance near 1e-3 m by about 1e-19 m).
- `analytic`: the supremum of the nearest-point distance over the continuous target. For a
  point it is the nearest distance itself. For a line segment it is computed exactly: the
  nearest-point distance along a segment is the lower envelope of the distances to each
  point, so its maximum lies at an end point or where two points are equally near (the
  segment crosses their bisector plane); every such crossing is a candidate, evaluated
  exactly. For the parallelogram and disc cases it is hand-labelled below from the geometry
  (the farthest point of the target from the plate) and the script asserts that the sampled
  value does not exceed it.

The sampling bound `h` (the leaf's conservative pass rule) is computed from its stated
formula in binary64 (`bound` below). Wherever the analytic supremum is known the script
asserts `sampled <= analytic <= sampled + h`, which checks the bound on these cases
independently of the derivation.

Inputs are decimal strings in metres; each value used is the binary64 nearest to it, which
is what a correctly rounding parser (Rust's `str::parse::<f64>`) reads. With `--check` the
file is rebuilt in memory and compared byte for byte with the committed one.

Synthetic geometry only; CC0.
"""

import json
import math
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

getcontext().prec = 80
HERE = Path(__file__).resolve().parent
OUT = HERE / "targets_plate.json"
MAX_SAMPLES = 1 << 20


def mm(v):
    """A length in millimetres as a decimal string in metres (exact decimal shift)."""
    return str(Decimal(str(v)) / 1000)


GRID = [(mm(i), mm(j), "0") for j in range(11) for i in range(11)]
GRID_F = [tuple(Fraction(float(c)) for c in p) for p in GRID]


def d2_exact(q):
    """Exact squared distance from sample q (floats) to the nearest grid point."""
    qf = [Fraction(c) for c in q]
    return min((qf[0] - p[0]) ** 2 + (qf[1] - p[1]) ** 2 + (qf[2] - p[2]) ** 2 for p in GRID_F)


def sqrt_str(fr):
    return str((Decimal(fr.numerator) / Decimal(fr.denominator)).sqrt())


def fnorm(v):
    # Pinned: sqrt((x*x + y*y) + z*z), correctly rounded.
    return math.sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2])


def fcross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def steps(q):
    c = math.ceil(q)
    assert c < MAX_SAMPLES
    return max(1, c)


def geom(t):
    return {
        k: ([float(x) for x in v] if isinstance(v, list) else float(v))
        for k, v in t.items()
        if k not in ("label", "kind", "expect")
    }


def unit_normal(nrm):
    # Divided by the largest absolute component first, then normalised.
    big = max(abs(x) for x in nrm)
    sc = [x / big for x in nrm]
    ln = fnorm(sc)
    return [x / ln for x in sc]


def bound(t, s):
    """The sampling bound h of target t at spacing s (binary64, the leaf's formulas)."""
    g = geom(t)
    kind = t["kind"]
    if kind == "point":
        return 0.0
    if kind == "line":
        a, b = g["start"], g["end"]
        ln = fnorm([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
        return (ln / steps(ln / s)) / 2.0
    if kind == "rect":
        eu, ev = g["edge_u"], g["edge_v"]
        nu, nv = steps(fnorm(eu) / s), steps(fnorm(ev) / s)
        a = [x / nu for x in eu]
        b = [x / nv for x in ev]
        return max(fnorm([a[k] + b[k] for k in range(3)]), fnorm([a[k] - b[k] for k in range(3)])) / 2.0
    if kind == "disc":
        r = g["radius"]
        m = max(4, steps((math.tau * r) / s))
        return s * math.sqrt(0.5) + (math.pi * r) / m
    raise ValueError(kind)


def samples(t, s):
    """The pinned sample set of target t at spacing s (binary64)."""
    g = geom(t)
    kind = t["kind"]
    if kind == "point":
        return [g["at"]]
    if kind == "line":
        a, b = g["start"], g["end"]
        n = steps(fnorm([b[0] - a[0], b[1] - a[1], b[2] - a[2]]) / s)
        out = []
        for i in range(n + 1):
            tt = i / n
            w = 1.0 - tt
            out.append([w * a[k] + tt * b[k] for k in range(3)])
        return out
    if kind == "rect":
        c, eu, ev = g["corner"], g["edge_u"], g["edge_v"]
        nu, nv = steps(fnorm(eu) / s), steps(fnorm(ev) / s)
        return [
            [(c[k] + (i / nu) * eu[k]) + (j / nv) * ev[k] for k in range(3)] for i in range(nu + 1) for j in range(nv + 1)
        ]
    if kind == "disc":
        c, nrm, r = g["centre"], g["normal"], g["radius"]
        n = unit_normal(nrm)
        k = 0
        for a in (1, 2):
            if abs(n[a]) < abs(n[k]):
                k = a
        e = [0.0, 0.0, 0.0]
        e[k] = 1.0
        w = fcross(e, n)
        wl = fnorm(w)
        u = [x / wl for x in w]
        v = fcross(n, u)

        def at(x, y):
            return [(c[q] + x * u[q]) + y * v[q] for q in range(3)]

        kk = int(math.floor(r / s))
        out = []
        for j in range(-kk, kk + 1):
            y = j * s
            for i in range(-kk, kk + 1):
                x = i * s
                if x * x + y * y <= r * r:
                    out.append(at(x, y))
        m = max(4, steps((math.tau * r) / s))
        for i in range(m):
            th = (math.tau * i) / m
            out.append(at(r * math.cos(th), r * math.sin(th)))
        return out
    raise ValueError(kind)


def line_sup(t):
    """Exact supremum of the nearest-point distance over a segment (squared, Fraction)."""
    a = [Fraction(float(x)) for x in t["start"]]
    b = [Fraction(float(x)) for x in t["end"]]
    d = [b[k] - a[k] for k in range(3)]

    def f(tt):
        p = [a[k] + tt * d[k] for k in range(3)]
        return min((p[0] - q[0]) ** 2 + (p[1] - q[1]) ** 2 + (p[2] - q[2]) ** 2 for q in GRID_F)

    cands = {Fraction(0), Fraction(1)}
    for i in range(len(GRID_F)):
        qi = GRID_F[i]
        ni = sum(c * c for c in qi)
        for j in range(i + 1, len(GRID_F)):
            qj = GRID_F[j]
            diff = [qj[k] - qi[k] for k in range(3)]
            den = 2 * sum(d[k] * diff[k] for k in range(3))
            if den == 0:
                continue
            num = sum(c * c for c in qj) - ni - 2 * sum(a[k] * diff[k] for k in range(3))
            tt = num / den
            if 0 <= tt <= 1:
                cands.add(tt)
    # Only crossings where both points are among the nearest matter; evaluating all
    # candidates is simply exhaustive.
    return max(f(tt) for tt in sorted(cands))


def target(label, kind, **g):
    t = {"label": label, "kind": kind}
    for k, v in g.items():
        t[k] = [mm(x) for x in v] if isinstance(v, (list, tuple)) else mm(v)
    return t


def build():
    plate_points = [
        target("A1", "point", at=(2.3, 4.6, 0.2)),
        target("A2", "point", at=(7.4, 1.2, -0.1)),
        target("A3", "point", at=(5, 8.7, 0.3)),
    ]
    a2_out = target("A2", "point", at=(12, 5, 0))
    a3_out = target("A3", "point", at=(5, 13.5, 0))
    cases = [
        # AC-1: three point targets on the plate, threshold 0.8 mm.
        {"name": "ac1_plate", "threshold": mm(0.8), "spacing": mm(0.5), "targets": plate_points},
        # AC-2: A2 moved 2 mm outside the grid (nearest grid point (10, 5, 0) mm).
        {"name": "ac2_a2_outside", "threshold": mm(0.8), "spacing": mm(0.5),
         "targets": [plate_points[0], a2_out, plate_points[2]]},
        # REQ-002: two targets fail; the first in definition order (A2, 2 mm) is named, not
        # the worst (A3, 3.5 mm).
        {"name": "first_in_definition_order", "threshold": mm(0.8), "spacing": mm(0.5),
         "targets": [plate_points[0], a2_out, a3_out]},
        # Equal to the threshold passes: 0.5 mm straight above grid point (5, 5, 0) mm.
        {"name": "equal_threshold_passes", "threshold": mm(0.5), "spacing": mm(0.5),
         "targets": [target("B1", "point", at=(5, 5, 0.5))]},
        # AC-3: a line target half outside the data; the largest gap is its outer end,
        # 5 mm from (10, 5, 0) mm.
        {"name": "ac3_line_half_outside", "threshold": mm(0.8), "spacing": mm(0.5),
         "targets": [target("C1", "line", start=(5, 5, 0), end=(15, 5, 0))]},
        {"name": "ac3_line_half_outside_passes", "threshold": mm(6), "spacing": mm(0.5),
         "targets": [target("C1", "line", start=(5, 5, 0), end=(15, 5, 0))]},
        # A line inside the plate whose largest gap is interior (between grid points) and a
        # sample; it passes at 1.1 mm (sampled 0.77 mm + h 0.25 mm).
        {"name": "line_interior_gap", "threshold": mm(1.1), "spacing": mm(0.5),
         "targets": [target("C2", "line", start=(0.5, 0.5, 0.3), end=(9.5, 0.5, 0.3))]},
        # A coarse spacing that misses the interior gaps: sampled 0.66 mm < true 0.71 mm. At a
        # 0.7 mm threshold the true gap is not covered, and the conservative rule (sampled + h)
        # fails it although the sampled value alone is below 0.7 mm.
        {"name": "line_coarse_spacing", "threshold": mm(0.7), "spacing": mm(0.7),
         "targets": [target("C3", "line", start=(0.2, 2.5, 0), end=(9.8, 2.5, 0))]},
        # The same line at a spacing well below the threshold passes at 0.8 mm.
        {"name": "line_fine_spacing", "threshold": mm(0.8), "spacing": mm(0.05),
         "targets": [target("C3", "line", start=(0.2, 2.5, 0), end=(9.8, 2.5, 0))]},
        # An oblique line crossing the plate edge.
        {"name": "line_oblique", "threshold": mm(3), "spacing": mm(0.25),
         "targets": [target("C4", "line", start=(8.2, 7.1, 0.4), end=(11.3, 9.6, -0.2))]},
        # A rectangular area half outside: its farthest points from the plate are on its far
        # edge x = 12 mm midway between grid rows (y = 2.5, 3.5, 4.5 mm), sqrt(2^2 + 0.5^2) mm.
        {"name": "rect_half_outside", "threshold": mm(0.8), "spacing": mm(0.5),
         "targets": [target("D1", "rect", corner=(8, 2, 0), edge_u=(4, 0, 0), edge_v=(0, 3, 0))]},
        # A rectangle inside the plate, lifted 0.1 mm: farthest point at a cell centre,
        # sqrt(0.5^2 + 0.5^2 + 0.1^2) mm; passes at 1.1 mm (h = half the 0.5 mm cell diagonal).
        {"name": "rect_inside", "threshold": mm(1.1), "spacing": mm(0.5),
         "targets": [target("D2", "rect", corner=(1, 1, 0.1), edge_u=(3, 0, 0), edge_v=(0, 2, 0))]},
        # A disc of radius 1.5 mm centred 1 mm outside the plate edge: its farthest point is
        # (12.5, 5, 0) mm, 2.5 mm from (10, 5, 0) mm.
        {"name": "disc_outside", "threshold": mm(0.8), "spacing": mm(0.5),
         "targets": [target("E1", "disc", centre=(11, 5, 0), normal=(0, 0, 1), radius=1.5)]},
        # A tilted disc on the plate (normal (0, 0.6, 0.8)), radius 2 mm, centre lifted 0.05 mm.
        {"name": "disc_tilted_inside", "threshold": mm(2), "spacing": mm(0.4),
         "targets": [target("E2", "disc", centre=(5.5, 5.5, 0.05), normal=(0, 0.6, 0.8), radius=2)]},
        # The same disc with a tiny normal (scaled by its largest component first).
        {"name": "disc_tiny_normal", "threshold": mm(2), "spacing": mm(0.4),
         "targets": [{**target("E3", "disc", centre=(5.5, 5.5, 0.05), radius=2), "normal": ["0", "6e-171", "8e-171"]}]},
        # All kinds together; every one is checked, A1 first.
        {"name": "mixed_all_pass", "threshold": mm(6), "spacing": mm(0.5),
         "targets": [plate_points[0], target("C1", "line", start=(5, 5, 0), end=(15, 5, 0)),
                     target("D1", "rect", corner=(8, 2, 0), edge_u=(4, 0, 0), edge_v=(0, 3, 0)),
                     target("E1", "disc", centre=(11, 5, 0), normal=(0, 0, 1), radius=1.5)]},
    ]
    # Hand-labelled squared suprema (m^2) of the area targets, from the comments above:
    # D1 sqrt(2^2 + 0.5^2) mm, D2 sqrt(0.5^2 + 0.5^2 + 0.1^2) mm, E1 2.5 mm. E2 has none.
    hand = {
        "D1": (Fraction(4) + Fraction(1, 4)) / 10**6,
        "D2": (Fraction(1, 4) + Fraction(1, 4) + Fraction(1, 100)) / 10**6,
        "E1": Fraction(25, 10000) ** 2,
    }
    for c in cases:
        c["targets"] = [dict(t) for t in c["targets"]]
        s = float(c["spacing"])
        # The threshold as the binary64 the Rust code compares with; the distances are exact,
        # so a distance equal to that binary64 (the `equal_threshold_passes` case: a pure
        # z offset, whose exact distance is the offset itself) passes, as in the leaf.
        thr = Decimal(float(c["threshold"]))
        first_fail = None
        for t in c["targets"]:
            pts = samples(t, s)
            worst = max(d2_exact(q) for q in pts)
            h = bound(t, s)
            t_exp = {"sampled": sqrt_str(worst), "bound": repr(h), "samples": len(pts)}
            if t["kind"] == "point":
                t_exp["analytic"] = t_exp["sampled"]
            elif t["kind"] == "line":
                t_exp["analytic"] = sqrt_str(line_sup(t))
            else:
                if t["label"] in hand:
                    t_exp["analytic"] = sqrt_str(hand[t["label"]])
            if "analytic" in t_exp:
                assert Decimal(t_exp["sampled"]) <= Decimal(t_exp["analytic"]) + Decimal("1e-18"), (c["name"], t_exp)
                assert Decimal(t_exp["analytic"]) <= Decimal(t_exp["sampled"]) + Decimal(h), (c["name"], t_exp)
            t["expect"] = t_exp
            upper = Decimal(t_exp["sampled"]) + Decimal(h)
            # No case sits within rounding of its threshold except a point at exactly it.
            assert h == 0.0 or abs(upper - thr) > Decimal("1e-12"), (c["name"], t["label"])
            if first_fail is None and upper > thr:
                first_fail = {"target": t["label"], "distance": t_exp["sampled"], "bound": repr(h)}
        c["expect"] = {"error": "target_not_covered", **first_fail} if first_fail else {"ok": True}
    return {
        "about": "GDT.DRF.COMMON.COVER reference cases; generated by make_targets_plate.py; synthetic, CC0",
        "units": "m",
        "points": [list(p) for p in GRID],
        "cases": cases,
    }


def render():
    return json.dumps(build(), indent=1, sort_keys=False) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        old = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if old != text:
            print(f"{OUT.name} is stale; run {Path(__file__).name}", file=sys.stderr)
            return 1
        print(f"{OUT.name} is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
