#!/usr/bin/env python3
"""Independent exact reference for the cylindrical path of GDT.DRF.BUILD (stdlib only).

Writes `cases.json`: DRFs built from the measured points of planar faces and external
cylindrical bosses of a synthetic plate, in precedence order (AC-2: plane A, axis B
perpendicular to it; AC-4: a single primary axis; and the other precedences of an axis datum),
and the refusals the leaf's rules label by hand. It also writes the DMT table input of the
command tests, `frame_table_minimax_mcc.json` (one synthetic `datum_frame/rmb` row whose chain
names both simulators); it is an input, not an expected value, and not an interpretation of any
standard.

Nothing here runs the Rust code. Every value is computed from the exact rational value of each
binary64 coordinate written to the fixture, in `Fraction` or in `decimal` at 60 digits:

- **Planes** use the exact tangential-plane references of `../measured/make_cases.py`
  (imported): the free normal by enumerating every point-triple and edge-edge direction, the
  normal held perpendicular to an axis by enumerating every point-pair direction (redone here in
  `decimal` when that axis is a cylinder's, whose direction is not rational), and the fixed
  normal's exact support value.
- **A cylinder with a free direction** (a primary axis) is a planted minimum circumscribed
  cylinder: five contacts whose KKT multipliers are at least 0.05 and every other point inside
  by at least 20 um, recomputed on the rounded points by the Newton refinement of
  `../../../fit/make_mcc_cyl_fixtures.py` (imported; 60-digit `decimal`, no libm), which also
  rechecks the multipliers. The axis point is the foot of the points' centroid on the axis.
- **A cylinder with a fixed direction** `u` (held by the frame) is the smallest circle
  enclosing the points projected on the plane normal to `u`: every circle through two points
  (diametral) or three points (circumscribed) is a candidate, and the smallest one containing
  every point wins. It must be a three-point circle whose centre has barycentric weights of at
  least 0.05 (an acute triangle: a strict minimum with positive multipliers) and every other
  point must be inside it by at least 1e-7 m; seeds are redrawn until that holds, so the case
  is well posed for a certifying solver. The axis is the line through the centre along `u`.
- **The frame** follows the leaf's pinned rules (stated per case in `frame_*` below, not by a
  generic builder): `z` the primary direction; a cylinder held parallel to `z` moves the origin
  across `z` onto its axis, or, when that is already fixed, sets `x` towards its axis; a plane
  normal to `z` moves the origin along `z` onto it; a plane parallel to `z` sets `y` to its
  normal; with no clocking datum, `x` is the world axis least aligned with `z` made
  perpendicular to it. The DoF sets are hand-labelled from REQ-003 per case.

Seeded faults (wrong precedence involving cylinders) are visible by construction: `ab` against
`ba` (the boss is tilted 3e-4 rad from the face normal) and `abc` against `acb` (origin and
clocking swap between the two bosses); the scripts assert the frames differ by far more than
the 1e-12 test tolerance. `ab` also records `free_axis_origin`, the origin a build would get if
it used the boss's free-direction envelope instead of holding it perpendicular to A.

With `--check` the files are rebuilt in memory and compared byte for byte with the committed ones.
All geometry is synthetic, in metres; no standards text; CC0.
"""

import importlib.util
import json
import random
import sys
from decimal import Decimal, getcontext
from fractions import Fraction as F
from itertools import combinations
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "cases.json"
TABLE = HERE / "frame_table_minimax_mcc.json"


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# The planar reference (exact tangential planes, the rational placement of the block).
meas = _load("drf_measured_reference", HERE.parent / "measured" / "make_cases.py")
# The minimum circumscribed cylinder reference (Decimal Newton, KKT multipliers, sin/cos).
mcc = _load("mcc_cyl_reference", HERE.parent.parent.parent / "fit" / "make_mcc_cyl_fixtures.py")
getcontext().prec = 60

