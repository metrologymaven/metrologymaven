#!/usr/bin/env python3
"""Independent reference for GDT.DIM.PTDEV (point deviations, feature versus nominal), stdlib only.

Writes three files next to this script:

- `ptdev_plane.json` (AC-1): two measured points offset +0.05 mm and -0.02 mm along the
  outward unit normal (2, 3, 6) / 7 of a nominal plane, and the frame `rot30_z` (rotated
  30 degrees about z, translated) in which dx, dy, dz are reported.
- `ptdev_cylinder_feature.json` (AC-2): a fitted cylinder whose axis is offset (0.01, 0, 0) mm
  from the nominal axis, tilted 1e-4 rad, with radius +0.003 mm against nominal. Its stored
  axis point lies 20 mm up the actual axis, so the location deviation needs the axis point
  nearest the nominal reference point (REQ-GDT.DIM.PTDEV-004), not the stored one.
- `ptdev_features.json` (REQ-004, REQ-005): one point, circle, sphere and plane each against
  its nominal, reported in the frame `rot30_z`.

This script does not run or mirror the Rust code. Expected values follow from the leaf's rules
by hand: d = p - n, dx, dy, dz = R^T d for the frame-to-world rotation R, dn = d . u
(REQ-001); for features the reference-point offset in the frame, the angle between the actual
and nominal directions, and the size difference (REQ-004). Inputs are planted in exact
rational arithmetic (`Fraction`) where possible; sin, cos and the square root of 3 are
evaluated in `decimal` at 60 digits (Taylor series for sin and cos). Each input is rounded once
to binary64 and written with `repr`, which round-trips. Expected values are then computed
from the *rounded* inputs at 60 digits and rounded once to binary64. For every expected value
the script also asserts that it is within 1e-15 (m or rad) of the planted value, so the
fixtures say what the acceptance criteria say.

With `--check` the files are rebuilt in memory and compared byte for byte with the committed
ones. All geometry is synthetic, in metres and radians; no standards text; CC0.
"""

import json
import sys
from decimal import Decimal as D, getcontext
from fractions import Fraction as F
from pathlib import Path

getcontext().prec = 60
HERE = Path(__file__).resolve().parent
PLANT_TOL = D("1e-15")


def dec(x):
    """Exact Decimal of a Fraction or binary64 float (at the context precision)."""
    if isinstance(x, float):
        x = F(x)
    return D(x.numerator) / D(x.denominator)


def dsin(x):
    x = dec(x) if not isinstance(x, D) else x
    term, total, k = x, x, 1
    while abs(term) > D("1e-70"):
        term = -term * x * x / D((2 * k) * (2 * k + 1))
        total += term
        k += 1
    return total


def dcos(x):
    x = dec(x) if not isinstance(x, D) else x
    term, total, k = D(1), D(1), 1
    while abs(term) > D("1e-70"):
        term = -term * x * x / D((2 * k - 1) * (2 * k))
        total += term
        k += 1
    return total


def datan(x):
    """atan for |x| <= 0.1 by its Taylor series (all angles here are small)."""
    assert abs(x) <= D("0.1")
    term, total, k = x, x, 1
    while abs(term) > D("1e-70"):
        term = -term * x * x
        total += term / D(2 * k + 1)
        k += 1
    return total


def vdot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def vsub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def vadd(a, b):
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def vscale(k, a):
    return [k * a[0], k * a[1], k * a[2]]


def vcross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def vnorm(a):
    return vdot(a, a).sqrt()


def rnd(v):
    """Round a vector of Fractions or Decimals once to binary64."""
    return [float(x) for x in v]


def dvec(v):
    return [dec(x) for x in v]


def s(x):
    return repr(float(x))


def svec(v):
    return [s(x) for x in v]


def near(got, planted, what):
    err = abs(got - planted)
    assert err <= PLANT_TOL, f"{what}: {got} vs planted {planted} (err {err})"


# ------------------------------------------------------------------ frame rot30_z

SQRT3 = D(3).sqrt()
COS30 = SQRT3 / 2
SIN30 = D(1) / 2
# Frame-to-world rotation about z by +30 degrees; rows of R.
R_FRAME = [
    [float(COS30), float(-SIN30), 0.0],
    [float(SIN30), float(COS30), 0.0],
    [0.0, 0.0, 1.0],
]
T_FRAME = [0.1, -0.05, 0.02]


def frame_doc():
    return {"id": "rot30_z", "r": [svec(row) for row in R_FRAME], "t": svec(T_FRAME)}


def to_frame(d):
    """R^T d for the rounded R, at 60 digits (directions ignore the translation)."""
    r = [[dec(x) for x in row] for row in R_FRAME]
    return [sum(r[i][j] * d[i] for i in range(3)) for j in range(3)]


