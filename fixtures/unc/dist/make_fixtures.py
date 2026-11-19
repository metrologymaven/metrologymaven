#!/usr/bin/env python3
"""Independent reference for UNC.MODEL.DIST fixtures (stdlib only; CC-BY-4.0 data).

All parameter sets are synthetic. Nothing here runs or imports the Rust code.

moments.csv       40 univariate parameter sets with 50-significant-digit references for the
                  expectation and the standard uncertainty, plus the kurtosis used for the
                  standard error of the sample standard deviation (AC-1, AC-2). Rectangular,
                  triangular and trapezoidal moments are exact rational integrals of the
                  piecewise-linear density (fractions.Fraction), not the closed forms the
                  crate uses; arcsine, normal and scaled-t use textbook moments in exact
                  rationals. The square root is decimal.Decimal at 80 digits.
invalid.csv       hand-labelled invalid parameter sets and the parameter each must name
                  (REQ-UNC.MODEL.DIST-002, AC-4).
corr_valid.json   correlation matrices that must pass; corr_invalid.json ones that must fail
                  with the hand-labelled reason (AC-5). Every label is confirmed here by an
                  exact rational test: min eigenvalue >= -1e-12 * n iff C + (1e-12 * n) I is
                  positive semi-definite, decided by exact symmetric elimination in Fractions.
as241.csv         the Wichura AS241 PPND16 coefficients (Applied Statistics 37(3), 1988,
                  477-484), as Python reprs. They are checked here against a 60-digit inverse
                  normal before anything is written; the crate test asserts its constants
                  equal this table.
ppnd.csv          probabilities and their 50-digit inverse normal, by Newton iteration on a
                  Decimal normal CDF (series for |z| <= 3, continued fraction above).

Usage: make_fixtures.py [--check]   (--check compares with the files on disk, writes nothing)
"""

import json
import math
import pathlib
import sys
from decimal import Decimal, getcontext, localcontext
from fractions import Fraction as F

HERE = pathlib.Path(__file__).resolve().parent
getcontext().prec = 80

# --------------------------------------------------------------------------- formatting


def d50(x):
    """A Fraction or Decimal as a 50-significant-digit decimal string."""
    if x == 0:
        return "0"
    with localcontext() as ctx:
        ctx.prec = 80
        d = Decimal(x.numerator) / Decimal(x.denominator) if isinstance(x, F) else +x
    return format(d, ".49E")


def dsqrt(fr):
    with localcontext() as ctx:
        ctx.prec = 80
        return (Decimal(fr.numerator) / Decimal(fr.denominator)).sqrt()


def r(x):
    return repr(float(x))


# --------------------------------------------------------------------------- moments


def piecewise_moments(pieces):
    """pieces: (x0, x1, f0, f1) with a linear density between (Fractions). Returns
    (mean, variance, kurtosis) exactly, after checking total mass 1."""

    def raw(k):
        tot = F(0)
        for x0, x1, f0, f1 in pieces:
            if x1 == x0:
                continue
            beta = (f1 - f0) / (x1 - x0)
            alpha = f0 - beta * x0
            tot += alpha * (x1 ** (k + 1) - x0 ** (k + 1)) / (k + 1)
            tot += beta * (x1 ** (k + 2) - x0 ** (k + 2)) / (k + 2)
        return tot

    m = [raw(k) for k in range(5)]
    assert m[0] == 1, m[0]
    mu = m[1]
    var = m[2] - mu * mu
    c4 = m[4] - 4 * mu * m[3] + 6 * mu * mu * m[2] - 3 * mu**4
    return mu, var, c4 / (var * var)


def moments(kind, p):
    q = [F(x) for x in p]
    if kind == "normal":
        mu, s = q
        return mu, s * s, F(3)
    if kind == "rect":
        a, b = q
        h = 1 / (b - a)
        return piecewise_moments([(a, b, h, h)])
    if kind == "tri":
        a, b, c = q
        h = 2 / (b - a)
        return piecewise_moments([(a, c, F(0), h), (c, b, h, F(0))])
    if kind == "arcsine":
        a, b = q
        # x = m - h cos(theta), theta uniform on (0, pi): E[cos^2] = 1/2, E[cos^4] = 3/8.
        return (a + b) / 2, (b - a) ** 2 / 8, F(3, 2)
    if kind == "t":
        mu, s, nu = q
        k = 3 + 6 / (nu - 4) if nu > 4 else None
        return mu, s * s * nu / (nu - 2), k
    if kind == "trap":
        a, b, beta = q
        m, h = (a + b) / 2, (b - a) / 2
        top = 1 / (h * (1 + beta))
        mean, var, kurt = piecewise_moments(
            [(a, m - beta * h, F(0), top), (m - beta * h, m + beta * h, top, top), (m + beta * h, b, top, F(0))]
        )
        # Cross-check against the GUM-style closed form h^2 (1 + beta^2) / 6.
        assert var == h * h * (1 + beta * beta) / 6
        return mean, var, kurt
    raise ValueError(kind)


