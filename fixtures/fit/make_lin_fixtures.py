#!/usr/bin/env python3
"""Independent reference for KRN.LSQ.LIN (least-squares point, line and plane) fixtures
(CC-BY-4.0 project data).

Writes
  fixtures/fit/lin-synth-v1/cases.txt   AC-3 and AC-4: seeded points 1 km from the origin

Nothing here runs the Rust code (stdlib only). Three independent parts:

1. Data generators. Coordinates use only IEEE-754 + - * / and the Mersenne Twister of
   `random.Random(seed).random()` (through `uniform`, which is a product and a sum), so the
   bytes do not depend on the platform; circle angles come from a 50-digit Decimal Taylor
   series rounded once.
2. An exact reference: exact rational centroid and scatter matrix of the binary64 inputs
   (fractions.Fraction), eigenpairs by an 80-digit Decimal Jacobi iteration, the fitted
   point, direction and RMS residual from them. It knows nothing about the pinned Rust
   rules; the Rust test compares with it under a tolerance, and so does this script for
   the model below. For AC-4 it labels the circle case ill-conditioned when its exact
   eigenvalue-gap condition exceeds the limit.
3. A bit-exact model of the pinned rules of `fid_fit::ls_lin` (module documentation): the
   `fid_num::red` reductions, the contract's centring and direction normalisation, the
   cyclic Jacobi of `ls_lin::eig`, residuals, statistics, diagnostics and the canonical
   result encoding of `contract::encode`, with its SHA-256. CPython floats are binary64,
   round-to-nearest-even, never fused; `math.sqrt` is correctly rounded. The Rust test
   requires identical bytes on every CI target (x86-64, ARM64, wasm32), which is AC-3.

Run:  python fixtures/fit/make_lin_fixtures.py          (writes the fixture)
      python fixtures/fit/make_lin_fixtures.py --check  (fails if the fixture is stale)
"""

import hashlib
import math
import random
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "lin-synth-v1" / "cases.txt"
BLOCK = 256
U = 2.0 ** -53
BIG = 2.0 ** 256
MAX_SWEEPS = 50
COND_LIMIT = 1e12  # fid_solv NllsConfig::default().cond_limit
RANK_TOL = 1e-9  # contract DEFAULT_RANK_TOL
MIN_NORMAL = 2.2250738585072014e-308
PI50 = Decimal("3.14159265358979323846264338327950288419716939937510582097494459")


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return f"{bits(x):016x}"


# ---------------------------------------------------------------- 1. generators


def dsincos(deg):
    """sin and cos of an angle in degrees (Fraction), each rounded once to binary64."""
    getcontext().prec = 50
    x = Decimal(deg.numerator) / Decimal(deg.denominator) * PI50 / 180
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


B = (600.0, 640.0, 480.0)  # |B| = 1000 m exactly
D = (2 / 7, 3 / 7, 6 / 7)  # unit (2, 3, 6) / 7, rounded components
N3 = (1 / 3, 2 / 3, 2 / 3)  # unit (1, 2, 2) / 3
E1 = (2 / 3, -2 / 3, 1 / 3)
E2 = (2 / 3, 1 / 3, -2 / 3)
NOISE = 1e-6


def add(*terms):
    """Component-wise sum of 3-vectors, left to right."""
    out = list(terms[0])
    for t in terms[1:]:
        out = [out[k] + t[k] for k in range(3)]
    return tuple(out)


def scale(a, v):
    return tuple(a * x for x in v)


def gen_point(rnd, n):
    return [tuple(B[k] + rnd.uniform(-5e-4, 5e-4) for k in range(3)) for _ in range(n)]


def gen_line3(rnd, n):
    pts = []
    for _ in range(n):
        t = rnd.uniform(-0.25, 0.25)
        pts.append(tuple(B[k] + t * D[k] + rnd.uniform(-NOISE, NOISE) for k in range(3)))
    return pts


def gen_plane(rnd, n):
    pts = []
    for _ in range(n):
        a = rnd.uniform(-0.3, 0.3)
        b = rnd.uniform(-0.2, 0.2)
        h = rnd.uniform(-NOISE, NOISE)
        pts.append(add(B, scale(a, E1), scale(b, E2), scale(h, N3)))
    return pts