MINIMAX = meas.MINIMAX
MCC = "fit.cylinder.mc@1.0.0"
ROW = meas.ROW
UM = Decimal("1e-6")
INSIDE_3D = 10 * UM  # every non-contact point inside the free envelope by at least this
GAP_2D = Decimal("1e-7")  # every non-contact point inside the fixed-direction circle by this
MIN_WEIGHT = Decimal("0.05")


def D(x):
    """Exact Decimal of a Fraction (60 digits) or pass-through."""
    if isinstance(x, Decimal):
        return x
    x = F(x)
    return Decimal(x.numerator) / Decimal(x.denominator)


def dv(v):
    return [D(c) for c in v]


def ddot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def dcross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dunit(v):
    n = ddot(v, v).sqrt()
    return [c / n for c in v]


def dsub(a, b):
    return [a[k] - b[k] for k in range(3)]


def dadd(a, b, s=Decimal(1)):
    return [a[k] + s * b[k] for k in range(3)]


def reject(a, u):
    t = ddot(a, u)
    return [a[k] - t * u[k] for k in range(3)]


def sign_toward(u, nominal):
    return [-c for c in u] if ddot(u, nominal) < 0 else u


def text(d):
    return format(D(d), ".25e")


def ftext(x):
    return repr(float(x))


def default_clock(z):
    k = min(range(3), key=lambda i: (abs(z[i]), i))
    e = [Decimal(int(i == k)) for i in range(3)]
    x = dunit(reject(e, z))
    return x, dcross(z, x)


# ---------------------------------------------------------------- the plate (local frame)
# Face A (top, z = 0, outward +z), face S (side near y = 0, outward -y, tilted 2e-4 rad) and
# face E (bottom, z = -0.05, outward -z) are the block faces A, B and A2 of the planar reference.
# Bosses stand on A: B at (0.03, 0.04), radius 6 mm, and C at (0.075, 0.02), radius 4 mm, from
# 2 mm to 12 mm above A, their axes tilted from A's normal by about 3e-4 rad.


def local_face(label):
    pts, nominal = meas.FACES[label]()
    return pts, nominal


def to_frac(v):
    return tuple(F(c) if not isinstance(c, Decimal) else F(c) for c in v)


def placed(local_pts):
    """Binary64 points of local exact points (placed by the planar reference's rotation)."""
    floats = [tuple(float(c) for c in meas.place(to_frac(p))) for p in local_pts]
    exact = [tuple(F(c) for c in p) for p in floats]
    return floats, exact


def world_dir(v):
    return meas.mat(meas.ROT, to_frac(v))


def boss(name, cx, cy, r, tilt, n):
    """A planted external cylinder (module documentation): local contact and inside points,
    and the redraw attempt that made every check pass."""
    for attempt in range(200):
        rng = random.Random(f"gdt-drf-cyl/{name}/{attempt}")
        d = mcc.unit([tilt[0], tilt[1], Decimal(1)])
        e1, e2 = mcc.basis(d)
        p0 = [cx, cy, Decimal("0.007")]
        length = Decimal("0.010")
        while True:
            angles = [2 * mcc.PI * mcc.u01(rng) for _ in range(5)]
            heights = [length * (mcc.u01(rng) - Decimal("0.5")) for _ in range(5)]
            lam = mcc.kkt(angles, heights)
            if lam is not None and min(lam) >= MIN_WEIGHT:
                break
        contacts = []
        for a, t in zip(angles, heights):
            s, c = mcc.sin_cos(a)
            contacts.append(mcc.at(p0, d, e1, e2, t, r * c, r * s))
        others = []
        for _ in range(n - 5):
            s, c = mcc.sin_cos(2 * mcc.PI * mcc.u01(rng))
            t = length * (mcc.u01(rng) - Decimal("0.5"))
            rr = r - (20 + 40 * mcc.u01(rng)) * UM
            others.append(mcc.at(p0, d, e1, e2, t, rr * c, rr * s))
        order = list(range(n))
        rng.shuffle(order)
        pts = [None] * n
        for k in range(5):
            pts[order[k]] = contacts[k]
        for k, q in enumerate(others):
            pts[order[5 + k]] = q
        yield attempt, pts, sorted(order[:5]), [order[k] for k in range(5)], d, p0, r


