#!/usr/bin/env python3
"""Generate the UNC.PROP.MVAR.PARAM fixtures (CC-BY-4.0 project data, hand-built values).

Independent reference for `fid_kunc::prop::mvar::param` (D-038): every expected value comes
from the Python standard library only (decimal at 80 significant digits, fractions, json,
struct) and from the rules written in spec/unc/prop/mvar/param/NODE.md and the pinned rules
stated below, never from the Rust code.

Pinned rules this reference implements on its own (the Rust module documents the same rules):

- Tangent-plane basis of a unit reference direction r: k is the index of the component of
  smallest absolute value (ties to the lower index, x before y before z); e1 = unit(e_k x r),
  e2 = unit(r x e1).
- Tangent-plane (gnomonic) coordinates of a direction d about r: u = (d.e1)/(d.r),
  v = (d.e2)/(d.r); the inverse is d = (r + u e1 + v e2) / sqrt(1 + u^2 + v^2).
- fid-fit canonical directions (REQ-KRN.LSQ.TRAIT-007): d / |d|, then for every direction
  except the cone axis the first non-zero component is made positive by negating all.
- Cone parameters: with the caller's reference point q, s = (q - apex).d (d unit),
  reference point p = apex + s d, radius at the reference rho = s tan(half_angle); the
  inverse is apex = p - (rho / tan(half_angle)) d.
- Conversion Jacobians: by central differences of the maps above in 80-digit decimal
  arithmetic (step 1e-28), so they do not share the analytic formulas of the Rust code.
- Eigenvalues: cyclic Jacobi in 80-digit decimal arithmetic, run to an off-diagonal norm
  below 1e-60 (relative).

Files written next to this script:

- features.json   one fitted feature per supported type (AC-1, AC-4), its kernel-form
                  covariance and the expected parameters, basis, Jacobian, parameter
                  covariance and its extreme eigenvalues; plus `from_cases` (parameters with
                  non-zero tangent coordinates and the expected feature and Jacobian)
- not_psd.json    covariance validation cases (AC-2): an indefinite matrix with its smallest
                  eigenvalue, a matrix near the PSD tolerance, an asymmetric matrix with its
                  relative asymmetry and one inside the symmetry tolerance, a singular and the
                  zero matrix (accepted, flagged rank deficient), a negative variance, and
                  mixed-unit matrices whose defect shows only after correlation scaling

Every f64 is written as its IEEE-754 bit pattern in 16 lowercase hex digits.

Run:    python fixtures/unc/mvar/param/make_fixtures.py
Check:  python fixtures/unc/mvar/param/make_fixtures.py --check
"""

import json
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
getcontext().prec = 80
D = Decimal
VERSION = "fcov-param@1"
STEP = D("1e-28")


def hex64(x):
    """IEEE-754 bit pattern of a finite f64 as 16 lowercase hex digits (-0.0 written as +0.0)."""
    x = float(x)
    assert x == x and x not in (float("inf"), float("-inf"))
    if x == 0.0:
        x = 0.0
    return struct.pack(">d", x).hex()


def f64(d):
    """Nearest double (float(Decimal) rounds correctly through the decimal string)."""
    return float(d)


def hexd(d):
    """Nearest double of a decimal; values below 1e-60 in magnitude are exact zeros of the
    construction (for example the coordinates of a direction about itself) seen through
    80-digit rounding, and are written as zero."""
    if abs(d) < D("1e-60"):
        return hex64(0.0)
    return hex64(f64(d))


# ------------------------------------------------------------------ decimal elementary maths


def d_pi():
    # Machin: pi = 16 atan(1/5) - 4 atan(1/239)
    def atan_inv(n):
        x = D(1) / n
        x2 = x * x
        term, total, k = x, x, 1
        while True:
            term *= -x2
            add = term / (2 * k + 1)
            if abs(add) < D("1e-90"):
                return total
            total += add
            k += 1

    return 16 * atan_inv(5) - 4 * atan_inv(239)