def gen_line2_xy(rnd, n):
    pts = []
    for _ in range(n):
        t = rnd.uniform(-0.25, 0.25)
        pts.append((600.0 + t * 0.28 + rnd.uniform(-NOISE, NOISE),
                    800.0 + t * 0.96 + rnd.uniform(-NOISE, NOISE),
                    5.0 + rnd.uniform(-1e-3, 1e-3)))
    return pts


def gen_line2_tilted(rnd, n):
    """A line along E1 in the plane through B with normal N3, offset 0.1 m along E2, with
    in-plane noise and up to 1 mm off the plane (removed by the projection)."""
    pts = []
    for _ in range(n):
        t = rnd.uniform(-0.25, 0.25)
        h = 0.1 + rnd.uniform(-NOISE, NOISE)
        off = rnd.uniform(-1e-3, 1e-3)
        pts.append(add(B, scale(t, E1), scale(h, E2), scale(off, N3)))
    return pts


def gen_circle(n, r):
    pts = []
    for k in range(n):
        s, c = dsincos(Fraction(360 * k, n))
        pts.append(add(B, scale(r * c, E1), scale(r * s, E2)))
    return pts


# ---------------------------------------------------------------- 2. exact reference


def dec(f):
    return Decimal(f.numerator) / Decimal(f.denominator)


def dec_eig(m):
    """Eigenvalues (ascending) and vectors of a symmetric Decimal matrix, 80 digits."""
    getcontext().prec = 80
    n = len(m)
    a = [row[:] for row in m]
    v = [[Decimal(1) if i == j else Decimal(0) for j in range(n)] for i in range(n)]
    fro = sum(a[i][j] * a[i][j] for i in range(n) for j in range(n))
    for _ in range(200):
        off = sum(a[p][q] * a[p][q] for p in range(n) for q in range(p + 1, n))
        if off <= fro * Decimal(10) ** -150 or off == 0:
            break
        for p in range(n):
            for q in range(p + 1, n):
                if a[p][q] == 0:
                    continue
                theta = (a[q][q] - a[p][p]) / (2 * a[p][q])
                t = (1 if theta >= 0 else -1) / (abs(theta) + (theta * theta + 1).sqrt())
                c = 1 / (t * t + 1).sqrt()
                s = t * c
                apq = a[p][q]
                for r in range(n):
                    if r in (p, q):
                        continue
                    arp, arq = a[r][p], a[r][q]
                    a[r][p] = a[p][r] = c * arp - s * arq
                    a[r][q] = a[q][r] = s * arp + c * arq
                a[p][p] -= t * apq
                a[q][q] += t * apq
                a[p][q] = a[q][p] = Decimal(0)
                for row in v:
                    vp, vq = row[p], row[q]
                    row[p] = c * vp - s * vq
                    row[q] = s * vp + c * vq
    else:
        raise AssertionError("decimal Jacobi did not converge")
    order = sorted(range(n), key=lambda i: a[i][i])
    return [a[i][i] for i in order], [[v[r][i] for r in range(n)] for i in order]


def dec_sign(v):
    norm = sum(x * x for x in v).sqrt()
    v = [x / norm for x in v]
    for x in v:
        if x != 0:
            return [-y for y in v] if x < 0 else v
    return v


def exact_fit(prim, coords, fixed):
    """coords: list of tuples (3D, or 2D plane coordinates for line2) of binary64 values.
    Returns (point, direction, rms, cond) in Decimal; cond is None for fixed directions."""
    getcontext().prec = 80
    dim = len(coords[0])
    n = len(coords)
    c = [sum(Fraction(p[k]) for p in coords) / n for k in range(dim)]
    dev = [[Fraction(p[k]) - c[k] for k in range(dim)] for p in coords]
    cd = [dec(x) for x in c]
    if prim == "point":
        rms = (sum(sum(dec(x) ** 2 for x in d) for d in dev) / n).sqrt()
        return cd, None, rms, None
    m = [[dec(sum(d[j] * d[k] for d in dev)) for k in range(dim)] for j in range(dim)]
    vals, vecs = dec_eig(m)
    cond = None
    if fixed is not None:
        u = dec_sign([Decimal(x) for x in fixed])
    elif prim == "plane":
        u = dec_sign(vecs[0])
        cond = vals[1] / (vals[1] - vals[0])
    else:
        u = dec_sign(vecs[-1])
        cond = vals[-1] / (vals[-1] - vals[-2])
    total = Decimal(0)
    for d in dev:
        dd = [dec(x) for x in d]
        t = sum(dd[k] * u[k] for k in range(dim))
        if prim == "plane":
            total += t * t
        else:
            total += sum((dd[k] - t * u[k]) ** 2 for k in range(dim))
    return cd, u, (total / n).sqrt(), cond