def free_envelope(floats, contact_order, d_local, p0_local, r):
    """The exact free-direction envelope of the rounded points (planted, Newton-refined)."""
    pts = [[D(c) for c in p] for p in floats]
    d0 = dv(world_dir(d_local))
    p0 = dv(meas.place(to_frac(p0_local)))
    rd, rp, rr, lam = mcc.reference(pts, contact_order, d0, p0, r)
    if min(lam) < MIN_WEIGHT:
        return None
    for i, q in enumerate(pts):
        if i in contact_order:
            continue
        if mcc.dist(q, rp, rd)[0] > rr - INSIDE_3D:
            return None
    c = mcc.centroid(pts)
    return rd, mcc.foot(c, rp, rd), rr


def mec_fixed(floats, u):
    """The smallest circle enclosing the points projected along unit `u` (Decimal): returns
    (centre on the plane through the world origin normal to u, radius, contacts) or None when it
    is not a well-posed three-point circle (module documentation)."""
    a, b = mcc.basis(u)
    q = [(ddot(dv(p), a), ddot(dv(p), b)) for p in floats]
    n = len(q)

    def d2(p, c):
        return (p[0] - c[0]) ** 2 + (p[1] - c[1]) ** 2

    cands = []
    for i, j in combinations(range(n), 2):
        c = ((q[i][0] + q[j][0]) / 2, (q[i][1] + q[j][1]) / 2)
        cands.append((d2(q[i], c), c, (i, j)))
    for i, j, k in combinations(range(n), 3):
        ax, ay = q[i]
        bx, by = q[j][0] - ax, q[j][1] - ay
        cx, cy = q[k][0] - ax, q[k][1] - ay
        det = 2 * (bx * cy - by * cx)
        if det == 0:
            continue
        b2, c2 = bx * bx + by * by, cx * cx + cy * cy
        c = (ax + (cy * b2 - by * c2) / det, ay + (bx * c2 - cx * b2) / det)
        cands.append((d2(q[i], c), c, (i, j, k)))
    cands.sort(key=lambda t: t[0])
    slack = Decimal("1e-40")
    for r2, c, who in cands:
        if all(d2(p, c) <= r2 + slack for p in q):
            break
    else:
        raise SystemExit("no enclosing circle")
    if len(who) != 3:
        return None
    r = r2.sqrt()
    # Barycentric weights of the centre in the contact triangle (the multipliers).
    (x1, y1), (x2, y2), (x3, y3) = (q[k] for k in who)
    det = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
    w1 = ((y2 - y3) * (c[0] - x3) + (x3 - x2) * (c[1] - y3)) / det
    w2 = ((y3 - y1) * (c[0] - x3) + (x1 - x3) * (c[1] - y3)) / det
    if min(w1, w2, 1 - w1 - w2) < MIN_WEIGHT:
        return None
    for i, p in enumerate(q):
        if i not in who and d2(p, c).sqrt() > r - GAP_2D:
            return None
    centre = [c[0] * a[k] + c[1] * b[k] for k in range(3)]
    return centre, r, sorted(who)


# ---------------------------------------------------------------- the datums

DATA = {}


def plane_datum(label, face):
    floats, exact, nom, nom_f = meas.measured(face)
    DATA[label] = {"kind": "plane", "floats": floats, "exact": exact, "nominal": nom, "nominal_f": nom_f}


def nominal_axis():
    nom = world_dir((F(0), F(0), F(1)))
    return nom, tuple(float(c) for c in nom)