MOMENT_SETS = [
    ("normal", "si:m", (0.0, 1.0)),
    ("normal", "si:m", (10.5, 0.25)),
    ("normal", "si:m", (-3.2e-3, 1.5e-6)),
    ("normal", "si:m", (1.0e5, 3.0)),
    ("normal", "si:rad", (0.7853981633974483, 1e-4)),
    ("normal", "si:1", (-42.0, 1e-9)),
    ("rect", "si:m", (0.0, 1.0)),
    ("rect", "si:m", (-1.0, 1.0)),
    ("rect", "si:m", (10.0, 10.002)),
    ("rect", "si:m", (-2.5e-3, 1.7e-3)),
    ("rect", "si:rad", (0.1, 0.3)),
    ("rect", "si:1", (1e-9, 3e-9)),
    ("tri", "si:m", (0.0, 1.0, 0.5)),
    ("tri", "si:m", (0.0, 1.0, 0.0)),
    ("tri", "si:m", (0.0, 1.0, 1.0)),
    ("tri", "si:m", (-1.0, 2.0, 0.25)),
    ("tri", "si:m", (10.0, 10.01, 10.007)),
    ("tri", "si:rad", (-3e-3, -1e-3, -2.5e-3)),
    ("tri", "si:1", (-1.0, 1.0, 0.0)),
    ("arcsine", "si:m", (0.0, 1.0)),
    ("arcsine", "si:m", (-1.0, 1.0)),
    ("arcsine", "si:m", (2.0, 2.000002)),
    ("arcsine", "si:m", (-5e-4, 5e-4)),
    ("arcsine", "si:rad", (0.1, 0.7)),
    ("arcsine", "si:1", (100.0, 101.0)),
    ("t", "si:m", (0.0, 1.0, 3.0)),
    ("t", "si:m", (0.0, 1.0, 5.0)),
    ("t", "si:m", (1.5, 0.2, 10.0)),
    ("t", "si:m", (-2e-3, 5e-6, 30.0)),
    ("t", "si:m", (0.0, 1.0, 2.5)),
    ("t", "si:rad", (100.0, 0.01, 9.0)),
    ("t", "si:1", (0.0, 1.0, 100.0)),
    ("trap", "si:m", (0.0, 1.0, 0.0)),
    ("trap", "si:m", (0.0, 1.0, 1.0)),
    ("trap", "si:m", (0.0, 1.0, 0.5)),
    ("trap", "si:m", (-1.0, 1.0, 0.25)),
    ("trap", "si:m", (10.0, 10.004, 0.75)),
    ("trap", "si:rad", (-2e-3, 2e-3, 0.1)),
    ("trap", "si:1", (0.3, 0.9, 0.9)),
    ("trap", "si:m", (5.0, 6.0, 0.333)),
]


def moments_csv():
    lines = ["id,kind,unit,p1,p2,p3,expectation,std_uncertainty,kurtosis"]
    for i, (kind, unit, p) in enumerate(MOMENT_SETS):
        mean, var, kurt = moments(kind, p)
        ps = [r(x) for x in p] + [""] * (3 - len(p))
        k = d50(kurt) if kurt is not None else "inf"
        lines.append(",".join([f"m{i:02d}", kind, unit, *ps, d50(mean), d50(dsqrt(var)), k]))
    return "\n".join(lines) + "\n"


# --------------------------------------------------------------------------- invalid sets