# ---------------------------------------------------------------- 3. bit-exact model


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


def red_dot(a, b):
    assert len(a) == len(b)
    return red_sum([x * y for x, y in zip(a, b)])


def red_mean(xs):
    return red_sum(xs) / float(len(xs))


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


def tkey(x):
    b = bits(x)
    return (b ^ 0xFFFFFFFFFFFFFFFF) if b >> 63 else (b | (1 << 63))


def tmax(a, b):
    return b if tkey(a) < tkey(b) else a


def tmin(a, b):
    return b if tkey(a) > tkey(b) else a


def centre(pts):
    """contract::Centred::new: centroid, scale s = 2^e and the scaled coordinates q."""
    n = len(pts)
    c = [red_mean([p[k] for p in pts]) for k in range(3)]
    d = [p[k] - c[k] for p in pts for k in range(3)]
    rms = red_norm2(d) / math.sqrt(float(n))
    e = 0 if rms == 0 else exponent(rms)
    s = math.ldexp(1.0, e)
    q = [x / s for x in d]
    return c, s, [q[3 * i:3 * i + 3] for i in range(n)]


def unit(d):
    ln = red_norm2(d)
    assert ln >= MIN_NORMAL
    return [x / ln for x in d]


def sign_norm(u):
    for x in u:
        if x != 0.0:
            if x < 0:
                u = [-y for y in u]
            break
    return [x + 0.0 for x in u]


def nd(d):
    """contract::normalize_direction (and normalize_direction2)."""
    return sign_norm(unit(d))


def ud(d):
    """contract::unit_direction."""
    return [x + 0.0 for x in unit(d)]


def sym_eig(a):
    """ls_lin::eig::sym_eig, rules 1-5."""
    n = len(a)
    m = [[a[min(i, j)][max(i, j)] for j in range(n)] for i in range(n)]
    fro = red_norm2([m[i][j] for i in range(n) for j in range(n)])
    tol = (float(n) * U) * fro
    v = [[1.0 if i == j else 0.0 for j in range(n)] for i in range(n)]
    sweeps = 0
    while True:
        off = red_norm2([m[p][q] for p in range(n) for q in range(p + 1, n)])
        if off <= tol:
            break
        assert sweeps < MAX_SWEEPS, "model Jacobi did not converge"
        for p in range(n):
            for q in range(p + 1, n):
                a_ = m[p][q]
                if a_ == 0.0:
                    continue
                theta = (m[q][q] - m[p][p]) / (2.0 * a_)
                if abs(theta) >= BIG:
                    t = 0.5 / theta
                else:
                    mag = 1.0 / (abs(theta) + math.sqrt(theta * theta + 1.0))
                    t = -mag if theta < 0.0 else mag
                c = 1.0 / math.sqrt(t * t + 1.0)
                s = t * c
                for r in range(n):
                    if r in (p, q):
                        continue
                    arp, arq = m[r][p], m[r][q]
                    nrp = c * arp - s * arq
                    nrq = s * arp + c * arq
                    m[r][p] = m[p][r] = nrp
                    m[r][q] = m[q][r] = nrq
                m[p][p] = m[p][p] - t * a_
                m[q][q] = m[q][q] + t * a_
                m[p][q] = m[q][p] = 0.0
                for row in v:
                    vp, vq = row[p], row[q]
                    row[p] = c * vp - s * vq
                    row[q] = s * vp + c * vq
        sweeps += 1
    order = sorted(range(n), key=lambda i: tkey(m[i][i]))
    return [m[i][i] for i in order], [[v[r][i] for r in range(n)] for i in order]


