#!/usr/bin/env python3
"""Independent reference for KRN.NUM.LA.MAT (CC-BY-4.0 project data).

Writes
  fixtures/num/la/mat-ops-v1.txt   500 seeded cases (1x1 .. 200x200): mat_vec, mat_t_vec,
                                   mat_mul, solve_upper, solve_lower (AC-1)
  fixtures/num/la/mat-edge-v1.txt  hand-chosen edge, error and ill-conditioned cases
  fixtures/num/la/cond-v1.txt      triangular matrices with condition 1e0 .. 1e14 (AC-2)

Two independent parts, neither of which runs the Rust code (stdlib only):

1. An exact oracle. Products are exact rational dot products (binary64 values are dyadic
   rationals, so each dot is an integer sum after a common power-of-two scaling; the self
   test checks this against `fractions.Fraction`), rounded once. Triangular solves are exact
   `Fraction` substitution, rounded once per component. Condition numbers are exact: the
   eigenvalues of R^T R are bracketed by exact inertia counts (Sylvester's law of inertia on
   fraction-free Bareiss elimination of R^T R - mu I in integers), so every reported
   condition number is a rigorous interval [lo, hi] with hi / lo <= 1 + 2^-14.
   The oracle knows nothing of blocks, compensation, operation order or power iterations.
   Test inputs are built with correctly rounded operations only (no libm sin, pow, log), so
   `--check` gives the same bytes on every platform.
2. A bit-exact Python model of the rules pinned in the leaf's Design section: the KRN.NUM.RED
   block-and-tree Neumaier dot (copied from its Design text), the product and substitution
   formulas and the power-iteration condition estimator. CPython floats are binary64 with
   round-to-nearest-even and never fuse a multiply-add, so the model's bit patterns are the
   expected Rust results on every target.

Before anything is written the script asserts the AC bounds of the model against the oracle:
products within 2 k u (|A||x|)_i per element, solves within 2 n u ||x||_inf in max norm
(u = 2^-53), condition estimates within a factor of 10 of the exact interval.

Run:  python fixtures/num/la/make_la_fixtures.py          (writes the fixtures)
      python fixtures/num/la/make_la_fixtures.py --check  (fails if a fixture is stale)
"""

import hashlib
import math
import random
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
BLOCK = 256
ITERATIONS = 8
MAX = sys.float_info.max
TINY = 5e-324
MASK64 = (1 << 64) - 1
U = Fraction(1, 2 ** 53)


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def fb(u):
    return struct.unpack("<d", struct.pack("<Q", u & MASK64))[0]


def hx(x):
    return f"{bits(x):016x}"


def finite(x):
    return not (math.isnan(x) or math.isinf(x))


class Overflow(Exception):
    pass


# ---------------------------------------------------------------- 2. model: KRN.NUM.RED


def block_sum(terms):
    s, c = -0.0, -0.0
    for x in terms:
        t = s + x
        if abs(s) >= abs(x):
            c = c + ((s - t) + x)
        else:
            c = c + ((x - t) + s)
        s = t
    return s, c


def combine(p, q):
    s1, c1 = p
    s2, c2 = q
    s = s1 + s2
    e = (s1 - s) + s2 if abs(s1) >= abs(s2) else (s2 - s) + s1
    return s, (c1 + c2) + e


def tree(pairs):
    if len(pairs) == 1:
        return pairs[0]
    left = len(pairs) // 2
    return combine(tree(pairs[:left]), tree(pairs[left:]))


def fixed_sum(terms):
    """The RED rule on terms that are all finite; raises Overflow like red::fixed_sum."""
    if not terms:
        return 0.0
    for t in terms:
        if not finite(t):
            raise Overflow()
    s, c = tree([block_sum(terms[i:i + BLOCK]) for i in range(0, len(terms), BLOCK)])
    if not (finite(s) and finite(c)):
        raise Overflow()
    r = s if c == 0 else s + c
    if not finite(r):
        raise Overflow()
    return r


def dot(a, b):
    assert len(a) == len(b)
    return fixed_sum([x * y for x, y in zip(a, b)])


def exponent(m):
    return math.frexp(m)[1] - 1  # 2^e <= m < 2^(e+1), subnormals included


def scale_factors(e):
    if e > 1022:
        return math.ldexp(1.0, -1022), math.ldexp(1.0, 1022 - e)
    if e >= -1022:
        return math.ldexp(1.0, -e), 1.0
    return math.ldexp(1.0, 1022), math.ldexp(1.0, -e - 1022)


def norm2(xs):
    m = max((abs(x) for x in xs), default=0.0)
    if m == 0:
        return 0.0
    e = exponent(m)
    d1, d2 = scale_factors(e)
    r = fixed_sum([((x * d1) * d2) * ((x * d1) * d2) for x in xs])
    r = math.sqrt(r)
    if e >= -1022:
        r = r * math.ldexp(1.0, e)
    else:
        r = (r * math.ldexp(1.0, e + 1022)) * math.ldexp(1.0, -1022)
    if not finite(r):
        raise Overflow()
    return r


# ---------------------------------------------------------------- 2. model: KRN.NUM.LA.MAT
# Results: ("ok", [floats]) | ("rank", n) | ("err", kind, info). Matrices: (rows, cols, data).


def scan(xs):
    for i, x in enumerate(xs):
        if math.isnan(x):
            return ("err", "NotANumber", i)
        if math.isinf(x):
            return ("err", "Infinite", i)
    return None


