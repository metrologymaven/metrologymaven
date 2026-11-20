#!/usr/bin/env python3
"""Independent reference for KRN.LSQ.TRAIT (fit contract) fixtures (CC-BY-4.0 project data).

Writes
  fixtures/fit/trait-degenerate-v1/cases.txt   AC-1: 4 degenerate cases and 8 well-posed controls
  fixtures/fit/trait-far-origin-v1/points.txt  AC-2: 1000 points 1 km from the origin, 1 mm spread
  fixtures/fit/trait-encode-v1/result.txt      AC-3: one FitResult and its canonical bytes
  fixtures/fit/trait-rows-v1/fit.ls_plane.json AC-6: the data row of fit.ls_plane@1

Nothing here runs the Rust code (stdlib only). Three independent parts:

1. Data generators. Point coordinates use only IEEE-754 + - * / and math.sqrt (correctly
   rounded on every platform); sines and cosines come from a 50-digit Decimal Taylor series,
   so the bytes do not depend on the platform's libm. Degenerate cases are exactly degenerate
   in real arithmetic (dyadic coordinates on a line or plane).
2. An exact oracle for AC-1: exact rational centroid and scatter matrix (fractions.Fraction),
   eigenvalues by a 80-digit Decimal Jacobi iteration, singular values as their square
   roots, rank by the rule s_i > rank_tol * s_0, condition s_0 / s_(k-1). Angular coverage is
   hand-labelled from the generator's known angles. The oracle knows nothing about the
   blocked QR or the one-sided Jacobi of the Rust code; the test compares with a tolerance.
3. A bit-exact model of the pinned fid_num::red rules (256-element blocks, Neumaier in index
   order, pairwise tree, `s if c == 0 else s + c`; norm2 scaled by a power of two) and of the
   pinned centring rule, for AC-2's centroid, scale exponent and scaled coordinates and AC-3's
   residual statistics. CPython floats are binary64, round-to-nearest-even, never fused.

Run:  python fixtures/fit/make_trait_fixtures.py          (writes the fixtures)
      python fixtures/fit/make_trait_fixtures.py --check  (fails if a fixture is stale)
"""

import math
import random
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
BLOCK = 256
RANK_TOL = 1e-9
MIN_COVERAGE = 0.3490658503988659  # 20 degrees, bits 3fd657184ae74487
PI50 = Decimal("3.14159265358979323846264338327950288419716939937510582097494459")


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return f"{bits(x):016x}"


# ---------------------------------------------------------------- 1. generators


def dsincos(deg):
    """sin and cos of an angle given in degrees (Fraction or int), rounded once to binary64."""
    getcontext().prec = 50
    x = Decimal(Fraction(deg).numerator) / Decimal(Fraction(deg).denominator) * PI50 / 180
    s, c = Decimal(0), Decimal(0)
    term_s, term_c = x, Decimal(1)
    k = 0
    while True:
        s += term_s
        c += term_c
        term_s = -term_s * x * x / ((2 * k + 2) * (2 * k + 3))
        term_c = -term_c * x * x / ((2 * k + 1) * (2 * k + 2))
        k += 1
        if abs(term_s) < Decimal("1e-45") and abs(term_c) < Decimal("1e-45"):
            break
    return float(s), float(c)


N3 = (1 / 3, 2 / 3, 2 / 3)  # unit normal (1, 2, 2) / 3 (rounded components)
E1 = (2 / 3, -2 / 3, 1 / 3)
E2 = (2 / 3, 1 / 3, -2 / 3)


def circle_points(c, r, degs, e1=E1, e2=E2):
    pts = []
    for d in degs:
        s, co = dsincos(d)
        pts.append(tuple(c[k] + r * (co * e1[k] + s * e2[k]) for k in range(3)))
    return pts