# kind, ((value, unit), ...), expected parameter name; labelled by hand from REQ-002 and the
# check order pinned in the NODE.md Design (per parameter in order: finite, unit; then the
# relations).
INVALID = [
    ("normal", (("0.0", "si:m"), ("0.0", "si:m")), "sigma"),
    ("normal", (("0.0", "si:m"), ("-1.0", "si:m")), "sigma"),
    ("normal", (("NaN", "si:m"), ("1.0", "si:m")), "mu"),
    ("normal", (("0.0", "si:m"), ("inf", "si:m")), "sigma"),
    ("normal", (("0.0", "si:m"), ("1.0", "si:rad")), "sigma"),
    ("rect", (("1.0", "si:m"), ("1.0", "si:m")), "b"),
    ("rect", (("2.0", "si:m"), ("1.0", "si:m")), "b"),
    ("rect", (("0.0", "si:m"), ("-inf", "si:m")), "b"),
    ("rect", (("0.0", "si:m"), ("1.0", "si:1")), "b"),
    ("rect", (("-1.7976931348623157e308", "si:m"), ("1.7976931348623157e308", "si:m")), "b"),
    ("tri", (("0.0", "si:m"), ("1.0", "si:m"), ("1.5", "si:m")), "c"),
    ("tri", (("0.0", "si:m"), ("1.0", "si:m"), ("-0.5", "si:m")), "c"),
    ("tri", (("1.0", "si:m"), ("0.0", "si:m"), ("0.5", "si:m")), "b"),
    ("tri", (("0.0", "si:m"), ("1.0", "si:m"), ("NaN", "si:m")), "c"),
    ("tri", (("0.0", "si:rad"), ("1.0", "si:m"), ("0.5", "si:m")), "b"),
    ("tri", (("0.0", "si:m"), ("1.0", "si:m"), ("0.5", "si:1")), "c"),
    ("arcsine", (("0.0", "si:m"), ("0.0", "si:m")), "b"),
    ("arcsine", (("inf", "si:m"), ("1.0", "si:m")), "a"),
    ("arcsine", (("0.0", "si:1"), ("1.0", "si:m")), "b"),
    ("t", (("0.0", "si:m"), ("1.0", "si:m"), ("2.0", "si:1")), "nu"),
    ("t", (("0.0", "si:m"), ("1.0", "si:m"), ("1.5", "si:1")), "nu"),
    ("t", (("0.0", "si:m"), ("0.0", "si:m"), ("5.0", "si:1")), "s"),
    ("t", (("0.0", "si:m"), ("-1.0", "si:m"), ("5.0", "si:1")), "s"),
    ("t", (("0.0", "si:m"), ("1.0", "si:m"), ("5.0", "si:m")), "nu"),
    ("t", (("0.0", "si:m"), ("1.0", "si:rad"), ("5.0", "si:1")), "s"),
    ("t", (("0.0", "si:m"), ("1.0", "si:m"), ("inf", "si:1")), "nu"),
    ("trap", (("0.0", "si:m"), ("1.0", "si:m"), ("1.5", "si:1")), "beta"),
    ("trap", (("0.0", "si:m"), ("1.0", "si:m"), ("-0.1", "si:1")), "beta"),
    ("trap", (("0.0", "si:m"), ("1.0", "si:m"), ("0.5", "si:m")), "beta"),
    ("trap", (("1.0", "si:m"), ("1.0", "si:m"), ("0.5", "si:1")), "b"),
    ("trap", (("0.0", "si:m"), ("1.0", "si:m"), ("NaN", "si:1")), "beta"),
]


def invalid_csv():
    lines = ["id,kind,p1,u1,p2,u2,p3,u3,expected"]
    for i, (kind, ps, name) in enumerate(INVALID):
        cells = []
        for v, u in ps:
            cells += [v, u]
        cells += [""] * (6 - len(cells))
        lines.append(",".join([f"x{i:02d}", kind, *cells, name]))
    return "\n".join(lines) + "\n"


# --------------------------------------------------------------------------- correlation


def exact_psd(m, n):
    """Exact positive semi-definiteness of a symmetric Fraction matrix by symmetric
    elimination with diagonal pivoting."""
    a = [row[:] for row in m]
    idx = list(range(n))
    while idx:
        p = max(idx, key=lambda i: a[i][i])
        if a[p][p] < 0:
            return False
        if a[p][p] == 0:
            return all(a[i][j] == 0 for i in idx for j in idx)
        idx.remove(p)
        for i in idx:
            for j in idx:
                a[i][j] -= a[i][p] * a[p][j] / a[p][p]
    return True


def classify(n, data):
    """The reason (or 'valid') by the order of REQ-005 / the Design."""
    if any(not math.isfinite(x) for x in data):
        return "non_finite"
    m = [[F(data[i * n + j]) for j in range(n)] for i in range(n)]
    if any(m[i][j] != m[j][i] for i in range(n) for j in range(n)):
        return "asymmetric"
    if any(m[i][i] != 1 for i in range(n)):
        return "diagonal"
    if any(abs(m[i][j]) > 1 for i in range(n) for j in range(n)):
        return "off_diagonal_range"
    tau = F(1, 10**12) * n
    shifted = [[m[i][j] + (tau if i == j else 0) for j in range(n)] for i in range(n)]
    return "valid" if exact_psd(shifted, n) else "indefinite"