def check_tol(t):
    if math.isnan(t):
        return ("err", "NotANumber", None)
    if math.isinf(t):
        return ("err", "Infinite", None)
    if t < 0:
        return ("err", "Domain", "bad_tolerance")
    return None


def m_new(rows, cols, data):
    if rows == 0 or cols == 0:
        return ("err", "Domain", "bad_shape")
    if rows > 4096 or cols > 4096:
        return ("err", "Domain", "too_large")
    if len(data) != rows * cols:
        return ("err", "Domain", "bad_shape")
    return scan(data) or ("ok", list(data))


def row(a, i):
    r, c, d = a
    return d[i * c:(i + 1) * c]


def col(a, j):
    r, c, d = a
    return [d[i * c + j] for i in range(r)]


def m_mat_vec(a, x):
    if len(x) != a[1]:
        return ("err", "Domain", "bad_shape")
    e = scan(x)
    if e:
        return e
    y = []
    for i in range(a[0]):
        try:
            y.append(dot(row(a, i), x))
        except Overflow:
            return ("err", "Infinite", i)
    return ("ok", y)


def m_mat_t_vec(a, x):
    if len(x) != a[0]:
        return ("err", "Domain", "bad_shape")
    e = scan(x)
    if e:
        return e
    y = []
    for j in range(a[1]):
        try:
            y.append(dot(col(a, j), x))
        except Overflow:
            return ("err", "Infinite", j)
    return ("ok", y)


def m_mat_mul(a, b):
    if a[1] != b[0]:
        return ("err", "Domain", "bad_shape")
    cols = [col(b, j) for j in range(b[1])]
    out = []
    for i in range(a[0]):
        ai = row(a, i)
        for j in range(b[1]):
            try:
                out.append(dot(ai, cols[j]))
            except Overflow:
                return ("err", "Infinite", i * b[1] + j)
    return ("ok", out)


def singular_scan(n, d, tol):
    m = 0.0
    for i in range(n):
        m = max(m, abs(d[i * n + i]))
    thr = tol * m
    for i in range(n):
        if abs(d[i * n + i]) <= thr:
            return ("err", "Singular", i)
    return None


def div(a, b):
    """IEEE a / b for finite a, b; None when the quotient is not finite (incl. b == 0)."""
    if b == 0:
        return None
    q = a / b
    return q if finite(q) else None


def back_sub(n, d, b):
    """None-index pair: (x, None) or (None, failing i)."""
    x = [0.0] * n
    for i in range(n - 1, -1, -1):
        try:
            s = dot(d[i * n + i + 1:(i + 1) * n], x[i + 1:])
        except Overflow:
            return None, i
        t = b[i] - s
        q = div(t, d[i * n + i]) if finite(t) else None
        if q is None:
            return None, i
        x[i] = q
    return x, None


def fwd_sub(n, d, b):
    x = []
    for i in range(n):
        try:
            s = dot(d[i * n:i * n + i], x[:i])
        except Overflow:
            return None, i
        t = b[i] - s
        q = div(t, d[i * n + i]) if finite(t) else None
        if q is None:
            return None, i
        x.append(q)
    return x, None


def m_solve(kind, a, b, tol):
    n = a[0]
    if a[1] != n or len(b) != n:
        return ("err", "Domain", "bad_shape")
    e = scan(b) or check_tol(tol) or singular_scan(n, a[2], tol)
    if e:
        return e
    x, bad = (back_sub if kind == "upper" else fwd_sub)(n, a[2], b)
    return ("err", "Infinite", bad) if x is None else ("ok", x)


def m_rank(ds, tol):
    if not ds:
        return ("err", "EmptyInput", None)
    e = scan(ds) or check_tol(tol)
    if e:
        return e
    if any(d < 0 for d in ds) or any(ds[i] < ds[i + 1] for i in range(len(ds) - 1)):
        return ("err", "Domain", "bad_spectrum")
    thr = tol * ds[0]
    return ("rank", sum(1 for d in ds if d > thr))


def m_cond(a, tol):
    n = a[0]
    if a[1] != n:
        return ("err", "Domain", "bad_shape")
    d = a[2]
    e = check_tol(tol) or singular_scan(n, d, tol)
    if e:
        return e
    m = max(abs(d[i * n + j]) for i in range(n) for j in range(i, n))
    d1, d2 = scale_factors(exponent(m))
    s = [0.0] * (n * n)
    st = [0.0] * (n * n)
    for i in range(n):
        for j in range(i, n):
            v = (d[i * n + j] * d1) * d2
            s[i * n + j] = v
            st[j * n + i] = v

    def normalize(x):
        nu = norm2(x)
        if nu == 0:
            raise Overflow()
        return [xi / nu for xi in x]

    def up(u):
        return [dot(s[i * n + i:(i + 1) * n], u[i:]) for i in range(n)]

    def lo(v):
        return [dot(st[j * n:j * n + j + 1], v[:j + 1]) for j in range(n)]

    def inv(u):
        x, bad = back_sub(n, s, u)
        if x is None:
            raise Overflow()
        return x

    def inv_t(v):
        x, bad = fwd_sub(n, st, v)
        if x is None:
            raise Overflow()
        return x

    def estimate(p, q):
        x = [1.0] * n
        for _ in range(ITERATIONS):
            u = normalize(x)
            v = normalize(p(u))
            x = q(v)
        return norm2(p(normalize(x)))

    try:
        c = estimate(up, lo) * estimate(inv, inv_t)
    except Overflow:
        return ("err", "Infinite", None)
    if not finite(c) or c <= 0:
        return ("err", "Infinite", None)
    return ("ok", [c])


