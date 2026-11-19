#!/usr/bin/env python3
"""Independent reference for GEO.NURBS.INVERT (CC-BY-4.0 project data).

Stdlib only; shares no code with any Rust crate and never runs the Rust code.

  python fixtures/geo/nurbs/invert/make_invert_fixtures.py --fuzz    (writes fuzz_set.json; slow)
  python fixtures/geo/nurbs/invert/make_invert_fixtures.py           (writes digests-v1.txt)
  python fixtures/geo/nurbs/invert/make_invert_fixtures.py --check   (fails if either is stale)

1. fuzz_set.json: extra entities and queries aimed at the hard cases of the leaf (curvature
   centres, poles, kinks and creases, unclamped and periodic knot vectors, high degrees, near
   ties, random rational curves and surfaces). Every reference comes from the GEO.NURBS.REF
   oracle `tools/fixtures/nurbs_oracle.py` (dense f64 grid search, then 60-digit Decimal
   Newton from every grid local minimum and on every break line, exact Fraction evaluation),
   imported here; queries whose answer is known in closed form (sphere and circle centre and
   axis, sphere radial projection) are checked against it as well.
2. invert_model.py: the bit-exact binary64 model of the algorithm pinned in
   spec/geo/nurbs/invert/NODE.md. Before anything is written, the model is checked against
   the references of ref_set_v1.json, multi_minima.json (GEO.NURBS.REF) and fuzz_set.json:
   foot and distance within 1e-12 m, the boundary flag equal, NonUnique exactly on tied
   queries, NonConvergence exactly where the foot is a cusp tip. The worst errors are printed.
   Known defect of the REF fixture: sphere-q0121 (ref_set_v1) is not the global minimum
   (the radial projection is 2.7e-6 m closer); it is checked against the closed form instead.
3. digests-v1.txt: SHA-256 of the model's results per entity and per file (AC-5). Digest
   text, one line per query in file order, every f64 as 16 lowercase hex digits of its bit
   pattern:
     curve:   c <t> <foot x y z> <distance> <B|I> <status>
     surface: s <u> <v> <foot x y z> <distance> <B|I> <status>
     error:   x <code>
   (B on the boundary, I inside; status C0 certified, C1 certified tie, UB / UI / UC
   unresolved by budget or depth, by a non-converging Newton, or by an uncertified winner.)
"""

import hashlib
import json
import math
import random
import struct
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "tools" / "fixtures"))

import invert_model as IM  # noqa: E402

EM = IM.EM
FUZZ = HERE / "fuzz_set.json"
DIGESTS = HERE / "digests-v1.txt"
FILES = [("ref_set_v1.json", HERE.parent / "ref_set_v1.json"), ("multi_minima.json", HERE.parent / "multi_minima.json"),
         ("invert/fuzz_set.json", FUZZ)]
SEED = 44
# query tags whose answer may honestly be Unresolved (degenerate or flat minima); a certified
# answer must still be the reference
HARD_TAGS = {"centre", "axis", "curvature_centre", "symmetry", "decoy"}
TOL = 1e-12
HARD_FOOT_TOL = 1e-9  # flat minima near curvature centres: the distance is exact, the foot is not


def hx(x):
    return "%016x" % struct.unpack(">Q", struct.pack(">d", x))[0]


# ---------------------------------------------------------------- 1. fuzz set (GEO.NURBS.REF oracle)

def unit(rng):
    """Uniform random unit vector by rejection from the cube: only + - * / and sqrt (correctly
    rounded everywhere), so the bytes do not depend on the platform's libm."""
    while True:
        d = [2.0 * rng.random() - 1.0 for _ in range(3)]
        n2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        if 1e-4 < n2 <= 1.0:
            n = math.sqrt(n2)
            return [x / n for x in d]