def scatter(q, k):
    cols = [[p[j] for p in q] for j in range(k)]
    return [[red_dot(cols[min(i, j)], cols[max(i, j)]) for j in range(k)] for i in range(k)]


class Ill(Exception):
    def __init__(self, cond):
        super().__init__(cond)
        self.cond = cond


def gap_cond(la, lb):
    gap = la - lb
    cond = la / gap if gap > 0.0 else math.inf
    if math.isfinite(cond) and cond <= COND_LIMIT:
        return cond
    raise Ill(cond)


def stats(r):
    n = len(r)
    sq = math.sqrt(float(n))
    rms = red_norm2(r) / sq
    mx = mn = r[0]
    for x in r:
        mx = tmax(mx, x)
        mn = tmin(mn, x)
    mean = red_mean(r)
    std = red_norm2([x - mean for x in r]) / sq
    return rms, mx, mn, mean, std


def line_parts(q, d):
    k = len(d)
    t, w = [], [[] for _ in range(k)]
    for p in q:
        ti = red_dot(p[:k], d)
        t.append(ti)
        for j in range(k):
            w[j].append(p[j] - ti * d[j])
    return t, w


def line_grad(t, w, free):
    g = [red_sum(col) for col in w]
    if free:
        g += [red_dot(t, col) for col in w]
    return g


def frame(plane):
    p0, n0 = plane if plane is not None else ((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))
    n = nd(list(n0))
    k = 0
    for j in (1, 2):
        if abs(n[j]) < abs(n[k]):
            k = j
    a = [0.0, 0.0, 0.0]
    a[k] = 1.0
    nk = n[k]
    e1 = nd([a[0] - nk * n[0], a[1] - nk * n[1], a[2] - nk * n[2]])
    cross = [n[1] * e1[2] - n[2] * e1[1], n[2] * e1[0] - n[0] * e1[2], n[0] * e1[1] - n[1] * e1[0]]
    return list(p0), e1, ud(cross)


def flush(x):
    return 0.0 if x != 0.0 and abs(x) < MIN_NORMAL else x


def model(prim, pts, fixed=None, plane=None):
    """Returns (feature fields, residuals, (grad_norm, rank, cond)) or raises Ill."""
    if prim == "line2":
        p0, e1, e2 = frame(plane)
        fixed2 = None
        if fixed is not None:
            f = nd(list(fixed))
            g = [red_dot(f, e1), red_dot(f, e2)]
            assert red_norm2(g) > RANK_TOL
            fixed2 = nd(g)
        proj = []
        for x in pts:
            dx = [x[k] - p0[k] for k in range(3)]
            proj.append((flush(red_dot(dx, e1)), flush(red_dot(dx, e2)), 0.0))
        c, s, q = centre(proj)
        if fixed2 is None:
            vals, vecs = sym_eig(scatter(q, 2))
            cond = gap_cond(vals[1], vals[0])
            d = nd(vecs[1])
        else:
            d, cond = fixed2, 1.0
        t, w = line_parts(q, d)
        res = [(d[0] * p[1] - d[1] * p[0]) * s for p in q]
        grad = line_grad(t, w, fixed2 is None)
        rank = 1 if fixed2 is not None else 2
        feat = [("p", [0.0 * s + c[0], 0.0 * s + c[1]]), ("d", nd(d))]
        return feat, res, (red_norm2(grad), rank, cond), (e1, e2, p0)
    c, s, q = centre(pts)
    cpt = [0.0 * s + c[k] for k in range(3)]
    if prim == "point":
        res = [red_norm2(p) * s for p in q]
        grad = [red_sum([p[k] for p in q]) for k in range(3)]
        return [("c", cpt)], res, (red_norm2(grad), 3, 1.0), None
    if prim == "line3":
        if fixed is None:
            vals, vecs = sym_eig(scatter(q, 3))
            cond = gap_cond(vals[2], vals[1])
            d = nd(vecs[2])
        else:
            d, cond = nd(list(fixed)), 1.0
        t, w = line_parts(q, d)
        res = [red_norm2([w[0][i], w[1][i], w[2][i]]) * s for i in range(len(q))]
        grad = line_grad(t, w, fixed is None)
        rank = 2 if fixed is not None else 4
        return [("p", cpt), ("d", nd(d))], res, (red_norm2(grad), rank, cond), None
    assert prim == "plane"
    if fixed is None:
        vals, vecs = sym_eig(scatter(q, 3))
        cond = gap_cond(vals[1], vals[0])
        n = nd(vecs[0])
    else:
        n, cond = nd(list(fixed)), 1.0
    r = [red_dot(p, n) for p in q]
    g = [red_sum(r)]
    if fixed is None:
        pc = [[p[k] - ri * n[k] for p, ri in zip(q, r)] for k in range(3)]
        g += [red_dot(r, col) for col in pc]
    rank = 1 if fixed is not None else 3
    return [("p", cpt), ("n", nd(n))], [ri * s for ri in r], (red_norm2(g), rank, cond), None