# ---------------------------------------------------------------- 1. exact oracle


def ratio(x):
    """Exact (integer, exponent) with x = n * 2^e."""
    if x == 0:
        return 0, 0
    m, e = math.frexp(x)
    return int(m * 2 ** 53), e - 53


def exact_dot(a, b):
    """(sum, abs_sum, e): exact a.b = sum * 2^e and sum |a_i b_i| = abs_sum * 2^e."""
    terms = []
    for x, y in zip(a, b):
        (mx, ex), (my, ey) = ratio(x), ratio(y)
        if mx and my:
            terms.append((mx * my, ex + ey))
    if not terms:
        return 0, 0, 0
    e0 = min(e for _, e in terms)
    s = sa = 0
    for p, e in terms:
        v = p << (e - e0)
        s += v
        sa += abs(v)
    return s, sa, e0


def as_fraction(n, e):
    return Fraction(n) * Fraction(2) ** e


def exact_solve(kind, n, d, b):
    x = [Fraction(0)] * n
    order = range(n - 1, -1, -1) if kind == "upper" else range(n)
    for i in order:
        js = range(i + 1, n) if kind == "upper" else range(i)
        s = sum((Fraction(d[i * n + j]) * x[j] for j in js), Fraction(0))
        x[i] = (Fraction(b[i]) - s) / Fraction(d[i * n + i])
    return x


def count_below(mint, p, q):
    """Number of eigenvalues of the integer symmetric matrix mint below p / 2^q (q >= 0),
    by the signs of the leading principal minors of 2^q mint - p I (fraction-free Bareiss).
    None if a leading minor is zero."""
    n = len(mint)
    a = [[(mint[i][j] << q) - (p if i == j else 0) for j in range(n)] for i in range(n)]
    prev, sign_prev, neg = 1, 1, 0
    for k in range(n):
        piv = a[k][k]
        if piv == 0:
            return None
        sgn = 1 if piv > 0 else -1
        if sgn != sign_prev:
            neg += 1
        sign_prev = sgn
        for i in range(k + 1, n):
            aik = a[i][k]
            for j in range(i, n):
                a[i][j] = (a[i][j] * piv - aik * a[k][j]) // prev
                a[j][i] = a[i][j]
        prev = piv
    return neg


def mu_parts(t, k, bits_):
    """mu = 2^t (1 + k / 2^bits_) as (p, q) with mu = p / 2^q, q >= 0."""
    p = (1 << bits_) + k
    q = bits_ - t
    if q < 0:
        return p << -q, 0
    return p, q


def count_at(mint, t, k, bits_):
    p, q = mu_parts(t, k, bits_)
    c = count_below(mint, p, q)
    while c is None:  # mu is an eigenvalue of a leading block: move it up by one step
        p, q = 2 * p + 1, q + 1
        c = count_below(mint, p, q)
    return c


def bracket_eig(mint, target, t_lo, t_hi, bits_=14):
    """Bracket [lo, hi] (Fractions) of the target-th smallest eigenvalue (1-based), with
    count(lo) < target <= count(hi)."""
    while t_hi - t_lo > 1:  # count(2^t_lo) < target <= count(2^t_hi)
        t = (t_lo + t_hi) // 2
        if count_at(mint, t, 0, bits_) >= target:
            t_hi = t
        else:
            t_lo = t
    k_lo, k_hi = 0, 1 << bits_  # mu in [2^t_lo, 2^(t_lo + 1)]
    while k_hi - k_lo > 1:
        k = (k_lo + k_hi) // 2
        if count_at(mint, t_lo, k, bits_) >= target:
            k_hi = k
        else:
            k_lo = k

    def val(k):
        p, q = mu_parts(t_lo, k, bits_)
        return Fraction(p, 2 ** q)

    return val(k_lo), val(k_hi)


def exact_cond(n, d):
    """Rigorous interval for the 2-norm condition number of the upper triangle of d."""
    up = [(i, j) for i in range(n) for j in range(i, n)]
    rs = {ij: ratio(d[ij[0] * n + ij[1]]) for ij in up}
    e0 = min(e for (m, e) in rs.values() if m)
    rint = [[0] * n for _ in range(n)]
    for (i, j), (m, e) in rs.items():
        if m:
            rint[i][j] = m << (e - e0)
    mint = [[sum(rint[k][i] * rint[k][j] for k in range(n)) for j in range(n)] for i in range(n)]
    trace = sum(mint[i][i] for i in range(n))
    t_hi = trace.bit_length() + 1
    t_lo = -(n - 1) * trace.bit_length() - 2
    assert count_at(mint, t_lo, 0, 14) == 0 and count_at(mint, t_hi, 0, 14) == n
    lmax = bracket_eig(mint, n, t_lo, t_hi)
    lmin = bracket_eig(mint, 1, t_lo, t_hi)
    lo2, hi2 = lmax[0] / lmin[1], lmax[1] / lmin[0]
    widen = 1e-9
    return math.sqrt(float(lo2)) * (1 - widen), math.sqrt(float(hi2)) * (1 + widen)


# ---------------------------------------------------------------- shared input generator


def splitmix64(state):
    state = (state + 0x9E3779B97F4A7C15) & MASK64
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
    return state, z ^ (z >> 31)


