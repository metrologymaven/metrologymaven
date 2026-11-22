#!/usr/bin/env python3
"""Independent reference for GDT.DIM.RADIAL (diameter and radius checks), stdlib only.

Writes two files next to this script:

- `radial_analytic.json`: the measured points of five synthetic features with their planted
  geometry and the expected values of AC-1 and AC-2: an LS circle in the plane z = 2 mm
  (r = 5 mm, fitted as `circle2`), an LS circle in space (r = 5 mm, `circle3`), a cylinder
  (r = 12.5 mm), a sphere (r = 10 mm) and a cone (half-angle 30 degrees, apex at the origin).
- `radial_table.json`: the synthetic DMT rows `dmt:asme/diameter/none@1` and
  `dmt:asme/radius/none@1` the command tests resolve. Each variant's chain names one
  association algorithm (an LS fit per primitive, a maximum inscribed circle for AC-4, and a
  chain with two association steps). They are inputs, not expected values, and are not an
  interpretation of any standard.

This script does not run or mirror the Rust code. The expected values come from the planted
geometry, by hand from REQ-GDT.DIM.RADIAL-001 and -002: radius = planted r, diameter = 2 r,
cone diameter at gauge distance g from the apex = 2 g tan(half-angle). The points are built
in exact rational arithmetic (`Fraction`) on the planted features: unit-circle points from
the rational parametrisation ((1 - t^2), 2 t) / (1 + t^2), unit-sphere points by inverse
stereographic projection, placed by a rotation with exact rational entries (unit quaternion
(5, 1, 2, 1) / 31 squared norm). Only the cone's radii (z tan 30 degrees) are irrational; they
are computed in `decimal` at 60 digits. Each coordinate is then rounded once to binary64
(`float(Fraction)` / `float(Decimal)` are correctly rounded) and written with `repr`, which
round-trips. For every feature the script then measures, exactly (Fraction, with a 60-digit
`decimal` square root), the largest distance of the rounded points from the planted feature
(`max_point_error_m`): it is below 1e-17 m, so the least-squares radius of the rounded
points differs from the planted one by far less than the 1e-12 m tolerance of AC-1.

With `--check` the files are rebuilt in memory and compared byte for byte with the committed
ones. All geometry is synthetic, in metres; no standards text; CC0.
"""

import json
import sys
from decimal import Decimal, getcontext
from fractions import Fraction as F
from pathlib import Path

getcontext().prec = 60
HERE = Path(__file__).resolve().parent

LS = {
    "circle2": "fit.circle2.ls@1.0.0",
    "circle3": "fit.circle3.ls@1.0.0",
    "cylinder": "fit.cylinder.ls@1.0.0",
    "sphere": "fit.sphere.ls@1.0.0",
    "cone": "fit.cone.ls@1.0.0",
}
MI_CIRCLE3 = "fit.circle3.mi@1.0.0"


def quat_rot(a, b, c, d):
    s = a * a + b * b + c * c + d * d
    m = [
        [a * a + b * b - c * c - d * d, 2 * (b * c - a * d), 2 * (b * d + a * c)],
        [2 * (b * c + a * d), a * a - b * b + c * c - d * d, 2 * (c * d - a * b)],
        [2 * (b * d - a * c), 2 * (c * d + a * b), a * a - b * b - c * c + d * d],
    ]
    return [[F(x, s) for x in row] for row in m]


ROT = quat_rot(5, 1, 2, 1)


def col(m, j):
    return (m[0][j], m[1][j], m[2][j])


def add(*vs):
    return tuple(sum((v[i] for v in vs), F(0)) for i in range(3))


def scale(k, v):
    return tuple(k * x for x in v)


def dot(a, b):
    return sum((a[i] * b[i] for i in range(3)), F(0))


def sub(a, b):
    return tuple(a[i] - b[i] for i in range(3))


def unit_circle(ts):
    """Exact rational points of the unit circle: t -> ((1 - t^2), 2 t) / (1 + t^2), plus (-1, 0)."""
    pts = [((1 - t * t) / (1 + t * t), (2 * t) / (1 + t * t)) for t in ts]
    pts.append((F(-1), F(0)))
    return pts


TS = [F(-3), F(-2), F(-1), F(-1, 2), F(-1, 3), F(0), F(1, 3), F(1, 2), F(1), F(2), F(3)]


def rounded(p):
    return [float(x) for x in p]