def build_fuzz():
    import nurbs_oracle as O

    rng = random.Random(SEED)
    out = []

    def add(sp, queries, per_span, extra=None, expect=None):
        t0 = time.time()
        grid = O.Grid(sp, per_span)
        g = sp.json_geometry()
        if extra:
            g.update(extra)
        recs = []
        for k, (Pf, tag) in enumerate(queries):
            rec = O.invert(sp, grid, Pf, tag)
            rec = {"id": f"{sp.id}-q{k:04d}", **rec}
            if tag in HARD_TAGS:
                rec["allow_unresolved"] = True
            recs.append(rec)
        g["invert"] = recs
        if expect:
            g["expect"] = expect
        out.append(g)
        sys.stderr.write(f"  {sp.id}: {len(recs)} queries, {time.time() - t0:.1f} s\n")
        return g

    def around(sp, n, mags, rng):
        """Points at a random parameter, offset along a random direction by a random magnitude."""
        qs = []
        lo, hi = sp.domain_u()
        for k in range(n):
            u = lo + (hi - lo) * rng.random()
            if sp.surface:
                vlo, vhi = sp.domain_v()
                v = vlo + (vhi - vlo) * rng.random()
                base = [float(x) for x in sp.surf_ders(O.Fraction(u), O.Fraction(v), 0, "frac")[(0, 0)]]
            else:
                base = [float(x) for x in sp.curve_ders(O.Fraction(u), 0, "frac")[0]]
            d = unit(rng)
            nn = 1.0
            mag = mags[k % len(mags)]
            qs.append(([base[i] + mag * d[i] / nn for i in range(3)], "random"))
        return qs

    # curvature centre and axis of the NURBS circle: every point is closest
    circ = O.circle_curve("circle_c")
    add(circ, [([0.0, 0.0, 0.0], "centre"), ([0.0, 0.0, 0.25], "axis"), ([0.0, 0.0, -2.0], "axis")], 200)
    # sphere: centre (all points closest), axis (poles), radial
    sph = O.sphere_surface("sphere_c", 0.5)
    qs = [([0.0, 0.0, 0.0], "centre"), ([0.0, 0.0, 0.75], "axis"), ([0.0, 0.0, -0.3], "axis"),
          ([0.0, 0.0, 1e-7], "axis")]
    for k in range(12):
        rr = 0.5 + (0.1 if k % 2 else -0.1) * rng.random()
        if k < 6:  # near a pole: tilt the axis by up to about 0.006 rad
            e = 1e-6 + 0.006 * rng.random()
            a, b = 2.0 * rng.random() - 1.0, 2.0 * rng.random() - 1.0
            d = [e * a, e * b, -1.0 if k % 3 else 1.0]
            n = math.sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
            d = [x / n for x in d]
        else:
            d = unit(rng)
        qs.append(([rr * d[0], rr * d[1], rr * d[2]], "radial"))
    add(sph, qs, 40)
    # polyline (degree 1, every vertex a kink) with queries outside the vertices
    pts = [[0.0, 0.0, 0.0], [0.3, 0.2, 0.0], [0.5, -0.1, 0.05], [0.8, 0.3, 0.0], [1.0, 0.0, 0.1]]
    poly = O.Spline("polyline", "degree 1 polyline (kinks at the vertices)", 1, [0.0, 0.0, 0.2, 0.45, 0.7, 1.0, 1.0],
                    pts, None)
    qs = [([0.3, 0.35, 0.02], "vertex"), ([0.5, -0.3, 0.05], "vertex"), ([0.8, 0.45, -0.05], "vertex")]
    qs += around(poly, 9, [1e-3, 0.05, 0.2], rng)
    add(poly, qs, 200)
    # C0 crease: biquadratic surface with a double interior u knot (a ridge along u = 0.5)
    net = []
    for i in range(5):
        for j in range(3):
            x = [-0.5, -0.25, 0.0, 0.25, 0.5][i]
            z = [0.0, 0.1, 0.3, 0.1, 0.0][i] + 0.02 * j * j
            net.append([x, -0.4 + 0.4 * j, z])
    crease = O.Spline("crease", "biquadratic surface with a C0 ridge (double knot u = 0.5)", 2,
                      [0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], net, None, degree_v=2, knots_v=[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                      n_u=5, n_v=3)
    qs = [([0.0, -0.2 + 0.1 * k, 0.5 + 0.05 * k], "ridge") for k in range(4)]
    qs += around(crease, 8, [1e-4, 0.05, 0.2], rng)
    add(crease, qs, 40)
    # unclamped curve and surface
    U = O.random_knots(rng, 3, 8, 0.0, 1.0, 0, clamped=False)
    unc = O.Spline("unclamped_c", "unclamped cubic curve", 3, U, O.random_ctrl(rng, 8), O.random_weights(rng, 8))
    add(unc, around(unc, 12, [1e-6, 0.01, 0.1, 0.4], rng), 200)
    Uu = O.random_knots(rng, 2, 5, 0.0, 1.0, 0, clamped=False)
    Vv = O.random_knots(rng, 2, 4, 0.0, 1.0, 0, clamped=False)
    uns = O.Spline("unclamped_s", "unclamped biquadratic surface", 2, Uu, O.height_net(rng, 5, 4), None, degree_v=2,
                   knots_v=Vv, n_u=5, n_v=4)
    add(uns, around(uns, 12, [1e-6, 0.01, 0.1, 0.4], rng), 40)
    # periodic closed cubic (flag set; the geometry is an ordinary unclamped B-spline)
    h3 = math.sqrt(3.0) / 2.0  # exact hexagon: no libm trigonometry (portable bytes)
    ring = [[c, s_, 0.1 * (k % 2)] for k, (c, s_) in
            enumerate([(1.0, 0.0), (0.5, h3), (-0.5, h3), (-1.0, 0.0), (-0.5, -h3), (0.5, -h3)])]
    ring += ring[:3]
    per = O.Spline("periodic_c", "closed periodic cubic B-spline", 3, [k / 6.0 - 0.5 for k in range(13)], ring, None)
    add(per, around(per, 12, [1e-5, 0.05, 0.3, 0.9], rng), 200, extra={"periodic": True})
    # high degrees (the ref_set surfaces without inversion queries)
    for ident, p, q, nu, nv, rational in (("hd2502", 25, 2, 26, 4, False), ("hd0425", 4, 25, 5, 26, True),
                                          ("hd1210", 12, 10, 14, 11, False)):
        Uk = O.random_knots(rng, p, nu)
        Vk = O.random_knots(rng, q, nv)
        sp = O.Spline(ident, f"degree ({p}, {q}) surface", p, Uk, O.height_net(rng, nu, nv),
                      O.random_weights(rng, nu * nv) if rational else None, degree_v=q, knots_v=Vk, n_u=nu, n_v=nv)
        add(sp, around(sp, 6, [1e-5, 0.05, 0.3], rng), 6)
    # near ties on the wavy surface: just off the symmetry plane
    wavy = O.wavy_surface()
    wavy.id = "wavy_near"
    qs = []
    for k in range(10):
        y = -0.3 + 0.06 * k
        qs.append(([(1e-9 if k % 2 else -1e-9) * (1 + k), y, 0.1 + 0.01 * k], "near_tie"))
    add(wavy, qs, 48)
    # random curves and surfaces
    for k in range(40):
        p = 1 + int(rng.random() * 8)
        n = p + 1 + int(rng.random() * 5)
        rational = k % 2 == 1
        sp = O.Spline(f"rc{k:02d}", f"random degree {p} curve", p, O.random_knots(rng, p, n, 0.0, 1.0, p if k % 4 == 3 else 0),
                      O.random_ctrl(rng, n), O.random_weights(rng, n) if rational else None)
        add(sp, around(sp, 20, [1e-7, 1e-3, 0.05, 0.3, 1.0], rng), 200)
    for k in range(24):
        p = 1 + int(rng.random() * 4)
        q = 1 + int(rng.random() * 4)
        nu = p + 1 + int(rng.random() * 3)
        nv = q + 1 + int(rng.random() * 3)
        rational = k % 2 == 1
        sp = O.Spline(f"rs{k:02d}", f"random degree ({p}, {q}) surface", p, O.random_knots(rng, p, nu),
                      O.height_net(rng, nu, nv), O.random_weights(rng, nu * nv) if rational else None, degree_v=q,
                      knots_v=O.random_knots(rng, q, nv), n_u=nu, n_v=nv)
        add(sp, around(sp, 20, [1e-7, 1e-3, 0.05, 0.3], rng), 24)
    # review of #271: hard classes (curvature centres, saddles, exact symmetry planes, a decoy)
    r2 = math.sqrt(0.5)
    ell = O.Spline("ellipse", "rational quadratic ellipse a = 2, b = 1", 2, [0, 0, 0, .25, .25, .5, .5, .75, .75, 1, 1, 1],
                   [[2, 0, 0], [2, 1, 0], [0, 1, 0], [-2, 1, 0], [-2, 0, 0], [-2, -1, 0], [0, -1, 0], [2, -1, 0], [2, 0, 0]],
                   [1, r2, 1, r2, 1, r2, 1, r2, 1])
    qs = [([1.5, 0.0, 0.0], "curvature_centre"), ([0.0, 0.0, 0.0], "curvature_centre"), ([0.0, -3.0, 0.0], "curvature_centre")]
    qs += [([1.5, y, 0.0], "curvature_centre") for y in (1e-4, 1e-5, 1e-6, 1e-7, -1e-8)]
    qs += [([x, 1e-7, 0.0], "curvature_centre") for x in (1.49, 1.499, 1.50001, 1.501)]
    add(ell, qs, 200)
    scurve = O.Spline("s_curve", "cubic S-curve (one inflection)", 3, [0, 0, 0, 0, 0.5, 1, 1, 1, 1],
                      [[0, 0, 0], [0.3, 0.6, 0], [0.5, 0.0, 0], [0.7, -0.6, 0], [1.0, 0, 0]], None)
    qs = []
    for t in (0.12, 0.2, 0.33, 0.7, 0.85):
        C = scurve.curve_ders(O.Fraction(t), 2, "frac")
        d1 = [float(x) for x in C[1]]
        d2 = [float(x) for x in C[2]]
        P0 = [float(x) for x in C[0]]
        cr = d1[0] * d2[1] - d1[1] * d2[0]
        sp2 = d1[0] * d1[0] + d1[1] * d1[1]
        k = cr / (sp2 * math.sqrt(sp2))
        nrm = [-d1[1] / math.sqrt(sp2), d1[0] / math.sqrt(sp2), 0.0]
        c = [P0[i] + nrm[i] / k for i in range(3)]
        qs.append((c, "curvature_centre"))
        qs.append(([c[0] + 1e-7, c[1], 0.0], "curvature_centre"))
    add(scurve, qs, 400)
    sad = O.Spline("saddle", "biquadratic saddle z = x^2 - y^2", 2, [0, 0, 0, 1, 1, 1],
                   [[-1 + i, -1 + j, [1, -1, 1][i] - [1, -1, 1][j]] for i in range(3) for j in range(3)], None,
                   degree_v=2, knots_v=[0, 0, 0, 1, 1, 1], n_u=3, n_v=3)
    qs = [([0.1, 0.0, 0.5], "saddle"), ([0.3, 0.0, 0.5], "saddle"), ([0.1, 0.0, 0.3], "saddle"), ([0.1, 0.05, 0.5], "saddle")]
    qs += [([x, 0.0, z], "symmetry") for x, z in ((0.2, 0.8), (-0.4, 0.4), (0.0, 0.5), (0.6, -0.2))]
    qs += [([0.0, y, z], "symmetry") for y, z in ((0.2, -0.8), (0.4, 0.4), (0.0, -0.5))]
    add(sad, qs, 60)
    wsym = O.wavy_surface()
    wsym.id = "wavy_sym"
    add(wsym, [([0.0, -0.35 + 0.1 * k, 0.05 + 0.03 * k], "symmetry") for k in range(8)], 48)
    zz = 0.4999999
    dec = O.Spline("decoy", "quarter circle near its centre plus a decoy segment", 2,
                   [0, 0, 0, 1 / 3, 1 / 3, 2 / 3, 2 / 3, 1, 1, 1],
                   [[2, 0, 0], [2, 1, 0], [0, 1, 0], [0.75, 1, zz / 2], [1.5, 1, zz], [1.5, (1 + 1e-5) / 2, zz],
                    [1.5, 1e-5, zz]], [1, r2, 1, 1, 1, 1, 1])
    add(dec, [([1.5, y, 0.0], "decoy") for y in (1e-5, 1e-6, 1e-4, 0.0)], 400)
    head = {"format": "fid-nurbs-invert-fuzz/1", "generator": "fixtures/geo/nurbs/invert/make_invert_fixtures.py",
            "oracle": O.ORACLE_VERSION, "seed": SEED, "license": "CC-BY-4.0", "unit": "m"}
    body = json.dumps({**head, "entities": out}, indent=None, separators=(",", ":"))
    return (body + "\n").encode("utf-8")


# ---------------------------------------------------------------- 2. model checks and 3. digests

def closed_form(e, q):
    """(distance, foot) in closed form for the named circle and spheres (Decimal, 60 digits),
    else None. The foot is meaningful only off the centre and axis."""
    from decimal import Decimal, localcontext
    Q = [Decimal(x) for x in EM.jf(q["q"])]
    with localcontext() as ctx:
        ctx.prec = 60
        if e["id"] in ("sphere", "sphere_c"):
            n = (Q[0] * Q[0] + Q[1] * Q[1] + Q[2] * Q[2]).sqrt()
            if n == 0:
                return float(Decimal("0.5")), [0.0, 0.0, 0.0]
            foot = [float(x * Decimal("0.5") / n) for x in Q]
            return float(abs(n - Decimal("0.5"))), foot
        if e["id"] == "circle_c":
            rho = (Q[0] * Q[0] + Q[1] * Q[1]).sqrt()
            d = ((rho - 1) * (rho - 1) + Q[2] * Q[2]).sqrt()
            foot = [float(Q[0] / rho), float(Q[1] / rho), 0.0] if rho != 0 else [1.0, 0.0, 0.0]
            return float(d), foot
    return None


def expect_nonconvergence(e, q):
    # the cusp tip: S_u = 0 at u = 1/2 (GEO.NURBS.REF multi_minima "cusp")
    return e["id"] == "cusp" and EM.jf(q["u"]) == 0.5


def expect_on_boundary(e, q, cf):
    """A reference parameter equals a domain limit (the GEO.NURBS.REF convention; its own flag
    follows the candidate's origin and can disagree on tied queries). Closed-form queries that
    are not tied have no reference parameters (the oracle is not used): never on the boundary
    for the radial queries of the fuzz set."""
    if cf is not None and not expect_nonunique(q):
        return False
    if e["kind"] == "surface":
        pairs = [(EM.jf(q["u"]), EM.jf(e["domain_u"])), (EM.jf(q["v"]), EM.jf(e["domain_v"]))]
    else:
        pairs = [(EM.jf(q["t"]), EM.jf(e["domain"]))]
    return any(x == d[0] or x == d[1] for x, d in pairs)


def expect_nonunique(q):
    return "tie" in q["tags"]


def run_entity(e, worst, problems, counts):
    ent = IM.Entity(e)
    lines = []
    for q in e["invert"]:
        Q = EM.jf(q["q"])
        try:
            if ent.surface:
                params, foot, dist, onb, st = IM.invert_surface(ent, Q)
            else:
                params, foot, dist, onb, st = IM.invert_curve(ent, Q)
        except IM.NonFiniteError:
            lines.append("x NonFinite")
            problems.append(f"{q['id']}: NonFinite")
            continue
        kind = "s" if ent.surface else "c"
        lines.append(" ".join([kind] + [hx(x) for x in params] + [hx(x) for x in foot] + [hx(dist)] +
                              ["B" if onb else "I", st]))
        counts[st] = counts.get(st, 0) + 1
        qid = q["id"]
        if st.startswith("U"):
            # an unresolved answer is allowed only where pinned: the cusp tip (convergence) and
            # the hard degenerate classes; it is never checked for accuracy, only for being a
            # real point no closer than the reference
            if expect_nonconvergence(e, q):
                if st != "UC":
                    problems.append(f"{qid}: status {st}, expected UC")
            elif not q.get("allow_unresolved"):
                problems.append(f"{qid}: status {st}")
            if dist < EM.jf(q["distance"]) - TOL:
                problems.append(f"{qid}: unresolved best closer than the reference")
            if not expect_nonconvergence(e, q):
                continue
        elif expect_nonconvergence(e, q):
            problems.append(f"{qid}: certified at a cusp tip")
        cf = closed_form(e, q)
        if cf is not None and not expect_nonunique(q):
            # closed form: the radial projection (sphere) or the nearest circle point
            rf = cf[1]
            ferr = max(abs(foot[i] - rf[i]) for i in range(3))
            derr = abs(dist - cf[0])
        else:
            rf = EM.jf(q["foot"])
            ferr = max(abs(foot[i] - rf[i]) for i in range(3))
            derr = abs(dist - EM.jf(q["distance"]))
            if cf is not None:
                derr = max(derr, abs(dist - cf[0]))
        worst["foot"] = max(worst["foot"], ferr)
        worst["dist"] = max(worst["dist"], derr)
        ftol = HARD_FOOT_TOL if q.get("allow_unresolved") else TOL
        if ferr > ftol or derr > TOL:
            problems.append(f"{qid}: foot error {ferr:.3e}, distance error {derr:.3e} ({st})")
        if onb != expect_on_boundary(e, q, cf):
            problems.append(f"{qid}: on_boundary {onb}")
        if st.startswith("C") and (st == "C1") != expect_nonunique(q):
            problems.append(f"{qid}: status {st}")
    return "".join(x + "\n" for x in lines)


def digests(only=None):
    out = []
    total = hashlib.sha256()
    worst = {"foot": 0.0, "dist": 0.0}
    problems = []
    counts = {}
    for name, path in FILES:
        if only and name not in only:
            continue
        doc = json.loads(path.read_text(encoding="utf-8"))
        ftotal = hashlib.sha256()
        for e in doc["entities"]:
            if not e["invert"]:
                continue
            t0 = time.time()
            text = run_entity(e, worst, problems, counts).encode("utf-8")
            sys.stderr.write(f"  {name}:{e['id']}: {len(e['invert'])} queries, {time.time() - t0:.1f} s\n")
            ftotal.update(text)
            total.update(text)
            out.append(f"{name}:{e['id']} {hashlib.sha256(text).hexdigest()}")
        out.append(f"{name} {ftotal.hexdigest()}")
    out.append(f"all {total.hexdigest()}")
    print("statuses:", dict(sorted(counts.items())))
    print(f"model vs references: worst foot error {worst['foot']:.3e} m, worst distance error {worst['dist']:.3e} m")
    if problems:
        for p in problems:
            print("PROBLEM", p)
        raise SystemExit(f"{len(problems)} problems")
    body = ("# GEO.NURBS.INVERT AC-5: SHA-256 of the pinned inversion results per entity, per file and in total.\n"
            "# Written by fixtures/geo/nurbs/invert/make_invert_fixtures.py (CC-BY-4.0); do not edit.\n"
            + "".join(x + "\n" for x in out)).encode("utf-8")
    return body


def main():
    args = sys.argv[1:]
    check = "--check" in args
    if "--fuzz" in args:
        body = build_fuzz()
        if check:
            if FUZZ.read_bytes() != body:
                raise SystemExit(f"{FUZZ} is stale")
            print("fuzz_set.json up to date")
        else:
            FUZZ.write_bytes(body)
            print(f"wrote {FUZZ}")
        return
    only = [a.split("=", 1)[1] for a in args if a.startswith("--only=")]
    body = digests(only)
    if only:
        return
    if check:
        if DIGESTS.read_bytes() != body:
            raise SystemExit(f"{DIGESTS} is stale")
        print("digests-v1.txt up to date")
    else:
        DIGESTS.write_bytes(body)
        print(f"wrote {DIGESTS}")


if __name__ == "__main__":
    main()