class Gen:
    """The value stream the Rust test rebuilds from the seed (see the fixture header)."""

    def __init__(self, seed):
        self.st = seed

    def next(self):
        self.st, u = splitmix64(self.st)
        return u

    def general(self):
        u = self.next()
        return ((u >> 11) * 2.0 ** -52 - 1.0) * 2.0 ** ((u & 15) - 8)

    def diag(self):
        u = self.next()
        v = (u >> 12) * 2.0 ** -51 + 1.0
        return -v if (u >> 11) & 1 else v

    def off(self, k):
        u = self.next()
        return ((u >> 11) * 2.0 ** -52 - 1.0) * 2.0 ** -k.bit_length()

    def tri(self, k, upper):
        d = []
        for i in range(k):
            for j in range(k):
                if i == j:
                    d.append(self.diag())
                elif (j > i) == upper:
                    d.append(self.off(k))
                else:
                    d.append(self.general())
        return d


def gen_case(seed, m, k, n):
    g = Gen(seed)
    a = [g.general() for _ in range(m * k)]
    x = [g.general() for _ in range(k)]
    w = [g.general() for _ in range(m)]
    b = [g.general() for _ in range(k * n)]
    up = g.tri(k, True)
    bu = [g.general() for _ in range(k)]
    lo = g.tri(k, False)
    bl = [g.general() for _ in range(k)]
    return a, x, w, b, up, bu, lo, bl


# ---------------------------------------------------------------- AC checks of model vs oracle


def check_products(name, a_rows, x_rows, got):
    """Every element within 2 k u sum|a_i x_i| (+ k 2^-1074) of the exact dot."""
    worst = 0.0
    for ai, xi, y in zip(a_rows, x_rows, got):
        s, sa, e = exact_dot(ai, xi)
        k = len(ai)
        err = abs(Fraction(y) - as_fraction(s, e))
        # Plus k * 2^-1074 for products that underflow (gradual underflow: absolute error).
        bound = 2 * k * U * as_fraction(sa, e) + k * Fraction(1, 2 ** 1074)
        assert err <= bound, (name, float(err), float(bound))
        if bound:
            worst = max(worst, float(err / bound))
    return worst


def check_solve(name, kind, n, d, b, got):
    x = exact_solve(kind, n, d, b)
    xmax = max(abs(v) for v in x)
    err = max(abs(Fraction(g) - v) for g, v in zip(got, x))
    bound = 2 * n * U * xmax + n * Fraction(1, 2 ** 1074)  # + gradual-underflow term
    assert err <= bound, (name, kind, float(err), float(bound))
    return float(err / bound) if bound else 0.0


# ---------------------------------------------------------------- mat-ops-v1


def ops_sizes():
    rng = random.Random(20261118)
    fixed = [(1, 1, 1), (200, 200, 200), (200, 1, 200), (1, 200, 1), (200, 200, 1), (1, 200, 200),
             (128, 64, 32), (3, 200, 5), (200, 150, 7), (64, 128, 64), (17, 100, 3), (100, 100, 100)]
    out = list(fixed)
    while len(out) < 500:
        r = rng.random()
        if r < 0.1:
            out.append((rng.randint(1, 4), rng.randint(1, 4), rng.randint(1, 4)))
        elif r < 0.9:
            out.append((rng.randint(1, 24), rng.randint(1, 24), rng.randint(1, 24)))
        else:
            out.append((rng.randint(1, 64), rng.randint(25, 80), rng.randint(1, 16)))
    return out


SEED0 = 0x4C41_4D41_5400_0000


def short(vals):
    h = hashlib.sha256()
    for v in vals:
        h.update(struct.pack("<Q", bits(v)))
    return h.hexdigest()[:16]


def make_ops():
    out = [
        "# KRN.NUM.LA.MAT AC-1: 500 seeded cases (1x1 .. 200x200), CC-BY-4.0.",
        "# Written by make_la_fixtures.py; do not edit. Inputs are rebuilt from `seed` by the",
        "# splitmix64 stream: draws u_1, u_2, ...; general(u) = ((u >> 11) * 2^-52 - 1) * 2^((u & 15) - 8);",
        "# diag(u) = +-((u >> 12) * 2^-51 + 1), minus iff bit 11 of u is set;",
        "# off(u) = ((u >> 11) * 2^-52 - 1) * 2^-bitlen(k). Draw order: A (m x k row-major, general),",
        "# x (k), w (m), B (k x n), U (k x k: diag on the diagonal, off above, general below),",
        "# bu (k), L (k x k: diag on the diagonal, off below, general above), bl (k).",
        "# Results: mat_vec(A, x), mat_t_vec(A, w), mat_mul(A, B), solve_upper(U, bu, 0),",
        "# solve_lower(L, bl, 0); each column is the first 16 hex digits of SHA-256 of the",
        "# little-endian result bits. `digest` = SHA-256 of all result bits, case by case in",
        "# that operation order. The script asserts each product element within 2 k u (|A||x|)_i",
        "# and each solve within 2 k u ||x||_inf of the exact rational result.",
    ]
    body = []
    h = hashlib.sha256()
    worst = [0.0, 0.0]
    for idx, (m, k, n) in enumerate(ops_sizes()):
        seed = SEED0 + idx
        a, x, w, b, up, bu, lo, bl = gen_case(seed, m, k, n)
        am, bm, um, lm = (m, k, a), (k, n, b), (k, k, up), (k, k, lo)
        res = []
        r = m_mat_vec(am, x)
        res.append(r[1])
        worst[0] = max(worst[0], check_products("mat_vec", [row(am, i) for i in range(m)], [x] * m, r[1]))
        r = m_mat_t_vec(am, w)
        res.append(r[1])
        worst[0] = max(worst[0], check_products("mat_t_vec", [col(am, j) for j in range(k)], [w] * k, r[1]))
        r = m_mat_mul(am, bm)
        res.append(r[1])
        if m * k * n <= 200_000 or idx < 3:
            arows = [row(am, i) for i in range(m) for _ in range(n)]
            bcols = [col(bm, j) for _ in range(m) for j in range(n)]
            worst[0] = max(worst[0], check_products("mat_mul", arows, bcols, r[1]))
        for kind, mat, rhs in (("upper", um, bu), ("lower", lm, bl)):
            r = m_solve(kind, mat, rhs, 0.0)
            assert r[0] == "ok", (idx, kind, r)
            res.append(r[1])
            if k <= 64 or idx < 3:
                worst[1] = max(worst[1], check_solve(idx, kind, k, mat[2], rhs, r[1]))
        for v in res:
            for y in v:
                h.update(struct.pack("<Q", bits(y)))
        body.append(f"case {idx} seed {seed:016x} m {m} k {k} n {n} " + " ".join(
            f"{op} {short(v)}" for op, v in zip(("mat_vec", "mat_t_vec", "mat_mul", "solve_upper", "solve_lower"), res)))
    out.append(f"# worst error / bound: products {worst[0]:.3f}, solves {worst[1]:.3f}")
    out.append(f"digest sha256:{h.hexdigest()}")
    return "\n".join(out + body) + "\n"


