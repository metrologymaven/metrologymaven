#!/usr/bin/env python3
"""Independent exact reference for the measured planar path of GDT.DRF.BUILD (stdlib only).

Writes `cases.json`: DRFs built from the measured points of one to three planar datum faces of
a synthetic block, in precedence order, with the tangential plane of each datum in the variant
the DMT row names (external minimax or constrained L1), and the refusals the leaf's rules
label by hand (a parallel secondary, an oblique datum, constrained L2, an unknown simulator).

This script does not mirror the Rust code (which runs `fid-fit` and a convex hull). It works
on the exact rational value of every binary64 coordinate written to the fixture (`Fraction`):

- **Free normal** (primary): every candidate direction of the classical width and support
  arguments is tried: the normal of every point triple, and for minimax also the cross product
  of every pair of point pairs (edge-edge). The objective of a direction `n` (oriented to the
  nominal outward side) is `(max n.p - min n.p) / |n|` (minimax) or
  `(N max n.p - sum n.p) / |n|` (L1); candidates are compared exactly by squared ratios.
- **Held perpendicular to the primary normal `nA`**: candidates `nA x (p_j - p_i)` over every
  point pair (a superset of the hull edges), same objectives; the window ends
  `+-(nA x nominal)` are checked to be worse.
- **Fixed normal** (tertiary): the cross product of the secondary and primary normals, signed
  by the nominal; the offset is the exact maximum of `n.p`.
- The frame follows the leaf's pinned rules: `z` the primary normal, `y` the secondary normal
  (exactly perpendicular to `z`), `x = y x z`; one datum: `x` from the world axis least aligned
  with `z`. The origin is the foot of the primary's point set centroid on the primary plane,
  moved along `y` onto the secondary plane and along `x` onto the tertiary plane, all in exact
  rational arithmetic; unit vectors use `decimal` at 60 digits.

The block is placed by a rotation with exact rational entries (unit quaternion (5, 1, 2, 1)/31
squared norm) and a rational shift. Faces B and C are tilted by 2e-4 and 3e-4 rad from
perpendicular, so the datum precedence changes the frame by far more than the 1e-12 test
tolerance: case `abc_minimax` and its precedence fault `bac_minimax` must differ.

It also writes the DMT table inputs of the command tests (`frame_table_<variant>.json`): one
synthetic `dmt:asme/datum_frame/rmb@1` row whose default chain's association step names the
simulator (minimax, L1, L2, or a non-tangential `syn.assoc.min_zone@0.1.0`). They are inputs,
not expected values, and are not an interpretation of any standard.

With `--check` the files are rebuilt in memory and compared byte for byte with the committed one.
All geometry is synthetic, in metres; no standards text; CC0.
"""

import json
import sys
from decimal import Decimal, getcontext
from fractions import Fraction as F
from itertools import combinations
from pathlib import Path

getcontext().prec = 60
HERE = Path(__file__).resolve().parent
OUT = HERE / "cases.json"

MINIMAX = "fit.plane.tan_minimax@1.0.0"
L1 = "fit.plane.tan_l1@1.0.0"
L2 = "fit.plane.tan_l2@1.0.0"
ROW = "dmt:asme/datum_frame/rmb@1"


def quat_rot(a, b, c, d):
    s = a * a + b * b + c * c + d * d
    m = [
        [a * a + b * b - c * c - d * d, 2 * (b * c - a * d), 2 * (b * d + a * c)],
        [2 * (b * c + a * d), a * a - b * b + c * c - d * d, 2 * (c * d - a * b)],
        [2 * (b * d - a * c), 2 * (c * d + a * b), a * a - b * b - c * c + d * d],
    ]
    return [[F(x, s) for x in row] for row in m]


ROT = quat_rot(5, 1, 2, 1)
SHIFT = (F(1, 8), F(-3, 16), F(1, 4))


def mat(m, v):
    return tuple(sum((m[i][k] * v[k] for k in range(3)), F(0)) for i in range(3))


def place(p):
    q = mat(ROT, p)
    return tuple(q[i] + SHIFT[i] for i in range(3))


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def neg(a):
    return (-a[0], -a[1], -a[2])


def um(k):
    return F(k, 1_000_000)