def encode(prim, feat, res, diag):
    f = lambda x: '"' + hx(x) + '"'
    vec = lambda v: "[" + ",".join(f(x) for x in v) + "]"
    rms, mx, mn, mean, std = stats(res)
    grad, rank, cond = diag
    feature = '{"kind":"' + prim + '"' + "".join(',"' + k + '":' + vec(v) for k, v in feat) + "}"
    return (
        '{"algorithm":"fit.' + prim + '.ls@1.0.0","feature":' + feature
        + ',"residuals":' + vec(res)
        + ',"stats":{"n":"' + str(len(res)) + '","rms":' + f(rms) + ',"max":' + f(mx) + ',"min":' + f(mn)
        + ',"mean":' + f(mean) + ',"std_dev":' + f(std) + "}"
        + ',"diagnostics":{"grad_norm":' + f(grad) + ',"rank":"' + str(rank) + '","cond":' + f(cond)
        + ',"iterations":"0","stop":"closed_form"},"certificate":null}'
    )


# ---------------------------------------------------------------- cases


def max_abs_diff(a, b):
    return max(abs(Decimal(x) - Decimal(y)) for x, y in zip(a, b))


def make_case(name, prim, pts, fixed=None, plane=None, note=""):
    lines = [f"case {name}", f"# {note}", f"primitive {prim}"]
    if fixed is not None:
        lines.append("fixed " + " ".join(hx(x) for x in fixed))
    if plane is not None:
        lines.append("plane_p " + " ".join(hx(x) for x in plane[0]))
        lines.append("plane_n " + " ".join(hx(x) for x in plane[1]))
    lines.append(f"n {len(pts)}")
    lines.append("xyz " + " ".join(hx(v) for p in pts for v in p))
    try:
        feat, res, diag, extra = model(prim, pts, fixed, plane)
    except Ill as e:
        # AC-4: the exact reference must agree that no direction is unique.
        _, _, _, cond_exact = exact_fit(prim, pts, None)
        assert cond_exact > COND_LIMIT, (name, cond_exact)
        lines.append("expect ill_conditioned")
        lines.append("cond " + ("none" if not math.isfinite(e.cond) else hx(e.cond)))
        lines.append(f"# exact eigenvalue-gap condition {cond_exact:.6e}")
        lines.append("end")
        return "\n".join(lines)
    # Exact reference on the same inputs (line2: on the exact projection with the pinned frame).
    if prim == "line2":
        e1, e2, p0 = extra
        coords = []
        for x in pts:
            dx = [Fraction(x[k]) - Fraction(p0[k]) for k in range(3)]
            coords.append((sum(dx[k] * Fraction(e1[k]) for k in range(3)),
                           sum(dx[k] * Fraction(e2[k]) for k in range(3))))
        fixed_ref = None
        if fixed is not None:
            fixed_ref = [x for x in dict(feat)["d"]]
        cref, uref, rmsref, _ = exact_fit(prim, coords, fixed_ref)
    else:
        cref, uref, rmsref, _ = exact_fit(prim, pts, fixed)
    fd = dict(feat)
    pkey = "c" if prim == "point" else "p"
    # The model against the exact reference: centroid within 1e-12 m, direction within 1e-12,
    # RMS residual within 1e-6 relative plus 1e-12 m (line2 projects 1 km coordinates, and
    # the circle's plane residuals are rounding noise, both at the 1e-13 m level).
    assert max_abs_diff(fd[pkey], cref) <= Decimal("1e-12"), name
    if uref is not None:
        dkey = "n" if prim == "plane" else "d"
        assert max_abs_diff(fd[dkey], uref) <= Decimal("1e-12"), name
    rms = stats(res)[0]
    assert abs(Decimal(rms) - rmsref) <= rmsref * Decimal("1e-6") + Decimal("1e-12"), name
    enc = encode(prim, feat, res, diag)
    lines.append("expect ok")
    lines.append("ref_point " + " ".join(f"{x:.25e}" for x in cref))
    if uref is not None:
        lines.append("ref_direction " + " ".join(f"{x:.25e}" for x in uref))
    lines.append(f"ref_rms {rmsref:.25e}")
    lines.append("encoded " + enc)
    lines.append("digest sha256:" + hashlib.sha256(enc.encode("ascii")).hexdigest())
    lines.append("end")
    return "\n".join(lines)