def boss_datum(label, cx, cy, r, tilt, n, need_free, fixed_dirs):
    nom, nom_f = nominal_axis()
    for attempt, local, contacts, order, d, p0, rr in boss(label, cx, cy, r, tilt, n):
        floats, exact = placed(local)
        env = free_envelope(floats, order, d, p0, rr) if need_free else None
        if need_free and env is None:
            continue
        fixed = {}
        for key, u in fixed_dirs(env).items():
            m = mec_fixed(floats, u)
            if m is None:
                break
            fixed[key] = (u, m)
        else:
            DATA[label] = {
                "kind": "cylinder",
                "side": "external",
                "floats": floats,
                "nominal": nom,
                "nominal_f": nom_f,
                "attempt": attempt,
                "free": env,
                "free_contacts": contacts,
                "fixed": fixed,
            }
            return
    raise SystemExit(f"{label}: no attempt passed")


def oval_datum(label, n):
    """An oval boss on A (axis exactly the local z through (0.05, 0.06), radius 5 mm, two-lobe
    ovality 20 um, 2 mm to 12 mm above A): four exact contacts at the major-axis ends of both end
    faces, every other point inside by 0.5 to 2 um. Its envelope's active set is degenerate (four
    rows for five parameters, two in projection for three), so `KRN.ENV.MCC.CYL` refuses it
    (`solver`, the leaf's documented fail-closed outcome; hand-labelled)."""
    rng = random.Random(f"gdt-drf-cyl/{label}")
    r, amp = Decimal("0.005"), 20 * UM
    pts = []
    for t in (Decimal("0.002"), Decimal("0.012")):
        for sg in (1, -1):
            pts.append([Decimal("0.05") + sg * (r + amp), Decimal("0.06"), t])
    for _ in range(n - 4):
        th = 2 * mcc.PI * mcc.u01(rng)
        s, c = mcc.sin_cos(th)
        _, c2 = mcc.sin_cos(2 * th)
        t = Decimal("0.002") + Decimal("0.010") * mcc.u01(rng)
        rr = r + amp * c2 - (Decimal("0.5") + Decimal("1.5") * mcc.u01(rng)) * UM
        pts.append([Decimal("0.05") + rr * c, Decimal("0.06") + rr * s, t])
    floats, _ = placed(pts)
    nom, nom_f = nominal_axis()
    DATA[label] = {"kind": "cylinder", "side": "external", "floats": floats, "nominal": nom, "nominal_f": nom_f}


def arbitrary_cylinder(label, source, nominal_local, side):
    """A refusal case's cylinder: the points of `source` with another nominal axis or side
    (the refusal happens before any fit)."""
    nom = world_dir(nominal_local)
    DATA[label] = {
        "kind": "cylinder",
        "side": side,
        "floats": DATA[source]["floats"],
        "nominal": nom,
        "nominal_f": tuple(float(c) for c in nom),
    }


# ---------------------------------------------------------------- planes against frames


def free_plane(label):
    d = DATA[label]
    n, num, nn, hi = meas.free_plane(d["exact"], d["nominal"], MINIMAX)
    c = tuple(sum((p[k] for p in d["exact"]), F(0)) / len(d["exact"]) for k in range(3))
    s = (hi - meas.dot(n, c)) / nn
    origin = tuple(c[k] + s * n[k] for k in range(3))
    return {"n": n, "objective": D(num) / D(nn).sqrt(), "origin": origin, "hi": hi, "nn": nn}


def fixed_plane_along(label, z):
    """Plane with its normal fixed at the frame's z (Decimal), signed by the nominal: the exact
    support value `max z.p` and the minimax objective."""
    d = DATA[label]
    n = sign_toward(z, dv(d["nominal"]))
    t = [ddot(n, dv(p)) for p in d["exact"]]
    return {"n": n, "hi": max(t), "objective": max(t) - min(t)}