def d_sin(x):
    term, total, k = x, x, 1
    while True:
        term *= -x * x / ((2 * k) * (2 * k + 1))
        if abs(term) < D("1e-90"):
            return total
        total += term
        k += 1


def d_cos(x):
    term, total, k = D(1), D(1), 1
    while True:
        term *= -x * x / ((2 * k - 1) * (2 * k))
        if abs(term) < D("1e-90"):
            return total
        total += term
        k += 1


def d_tan(x):
    assert D(0) < x < d_pi() / 2
    return d_sin(x) / d_cos(x)


def dot(a, b):
    return sum((x * y for x, y in zip(a, b)), D(0))


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def norm(a):
    return dot(a, a).sqrt()


def unit(a):
    n = norm(a)
    return [x / n for x in a]


def sign_rule(a):
    for x in a:
        if x != 0:
            return [-y for y in a] if x < 0 else list(a)
    return list(a)


def basis(r):
    """The pinned tangent-plane basis of a (decimal) reference direction."""
    mags = [abs(x) for x in r]
    k = 0
    for i in (1, 2):
        if mags[i] < mags[k]:
            k = i
    ek = [D(1) if i == k else D(0) for i in range(3)]
    e1 = unit(cross(ek, r))
    e2 = unit(cross(r, e1))
    return k, e1, e2


def gnomonic(dvec, r, e1, e2):
    dr = dot(dvec, r)
    assert dr > 0
    return dot(dvec, e1) / dr, dot(dvec, e2) / dr


def from_gnomonic(r, e1, e2, u, v):
    s = (1 + u * u + v * v).sqrt()
    return [(r[i] + u * e1[i] + v * e2[i]) / s for i in range(3)]


# ---------------------------------------------------------------------- feature layouts

# primitive slug (fid-fit) -> (fcov-param@1 type, kernel fields with sizes, param units)
LAYOUT = {
    "point": ("point", [("c", 3)], ["si:m"] * 3),
    "line3": ("line", [("p", 3), ("d", 3)], ["si:m"] * 3 + ["si:rad"] * 2),
    "plane": ("plane", [("p", 3), ("n", 3)], ["si:m"] * 3 + ["si:rad"] * 2),
    "circle3": ("circle", [("c", 3), ("n", 3), ("r", 1)], ["si:m"] * 3 + ["si:rad"] * 2 + ["si:m"]),
    "sphere": ("sphere", [("c", 3), ("r", 1)], ["si:m"] * 4),
    "cylinder": ("cylinder", [("p", 3), ("d", 3), ("r", 1)], ["si:m"] * 3 + ["si:rad"] * 2 + ["si:m"]),
    "cone": (
        "cone",
        [("apex", 3), ("d", 3), ("half_angle", 1)],
        ["si:m"] * 3 + ["si:rad"] * 2 + ["si:m", "si:rad"],
    ),
}
DIR_FIELD = {"line3": "d", "plane": "n", "circle3": "n", "cylinder": "d", "cone": "d"}


def kernel_vector(prim, feat):
    out = []
    for name, size in LAYOUT[prim][1]:
        v = feat[name]
        out.extend(v if size == 3 else [v])
    return [D(x) for x in out]


def split_kernel(prim, x):
    out, i = {}, 0
    for name, size in LAYOUT[prim][1]:
        out[name] = x[i : i + 3] if size == 3 else x[i]
        i += size
    return out


def sign_of(prim, dvec):
    """The sign the canonical rule applies at this direction (+1 for the oriented cone axis)."""
    du = unit(dvec)
    return 1 if prim == "cone" or sign_rule(du) == du else -1


def canonical_dir(prim, dvec, sigma=None):
    """Canonical direction; with `sigma` given, that sign is held fixed (the rule's local
    branch: the sign rule is discontinuous where a leading component is zero, so the
    derivative at the feature is taken with the sign of the feature itself)."""
    du = unit(dvec)
    if sigma is None:
        sigma = sign_of(prim, dvec)
    return du if sigma == 1 else [-x for x in du]