def to_frame_exact(d):
    """R^T d for the exact rotation (cos 30, sin 30), for the planted-value assertions."""
    r = [[COS30, -SIN30, D(0)], [SIN30, COS30, D(0)], [D(0), D(0), D(1)]]
    return [sum(r[i][j] * d[i] for i in range(3)) for j in range(3)]


def unoriented_angle(a, b):
    """Angle between two lines with directions a and b (Decimal vectors), in [0, pi/2]."""
    ua = vscale(1 / vnorm(a), a)
    ub = vscale(1 / vnorm(b), b)
    sn = vnorm(vcross(ua, ub))
    cs = abs(vdot(ua, ub))
    return datan(sn / cs)


# ------------------------------------------------------------------ AC-1: points on a plane


def plane_points():
    u = [F(2, 7), F(3, 7), F(6, 7)]  # exact rational unit normal
    c0 = [F(10, 1000), F(-5, 1000), F(3, 1000)]
    e1 = [F(3, 7), F(-6, 7), F(2, 7)]  # in-plane, orthogonal to u
    e2 = [F(6, 7), F(2, 7), F(-3, 7)]  # in-plane, orthogonal to u and e1
    assert vdot(u, u) == 1 and vdot(u, e1) == 0 and vdot(u, e2) == 0 and vdot(e1, e2) == 0
    planted = [F(5, 100000), F(-2, 100000)]  # +0.05 mm, -0.02 mm
    spots = [(F(4, 1000), F(-7, 1000)), (F(-9, 1000), F(2, 1000))]
    u_r = rnd(u)
    rows = []
    for (a, b), off in zip(spots, planted):
        n_pt = vadd(c0, vadd(vscale(a, e1), vscale(b, e2)))
        p_pt = vadd(n_pt, vscale(off, u))
        n_r, p_r = rnd(n_pt), rnd(p_pt)
        d = vsub(dvec(p_r), dvec(n_r))
        dn = vdot(d, dvec(u_r))
        dxyz = to_frame(d)
        near(dn, dec(off), "dn")
        for got, want in zip(dxyz, to_frame_exact(vscale(dec(off), dvec(u)))):
            near(got, want, "dxyz")
        rows.append(
            {
                "expected": {"dn": s(dn), "dx": s(dxyz[0]), "dy": s(dxyz[1]), "dz": s(dxyz[2])},
                "nominal": {"normal": svec(u_r), "point": svec(n_r)},
                "planted_dn": s(off),
                "point": svec(p_r),
            }
        )
    exp_dn = [float(F(r["expected"]["dn"])) for r in rows]
    return {
        "ac": "GDT.DIM.PTDEV AC-1",
        "expected_summary": {
            "max_abs_dn_si": s(max(abs(x) for x in exp_dn)),
            "max_dn_si": s(max(exp_dn)),
            "min_dn_si": s(min(exp_dn)),
        },
        "frame": frame_doc(),
        "license": "CC0-1.0",
        "points": rows,
        "schema": "fid-gdt-dim-ptdev-plane-fixture/2",
    }


# ------------------------------------------------------------------ AC-2: cylinder feature


def tilted(theta, axis):
    """Unit vector at angle theta from +z, tilted towards +y (axis "x") or +x (axis "y")."""
    sn, cs = dsin(theta), dcos(theta)
    if axis == "x":
        return [D(0), sn, cs]
    return [sn, D(0), cs]