# ---------------------------------------------------------------- mat-edge-v1


def kahan(n, s):
    """Kahan's matrix diag(1, s, .., s^(n-1)) (I - c * strict upper ones), c = sqrt(1 - s^2).
    Inputs use only correctly rounded operations, so they are the same on every platform."""
    c = math.sqrt(1.0 - s * s)
    d = []
    si = 1.0
    for i in range(n):
        if i:
            si = si * s
        for j in range(n):
            d.append(si if i == j else (-c * si if j > i else 0.0))
    return d


def hilbert_upper(n):
    return [1.0 / (i + j + 1) if j >= i else 0.0 for i in range(n) for j in range(n)]


def edge_cases():
    c = []

    def add(name, op, a, x=None, tol=None):
        c.append((name, op, a, x, tol))

    big, nan, inf = 1e200, math.nan, math.inf
    # Products: cancellation, signed zeros, subnormals, overflow, error order.
    add("dot-cancel", "mat_vec", (1, 3, [1e16, 1.0, -1e16]), [1.0, 1.0, 1.0])
    add("dot-cancel-tiny", "mat_vec", (2, 4, [1.0, 2.0 ** -60, -1.0, 3.0, 1e300, 1.0, -1e300, 1e-300]),
        [1.0, 1.0, 1.0, 2.0 ** -40])
    add("neg-zero", "mat_vec", (2, 1, [-0.0, 0.0]), [1.0])
    add("neg-zero-sum", "mat_vec", (1, 2, [-0.0, -1.0]), [5.0, -0.0])
    add("subnormal", "mat_vec", (1, 3, [TINY, 2.0 ** -1060, 3.0]), [0.5, 3.0, 2.0 ** -1070])
    add("overflow-row1", "mat_vec", (2, 2, [1.0, 1.0, MAX, MAX]), [1.0, 1.0])
    add("overflow-product", "mat_vec", (2, 2, [1.0, big, 1.0, 1.0]), [1.0, big])
    add("intermediate-overflow", "mat_vec", (1, 3, [MAX, MAX, -MAX]), [1.0, 1.0, 1.0])
    add("x-nan", "mat_vec", (1, 3, [1.0, 2.0, 3.0]), [1.0, nan, inf])
    add("x-inf", "mat_vec", (1, 3, [1.0, 2.0, 3.0]), [1.0, 2.0, -inf])
    add("x-short", "mat_vec", (1, 3, [1.0, 2.0, 3.0]), [1.0, nan])
    add("t-cancel", "mat_t_vec", (3, 1, [1e16, 1.0, -1e16]), [1.0, 1.0, 1.0])
    add("t-overflow-col2", "mat_t_vec", (2, 3, [1.0, 1.0, MAX, 1.0, 1.0, MAX]), [1.0, 1.0])
    add("t-shape", "mat_t_vec", (2, 3, [1.0] * 6), [1.0, 1.0, 1.0])
    add("t-nan", "mat_t_vec", (2, 3, [1.0] * 6), [nan, 1.0])
    add("mul-overflow-10", "mat_mul", (2, 2, [1.0, 1.0, MAX, 1.0]), (2, 2, [1.0, 1.0, MAX, 1.0]))
    add("mul-shape", "mat_mul", (2, 3, [1.0] * 6), (2, 3, [1.0] * 6))
    add("mul-cancel", "mat_mul", (2, 3, [1e16, 1.0, -1e16, 1.0, 1.0, 1.0]),
        (3, 2, [1.0, 1.0, 1.0, -0.0, 1.0, 1.0]))
    add("mul-neg-zero", "mat_mul", (1, 2, [-0.0, -0.0]), (2, 1, [1.0, 2.0]))
    long_row = [((i * 7919) % 1000 - 500) * 2.0 ** -9 for i in range(600)]
    add("mul-long-blocks", "mat_mul", (1, 600, long_row), (600, 1, [1.0 / 3.0] * 600))
    # Solves.
    ac4 = [1.0, 0.5, 0.25, 9.0, 1.0, 0.5, 7.0, 3.0, 1e-20]
    add("ac4-singular-2", "solve_upper", (3, 3, ac4), [1.0, 2.0, 3.0], 1e-12)
    add("ac4-tol0-ok", "solve_upper", (3, 3, ac4), [1.0, 2.0, 3.0], 0.0)
    add("zero-diag-tol0", "solve_upper", (2, 2, [1.0, 2.0, 0.0, 0.0]), [1.0, 1.0], 0.0)
    add("all-zero-diag", "solve_lower", (2, 2, [0.0, 5.0, 3.0, -0.0]), [1.0, 1.0], 0.0)
    add("tol-equal", "solve_upper", (2, 2, [1.0, 0.0, 0.0, 0.5]), [1.0, 1.0], 0.5)
    add("tol-just-below", "solve_upper", (2, 2, [1.0, 0.0, 0.0, 0.5]), [1.0, 1.0], 0.49999999999999994)
    add("tol-nan", "solve_upper", (1, 1, [1.0]), [1.0], nan)
    add("tol-inf", "solve_upper", (1, 1, [1.0]), [1.0], inf)
    add("tol-neg", "solve_lower", (1, 1, [1.0]), [1.0], -1e-12)
    add("tol-neg-zero", "solve_lower", (1, 1, [2.0]), [-0.0], -0.0)
    add("b-nan-before-tol", "solve_upper", (2, 2, [1.0, 0.0, 0.0, 1.0]), [1.0, nan], nan)
    add("b-inf", "solve_lower", (2, 2, [1.0, 0.0, 0.0, 1.0]), [inf, 1.0], 0.0)
    add("not-square", "solve_upper", (2, 3, [1.0] * 6), [1.0, 1.0], 0.0)
    add("b-short", "solve_lower", (2, 2, [1.0, 0.0, 0.0, 1.0]), [1.0], 0.0)
    add("overflow-tiny-diag", "solve_upper", (2, 2, [1.0, 1.0, 0.0, 1e-300]), [1.0, 1e10], 0.0)
    add("overflow-lower-1", "solve_lower", (2, 2, [1e-300, 0.0, MAX, 1.0]), [1e-10, 1.0], 0.0)
    add("lower-garbage-ignored", "solve_upper", (3, 3, [2.0, 1.0, 1.0, 1e300, 4.0, 1.0, -7e200, 3.0, 8.0]),
        [1.0, 2.0, 3.0], 0.0)
    add("upper-garbage-ignored", "solve_lower", (3, 3, [2.0, 1e300, -1e300, 1.0, 4.0, 5e250, 1.0, 1.0, 8.0]),
        [1.0, 2.0, 3.0], 0.0)
    add("neg-zero-rhs", "solve_upper", (2, 2, [2.0, 3.0, 0.0, -4.0]), [-0.0, 0.0], 0.0)
    add("subnormal-solution", "solve_upper", (1, 1, [2.0]), [TINY], 0.0)
    add("kahan-30", "solve_upper", (30, 30, kahan(30, 0.93)), [1.0] * 30, 0.0)
    add("kahan-30-lower", "solve_lower", (30, 30, [kahan(30, 0.93)[j * 30 + i] for i in range(30) for j in range(30)]),
        [1.0] * 30, 0.0)
    add("hilbert-upper-12", "solve_upper", (12, 12, hilbert_upper(12)), [1.0] * 12, 1e-15)
    # Rank rule (AC-5 and its edges).
    add("ac5-rank-2", "rank", [3.0, 1.0, 1e-13, 0.0], None, 1e-12)
    add("rank-threshold-equal", "rank", [1.0, 1e-12], None, 1e-12)
    add("rank-tol0", "rank", [2.0, 1e-300, TINY, 0.0, -0.0], None, 0.0)
    add("rank-all-zero", "rank", [0.0, 0.0], None, 1e-12)
    add("rank-full", "rank", [5.0, 4.0, 4.0], None, 1e-12)
    add("rank-empty", "rank", [], None, 1e-12)
    add("rank-nan", "rank", [1.0, nan], None, 1e-12)
    add("rank-tol-nan", "rank", [1.0], None, nan)
    add("rank-unsorted", "rank", [1.0, 2.0], None, 1e-12)
    add("rank-negative", "rank", [1.0, -0.5], None, 1e-12)
    add("rank-tol-huge", "rank", [1e300, 1e300], None, 1e300)
    # Condition estimator error paths and small exact cases.
    add("cond-singular-2", "cond", (3, 3, ac4), None, 1e-12)
    add("cond-zero", "cond", (2, 2, [0.0, 1.0, 0.0, 0.0]), None, 0.0)
    add("cond-not-square", "cond", (1, 2, [1.0, 1.0]), None, 1e-12)
    add("cond-tol-inf", "cond", (1, 1, [1.0]), None, inf)
    add("cond-1x1", "cond", (1, 1, [-3.0]), None, 0.0)
    add("cond-subnormal-1x1", "cond", (1, 1, [TINY]), None, 1e-12)
    add("cond-diag", "cond", (2, 2, [4.0, 0.0, 0.0, 0.25]), None, 0.0)
    add("cond-range-overflow", "cond", (2, 2, [1.0, 1e300, 0.0, 1.0]), None, 0.0)
    add("cond-huge-entries", "cond", (2, 2, [MAX, MAX, 0.0, MAX]), None, 0.0)
    return c