def to_param_map(prim, x, q, r, e1, e2, sigma=None):
    """Kernel-form vector -> fcov-param@1 vector, with the reference basis held fixed."""
    f = split_kernel(prim, x)
    if prim == "point":
        return list(f["c"])
    if prim == "sphere":
        return list(f["c"]) + [f["r"]]
    dc = canonical_dir(prim, f[DIR_FIELD[prim]], sigma)
    u, v = gnomonic(dc, r, e1, e2)
    if prim in ("line3", "cylinder"):
        out = list(f["p"]) + [u, v]
        return out + ([f["r"]] if prim == "cylinder" else [])
    if prim == "plane":
        return list(f["p"]) + [u, v]
    if prim == "circle3":
        return list(f["c"]) + [u, v, f["r"]]
    # cone
    apex, alpha = f["apex"], f["half_angle"]
    s = dot([q[i] - apex[i] for i in range(3)], dc)
    p = [apex[i] + s * dc[i] for i in range(3)]
    return p + [u, v, s * d_tan(alpha), alpha]


def from_param_map(prim, y, r, e1, e2):
    """fcov-param@1 vector -> kernel-form vector (directions on the reference hemisphere)."""
    if prim in ("point", "sphere"):
        return list(y)
    dvec = from_gnomonic(r, e1, e2, y[3], y[4])
    if prim in ("line3", "plane"):
        return list(y[:3]) + dvec
    if prim in ("circle3", "cylinder"):
        return list(y[:3]) + dvec + [y[5]]
    rho, alpha = y[5], y[6]
    t = d_tan(alpha)
    apex = [y[i] - (rho / t) * dvec[i] for i in range(3)]
    return apex + dvec + [alpha]


def jacobian(fn, x):
    """Central differences of fn at x: rows = outputs, columns = inputs."""
    cols = []
    for j in range(len(x)):
        xp = list(x)
        xm = list(x)
        xp[j] += STEP
        xm[j] -= STEP
        fp, fm = fn(xp), fn(xm)
        cols.append([(a - b) / (2 * STEP) for a, b in zip(fp, fm)])
    return [[cols[j][i] for j in range(len(x))] for i in range(len(cols[0]))]


def matmul(a, b):
    return [[sum((a[i][k] * b[k][j] for k in range(len(b))), D(0)) for j in range(len(b[0]))] for i in range(len(a))]


def transpose(a):
    return [list(r) for r in zip(*a)]


def jacobi_eigs(a):
    """Eigenvalues (ascending) of a symmetric decimal matrix by cyclic Jacobi."""
    n = len(a)
    w = [list(r) for r in a]
    fro = sum((w[i][j] * w[i][j] for i in range(n) for j in range(n)), D(0)).sqrt()
    if fro == 0:
        return [D(0)] * n
    for _ in range(100):
        off = sum((w[i][j] * w[i][j] for i in range(n) for j in range(n) if i != j), D(0)).sqrt()
        if off <= fro * D("1e-60"):
            return sorted(w[i][i] for i in range(n))
        for p in range(n):
            for q in range(p + 1, n):
                if w[p][q] == 0:
                    continue
                theta = (w[q][q] - w[p][p]) / (2 * w[p][q])
                sg = D(-1) if theta < 0 else D(1)
                t = sg / (abs(theta) + (1 + theta * theta).sqrt())
                c = 1 / (1 + t * t).sqrt()
                s = t * c
                for k in range(n):
                    wkp, wkq = w[k][p], w[k][q]
                    w[k][p] = c * wkp - s * wkq
                    w[k][q] = s * wkp + c * wkq
                for k in range(n):
                    wpk, wqk = w[p][k], w[q][k]
                    w[p][k] = c * wpk - s * wqk
                    w[q][k] = s * wpk + c * wqk
    raise RuntimeError("jacobi did not converge")


# ------------------------------------------------------------------ seeded kernel covariances


class Lcg:
    """64-bit LCG (Knuth MMIX constants); uniform doubles in [-1, 1) from the top 53 bits."""

    def __init__(self, seed):
        self.x = seed

    def next(self):
        self.x = (6364136223846793005 * self.x + 1442695040888963407) % (1 << 64)
        return float(Fraction((self.x >> 11) - (1 << 52), 1 << 52))