# ---------------------------------------------------------------- the block (local frame)
# A: top face z = 0, outward +z. Three contacts spanning the face, the rest below it.
A_CONTACTS = [(F(5, 1000), F(5, 1000)), (F(95, 1000), F(10, 1000)), (F(50, 1000), F(75, 1000))]
A_OTHERS = [
    ((F(0), F(0)), 7), ((F(100, 1000), F(0)), 4), ((F(0), F(80, 1000)), 9), ((F(100, 1000), F(80, 1000)), 3),
    ((F(30, 1000), F(40, 1000)), 12), ((F(70, 1000), F(40, 1000)), 6), ((F(50, 1000), F(20, 1000)), 2),
    ((F(45, 1000), F(45, 1000)), 15), ((F(20, 1000), F(70, 1000)), 5),
]
# B: side face near y = 0, outward -y, tilted: contact plane y = TB (z + 0.025); depth is +y.
TB = F(2, 10_000)
B_CONTACTS = [(F(10, 1000), F(-5, 1000)), (F(90, 1000), F(-8, 1000)), (F(50, 1000), F(-45, 1000))]
B_OTHERS = [
    ((F(0), F(0)), 5), ((F(100, 1000), F(0)), 8), ((F(0), F(-50, 1000)), 3), ((F(100, 1000), F(-50, 1000)), 6),
    ((F(30, 1000), F(-25, 1000)), 4), ((F(70, 1000), F(-25, 1000)), 9), ((F(50, 1000), F(-20, 1000)), 11),
]
# C: end face near x = 0, outward -x, tilted: contact plane x = TC (y - 0.04) + TC (z + 0.025).
TC = F(3, 10_000)
C_CONTACTS = [(F(5, 1000), F(-5, 1000)), (F(75, 1000), F(-10, 1000)), (F(40, 1000), F(-45, 1000))]
C_OTHERS = [
    ((F(0), F(0)), 6), ((F(80, 1000), F(0)), 3), ((F(0), F(-50, 1000)), 8), ((F(80, 1000), F(-50, 1000)), 4),
    ((F(40, 1000), F(-25, 1000)), 13), ((F(20, 1000), F(-30, 1000)), 5),
]
# A2: bottom face z = -0.05, outward -z (parallel to A).
A2_CONTACTS = [(F(10, 1000), F(10, 1000)), (F(90, 1000), F(5, 1000)), (F(40, 1000), F(70, 1000))]
A2_OTHERS = [((F(0), F(0)), 4), ((F(100, 1000), F(80, 1000)), 7), ((F(60, 1000), F(40, 1000)), 10)]
# D: a chamfer face, nominal outward (0, -3/5, 4/5): oblique to A.
D_POINTS = [(F(10, 1000), F(0), F(0)), (F(90, 1000), F(0), F(0)), (F(50, 1000), F(-8, 1000), F(-6, 1000))]


def face_a():
    pts = [(s, t, F(0)) for s, t in A_CONTACTS] + [(s, t, -um(k)) for (s, t), k in A_OTHERS]
    return pts, (F(0), F(0), F(1))


def face_b():
    pts = [(x, TB * (z + F(25, 1000)), z) for x, z in B_CONTACTS]
    pts += [(x, TB * (z + F(25, 1000)) + um(k), z) for (x, z), k in B_OTHERS]
    return pts, (F(0), F(-1), F(0))


def face_c():
    def plane_x(y, z):
        return TC * (y - F(40, 1000)) + TC * (z + F(25, 1000))

    pts = [(plane_x(y, z), y, z) for y, z in C_CONTACTS]
    pts += [(plane_x(y, z) + um(k), y, z) for (y, z), k in C_OTHERS]
    return pts, (F(-1), F(0), F(0))


def face_a2():
    pts = [(s, t, F(-50, 1000)) for s, t in A2_CONTACTS] + [(s, t, F(-50, 1000) + um(k)) for (s, t), k in A2_OTHERS]
    return pts, (F(0), F(0), F(-1))


def face_d():
    return D_POINTS, (F(0), F(-3, 5), F(4, 5))


FACES = {"A": face_a, "B": face_b, "C": face_c, "A2": face_a2, "D": face_d}


def measured(label):
    """Placed points as binary64 (what the fixture holds) and their exact values."""
    pts, nominal = FACES[label]()
    floats = [tuple(float(c) for c in place(p)) for p in pts]
    exact = [tuple(F(c) for c in p) for p in floats]
    nom = mat(ROT, nominal)
    return floats, exact, nom, tuple(float(c) for c in nom)


# ---------------------------------------------------------------- exact tangential planes