def corr_s(a):
    # 1 I + a S with S = [[0,1,1],[1,0,-1],[1,-1,0]]: eigenvalues 1 + a, 1 + a, 1 - 2a.
    return [1.0, a, a, a, 1.0, -a, a, -a, 1.0]


AR6 = [0.5 ** abs(i - j) for i in range(6) for j in range(6)]
CORR_VALID = [
    ("rho12_08_3x3", 3, [1.0, 0.8, 0.0, 0.8, 1.0, 0.0, 0.0, 0.0, 1.0]),
    ("mixed_3x3", 3, [1.0, 0.8, 0.3, 0.8, 1.0, 0.5, 0.3, 0.5, 1.0]),
    ("ar1_6x6", 6, AR6),
    ("identity_1x1", 1, [1.0]),
    ("singular_psd_2x2", 2, [1.0, 1.0, 1.0, 1.0]),
    ("singular_psd_3x3", 3, corr_s(0.5)),
    ("near_boundary_ok_3x3", 3, corr_s(0.5 + 2.0**-40)),
]
CORR_INVALID = [
    ("asymmetric_3x3", 3, [1.0, 0.5, 0.2, 0.4, 1.0, 0.1, 0.2, 0.1, 1.0], "asymmetric"),
    ("diagonal_099_3x3", 3, [1.0, 0.5, 0.2, 0.5, 0.99, 0.1, 0.2, 0.1, 1.0], "diagonal"),
    ("indefinite_eig_m01_3x3", 3, corr_s(0.55), "indefinite"),
    ("out_of_range_2x2", 2, [1.0, 1.25, 1.25, 1.0], "off_diagonal_range"),
    ("near_boundary_bad_3x3", 3, corr_s(0.5 + 2.0**-36), "indefinite"),
    ("indefinite_6x6", 6, [1.0 if i == j else -0.5 for i in range(6) for j in range(6)], "indefinite"),
]


def corr_json(cases, valid):
    out = []
    for c in cases:
        cid, n, data = c[0], c[1], c[2]
        want = "valid" if valid else c[3]
        got = classify(n, data)
        assert got == want, (cid, got, want)
        out.append({"id": cid, "n": n, "data": data, "expect": want})
    return json.dumps({"cases": out}, indent=1) + "\n"


# --------------------------------------------------------------------------- AS241

AS241 = {
    "a": [3.3871328727963666080e0, 1.3314166789178437745e2, 1.9715909503065514427e3, 1.3731693765509461125e4,
          4.5921953931549871457e4, 6.7265770927008700853e4, 3.3430575583588128105e4, 2.5090809287301226727e3],
    "b": [1.0, 4.2313330701600911252e1, 6.8718700749205790830e2, 5.3941960214247511077e3,
          2.1213794301586595867e4, 3.9307895800092710610e4, 2.8729085735721942674e4, 5.2264952788528545610e3],
    "c": [1.42343711074968357734e0, 4.63033784615654529590e0, 5.76949722146069140550e0, 3.64784832476320460504e0,
          1.27045825245236838258e0, 2.41780725177450611770e-1, 2.27238449892691845833e-2, 7.74545014278341407640e-4],
    "d": [1.0, 2.05319162663775882187e0, 1.67638483018380384940e0, 6.89767334985100004550e-1,
          1.48103976427480074590e-1, 1.51986665636164571966e-2, 5.47593808499534494600e-4, 1.05075007164441684324e-9],
    "e": [6.65790464350110377720e0, 5.46378491116411436990e0, 1.78482653991729133580e0, 2.96560571828504891230e-1,
          2.65321895265761230930e-2, 1.24266094738807843860e-3, 2.71155556874348757815e-5, 2.01033439929228813265e-7],
    "f": [1.0, 5.99832206555887937690e-1, 1.36929880922735805310e-1, 1.48753612908506148525e-2,
          7.86869131145613259100e-4, 1.84631831751005468180e-5, 1.42151175831644588870e-7, 2.04426310338993978564e-15],
}


def horner(c, x):
    acc = c[7]
    for k in range(6, -1, -1):
        acc = acc * x + c[k]
    return acc


def ppnd16(p):
    """AS241 in binary64, same operation order as the crate (Design section)."""
    q = p - 0.5
    if abs(q) <= 0.425:
        t = 0.180625 - q * q
        return q * horner(AS241["a"], t) / horner(AS241["b"], t)
    t = p if q < 0 else 1.0 - p
    t = math.sqrt(-math.log(t))
    if t <= 5.0:
        t -= 1.6
        v = horner(AS241["c"], t) / horner(AS241["d"], t)
    else:
        t -= 5.0
        v = horner(AS241["e"], t) / horner(AS241["f"], t)
    return -v if q < 0 else v