SCALE = {"si:m": 2e-6, "dir": 5e-5, "angle": 1e-4, "apex": 1e-5}


def kernel_scales(prim):
    out = []
    for name, size in LAYOUT[prim][1]:
        if name in ("d", "n"):
            out += [SCALE["dir"]] * size
        elif name == "half_angle":
            out.append(SCALE["angle"])
        elif name == "apex":
            out += [SCALE["apex"]] * size
        else:
            out += [SCALE["si:m"]] * size
    return out


def seeded_kernel_cov(prim, seed):
    """K = M M^T for a seeded M with per-row scales, rounded to doubles (symmetric)."""
    sc = kernel_scales(prim)
    n = len(sc)
    g = Lcg(seed)
    m = [[float(D(g.next()) * D(sc[i])) for _ in range(n)] for i in range(n)]
    md = [[D(x) for x in row] for row in m]
    k = matmul(md, transpose(md))
    return [[f64(k[i][j]) for j in range(n)] for i in range(n)]


def diag_kernel_cov(values):
    n = len(values)
    return [[values[i] if i == j else 0.0 for j in range(n)] for i in range(n)]


# ------------------------------------------------------------------------------ the features


def dvec(*xs):
    """A unit direction as the nearest doubles of the exact decimal unit vector."""
    u = unit([D(x) for x in xs])
    return [f64(x) for x in u]


FEATURES = [
    ("point", "point", {"c": [0.125, -0.0375, 0.5]}, None, 11),
    ("line", "line3", {"p": [0.01, 0.02, 0.03], "d": dvec(1, 2, 2)}, None, 12),
    ("line_sign_flipped", "line3", {"p": [0.01, 0.02, 0.03], "d": [-x for x in dvec(1, 2, 2)]}, None, 13),
    ("plane", "plane", {"p": [0.1, 0.2, 0.0], "n": dvec(2, -3, 6)}, None, 14),
    ("circle", "circle3", {"c": [0.05, -0.02, 0.3], "n": dvec(0, 3, 4), "r": 0.0125}, None, 15),
    ("sphere", "sphere", {"c": [0.2, 0.1, -0.05], "r": 0.02}, None, 16),
    ("cylinder_z", "cylinder", {"p": [0.03, -0.04, 0.0], "d": [0.0, 0.0, 1.0], "r": 0.01}, None, None),
    ("cylinder", "cylinder", {"p": [-0.02, 0.07, 0.11], "d": dvec(1, 1, 1), "r": 0.015}, None, 17),
    (
        "cone",
        "cone",
        {"apex": [0.0, 0.0, -0.1], "d": dvec(1, -2, 10), "half_angle": 0.3},
        [0.01, 0.02, 0.05],
        18,
    ),
]

# AC-4: cylinder along z with a singular 3 x 3 direction block diag(4e-10, 9e-10, 0)
CYL_Z_COV = diag_kernel_cov([1e-12, 2.25e-12, 4e-12, 4e-10, 9e-10, 0.0, 1e-12])