def dsqrt(x):
    return Decimal(x.numerator) / Decimal(x.denominator) if x == 0 else (Decimal(x.numerator) / Decimal(x.denominator)).sqrt()


def worst(errors):
    """The largest of exact squared errors, as a decimal string of its square root."""
    m = max(errors)
    return format(dsqrt(m), ".3e")


def circle2():
    r = F(5, 1000)
    c = (F(4, 1000), F(6, 1000))
    z = F(2, 1000)
    pts = [(c[0] + r * x, c[1] + r * y, z) for x, y in unit_circle(TS)]
    out = [rounded(p) for p in pts]
    errs = []
    for q in out:
        qx, qy = F(q[0]), F(q[1])
        rho2 = (qx - c[0]) ** 2 + (qy - c[1]) ** 2
        # (rho - r)^2 <= |rho^2 - r^2|^2 / r^2 since rho + r >= r.
        errs.append((rho2 - r * r) ** 2 / (r * r))
    return {"radius": r, "points": out, "err": worst(errs)}


def circle3():
    r = F(5, 1000)
    c = (F(10, 1000), F(-20, 1000), F(30, 1000))
    e1, e2, n = col(ROT, 0), col(ROT, 1), col(ROT, 2)
    pts = [add(c, scale(r * x, e1), scale(r * y, e2)) for x, y in unit_circle(TS)]
    out = [rounded(p) for p in pts]
    errs = []
    for q in out:
        w = sub(tuple(F(x) for x in q), c)
        h = dot(w, n)
        rho2 = dot(w, w) - h * h
        errs.append(h * h + (rho2 - r * r) ** 2 / (r * r))
    return {"radius": r, "points": out, "err": worst(errs)}


def cylinder():
    r = F(125, 10000)
    p0 = (F(-5, 1000), F(7, 1000), F(3, 1000))
    e1, e2, a = col(ROT, 0), col(ROT, 1), col(ROT, 2)
    pts = []
    for h in [F(-20, 1000), F(-10, 1000), F(0), F(10, 1000), F(20, 1000)]:
        for x, y in unit_circle(TS):
            pts.append(add(p0, scale(h, a), scale(r * x, e1), scale(r * y, e2)))
    out = [rounded(p) for p in pts]
    errs = []
    for q in out:
        w = sub(tuple(F(x) for x in q), p0)
        t = dot(w, a)
        rho2 = dot(w, w) - t * t
        errs.append((rho2 - r * r) ** 2 / (r * r))
    return {"radius": r, "points": out, "err": worst(errs)}


def sphere():
    r = F(10, 1000)
    c = (F(1, 1000), F(2, 1000), F(-3, 1000))
    grid = [F(-2), F(-1), F(-1, 2), F(0), F(1, 2), F(1), F(2)]
    unit = [(F(0), F(0), F(1))]
    for u in grid:
        for v in grid:
            d = 1 + u * u + v * v
            unit.append((2 * u / d, 2 * v / d, (u * u + v * v - 1) / d))
    pts = [add(c, scale(r, p)) for p in unit]
    out = [rounded(p) for p in pts]
    errs = []
    for q in out:
        w = sub(tuple(F(x) for x in q), c)
        errs.append((dot(w, w) - r * r) ** 2 / (r * r))
    return {"radius": r, "points": out, "err": worst(errs)}


def tan30():
    # tan 30 degrees = 1 / sqrt(3).
    return Decimal(1) / Decimal(3).sqrt()


def cone():
    t = tan30()
    out = []
    errs = []
    for z in [F(5, 1000), F(10, 1000), F(15, 1000), F(20, 1000)]:
        zd = Decimal(z.numerator) / Decimal(z.denominator)
        rz = zd * t
        for x, y in unit_circle(TS):
            xd = Decimal(x.numerator) / Decimal(x.denominator)
            yd = Decimal(y.numerator) / Decimal(y.denominator)
            q = [float(rz * xd), float(rz * yd), float(z)]
            out.append(q)
            # Distance to the cone surface: (rho - z' tan) cos 30, |.| <= |rho - z' tan|.
            qx, qy, qz = (Decimal(F(v).numerator) / Decimal(F(v).denominator) for v in q)
            rho = (qx * qx + qy * qy).sqrt()
            errs.append(abs(rho - qz * t))
    gauge = Decimal(10) / Decimal(1000)
    diameter = 2 * gauge * t
    return {
        "half_angle_deg": "30",
        "apex": [0.0, 0.0, 0.0],
        "axis": [0.0, 0.0, 1.0],
        "points": out,
        "err": format(max(errs), ".3e"),
        "gauge_from_apex_m": "0.01",
        "diameter_at_gauge": format(diameter, ".30f"),
        "radius_at_gauge": format(gauge * t, ".30f"),
    }