def about_axis_dec(label, z):
    """Plane held perpendicular to `z` (Decimal): every point-pair candidate `z x (pj - pi)`,
    minimax objective; the window ends must be worse."""
    d = DATA[label]
    pts = [dv(p) for p in d["exact"]]
    nom = dv(d["nominal"])

    def objective(n):
        t = [ddot(n, p) for p in pts]
        return (max(t) - min(t)) / ddot(n, n).sqrt()

    best = None
    for a, b in combinations(pts, 2):
        n = dcross(z, dsub(b, a))
        s = ddot(n, nom)
        if ddot(n, n) == 0 or s == 0:
            continue
        if s < 0:
            n = [-c for c in n]
        o = objective(n)
        if best is None or o < best[1]:
            best = (n, o)
    end = dcross(z, nom)
    for e in (end, [-c for c in end]):
        assert best[1] < objective(e), "window end not worse"
    return {"n": dunit(best[0]), "objective": best[1]}


# ---------------------------------------------------------------- frames per case


def cyl_fixed(label, key):
    u, (centre, r, contacts) = DATA[label]["fixed"][key]
    return u, centre, r, contacts


def axis_point(label, centre, u):
    """The foot of the feature's centroid on the axis through `centre` along `u` (the point a
    minimum circumscribed cylinder reports)."""
    pts = [dv(p) for p in DATA[label]["floats"]]
    c = [sum(p[k] for p in pts) / len(pts) for k in range(3)]
    return dadd(centre, u, ddot(c, u))


def result(origin, x, y, z, dofs, free, datums):
    return {
        "axes": [[text(v) for v in ax] for ax in (x, y, z)],
        "datums": datums,
        "dofs": dofs,
        "free": free,
        "origin": [text(v) for v in origin],
    }


def plane_row(orientation, optimality, objective):
    return {"kind": "plane", "objective": text(objective), "optimality": optimality, "orientation": orientation}


def cyl_row(orientation, optimality, radius, contacts, point, direction):
    return {
        "contacts": contacts,
        "direction": [text(v) for v in direction],
        "kind": "cylinder",
        "objective": text(radius),
        "optimality": optimality,
        "orientation": orientation,
        "point": [text(v) for v in point],
    }


P_A = ["tz", "rx", "ry"]
P_AXIS = ["tx", "ty", "rx", "ry"]


def frame_b():
    """AC-4: a single primary axis. Origin the foot of B's centroid on its axis; free tz, rz."""
    rd, foot, rr = DATA["B"]["free"]
    z = sign_toward(dunit(rd), dv(DATA["B"]["nominal"]))
    x, y = default_clock(z)
    row = cyl_row("free", "strict_local_cross_checked", rr, DATA["B"]["free_contacts"], foot, z)
    return result(foot, x, y, z, [P_AXIS], ["tz", "rz"], [row]), (foot, z)


def frame_a_cyl(second):
    """Plane A, then a boss held parallel to A's normal: the origin moves across z onto the
    boss axis (it constrains tx, ty); no datum clocks the frame."""
    a = free_plane("A")
    z = dunit(dv(a["n"]))
    u, centre, r, contacts = cyl_fixed(second, "zA")
    o = dadd(centre, u, ddot(dv(a["origin"]), u))
    x, y = default_clock(z)
    rows = [
        plane_row("free", "strict_local", a["objective"]),
        cyl_row("fixed", "global", r, contacts, axis_point(second, centre, u), sign_toward(u, dv(DATA[second]["nominal"]))),
    ]
    return o, z, rows


def frame_ab():
    o, z, rows = frame_a_cyl("B")
    x, y = default_clock(z)
    return result(o, x, y, z, [P_A, ["tx", "ty"]], ["rz"], rows)


def clock_toward(o, z, label, key):
    """A boss parallel to z when tx, ty are fixed: x towards its axis (it constrains rz)."""
    u, centre, r, contacts = cyl_fixed(label, key)
    foot = dadd(centre, u, ddot(o, u))
    x = dunit(reject(dsub(foot, o), z))
    row = cyl_row("fixed", "global", r, contacts, axis_point(label, centre, u), sign_toward(u, dv(DATA[label]["nominal"])))
    return x, dcross(z, x), row