def feature_case(name, prim, feat, q, seed):
    x = kernel_vector(prim, feat)
    kcov = CYL_Z_COV if seed is None else seeded_kernel_cov(prim, seed)
    case = {
        "name": name,
        "primitive": prim,
        "type": LAYOUT[prim][0],
        "feature": {k: ([hex64(c) for c in v] if isinstance(v, list) else hex64(v)) for k, v in feat.items()},
        "cone_reference": None if q is None else [hex64(c) for c in q],
        "kernel_cov": [hex64(c) for row in kcov for c in row],
    }
    exp = {"units": LAYOUT[prim][2]}
    qd = None if q is None else [D(c) for c in q]
    if prim in DIR_FIELD:
        rdec = canonical_dir(prim, [D(c) for c in feat[DIR_FIELD[prim]]])
        k, e1, e2 = basis(rdec)
        exp["ref_dir"] = [hexd(c) for c in rdec]
        exp["basis_axis"] = k
        exp["e1"] = [hexd(c) for c in e1]
        exp["e2"] = [hexd(c) for c in e2]
    else:
        rdec = e1 = e2 = None
        exp["ref_dir"] = None
    sigma = sign_of(prim, [D(c) for c in feat[DIR_FIELD[prim]]]) if prim in DIR_FIELD else 1
    exp["sign"] = sigma
    params = to_param_map(prim, x, qd, rdec, e1, e2)
    exp["params"] = [hexd(c) for c in params]
    jac = jacobian(lambda z: to_param_map(prim, z, qd, rdec, e1, e2, sigma), x)
    exp["to_jacobian"] = [hexd(c) for row in jac for c in row]
    kd = [[D(c) for c in row] for row in kcov]
    pcov = matmul(matmul(jac, kd), transpose(jac))
    exp["cov"] = [hexd(c) for row in pcov for c in row]
    eig = jacobi_eigs(pcov)
    exp["min_eig"] = hexd(eig[0])
    exp["max_eig"] = hexd(eig[-1])
    if prim in DIR_FIELD:
        i0 = 3
        blk = [[pcov[i0][i0], pcov[i0][i0 + 1]], [pcov[i0 + 1][i0], pcov[i0 + 1][i0 + 1]]]
        be = jacobi_eigs(blk)
        exp["dir_block"] = [hexd(c) for row in blk for c in row]
        exp["dir_block_min_eig"] = hexd(be[0])
    if prim == "cone":
        exp["reference_distance"] = hexd(dot([qd[i] - x[i] for i in range(3)], rdec))
    case["expected"] = exp
    return case


FROM_CASES = [
    ("line_tilted", "line3", [0.01, 0.02, 0.03, 0.01, -0.02], dvec(1, 2, 2)),
    ("cylinder_z_tilted", "cylinder", [0.03, -0.04, 0.0, 0.001, 0.0, 0.01], [0.0, 0.0, 1.0]),
    ("cone_tilted", "cone", [0.01, 0.02, 0.05, -0.005, 0.003, 0.02, 0.4], dvec(1, -2, 10)),
]


def from_case(name, prim, params, ref):
    rd = [D(c) for c in ref]
    _, e1, e2 = basis(rd)
    y = [D(c) for c in params]
    out = from_param_map(prim, y, rd, e1, e2)
    jac = jacobian(lambda z: from_param_map(prim, z, rd, e1, e2), y)
    return {
        "name": name,
        "primitive": prim,
        "params": [hex64(c) for c in params],
        "ref_dir": [hex64(c) for c in ref],
        "expected": {
            "kernel": [hexd(c) for c in out],
            "from_jacobian": [hexd(c) for row in jac for c in row],
        },
    }


# --------------------------------------------------------------------------- not_psd.json


def solve(a, b):
    """Exact Gauss-Jordan solve a X = b over Fractions (a square, b matrix)."""
    n = len(a)
    m = [list(a[i]) + list(b[i]) for i in range(n)]
    for c in range(n):
        piv = next(r for r in range(c, n) if m[r][c] != 0)
        m[c], m[piv] = m[piv], m[c]
        inv = 1 / m[c][c]
        m[c] = [x * inv for x in m[c]]
        for r in range(n):
            if r != c and m[r][c] != 0:
                fac = m[r][c]
                m[r] = [x - fac * y for x, y in zip(m[r], m[c])]
    return [row[n:] for row in m]


def cayley_orthogonal(n):
    """A rational orthogonal matrix Q = (I - A)(I + A)^-1 for a fixed skew-symmetric A."""
    a = [[Fraction(0)] * n for _ in range(n)]
    vals = [Fraction(1, 3), Fraction(-2, 7), Fraction(1, 5), Fraction(3, 11), Fraction(-1, 4)]
    idx = 0
    for i in range(n):
        for j in range(i + 1, n):
            v = vals[idx % len(vals)] * (1 + Fraction(idx, 13))
            a[i][j], a[j][i] = v, -v
            idx += 1
    ident = [[Fraction(int(i == j)) for j in range(n)] for i in range(n)]
    imina = [[ident[i][j] - a[i][j] for j in range(n)] for i in range(n)]
    iplusa = [[ident[i][j] + a[i][j] for j in range(n)] for i in range(n)]
    # Q = (I - A)(I + A)^-1 ; (I + A)^-1 = solve(I + A, I)
    inv = solve(iplusa, ident)
    q = [[sum((imina[i][k] * inv[k][j] for k in range(n)), Fraction(0)) for j in range(n)] for i in range(n)]
    for i in range(n):
        for j in range(n):
            assert sum((q[k][i] * q[k][j] for k in range(n)), Fraction(0)) == (1 if i == j else 0)
    return q