def gen_cases():
    rnd = random.Random(20261119)
    cases = []

    # Degenerate 1: 2 points for a plane.
    cases.append(("too_few_plane", "plane", [(0.0, 0.0, 0.0), (0.01, 0.02, 0.0)], ("too_few_points", 3)))

    # Degenerate 2: 100 exactly collinear points (dyadic) for a plane.
    col = []
    for k in range(100):
        t = k / 8 + 3
        col.append((0.5 + t, -1 + 2 * t, 2 + 2 * t))
    cases.append(("collinear_plane", "plane", col, ("collinear", None)))

    # Degenerate 3: 50 exactly coplanar points (dyadic, z = x/2 + y/4 + 1/8) for a sphere.
    cop = []
    for _ in range(50):
        x = rnd.randint(-512, 512) / 1024
        y = rnd.randint(-512, 512) / 1024
        cop.append((x, y, x / 2 + y / 4 + 0.125))
    cases.append(("coplanar_sphere", "sphere", cop, ("coplanar", None)))

    # Degenerate 4: a 10-degree arc (11 points, 30 to 40 degrees) for a 3D circle.
    arc = circle_points((0.1, -0.2, 0.3), 0.05, range(30, 41))
    cases.append(("arc10_circle3", "circle3", arc, ("insufficient_coverage", Fraction(10))))

    # Control 1: 100 points on a tilted plane with 1 um noise.
    pl = []
    for _ in range(100):
        a = rnd.uniform(-0.05, 0.05)
        b = rnd.uniform(-0.05, 0.05)
        e = rnd.uniform(-1e-6, 1e-6)
        pl.append(tuple(0.2 + a * E1[k] + b * E2[k] + e * N3[k] for k in range(3)))
    cases.append(("plane_ok", "plane", pl, ("ok", None)))

    # Control 2: 50 points on a sphere of radius 25 mm.
    sp = []
    while len(sp) < 50:
        v = [rnd.uniform(-1, 1) for _ in range(3)]
        nv = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
        if nv < 0.1 or nv > 1:
            continue
        sp.append(tuple((0.1, 0.2, 0.3)[k] + 0.025 * (v[k] / nv) for k in range(3)))
    cases.append(("sphere_ok", "sphere", sp, ("ok", None)))

    # Control 3: full 3D circle, 36 points every 10 degrees from 5 degrees: coverage 350.
    cases.append(("circle3_full", "circle3", circle_points((0.0, 0.1, -0.1), 0.02, range(5, 360, 10)), ("ok", Fraction(350))))

    # Control 4: 90-degree 3D arc, 19 points every 5 degrees: coverage 90.
    cases.append(("circle3_quarter", "circle3", circle_points((0.3, 0.3, 0.3), 0.04, range(0, 91, 5)), ("ok", Fraction(90))))

    # Control 5: 10 points on a line with 1 um noise across it.
    ln = []
    for k in range(10):
        t = k * 0.01
        e = rnd.uniform(-1e-6, 1e-6)
        ln.append(tuple((1.0, 2.0, 3.0)[j] + t * N3[j] + e * E1[j] for j in range(3)))
    cases.append(("line_ok", "line3", ln, ("ok", None)))

    # Control 6: 60 points on a cylinder (axis (1,2,2)/3, radius 10 mm, length 50 mm).
    cy = []
    for _ in range(60):
        deg = Fraction(rnd.randint(0, 3599), 10)
        s, co = dsincos(deg)
        h = rnd.uniform(-0.025, 0.025)
        cy.append(tuple((0.05, 0.0, 0.1)[k] + h * N3[k] + 0.01 * (co * E1[k] + s * E2[k]) for k in range(3)))
    cases.append(("cylinder_ok", "cylinder", cy, ("ok", None)))

    # Control 7: a single point for a point fit.
    cases.append(("point_ok", "point", [(0.1, 0.2, 0.3)], ("ok", None)))

    # Control 8: 2D circle in the xy plane, 12 points every 30 degrees: coverage 330.
    c2 = circle_points((0.02, 0.01, 0.0), 0.015, range(0, 360, 30), e1=(1.0, 0.0, 0.0), e2=(0.0, 1.0, 0.0))
    cases.append(("circle2_ok", "circle2", c2, ("ok", Fraction(330))))
    return cases


# ---------------------------------------------------------------- 2. exact oracle

REQ = {  # primitive: (min points, rank, coverage) -- the spec's requirement table
    "point": (1, 0, False),
    "line3": (2, 1, False),
    "plane": (3, 2, False),
    "circle2": (3, 2, True),
    "circle3": (3, 2, True),
    "sphere": (4, 3, False),
    "cylinder": (5, 3, False),
}