def frame_abc(second, third):
    o, z, rows = frame_a_cyl(second)
    x, y, row = clock_toward(o, z, third, "zA")
    return result(o, x, y, z, [P_A, ["tx", "ty"], ["rz"]], [], rows + [row])


def frame_ba(with_c):
    """Primary axis B, then plane A with its normal fixed at B's axis (tz), then optionally boss
    C parallel to B (rz)."""
    _, (foot, z) = frame_b()
    rd, _, rr = DATA["B"]["free"]
    a = fixed_plane_along("A", z)
    o = dadd(foot, z, a["hi"] - ddot(a["n"], foot))
    rows = [
        cyl_row("free", "strict_local_cross_checked", rr, DATA["B"]["free_contacts"], foot, z),
        plane_row("fixed", "closed_form", a["objective"]),
    ]
    if not with_c:
        x, y = default_clock(z)
        return result(o, x, y, z, [P_AXIS, ["tz"]], ["rz"], rows)
    x, y, row = clock_toward(o, z, "C", "zB")
    return result(o, x, y, z, [P_AXIS, ["tz"], ["rz"]], [], rows + [row])


def frame_abs():
    """Plane A, boss B, then side face S held perpendicular to z (rz; y its normal)."""
    o, z, rows = frame_a_cyl("B")
    a = free_plane("A")
    n, num, nn, hi = meas.about_axis(DATA["S"]["exact"], a["n"], DATA["S"]["nominal"], MINIMAX)
    y = dunit(dv(n))
    x = dcross(y, z)
    rows = rows + [plane_row("about_z", "closed_form_enumerated", D(num) / D(nn).sqrt())]
    return result(o, x, y, z, [P_A, ["tx", "ty"], ["rz"]], [], rows)


def frame_bs():
    """Primary axis B, then side face S parallel to it, held perpendicular to z (rz only)."""
    _, (foot, z) = frame_b()
    rd, _, rr = DATA["B"]["free"]
    s = about_axis_dec("S", z)
    y = dunit(reject(s["n"], z))
    x = dcross(y, z)
    rows = [
        cyl_row("free", "strict_local_cross_checked", rr, DATA["B"]["free_contacts"], foot, z),
        plane_row("about_z", "closed_form_enumerated", s["objective"]),
    ]
    return result(foot, x, y, z, [P_AXIS, ["rz"]], ["tz"], rows)


# ---------------------------------------------------------------- the cases


def datums_json(labels):
    out = []
    for lab in labels:
        d = DATA[lab]
        j = {
            "kind": d["kind"],
            "label": lab,
            "nominal": [ftext(v) for v in d["nominal_f"]],
            "xyz": [ftext(v) for p in d["floats"] for v in p],
        }
        if d["kind"] == "cylinder":
            j["side"] = d["side"]
        out.append(j)
    return out


def case(name, labels, expect):
    return {"algs": [MINIMAX, MCC], "datums": datums_json(labels), "expect": expect, "name": name, "row": ROW}


def setup():
    plane_datum("A", "A")
    plane_datum("S", "B")
    plane_datum("E", "A2")
    a = free_plane("A")
    z_a = dunit(dv(a["n"]))
    boss_datum("B", Decimal("0.03"), Decimal("0.04"), Decimal("0.006"), (Decimal("3e-4"), Decimal("-1e-4")), 36,
               True, lambda env: {"zA": z_a})
    z_b = sign_toward(dunit(DATA["B"]["free"][0]), dv(DATA["B"]["nominal"]))
    boss_datum("C", Decimal("0.075"), Decimal("0.02"), Decimal("0.004"), (Decimal("-2e-4"), Decimal("2.5e-4")), 30,
               False, lambda env: {"zA": z_a, "zB": z_b})
    oval_datum("O", 40)
    arbitrary_cylinder("H", "B", (F(0), F(0), F(1)), "internal")
    arbitrary_cylinder("L", "B", (F(1), F(0), F(0)), "external")
    arbitrary_cylinder("K", "B", (F(0), F(3, 5), F(4, 5)), "external")