def dpi():
    def atan_inv(k):
        x, k2, s, n, term = Decimal(1) / k, k * k, Decimal(0), 0, Decimal(1) / k
        while True:
            t = term / (2 * n + 1)
            if t == 0 or abs(t) < Decimal(10) ** -(PREC + 5):
                return s
            s += t if n % 2 == 0 else -t
            term /= k2
            n += 1
    return 16 * atan_inv(5) - 4 * atan_inv(239)


PREC = 100


def erfc_pos(z, pi):
    """erfc(z) for z >= 0 to ~PREC digits."""
    if z <= 3:
        s, term, n = Decimal(0), z, 0
        z2 = z * z
        while True:
            t = term / (2 * n + 1)
            if abs(t) < Decimal(10) ** -(PREC + 5):
                break
            s += t
            n += 1
            term = -term * z2 / n
        return 1 - 2 / pi.sqrt() * s

    def cf(depth):
        acc = z
        for k in range(depth, 0, -1):
            acc = z + (Decimal(k) / 2) / acc
        return (-z * z).exp() / pi.sqrt() / acc

    depth = 200
    prev = cf(depth)
    while True:
        depth *= 2
        cur = cf(depth)
        if abs(cur - prev) <= abs(cur) * Decimal(10) ** -(PREC - 10):
            return cur
        prev = cur


def inv_norm_lower(q, pi):
    """x < = 0 with Phi(x) = q (0 < q <= 1/2), Decimal."""
    if q == F(1, 2):
        return Decimal(0)
    qd = Decimal(q.numerator) / Decimal(q.denominator)
    x = Decimal(repr(ppnd16(float(q))))
    sq2 = Decimal(2).sqrt()
    for _ in range(60):
        phi_x = erfc_pos(-x / sq2, pi) / 2
        dens = (-(x * x) / 2).exp() / (2 * pi).sqrt()
        dx = (phi_x - qd) / dens
        x -= dx
        if abs(dx) <= abs(x) * Decimal(10) ** -(PREC - 20):
            return x
    raise RuntimeError("Newton did not converge")


def splitmix(seed):
    s = seed
    while True:
        s = (s + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        z = s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
        yield z ^ (z >> 31)


def ppnd_points():
    two53 = 2.0**-53
    pts = [two53, 3 * two53, 1e-300, 1e-100, 1e-20, 1e-15, 1.3887943864964021e-11, 1.4e-11, 1e-12, 1e-6, 0.01,
           0.07499999999999998, 0.075, 0.07500000000000001, 0.2, 0.4999, 0.5, 0.6, 0.925, 0.99, 1 - 1e-10,
           1 - two53]
    g = splitmix(0xA5241)
    for _ in range(150):
        k = next(g) >> 12
        pts.append((2 * k + 1) * two53)
    for e in range(1, 300, 7):
        pts.append(10.0**-e)
    return pts


def as241_files():
    with localcontext() as ctx:
        ctx.prec = PREC
        pi = dpi()
        rows = ["p,x"]
        worst = 0.0
        for p in ppnd_points():
            fp = F(p)
            if fp <= F(1, 2):
                x = inv_norm_lower(fp, pi)
            else:
                x = -inv_norm_lower(1 - fp, pi)
            approx = ppnd16(p)
            if x != 0:
                rel = abs((Decimal(repr(approx)) - x) / x)
                worst = max(worst, float(rel))
            else:
                assert approx == 0.0
            rows.append(f"{r(p)},{d50(x)}")
    # Wichura reports about 1e-16 relative accuracy; in binary64 a few more roundings.
    assert worst < 1e-15, worst
    coef = ["name,index,value"]
    for name in "abcdef":
        for i, v in enumerate(AS241[name]):
            coef.append(f"{name},{i},{r(v)}")
    return "\n".join(coef) + "\n", "\n".join(rows) + "\n", worst


# --------------------------------------------------------------------------- main


def main(argv):
    check = "--check" in argv[1:]
    coef, ppnd, worst = as241_files()
    files = {
        "moments.csv": moments_csv(),
        "invalid.csv": invalid_csv(),
        "corr_valid.json": corr_json(CORR_VALID, True),
        "corr_invalid.json": corr_json(CORR_INVALID, False),
        "as241.csv": coef,
        "ppnd.csv": ppnd,
    }
    bad = 0
    for name, text in files.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"stale: {name}")
                bad += 1
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
    print(f"AS241 binary64 worst relative error vs 100-digit reference: {worst:.3e}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