def model_edge(op, a, x, tol):
    if op == "rank":
        return m_rank(a, tol)
    am = (a[0], a[1], a[2])
    if op == "mat_vec":
        return m_mat_vec(am, x)
    if op == "mat_t_vec":
        return m_mat_t_vec(am, x)
    if op == "mat_mul":
        return m_mat_mul(am, x)
    if op == "solve_upper":
        return m_solve("upper", am, x, tol)
    if op == "solve_lower":
        return m_solve("lower", am, x, tol)
    if op == "cond":
        return m_cond(am, tol)
    raise ValueError(op)


def expect_str(r):
    if r[0] == "ok":
        return "ok " + " ".join(hx(v) for v in r[1]) if r[1] else "ok"
    if r[0] == "rank":
        return f"rank {r[1]}"
    _, kind, info = r
    return f"err {kind} {'none' if info is None else info}"


def vec_str(xs):
    return " ".join(hx(v) for v in xs)


def make_edge():
    out = [
        "# KRN.NUM.LA.MAT edge, error and ill-conditioned cases, CC-BY-4.0.",
        "# Written by make_la_fixtures.py; do not edit. Per case: `case <name> <op>`, then",
        "# `a <rows> <cols> <hex...>` (row-major; for `rank` the list), optional `x` (a vector,",
        "# or `<rows> <cols> <hex...>` for mat_mul), optional `tol <hex>`, then `expect` as",
        "# `ok <hex...>` (result bits), `rank <n>`, or `err <Kind> <index|none|detail>`.",
        "# `exact` gives the correctly rounded exact result (products, solves) or the rigorous",
        "# condition interval `lo hi` (cond), for information; the Rust test compares bits only.",
    ]
    for name, op, a, x, tol in edge_cases():
        r = model_edge(op, a, x, tol)
        out.append(f"case {name} {op}")
        if op == "rank":
            out.append(("a " + vec_str(a)).rstrip())
        else:
            out.append(f"a {a[0]} {a[1]} {vec_str(a[2])}")
        if x is not None:
            out.append(f"x {x[0]} {x[1]} {vec_str(x[2])}" if op == "mat_mul" else ("x " + vec_str(x)).rstrip())
        if tol is not None:
            out.append(f"tol {hx(tol)}")
        out.append("expect " + expect_str(r))
        if r[0] == "ok" and op in ("mat_vec", "mat_t_vec", "mat_mul"):
            am = (a[0], a[1], a[2])
            if op == "mat_vec":
                pairs = [(row(am, i), x) for i in range(a[0])]
            elif op == "mat_t_vec":
                pairs = [(col(am, j), x) for j in range(a[1])]
            else:
                pairs = [(row(am, i), col(x, j)) for i in range(a[0]) for j in range(x[1])]
            ex = [float(as_fraction(*exact_dot(p, q)[::2])) for p, q in pairs]
            check_products(name, [p for p, _ in pairs], [q for _, q in pairs], r[1])
            out.append("exact " + vec_str(ex))
        elif r[0] == "ok" and op.startswith("solve"):
            kind = op[6:]
            ex = exact_solve(kind, a[0], a[2], x)
            out.append("exact " + vec_str([float(v) for v in ex]))
            if name.startswith(("kahan", "hilbert")):
                continue  # ill-conditioned: bit-exact only, forward error not bounded by 2nu
            check_solve(name, kind, a[0], a[2], x, r[1])
        elif r[0] == "ok" and op == "cond":
            lo, hi = exact_cond(a[0], a[2])
            out.append(f"exact {hx(lo)} {hx(hi)}")
            assert lo / 10 <= r[1][0] <= hi * 10, (name, lo, hi, r[1][0])
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------- cond-v1