def spectral(lams):
    n = len(lams)
    q = cayley_orthogonal(n)
    c = [
        [sum((q[i][k] * lams[k] * q[j][k] for k in range(n)), Fraction(0)) for j in range(n)] for i in range(n)
    ]
    return [[float(c[i][j]) for j in range(n)] for i in range(n)]


CYL_PARAMS = [0.03, -0.04, 0.0, 0.0, 0.0, 0.01]


def psd_case(name, mat, note):
    """The pinned check (spec/unc/prop/mvar/param/NODE.md and the module docs): negative
    variances first, then zero variances with a non-zero covariance, then symmetry and PSD on the correlation-scaled matrix
    R = D^-1/2 C D^-1/2 (d_i = sqrt(c_ii), or 1 for a zero variance), then the rank flag."""
    n = len(mat)
    md = [[D(x) for x in row] for row in mat]
    exp = {"verdict": None}
    neg = [i for i in range(n) if mat[i][i] < 0]
    zero_corr = [
        (i, j) for i in range(n) if mat[i][i] == 0 for j in range(n) if j != i and (mat[i][j] != 0 or mat[j][i] != 0)
    ]
    if neg:
        exp.update(verdict="negative_variance", neg_index=neg[0], neg_value=hex64(mat[neg[0]][neg[0]]))
    elif zero_corr:
        exp.update(verdict="correlated_zero_variance", zero_index=zero_corr[0][0], zero_other=zero_corr[0][1])
    else:
        dd = [md[i][i].sqrt() if md[i][i] > 0 else D(1) for i in range(n)]
        r = [[md[i][j] / (dd[i] * dd[j]) for j in range(n)] for i in range(n)]
        mr = max(abs(x) for row in r for x in row)
        pairs = [(i, j) for i in range(n) for j in range(i + 1, n)]
        diff = {p: abs(md[p[0]][p[1]] - md[p[1]][p[0]]) / (dd[p[0]] * dd[p[1]]) for p in pairs}
        worst = max(pairs, key=lambda p: (diff[p], -p[0], -p[1]))
        asym = diff[worst] / mr if mr != 0 else D(0)
        if diff[worst] == 0:
            worst = (0, 1)
        sym = [[(md[i][j] + md[j][i]) / 2 for j in range(n)] for i in range(n)]
        rsym = [[sym[i][j] / (dd[i] * dd[j]) for j in range(n)] for i in range(n)]
        eig = jacobi_eigs(sym)
        reig = jacobi_eigs(rsym)
        tol = D("1e-12")
        if asym > tol:
            verdict = "asymmetric"
        elif reig[0] < -tol * reig[-1]:
            verdict = "not_psd"
        else:
            verdict = "ok"
        exp.update(
            verdict=verdict,
            asym=hexd(asym),
            asym_pair=list(worst),
            min_eig=hexd(eig[0]),
            max_eig=hexd(eig[-1]),
            corr_min_eig=hexd(reig[0]),
            corr_max_eig=hexd(reig[-1]),
            rank_deficient=bool(reig[0] <= tol * reig[-1]),
            cond=hexd(reig[-1] / reig[0]) if reig[0] > 0 else None,
        )
    return {
        "name": name,
        "note": note,
        "type": "cylinder",
        "params": [hex64(c) for c in CYL_PARAMS],
        "ref_dir": [hex64(c) for c in [0.0, 0.0, 1.0]],
        "cov": [hex64(c) for row in mat for c in row],
        "expected": exp,
    }