def build():
    n = 260  # two red blocks (256 + 4)
    cases = []
    cases.append(make_case("point-far", "point", gen_point(random.Random(11), n),
                           note="cluster 1 km from the origin, 1 mm spread"))
    line3 = gen_line3(random.Random(12), n)
    cases.append(make_case("line3-far", "line3", line3, note="0.5 m line along (2,3,6)/7, 1 um noise"))
    cases.append(make_case("line3-fixed", "line3", line3, fixed=(2.0, 3.0, 6.0),
                           note="same points, fixed direction (2,3,6)"))
    plane = gen_plane(random.Random(13), n)
    cases.append(make_case("plane-far", "plane", plane, note="0.6 x 0.4 m patch, normal (1,2,2)/3, 1 um noise"))
    cases.append(make_case("plane-fixed", "plane", plane, fixed=(-1.0, -2.0, -2.0),
                           note="same points, fixed normal (-1,-2,-2), sign-normalised"))
    cases.append(make_case("line2-xy", "line2", gen_line2_xy(random.Random(14), n),
                           note="default projection plane z = 0, line along (0.28,0.96) 1 km out"))
    tilted = gen_line2_tilted(random.Random(15), n)
    tplane = (B, (1.0, 2.0, 2.0))
    cases.append(make_case("line2-tilted", "line2", tilted, plane=tplane,
                           note="projection plane through B with normal (1,2,2); points up to 1 mm off it"))
    fdir = add(E1, scale(0.3, N3))
    cases.append(make_case("line2-fixed", "line2", tilted, fixed=fdir, plane=tplane,
                           note="same points, fixed 3D direction E1 + 0.3 N projected onto the plane"))
    circle = gen_circle(200, 5.0)
    cases.append(make_case("circle-line3", "line3", circle,
                           note="AC-4: 200 points evenly on a 5 m circle, line3 has no unique direction"))
    cases.append(make_case("circle-plane", "plane", circle,
                           note="control: the same circle has a well-defined plane"))
    head = [
        "# KRN.LSQ.LIN AC-3 / AC-4: seeded points 1 km from the origin (metres, binary64 hex).",
        "# Generated by fixtures/fit/make_lin_fixtures.py (CC-BY-4.0). Do not edit.",
        "# Per case: primitive, optional fixed direction and projection plane, the points;",
        "# expect ok: exact 80-digit reference point, direction and RMS residual (decimal),",
        "# then the canonical encoding and SHA-256 from the bit-exact model of the pinned rules;",
        "# expect ill_conditioned: the model's eigenvalue-gap condition (hex, or none if infinite).",
        "# Default parameters: FitParams::new(LeastSquares) (cond_limit 1e12, rank_tol 1e-9).",
    ]
    return "\n".join(head) + "\n" + "\n".join(cases) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        old = OUT.read_text(encoding="utf-8").replace("\r\n", "\n") if OUT.exists() else ""
        if old != text:
            print(f"stale: {OUT}", file=sys.stderr)
            return 1
        print("ok")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