def singular_values(pts):
    """Exact-data singular values of the centred n x 3 matrix, descending (80-digit Decimal)."""
    getcontext().prec = 80
    n = len(pts)
    fr = [[Fraction(p[k]) for k in range(3)] for p in pts]
    mean = [sum(p[k] for p in fr) / n for k in range(3)]
    d = [[p[k] - mean[k] for k in range(3)] for p in fr]
    s = [[sum(r[i] * r[j] for r in d) for j in range(3)] for i in range(3)]
    a = [[Decimal(x.numerator) / Decimal(x.denominator) for x in row] for row in s]
    for _ in range(100):
        off = sum(a[i][j] * a[i][j] for i in range(3) for j in range(3) if i != j)
        scale = sum(a[i][i] * a[i][i] for i in range(3))
        if off == 0 or off <= scale * Decimal("1e-150"):
            break
        for p, q in ((0, 1), (0, 2), (1, 2)):
            if a[p][q] == 0:
                continue
            theta = (a[q][q] - a[p][p]) / (2 * a[p][q])
            t = (1 if theta >= 0 else -1) / (abs(theta) + (theta * theta + 1).sqrt())
            c = 1 / (t * t + 1).sqrt()
            sn = t * c
            for k in range(3):  # A <- A J (columns p, q)
                akp, akq = a[k][p], a[k][q]
                a[k][p] = c * akp - sn * akq
                a[k][q] = sn * akp + c * akq
            for k in range(3):  # A <- J^T A (rows p, q)
                apk, aqk = a[p][k], a[q][k]
                a[p][k] = c * apk - sn * aqk
                a[q][k] = sn * apk + c * aqk
    lam = sorted((max(a[i][i], Decimal(0)) for i in range(3)), reverse=True)
    return [x.sqrt() for x in lam]


def oracle(prim, pts, expect):
    need_n, need_rank, cov = REQ[prim]
    n = len(pts)
    if n < need_n:
        assert expect[0] == "too_few_points"
        return {"result": "too_few_points", "n": n, "need": need_n}
    sv = singular_values(pts)
    rank = sum(1 for x in sv if x > Decimal(RANK_TOL) * sv[0])
    if need_rank == 0:
        cond = "1"
    elif sv[0] == 0 or sv[need_rank - 1] <= sv[0] * Decimal("1e-30"):
        cond = "inf"
    else:
        cond = f"{float(sv[0] / sv[need_rank - 1]):.17e}"
    out = {"n": n, "rank": rank, "cond": cond}
    if rank < need_rank:
        out["result"] = "collinear" if rank <= 1 else "coplanar"
    elif cov:
        coverage = float(Decimal(expect[1].numerator) / Decimal(expect[1].denominator) * PI50 / 180)
        out["coverage"] = f"{coverage:.17e}"
        out["result"] = "insufficient_coverage" if coverage < MIN_COVERAGE else "ok"
    else:
        out["result"] = "ok"
    assert out["result"] == expect[0], (prim, out, expect)
    return out


def make_degenerate():
    lines = [
        "# KRN.LSQ.TRAIT AC-1: 4 degenerate cases and 8 well-posed controls (units: metres, radians).",
        "# Generated by fixtures/fit/make_trait_fixtures.py (CC-BY-4.0). Do not edit.",
        f"# Default thresholds: rank_tol {RANK_TOL:.17e}, min_coverage {MIN_COVERAGE:.17e}.",
        "# Each case: `case <name> primitive <slug> result <ok|error code>`, then `xyz` (hex binary64,",
        "# point-major), then the oracle values: n, need (too_few_points), rank, cond (`inf`: the exact",
        "# value is infinite, so the code must report a value above 1 / rank_tol), coverage (rad).",
    ]
    for name, prim, pts, expect in gen_cases():
        o = oracle(prim, pts, expect)
        lines.append(f"case {name} primitive {prim} result {o['result']}")
        lines.append("xyz " + " ".join(hx(v) for p in pts for v in p))
        lines.append(f"n {o['n']}")
        for k in ("need", "rank", "cond", "coverage"):
            if k in o:
                lines.append(f"{k} {o[k]}")
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- 3. bit-exact red model


def block_sum(terms):
    s, c = -0.0, -0.0
    for x in terms:
        assert math.isfinite(x)
        t = s + x
        if abs(s) >= abs(x):
            c += (s - t) + x
        else:
            c += (x - t) + s
        s = t
    return (s, c)


def combine(p, q):
    s = p[0] + q[0]
    e = (p[0] - s) + q[0] if abs(p[0]) >= abs(q[0]) else (q[0] - s) + p[0]
    return (s, (p[1] + q[1]) + e)


def tree(pairs):
    if len(pairs) == 1:
        return pairs[0]
    h = len(pairs) // 2
    return combine(tree(pairs[:h]), tree(pairs[h:]))