def far(a, b, tol):
    return max(abs(Decimal(p) - Decimal(q)) for u, v in zip(a, b) for p, q in zip(u, v)) > tol


def build():
    DATA.clear()
    setup()
    b, _ = frame_b()
    ab = frame_ab()
    # Seeded fault: a build that used B's free-direction envelope instead of holding it
    # perpendicular to A would put the origin where that axis meets A's plane.
    rd, foot, _ = DATA["B"]["free"]
    a = free_plane("A")
    za = dunit(dv(a["n"]))
    s = (ddot(dv(a["origin"]), za) - ddot(foot, za)) / ddot(rd, za)
    wrong = dadd(foot, rd, s)
    ab["free_axis_origin"] = [text(v) for v in wrong]
    assert far([ab["origin"]], [[text(v) for v in wrong]], Decimal("1e-9"))
    abc = frame_abc("B", "C")
    acb = frame_abc("C", "B")
    ba = frame_ba(False)
    bac = frame_ba(True)
    cases = [
        case("b_axis", ["B"], b),
        case("ab", ["A", "B"], ab),
        case("abc", ["A", "B", "C"], abc),
        case("acb", ["A", "C", "B"], acb),
        case("ba", ["B", "A"], ba),
        case("bac", ["B", "A", "C"], bac),
        case("abs", ["A", "B", "S"], frame_abs()),
        case("bs", ["B", "S"], frame_bs()),
        # Hand-labelled refusals (the leaf's rules, and KRN.ENV.MCC.CYL's documented outcomes).
        case("h_internal", ["H"], {"error": "unsupported_datum_type_p1a", "label": "H"}),
        case("a_h_internal", ["A", "H"], {"error": "unsupported_datum_type_p1a", "label": "H"}),
        case("o_oval", ["O"], {"code": "solver", "error": "simulation_failed", "label": "O"}),
        case("a_o_oval", ["A", "O"], {"code": "solver", "error": "simulation_failed", "label": "O"}),
        case("a_l_lying", ["A", "L"], {"error": "orientation_constraint_unsupported", "label": "L"}),
        case("a_k_oblique", ["A", "K"], {"error": "oblique_datum_unsupported", "label": "K"}),
        case("abe_parallel", ["A", "B", "E"], {"error": "degenerate_datum_pair", "first": "A", "second": "E"}),
    ]
    # The precedence faults are visible.
    for x, y in ((ab, ba), (abc, acb), (abc, bac)):
        assert far(x["axes"], y["axes"], Decimal("1e-6")) or far([x["origin"]], [y["origin"]], Decimal("1e-6"))
    assert far(ab["axes"][2:], ba["axes"][2:], Decimal("1e-5"))
    doc = {
        "attempts": {k: DATA[k]["attempt"] for k in ("B", "C")},
        "cases": cases,
        "schema": "fid.test-drf-cyl/1",
    }
    return (json.dumps(doc, indent=1, sort_keys=True) + "\n").encode("utf-8")


def frame_table():
    row = meas.frame_row(MINIMAX)
    row["default_variant"] = "planar_cylindrical"
    row["notes"] = (
        "Synthetic planar and cylindrical datum frame row for GDT.DRF.BUILD tests; not an interpretation of any standard."
    )
    chain = row["variants"][0]
    chain["variant"] = "planar_cylindrical"
    chain["steps"].append({"alg": MCC, "kind": "association", "params": {}})
    doc = {"rows": [row], "schema": "fid.dmt-table/1", "version": "1.0.0"}
    return (json.dumps(doc, indent=1, sort_keys=True) + "\n").encode("utf-8")


def outputs():
    return {OUT: build(), TABLE: frame_table()}


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