def householder_r(n, a):
    """R factor of the n x n row-major float matrix a (plain float Householder QR; only
    used to build test inputs, never as a reference)."""
    m = [a[i * n:(i + 1) * n] for i in range(n)]
    for k in range(n):
        xs = [m[i][k] for i in range(k, n)]
        nx = math.sqrt(sum(v * v for v in xs))
        if nx == 0:
            continue
        alpha = -nx if xs[0] >= 0 else nx
        v = xs[:]
        v[0] -= alpha
        vv = sum(t * t for t in v)
        if vv == 0:
            continue
        for j in range(k, n):
            s = sum(v[i - k] * m[i][j] for i in range(k, n))
            f = 2 * s / vv
            for i in range(k, n):
                m[i][j] -= f * v[i - k]
    return [m[i][j] if j >= i else 0.0 for i in range(n) for j in range(n)]


def random_orthogonal(n, rng):
    q = []
    for _ in range(n):
        v = [rng.uniform(-1, 1) for _ in range(n)]
        for _ in range(2):
            for u in q:
                d = sum(a * b for a, b in zip(u, v))
                v = [a - d * b for a, b in zip(v, u)]
        nv = math.sqrt(sum(t * t for t in v))
        q.append([t / nv for t in v])
    return q


def grade(i, n, k2):
    """2^-round(k2 * i / (n - 1)): exact powers of two, so no libm call shapes the inputs."""
    return math.ldexp(1.0, -((2 * k2 * i + (n - 1)) // (2 * (n - 1)))) if n > 1 else 1.0


def svd_built(n, k2, rng):
    uq, vq = random_orthogonal(n, rng), random_orthogonal(n, rng)
    sig = [grade(i, n, k2) for i in range(n)]
    a = [sum(uq[k][i] * sig[k] * vq[k][j] for k in range(n)) for i in range(n) for j in range(n)]
    return householder_r(n, a)


def graded(n, k2, rng, garbage):
    d = []
    for i in range(n):
        di = grade(i, n, k2)
        di = -di if rng.random() < 0.5 else di
        for j in range(n):
            if j == i:
                d.append(di)
            elif j > i:
                d.append(di * rng.uniform(-1, 1) / n)
            else:
                d.append(rng.uniform(-1e3, 1e3) if garbage else 0.0)
    return d


def cond_cases():
    rng = random.Random(5150)
    cases = []
    for kappa_exp in range(0, 15):
        k2 = kappa_exp * 3322 // 1000  # condition about 2^k2 ~ 10^kappa_exp
        for n in (2, 5, 12):
            cases.append((f"graded-k{kappa_exp}-n{n}", n, graded(n, k2, rng, n == 5)))
        for n in (4, 10):
            cases.append((f"svd-k{kappa_exp}-n{n}", n, svd_built(n, k2, rng)))
    for n, s in ((8, 0.48), (12, 0.72), (16, 0.84), (20, 0.91), (20, 0.56), (25, 0.48), (30, 0.43)):
        cases.append((f"kahan-n{n}-s{s}", n, kahan(n, s)))
    cases.append(("identity-40", 40, [1.0 if i == j else 0.0 for i in range(40) for j in range(40)]))
    cases.append(("scaled-up-2^900", 6, [v * 2.0 ** 900 for v in graded(6, 17, rng, False)]))
    cases.append(("scaled-down-2^-1000", 6, [v * 2.0 ** -1000 for v in graded(6, 17, rng, False)]))
    cases.append(("hilbert-upper-8", 8, hilbert_upper(8)))
    return cases


def make_cond():
    out = [
        "# KRN.NUM.LA.MAT AC-2: upper triangular matrices with condition 1e0 .. 1e14 (Kahan cases to 1e19), CC-BY-4.0.",
        "# Written by make_la_fixtures.py; do not edit. `case <name> n <n> tol <hex> lo <hex> hi <hex>",
        "# est <hex>` followed by the n x n row-major entries (hex, n per line; entries below the",
        "# diagonal are ignored by the estimator and some cases fill them with noise). [lo, hi] is a",
        "# rigorous interval for the exact 2-norm condition number (exact inertia bisection);",
        "# est is the pinned estimator's result. The script asserts lo / 10 <= est <= 10 hi.",
        "# `digest` = SHA-256 of the little-endian bits of every est, in case order.",
    ]
    body = []
    h = hashlib.sha256()
    tol = 1e-16
    worst = (1.0, "")
    for name, n, d in cond_cases():
        r = m_cond((n, n, d), tol)
        assert r[0] == "ok", (name, r)
        est = r[1][0]
        lo, hi = exact_cond(n, d)
        ratio_ = est / ((lo + hi) / 2)
        assert lo / 10 <= est <= hi * 10, (name, lo, hi, est)
        if max(ratio_, 1 / ratio_) > max(worst[0], 1 / worst[0]):
            worst = (ratio_, name)
        h.update(struct.pack("<Q", bits(est)))
        body.append(f"case {name} n {n} tol {hx(tol)} lo {hx(lo)} hi {hx(hi)} est {hx(est)} # cond {lo:.4e} est/cond {ratio_:.4f}")
        for i in range(n):
            body.append(vec_str(d[i * n:(i + 1) * n]))
    out.append(f"# worst est/cond: {worst[0]:.4f} ({worst[1]})")
    out.append(f"digest sha256:{h.hexdigest()}")
    return "\n".join(out + body) + "\n"


# ---------------------------------------------------------------- main


def self_test():
    rng = random.Random(1)
    for _ in range(200):
        n = rng.randint(1, 9)
        a = [rng.uniform(-1, 1) * 2.0 ** rng.randint(-60, 60) for _ in range(n)]
        b = [rng.uniform(-1, 1) * 2.0 ** rng.randint(-60, 60) for _ in range(n)]
        s, sa, e = exact_dot(a, b)
        assert as_fraction(s, e) == sum((Fraction(x) * Fraction(y) for x, y in zip(a, b)), Fraction(0))
        assert as_fraction(sa, e) == sum((abs(Fraction(x) * Fraction(y)) for x, y in zip(a, b)), Fraction(0))
    assert dot([1e16, 1.0, -1e16], [1.0, 1.0, 1.0]) == 1.0
    assert norm2([3.0, 4.0]) == 5.0
    # Exact cond of diag(4, 1/4) is 16, of the identity 1.
    lo, hi = exact_cond(2, [4.0, 0.0, 0.0, 0.25])
    assert lo <= 16.0 <= hi and hi / lo < 1.001
    lo, hi = exact_cond(3, [1.0 if i == j else 0.0 for i in range(3) for j in range(3)])
    assert lo <= 1.0 <= hi
    # [[1, 1], [0, 1]]: sigma = (sqrt(5) +- 1) / 2, cond = (3 + sqrt(5)) / 2.
    lo, hi = exact_cond(2, [1.0, 1.0, 0.0, 1.0])
    assert lo <= (3 + math.sqrt(5)) / 2 <= hi
    g = Gen(SEED0)
    assert [g.next() for _ in range(2)] == [splitmix64(SEED0)[1], splitmix64(splitmix64(SEED0)[0])[1]]


def main():
    self_test()
    files = {"mat-ops-v1.txt": make_ops, "mat-edge-v1.txt": make_edge, "cond-v1.txt": make_cond}
    check = "--check" in sys.argv[1:]
    stale = False
    for name, make in files.items():
        data = make().encode("utf-8")
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: fixtures/num/la/{name}")
                stale = True
        else:
            path.write_bytes(data)
            print(f"wrote fixtures/num/la/{name} ({len(data)} bytes)")
    if stale:
        sys.exit(1)


if __name__ == "__main__":
    main()