def red_sum(terms):
    if not terms:
        return 0.0
    s, c = tree([block_sum(terms[i:i + BLOCK]) for i in range(0, len(terms), BLOCK)])
    r = s if c == 0 else s + c
    assert math.isfinite(r)
    return r


def red_mean(xs):
    return red_sum(xs) / len(xs)


def exponent(m):
    b = bits(m)
    biased = (b >> 52) & 0x7FF
    if biased == 0:
        return (b.bit_length() - 1) - 1074
    return biased - 1023


def scale_factors(e):
    if e > 1022:
        return (math.ldexp(1.0, -1022), math.ldexp(1.0, 1022 - e))
    if e >= -1022:
        return (math.ldexp(1.0, -e), 1.0)
    return (math.ldexp(1.0, 1022), math.ldexp(1.0, -e - 1022))


def red_norm2(xs):
    m = 0.0
    for x in xs:
        m = max(m, abs(x))
    if m == 0:
        return 0.0
    e = exponent(m)
    d1, d2 = scale_factors(e)
    sq = []
    for x in xs:
        y = x * d1 * d2
        sq.append(y * y)
    r = math.sqrt(red_sum(sq))
    if e >= -1022:
        return r * math.ldexp(1.0, e)
    return r * math.ldexp(1.0, e + 1022) * math.ldexp(1.0, -1022)


def centre(pts):
    """The pinned centring rule: centroid, scale exponent and scaled coordinates."""
    n = len(pts)
    c = [red_mean([p[k] for p in pts]) for k in range(3)]
    d = [p[k] - c[k] for p in pts for k in range(3)]
    rms = red_norm2(d) / math.sqrt(float(n))
    e = 0 if rms == 0 else exponent(rms)
    s = math.ldexp(1.0, e)
    return c, e, [x / s for x in d]


def make_far_origin():
    rnd = random.Random(1000)
    base = (600.0, 640.0, 480.0)  # |base| = 1000 m exactly
    pts = [tuple(base[k] + rnd.uniform(-0.0005, 0.0005) for k in range(3)) for _ in range(1000)]
    c, e, q = centre(pts)
    # Round trip in the model: q * 2^e + c must be within 1 ulp of each coordinate.
    s = math.ldexp(1.0, e)
    for i, p in enumerate(pts):
        for k in range(3):
            back = q[3 * i + k] * s + c[k]
            assert abs(back - p[k]) <= math.ulp(p[k]), (i, k)
    lines = [
        "# KRN.LSQ.TRAIT AC-2: 1000 seeded points 1 km from the origin with 1 mm spread (metres).",
        "# Generated by fixtures/fit/make_trait_fixtures.py (CC-BY-4.0). Do not edit.",
        "# centroid: red::mean per coordinate; scale_exp: binary exponent of the RMS distance;",
        "# scaled: (x - centroid) / 2^scale_exp, point-major. All binary64 as hex.",
        "n 1000",
        "xyz " + " ".join(hx(v) for p in pts for v in p),
        "centroid " + " ".join(hx(v) for v in c),
        f"scale_exp {e}",
        "scaled " + " ".join(hx(v) for v in q),
    ]
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- AC-3 encoding


def make_encode():
    p = [0.1, -2.5, 1000.0]
    n = [0.0, 0.0, 1.0]
    r = [1e-6, -2e-6, 0.0, -0.0, 5e-7]
    mean = red_mean(r)
    sq5 = math.sqrt(5.0)
    rms = red_norm2(r) / sq5
    dev = [x - mean for x in r]
    std = red_norm2(dev) / sq5
    mx, mn = 1e-6, -2e-6
    f = lambda x: '"' + hx(x) + '"'
    vec = lambda v: "[" + ",".join(f(x) for x in v) + "]"
    enc = (
        '{"algorithm":"fit.plane.ls@1.0.0",'
        '"feature":{"kind":"plane","p":' + vec(p) + ',"n":' + vec(n) + "},"
        '"residuals":' + vec(r) + ","
        '"stats":{"n":"5","rms":' + f(rms) + ',"max":' + f(mx) + ',"min":' + f(mn)
        + ',"mean":' + f(mean) + ',"std_dev":' + f(std) + "},"
        '"diagnostics":{"grad_norm":' + f(3.25e-12) + ',"rank":"3","cond":' + f(math.inf)
        + ',"iterations":"12","stop":"gradient"},'
        '"certificate":{"support":["0","4","17"],"gap":' + f(0.0) + "}}"
    )
    lines = [
        "# KRN.LSQ.TRAIT AC-3: a FitResult (plane, 5 residuals, certificate) and its canonical bytes.",
        "# Generated by fixtures/fit/make_trait_fixtures.py (CC-BY-4.0). Do not edit.",
        "# Inputs: p 0.1 -2.5 1000; n 0 0 -1 (canonicalised to 0 0 1); residuals 1e-6 -2e-6 0 -0 5e-7;",
        "# diagnostics grad_norm 3.25e-12 rank 3 cond inf iterations 12 stop gradient;",
        "# certificate support 0 4 17 gap 0; algorithm fit.plane.ls@1.0.0.",
        "encoded " + enc,
    ]
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- AC-6 data row