def diag6(values, extra=()):
    m = [[values[i] if i == j else 0.0 for j in range(6)] for i in range(6)]
    for i, j, x in extra:
        m[i][j] = x
    return m


def not_psd_cases():
    base = [4e-8, 1e-8, 2.5e-9, 1e-10, 4e-12, -2e-11]
    indefinite = spectral([Fraction(x) for x in base])
    barely = spectral([Fraction(x) for x in base[:5]] + [Fraction(-2, 10**21)])
    asym = [list(r) for r in spectral([Fraction(x) for x in base[:5]] + [Fraction(1, 10**12)])]
    asym[1][4] = asym[1][4] * (1 + 1e-9)
    near = [list(r) for r in spectral([Fraction(x) for x in base[:5]] + [Fraction(1, 10**12)])]
    near[0][2] = near[0][2] + 1e-27
    singular = spectral([Fraction(x) for x in base[:5]] + [Fraction(0)])
    mixed = [1e-24, 1e-24, 1e-8, 1e-8, 1e-8, 1e-10]
    return [
        psd_case("indefinite", indefinite, "eigenvalues 4e-8, 1e-8, 2.5e-9, 1e-10, 4e-12, -2e-11 (AC-2)"),
        psd_case("barely_psd", barely, "smallest eigenvalue -2e-21 of the unscaled matrix"),
        psd_case("asymmetric", asym, "entry (1, 4) scaled by 1 + 1e-9, entry (4, 1) unchanged"),
        psd_case("near_symmetric", near, "entry (0, 2) raised by 1e-27, below 1e-12 after scaling"),
        psd_case("singular", singular, "eigenvalues 4e-8, 1e-8, 2.5e-9, 1e-10, 4e-12, 0: rank deficient"),
        psd_case("zero", diag6([0.0] * 6), "the zero matrix: accepted and flagged rank deficient"),
        psd_case(
            "negative_variance",
            diag6([1e-8, 1e-8, 1e-8, -1e-21, 1e-12, 1e-10]),
            "variance -1e-21 at index 3, invisible next to 1e-8 without scaling (review of #673)",
        ),
        psd_case(
            "correlated_zero_variance",
            diag6([1e-8, 1e-8, 0.0, 1e-8, 1e-12, 1e-10], [(4, 2, 1e-15), (2, 4, 1e-15)]),
            "variance 0 at index 2 with covariance 1e-15 to index 4 (refused before scaling)",
        ),
        psd_case(
            "mixed_units_indefinite",
            diag6(mixed, [(0, 1, 2e-24), (1, 0, 2e-24)]),
            "an m^2 block with correlation 2 next to 1e-8 entries: indefinite only after scaling",
        ),
        psd_case(
            "mixed_units_asymmetric",
            diag6(mixed, [(0, 1, 5e-25), (1, 0, 6e-25)]),
            "an m^2 block with correlations 0.5 and 0.6: asymmetric only after scaling",
        ),
    ]


# ---------------------------------------------------------------------------------- output


def build():
    feats = [feature_case(*c) for c in FEATURES]
    froms = [from_case(*c) for c in FROM_CASES]
    files = {
        "features.json": {
            "version": VERSION,
            "generator": "fixtures/unc/mvar/param/make_fixtures.py",
            "cases": feats,
            "from_cases": froms,
        },
        "not_psd.json": {
            "version": VERSION,
            "generator": "fixtures/unc/mvar/param/make_fixtures.py",
            "tolerances": {"asym_rel": "1e-12", "psd_rel": "1e-12"},
            "cases": not_psd_cases(),
        },
    }
    return {k: (json.dumps(v, indent=1, sort_keys=True) + "\n").encode("ascii") for k, v in files.items()}


def main():
    check = "--check" in sys.argv[1:]
    out = build()
    bad = 0
    for name, data in out.items():
        target = HERE / name
        if check:
            if not target.exists() or target.read_bytes() != data:
                print(f"stale: {target}")
                bad += 1
        else:
            target.write_bytes(data)
            print(f"wrote {target}")
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