def cylinder_feature():
    theta = F(1, 10000)
    offset = [F(1, 100000), F(0), F(0)]  # 0.01 mm in x
    d_act = tilted(theta, "x")
    # Stored axis point 20 mm up the actual axis, so the nearest-point rule is exercised.
    pa = rnd(vadd(dvec(offset), vscale(D("0.02"), d_act)))
    da = rnd(d_act)
    ra = float(F(10003, 1000000))
    pn, dnom, rn = [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.01

    pa_d, da_d, pn_d = dvec(pa), dvec(da), dvec(pn)
    ua = vscale(1 / vnorm(da_d), da_d)
    t = vdot(vsub(pn_d, pa_d), ua)
    nearest = vadd(pa_d, vscale(t, ua))
    loc = vsub(nearest, pn_d)
    ang = unoriented_angle(da_d, dvec(dnom))
    size = dec(ra) - dec(rn)
    for got, want in zip(loc, dvec(offset)):
        near(got, want, "cyl location")
    near(ang, dec(theta), "cyl direction")
    near(size, D("0.000003"), "cyl size")
    return {
        "ac": "GDT.DIM.PTDEV AC-2",
        "actual": {"axis_dir": svec(da), "axis_point": svec(pa), "kind": "cylinder", "radius": s(ra)},
        "expected": {"direction_rad": s(ang), "location_si": svec(loc), "size_si": s(size)},
        "frame": {"id": "world"},
        "license": "CC0-1.0",
        "nominal": {"axis_dir": svec(dnom), "axis_point": svec(pn), "kind": "cylinder", "radius": s(rn)},
        "planted": {"direction_rad": s(theta), "location_si": svec(offset), "size_si": "3e-06"},
        "schema": "fid-gdt-dim-ptdev-cylinder-fixture/2",
    }


# ------------------------------------------------------------------ REQ-004: other features


def feature_case(name, kind, nominal, actual, ref_key, dir_key, size_key, planted):
    loc = to_frame(vsub(dvec(actual[ref_key]), dvec(nominal[ref_key])))
    near_loc = to_frame_exact(planted["offset"])
    for got, want in zip(loc, near_loc):
        near(got, want, f"{name} location")
    exp = {"location_si": svec(loc)}
    if dir_key is None:
        exp["direction_rad"] = None
    else:
        ang = unoriented_angle(dvec(actual[dir_key]), dvec(nominal[dir_key]))
        near(ang, planted["theta"], f"{name} direction")
        exp["direction_rad"] = s(ang)
    if size_key is None:
        exp["size_si"] = None
    else:
        size = dec(actual[size_key]) - dec(nominal[size_key])
        near(size, planted["size"], f"{name} size")
        exp["size_si"] = s(size)

    def enc(f):
        out = {"kind": kind}
        for k, v in f.items():
            out[k] = svec(v) if isinstance(v, list) else s(v)
        return out

    return {"actual": enc(actual), "expected": exp, "name": name, "nominal": enc(nominal)}


def features():
    cases = []
    # Point: offset (3, -4, 12) um.
    off = [F(3, 10**6), F(-4, 10**6), F(12, 10**6)]
    pn = [F(10, 1000), F(20, 1000), F(30, 1000)]
    cases.append(
        feature_case(
            "point",
            "point",
            {"position": rnd(pn)},
            {"position": rnd(vadd(pn, off))},
            "position",
            None,
            None,
            {"offset": dvec(off)},
        )
    )
    # Circle: centre offset (-2, 5, 1) um, normal tilted 2e-4 rad towards +y, radius -1.5 um.
    off = [F(-2, 10**6), F(5, 10**6), F(1, 10**6)]
    cn = [F(20, 1000), F(0), F(5, 1000)]
    theta = F(2, 10000)
    cases.append(
        feature_case(
            "circle",
            "circle",
            {"center": rnd(cn), "normal": [0.0, 0.0, 1.0], "radius": float(F(4, 1000))},
            {
                "center": rnd(vadd(cn, off)),
                "normal": rnd(tilted(theta, "x")),
                "radius": float(F(4, 1000) - F(15, 10**7)),
            },
            "center",
            "normal",
            "radius",
            {"offset": dvec(off), "theta": dec(theta), "size": dec(F(-15, 10**7))},
        )
    )
    # Sphere: centre offset (4, 4, -2) um, radius +2.5 um.
    off = [F(4, 10**6), F(4, 10**6), F(-2, 10**6)]
    sn = [F(-10, 1000), F(15, 1000), F(0)]
    cases.append(
        feature_case(
            "sphere",
            "sphere",
            {"center": rnd(sn), "radius": float(F(125, 10000))},
            {"center": rnd(vadd(sn, off)), "radius": float(F(125, 10000) + F(25, 10**7))},
            "center",
            None,
            "radius",
            {"offset": dvec(off), "size": dec(F(25, 10**7))},
        )
    )
    # Plane: centroid offset (10, -10, 20) um, normal tilted 5e-4 rad towards +x, no size.
    off = [F(10, 10**6), F(-10, 10**6), F(20, 10**6)]
    pc = [F(0), F(0), F(50, 1000)]
    theta = F(5, 10000)
    cases.append(
        feature_case(
            "plane",
            "plane",
            {"centroid": rnd(pc), "normal": [0.0, 0.0, 1.0]},
            {"centroid": rnd(vadd(pc, off)), "normal": rnd(tilted(theta, "y"))},
            "centroid",
            "normal",
            None,
            {"offset": dvec(off), "theta": dec(theta)},
        )
    )
    return {
        "cases": cases,
        "frame": frame_doc(),
        "license": "CC0-1.0",
        "req": "GDT.DIM.PTDEV REQ-004, REQ-005",
        "schema": "fid-gdt-dim-ptdev-features-fixture/1",
    }


def render(doc):
    return json.dumps(doc, indent=1, sort_keys=True) + "\n"


def main(argv):
    outputs = {
        "ptdev_plane.json": render(plane_points()),
        "ptdev_cylinder_feature.json": render(cylinder_feature()),
        "ptdev_features.json": render(features()),
    }
    check = "--check" in argv[1:]
    bad = 0
    for name, text in outputs.items():
        path = HERE / name
        if check:
            have = path.read_text(encoding="utf-8") if path.exists() else None
            if have != text:
                print(f"{name}: stale (run make_ptdev.py)", file=sys.stderr)
                bad += 1
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
    if check and bad == 0:
        print("ptdev fixtures up to date")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