QUANTITIES = {("length", "m"), ("angle", "rad"), ("dimensionless", "1"), ("count", "1")}
NUMERIC = {"number", "integer", "number_array", "integer_array", "points"}


def field(name, ty, q=None, required=True):
    out = '{"name":"' + name + '","type":"' + ty + '"'
    if q:
        out += ',"x-quantity":"' + q[0] + '","x-unit":"' + q[1] + '"'
    return out + ',"required":' + ("true" if required else "false") + "}", (name, ty, q)


def make_row():
    L, D, A, C = ("length", "m"), ("dimensionless", "1"), ("angle", "rad"), ("count", "1")
    params = [
        field("points", "points", L),
        field("normals", "points", D, False),
        field("weights", "number_array", D, False),
        field("seed", "integer", D, False),
        field("constraints.fixed_radius", "number", L, False),
        field("constraints.fixed_axis_direction", "number_array", D, False),
        field("constraints.projection_plane.p", "number_array", L, False),
        field("constraints.projection_plane.n", "number_array", D, False),
        field("thresholds.rank_tol", "number", D),
        field("thresholds.min_coverage", "number", A),
        field("thresholds.eig_ratio_max", "number", D),
        field("nlls.method", "string"),
        field("nlls.max_iter", "integer", C),
        field("nlls.tol_grad", "number", D),
        field("nlls.tol_step", "number", D),
        field("nlls.tol_cost", "number", D),
        field("nlls.cond_limit", "number", D),
        field("nlls.rank_tol", "number", D),
        field("nlls.lm_tau", "number", D),
    ]
    result = [
        field("algorithm", "string"),
        field("feature.p", "number_array", L),
        field("feature.n", "number_array", D),
        field("residuals", "number_array", L),
        field("stats.n", "integer", C),
        field("stats.rms", "number", L),
        field("stats.max", "number", L),
        field("stats.min", "number", L),
        field("stats.mean", "number", L),
        field("stats.std_dev", "number", L),
        field("diagnostics.grad_norm", "number", D),
        field("diagnostics.rank", "integer", C),
        field("diagnostics.cond", "number", D),
        field("diagnostics.iterations", "integer", C),
        field("diagnostics.stop", "string"),
        field("certificate.support", "integer_array", C, False),
        field("certificate.gap", "number", L, False),
    ]
    # Independent validation of the row rules (AC-6): every numeric field declares a known
    # (quantity, unit) pair, no other field declares one, names are unique.
    for lst in (params, result):
        names = [f[1][0] for f in lst]
        assert len(names) == len(set(names))
        for _, (name, ty, q) in lst:
            if ty in NUMERIC:
                assert q in QUANTITIES, name
            else:
                assert q is None, name
    text = (
        '{"row":"fit.command_row@1","command":"fit.ls_plane@1","algorithm":"fit.plane.ls@1.0.0",'
        '"side_effect":"W2","dry_run":"R1","determinism":"D0",'
        '"params":[' + ",".join(f[0] for f in params) + "],"
        '"result":[' + ",".join(f[0] for f in result) + "]}"
    )
    return text + "\n"


def main():
    files = {
        "trait-degenerate-v1/cases.txt": make_degenerate(),
        "trait-far-origin-v1/points.txt": make_far_origin(),
        "trait-encode-v1/result.txt": make_encode(),
        "trait-rows-v1/fit.ls_plane.json": make_row(),
    }
    check = "--check" in sys.argv[1:]
    stale = False
    for name, text in files.items():
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: fixtures/fit/{name}")
                stale = True
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            print(f"wrote fixtures/fit/{name} ({len(data)} bytes)")
    if stale:
        sys.exit(1)


if __name__ == "__main__":
    main()