def dec(x):
    """An exact rational with a finite decimal expansion, as a plain decimal string."""
    d = Decimal(x.numerator) / Decimal(x.denominator)
    s = format(d, "f")
    assert Decimal(s) * x.denominator == x.numerator, s
    return s


def analytic():
    feats = []
    for name, f in [("circle2", circle2), ("circle3", circle3), ("cylinder", cylinder), ("sphere", sphere)]:
        g = f()
        feats.append(
            {
                "feature": f"f-{name}",
                "fit": LS[name],
                "variant": f"ls_{name}",
                "radius": dec(g["radius"]),
                "diameter": dec(2 * g["radius"]),
                "max_point_error_m": g["err"],
                "xyz": [repr(x) for p in g["points"] for x in p],
            }
        )
    g = cone()
    feats.append(
        {
            "feature": "f-cone",
            "fit": LS["cone"],
            "variant": "ls_cone",
            "half_angle_deg": g["half_angle_deg"],
            "gauge_from_apex": g["gauge_from_apex_m"],
            "diameter_at_gauge": g["diameter_at_gauge"],
            "radius_at_gauge": g["radius_at_gauge"],
            "max_point_error_m": g["err"],
            "xyz": [repr(x) for p in g["points"] for x in p],
        }
    )
    doc = {
        "schema": "fid.test-radial-analytic/1",
        "note": "Synthetic features for GDT.DIM.RADIAL AC-1 and AC-2; metres; CC0.",
        "tolerance_m": "1e-12",
        "features": feats,
    }
    return (json.dumps(doc, indent=1, sort_keys=True) + "\n").encode("utf-8")


def chain(algs):
    steps = [
        {"alg": "syn.partition.whole_feature@0.1.0", "kind": "partition", "params": {}},
        {"alg": "syn.extract.all_points@0.1.0", "kind": "extraction", "params": {}},
    ]
    steps += [{"alg": a, "kind": "association", "params": {}} for a in algs]
    steps.append({"alg": "syn.eval.feature_size@0.1.0", "kind": "evaluation", "params": {}})
    return steps


VARIANTS = [(f"ls_{k}", [v]) for k, v in LS.items()] + [
    ("mi_circle3", [MI_CIRCLE3]),
    ("two_assoc", [LS["circle3"], LS["sphere"]]),
]


def row(characteristic):
    return {
        "actual_value": f"av.synthetic.{characteristic}@1",
        "characteristic": characteristic,
        "clauses": [{"clause": "0", "designator": "asme-y14.5", "edition": "2018"}],
        "decision_rule_hint": "dr.synthetic.default@1",
        "default_variant": "ls_circle3",
        "dialect": "asme",
        "editions": ["y14.5-2018"],
        "key": f"dmt:asme/{characteristic}/none@1",
        "limitations": [],
        "modifiers": [],
        "notes": f"Synthetic {characteristic} row for GDT.DIM.RADIAL tests; not an interpretation of any standard.",
        "ors_cases": [f"ors:synthetic/{characteristic}.001"],
        "review": {"state": "under_review"},
        "status": "supported",
        "synthetic": True,
        "variants": [{"steps": chain(algs), "variant": name} for name, algs in sorted(VARIANTS)],
    }


def table():
    doc = {"rows": [row("diameter"), row("radius")], "schema": "fid.dmt-table/1", "version": "1.0.0"}
    return (json.dumps(doc, indent=1, sort_keys=True) + "\n").encode("utf-8")


def outputs():
    return {HERE / "radial_analytic.json": analytic(), HERE / "radial_table.json": table()}


def main():
    files = outputs()
    if "--check" in sys.argv[1:]:
        stale = [p for p, data in files.items() if not p.exists() or p.read_bytes() != data]
        for p in stale:
            print(f"{p} is stale; run {Path(__file__).name}", file=sys.stderr)
        if stale:
            sys.exit(1)
        print("ok")
        return
    for p, data in files.items():
        p.write_bytes(data)
        print(f"wrote {p}")


if __name__ == "__main__":
    main()