def objective(n, pts, variant):
    t = [dot(n, p) for p in pts]
    hi = max(t)
    num = hi - min(t) if variant == MINIMAX else len(t) * hi - sum(t)
    return num, dot(n, n), hi


def better(a, b):
    """a < b for (num, nn) meaning num / sqrt(nn), num >= 0."""
    return a[0] * a[0] * b[1] < b[0] * b[0] * a[1]


def best_of(cands, pts, nominal, variant):
    best = None
    for n in cands:
        s = dot(n, nominal)
        if s == 0:
            continue
        if s < 0:
            n = neg(n)
        num, nn, hi = objective(n, pts, variant)
        if best is None or better((num, nn), (best[1], best[2])):
            best = (n, num, nn, hi)
    return best


def free_plane(pts, nominal, variant):
    cands = []
    for a, b, c in combinations(pts, 3):
        n = cross(sub(b, a), sub(c, a))
        if n != (0, 0, 0):
            cands.append(n)
    if variant == MINIMAX:
        pairs = list(combinations(pts, 2))
        for (a, b), (c, d) in combinations(pairs, 2):
            n = cross(sub(b, a), sub(d, c))
            if n != (0, 0, 0):
                cands.append(n)
    return best_of(cands, pts, nominal, variant)


def about_axis(pts, nz, nominal, variant):
    cands = []
    for a, b in combinations(pts, 2):
        n = cross(nz, sub(b, a))
        if n != (0, 0, 0):
            cands.append(n)
    best = best_of(cands, pts, nominal, variant)
    end = cross(nz, nominal)
    for e in (end, neg(end)):
        num, nn, _ = objective(e, pts, variant)
        assert better((best[1], best[2]), (num, nn)), "window end not worse"
    return best


def fixed_plane(pts, n, nominal, variant):
    if dot(n, nominal) < 0:
        n = neg(n)
    num, nn, hi = objective(n, pts, variant)
    return n, num, nn, hi


# ---------------------------------------------------------------- decimal helpers


def dec(x):
    return Decimal(x.numerator) / Decimal(x.denominator)


def dsqrt(x):
    return dec(x).sqrt()


def unit(n):
    s = dsqrt(dot(n, n))
    return [dec(c) / s for c in n]


def text(d):
    return format(d, ".25e")


def ftext(x):
    return repr(float(x))


# ---------------------------------------------------------------- the frame

DOFS = {0: ["tz", "rx", "ry"], 1: ["ty", "rz"], 2: ["tx"]}
FREE = {1: ["tx", "ty", "rz"], 2: ["tx"], 3: []}


def frame(labels, variant):
    planes = []
    data = []
    for i, lab in enumerate(labels):
        floats, exact, nom, nom_f = measured(lab)
        data.append((lab, floats, nom_f))
        if i == 0:
            planes.append(free_plane(exact, nom, variant) + ("free", "strict_local", exact))
        elif i == 1:
            planes.append(about_axis(exact, planes[0][0], nom, variant) + ("about_z", "closed_form_enumerated", exact))
        else:
            n = cross(planes[1][0], planes[0][0])
            planes.append(fixed_plane(exact, n, nom, variant) + ("fixed", "closed_form", exact))
    nz = planes[0][0]
    # Origin: foot of the primary centroid on the primary plane, then onto each later plane
    # along its normal (which is the DRF axis it constrains).
    pts0 = planes[0][6]
    c = tuple(sum((p[k] for p in pts0), F(0)) / len(pts0) for k in range(3))
    o = c
    for n, _num, nn, hi, *_ in planes:
        s = (hi - dot(n, o)) / nn
        o = tuple(o[k] + s * n[k] for k in range(3))
    z = unit(nz)
    if len(planes) == 1:
        k = min(range(3), key=lambda i: (abs(nz[i]), i))
        e = [Decimal(0)] * 3
        e[k] = Decimal(1)
        dz = sum(e[i] * z[i] for i in range(3))
        w = [e[i] - dz * z[i] for i in range(3)]
        s = sum(v * v for v in w).sqrt()
        x = [v / s for v in w]
        y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]]
    else:
        assert dot(planes[1][0], nz) == 0
        y = unit(planes[1][0])
        x = [y[1] * z[2] - y[2] * z[1], y[2] * z[0] - y[0] * z[2], y[0] * z[1] - y[1] * z[0]]
    expect = {
        "axes": [[text(v) for v in ax] for ax in (x, y, z)],
        "dofs": [DOFS[i] for i in range(len(planes))],
        "free": FREE[len(planes)],
        "objectives": [text(dec(p[1]) / dsqrt(p[2])) for p in planes],
        "optimality": [p[5] for p in planes],
        "orientation": [p[4] for p in planes],
        "origin": [text(dec(v)) for v in o],
    }
    return data, expect


def datums_json(data):
    return [
        {"label": lab, "nominal": [ftext(v) for v in nom], "xyz": [ftext(v) for p in floats for v in p]}
        for lab, floats, nom in data
    ]


def ok_case(name, labels, variant):
    data, expect = frame(labels, variant)
    return {"alg": variant, "datums": datums_json(data), "expect": expect, "name": name, "row": ROW}


def err_case(name, labels, variant, error, **fields):
    data = [(lab, measured(lab)[0], measured(lab)[3]) for lab in labels]
    return {"alg": variant, "datums": datums_json(data), "expect": dict(error=error, **fields), "name": name, "row": ROW}


def build():
    cases = [
        ok_case("a_minimax", ["A"], MINIMAX),
        ok_case("ab_minimax", ["A", "B"], MINIMAX),
        ok_case("abc_minimax", ["A", "B", "C"], MINIMAX),
        ok_case("abc_l1", ["A", "B", "C"], L1),
        # Seeded fault (wrong datum precedence): the same faces with B primary.
        ok_case("bac_minimax", ["B", "A", "C"], MINIMAX),
        ok_case("acb_minimax", ["A", "C", "B"], MINIMAX),
        # Hand-labelled refusals (the leaf's rules).
        err_case("a_a2_parallel", ["A", "A2"], MINIMAX, "degenerate_datum_pair", first="A", second="A2"),
        err_case("a_d_oblique", ["A", "D"], MINIMAX, "oblique_datum_unsupported", label="D"),
        err_case("a_l2", ["A"], L2, "simulation_failed", label="A", code="not_certifiable"),
        err_case("ab_l2", ["A", "B"], L2, "simulation_failed", label="A", code="not_certifiable"),
        err_case("unknown_simulator", ["A"], "fit.plane.ls@1.0.0", "unsupported_simulator"),
    ]
    abc = cases[2]["expect"]
    bac = cases[4]["expect"]
    # The fault must be visible: the two frames' z axes differ by about 2e-4 rad.
    dz = max(abs(Decimal(a) - Decimal(b)) for a, b in zip(abc["axes"][2], bac["axes"][2]))
    assert dz > Decimal("1e-5"), dz
    doc = {"cases": cases, "schema": "fid.test-drf-measured/1"}
    return (json.dumps(doc, indent=1, sort_keys=True) + "\n").encode("utf-8")


def frame_table(alg):
    row = {
        "actual_value": "av.synthetic.datum_frame@1",
        "characteristic": "datum_frame",
        "clauses": [{"clause": "0", "designator": "asme-y14.5", "edition": "2018"}],
        "decision_rule_hint": "dr.synthetic.default@1",
        "default_variant": "planar",
        "dialect": "asme",
        "editions": ["y14.5-2018"],
        "key": ROW,
        "limitations": [],
        "modifiers": ["rmb"],
        "notes": "Synthetic planar datum frame row for GDT.DRF.BUILD tests; not an interpretation of any standard.",
        "ors_cases": ["ors:synthetic/datum_frame.001"],
        "review": {"state": "draft"},
        "status": "supported",
        "synthetic": True,
        "variants": [
            {
                "steps": [
                    {"alg": "syn.partition.whole_feature@0.1.0", "kind": "partition", "params": {}},
                    {"alg": "syn.extract.all_points@0.1.0", "kind": "extraction", "params": {"min_points": "3"}},
                    {"alg": alg, "kind": "association", "params": {}},
                ],
                "variant": "planar",
            }
        ],
    }
    doc = {"rows": [row], "schema": "fid.dmt-table/1", "version": "1.0.0"}
    return (json.dumps(doc, indent=1, sort_keys=True) + "\n").encode("utf-8")


TABLES = {"minimax": MINIMAX, "l1": L1, "l2": L2, "minzone": "syn.assoc.min_zone@0.1.0"}


def outputs():
    out = {OUT: build()}
    for name, alg in TABLES.items():
        out[HERE / f"frame_table_{name}.json"] = frame_table(alg)
    return out


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
