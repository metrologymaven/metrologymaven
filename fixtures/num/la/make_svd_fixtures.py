#!/usr/bin/env python3
"""Independent reference for KRN.NUM.LA.SVD (CC-BY-4.0 project data).

Writes
  fixtures/num/la/svd-v1.txt       300 seeded matrices (1x1 .. 200x200, tall and wide; general,
                                   graded singular values 1 .. 1e-15, Hadamard products with
                                   exactly repeated singular values, rank-deficient, scaled by
                                   2^+-400) with 50-digit oracle singular values (AC-1, AC-5)
  fixtures/num/la/svd-edge-v1.txt  hand-chosen edge and error cases (AC-2, AC-4 and more)

Two independent parts, neither of which runs the Rust code (stdlib only):

1. An oracle that knows nothing of the binary64 operation order:
   - singular values by one-sided Jacobi in 240-bit fixed point (exact integer dot products,
     rotation parameters in 70-digit `decimal`) on A Q, where Q is the model's V
     re-orthonormalised (modified Gram-Schmidt) in the same fixed point; the absolute
     error of every value is below 1e-40 sigma_max (the relative error of values well above
     that is about 2^-200), and 50 significant digits are written. Q only speeds convergence: A Q has the singular values of A for any
     orthogonal Q, so the oracle does not depend on the model being right. For the Hadamard
     kind the exact values sqrt(m n) |d_k| are known in closed form and must agree too;
   - the reconstruction ||A - U diag(sigma) V^T||_F is computed exactly in integers (every
     binary64 value is a dyadic rational) and checked against 10 n u ||A||_F exactly;
   - |sigma_k - oracle_k| <= 10 n u sigma_max is checked in 70-digit decimal (REQ-005).
2. A bit-exact Python model of the rules pinned in the Rust module documentation
   (`crates/fid-num/src/la/svd.rs`): scaling, cyclic sweep order, convergence test,
   rotation formulas, sort, sign rule and diagnostics. It reuses the reviewed KRN.NUM.RED and
   KRN.NUM.LA.MAT model of `make_la_fixtures.py` (block-and-tree Neumaier dot, scaled norm2,
   rank rule). CPython floats are binary64 with round-to-nearest-even and never fuse a
   multiply-add, and math.sqrt is correctly rounded, so the model's bits are the expected Rust
   results on every target.

Run:  python fixtures/num/la/make_svd_fixtures.py          (writes the fixtures)
      python fixtures/num/la/make_svd_fixtures.py --check  (fails if a fixture is stale)
"""

import decimal
import hashlib
import math
import random
import struct
import sys
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE))
import make_la_fixtures as la  # noqa: E402  (the reviewed RED / LA.MAT model)
import make_qr_fixtures as qrf  # noqa: E402  (exact integer helpers, input generator pieces)

MAX = sys.float_info.max
MIN_NORMAL = 2.0 ** -1022
INF = math.inf
U = 2.0 ** -53
MAX_SWEEPS = 60
Z_BIG = 2.0 ** 256
bits, hx, finite, Gen = la.bits, la.hx, la.finite, la.Gen
Fail = qrf.Fail

CTX = decimal.Context(prec=70)


# ---------------------------------------------------------------- 2. model: KRN.NUM.LA.SVD


def pow2(k):
    return math.ldexp(1.0, k)


def rotate(x, p, q, cs, sn):
    xp, xq = x[p], x[q]
    x[p] = [cs * a - sn * b for a, b in zip(xp, xq)]
    x[q] = [sn * a + cs * b for a, b in zip(xp, xq)]


def fdot(a, b):
    """la.dot, bit for bit, specialised to one block (len <= BLOCK, finite terms): one Neumaier
    pass from (-0.0, -0.0), result s if c == 0 else s + c. Longer inputs use la.dot."""
    if len(a) > la.BLOCK or not a:
        return la.dot(a, b)
    s = c = -0.0
    for x, y in zip(a, b):
        p = x * y
        t = s + p
        if abs(s) >= abs(p):
            c = c + ((s - t) + p)
        else:
            c = c + ((p - t) + s)
        s = t
    if not (finite(s) and finite(c)):
        raise la.Overflow()
    r = s if c == 0 else s + c
    if not finite(r):
        raise la.Overflow()
    return r


def jacobi(r, c, w, v, max_sweeps):
    tol = c * U
    # sq[j] caches dot(w_j, w_j) while column j is unchanged (the value is a function of the
    # column only, so the cache cannot change a result).
    sq = [None] * c
    for sweep in range(1, max_sweeps + 1):
        rotated = False
        for p in range(c):
            for q in range(p + 1, c):
                ap, aq = w[p], w[q]
                if sq[p] is None:
                    sq[p] = fdot(ap, ap)
                if sq[q] is None:
                    sq[q] = fdot(aq, aq)
                alpha, beta, gamma = sq[p], sq[q], fdot(ap, aq)
                if alpha == 0 or beta == 0:
                    continue
                thr = tol * (math.sqrt(alpha) * math.sqrt(beta))
                if abs(gamma) <= thr:
                    continue
                zeta = (beta - alpha) / (2.0 * gamma)
                if not finite(zeta):
                    continue
                if abs(zeta) >= Z_BIG:
                    t = 0.5 / zeta
                else:
                    sg = -1.0 if zeta < 0 else 1.0
                    t = sg / (abs(zeta) + math.sqrt(1.0 + zeta * zeta))
                cs = 1.0 / math.sqrt(1.0 + t * t)
                sn = cs * t
                rotate(w, p, q, cs, sn)
                rotate(v, p, q, cs, sn)
                sq[p] = sq[q] = None
                rotated = True
        if not rotated:
            return sweep
    raise Fail("NoConvergence", max_sweeps)


class Svd:
    def __init__(self, m, n, u, sigma, v, rank, cond, sweeps):
        self.m, self.n, self.u, self.sigma, self.v = m, n, u, sigma, v
        self.rank, self.cond, self.sweeps = rank, cond, sweeps
        self.k = len(sigma)

    def singular(self):
        return self.sigma[-1] == 0


def svd(m, n, a, tol, max_sweeps=MAX_SWEEPS):
    """a: row-major list. Returns Svd (u m x k and v n x k row-major) or raises Fail."""
    qrf.check_tol(tol)
    mx = max((abs(x) for x in a), default=0.0)
    if mx != 0 and mx < MIN_NORMAL:
        raise Fail("Domain", "subnormal_input")
    if mx == 0:
        e, d1, d2 = 0, 1.0, 1.0
    else:
        e = la.exponent(mx)
        d1, d2 = la.scale_factors(e)
    wide = m < n
    r, c = (n, m) if wide else (m, n)
    if wide:
        w = [[(a[j * n + i] * d1) * d2 for i in range(r)] for j in range(c)]
    else:
        w = [[(a[i * n + j] * d1) * d2 for i in range(r)] for j in range(c)]
    v = [[1.0 if i == j else 0.0 for i in range(c)] for j in range(c)]
    sweeps = jacobi(r, c, w, v, max_sweeps)
    scale = pow2(e)
    nrm, sig = [], []
    for j in range(c):
        x = la.norm2(w[j])
        s = x * scale
        if not finite(s):
            raise Fail("Infinite", j)
        nrm.append(x)
        sig.append(s)
    order = sorted(range(c), key=lambda j: (-sig[j], j))
    k = c
    ud = [0.0] * (m * k)
    vd = [0.0] * (n * k)
    sigma = []
    for pos, j in enumerate(order):
        sigma.append(sig[j])
        zero = nrm[j] == 0
        left = [0.0] * r if zero else [x / nrm[j] for x in w[j]]
        right = v[j]
        ucol, vcol = (right, left) if wide else (left, right)
        uzero, vzero = (False, zero) if wide else (zero, False)
        best = 0
        for i, x in enumerate(vcol):
            if abs(x) > abs(vcol[best]):
                best = i
        flip = vcol[best] < 0
        for i, x in enumerate(ucol):
            ud[i * k + pos] = -x if flip and not uzero else x
        for i, x in enumerate(vcol):
            vd[i * k + pos] = -x if flip and not vzero else x
    rank = la.m_rank(sigma, tol)[1]
    smin = sigma[-1]
    if smin == 0:
        cond = INF
    else:
        q = sigma[0] / smin
        cond = q if finite(q) else INF
    return Svd(m, n, ud, sigma, vd, rank, cond, sweeps)


# ---------------------------------------------------------------- 1. oracle


def dec(x):
    return Decimal(x)  # exact conversion of a binary64 value


FB = 400  # oracle fixed point: integers scaled by 2^FB


def fix(x, e):
    """round(x * 2^(FB - e)) for a binary64 x, exactly (x is a dyadic rational)."""
    mt, ex = la.ratio(x)
    sh = ex + FB - e
    if sh >= 0:
        return mt << sh
    return (mt + (1 << (-sh - 1))) >> -sh  # round half up


def dround(x):
    """A Decimal (already scaled) to the nearest integer."""
    return int(x.to_integral_value(rounding=decimal.ROUND_HALF_EVEN))


def oracle_sigma(m, n, a, model):
    """Singular values of A, descending, as Decimals (see the module docstring).

    Fixed point: with e the binary exponent of max |a_ij|, entries are integers scaled by
    2^(FB - e); dots of such integers are exact. Rotation parameters are computed in 70-digit
    decimal and rounded to FB bits. A pair is converged when its Gram entry is below 2^-200
    relative to the column norms, or below 2^-300 ||A||_F^2 absolutely; the absolute error of
    every value is below 1e-40 sigma_max (values below that are written as 0)."""
    wide = m < n
    r, c = (n, m) if wide else (m, n)
    mx = max(abs(x) for x in a)
    if mx == 0:
        return [Decimal(0)] * c
    e = la.exponent(mx)
    one = 1 << FB
    if wide:
        cols = [[fix(a[j * n + i], e) for i in range(r)] for j in range(c)]
        qsrc = [[fix(model.u[i * model.k + j], 0) for i in range(c)] for j in range(c)]
    else:
        cols = [[fix(a[i * n + j], e) for i in range(r)] for j in range(c)]
        qsrc = [[fix(model.v[i * model.k + j], 0) for i in range(c)] for j in range(c)]
    q = mgs(qsrc)
    # W = B Q: column j = sum_l Q[l][j] B col l, rescaled by 2^-FB.
    w = []
    for j in range(c):
        acc = [0] * r
        for l in range(c):
            f = q[j][l]
            if f:
                acc = [x + f * y for x, y in zip(acc, cols[l])]
        w.append([x >> FB for x in acc])
    sq = [sum(x * x for x in col) for col in w]  # exact squared column norms, kept current
    fro2 = sum(sq)  # invariant under the rotations (up to the fixed-point rounding)
    with decimal.localcontext(CTX):
        for _ in range(40):
            rotated = False
            for p in range(c):
                for qq in range(p + 1, c):
                    ap, aq = w[p], w[qq]
                    alpha, beta = sq[p], sq[qq]
                    gamma = sum(x * y for x, y in zip(ap, aq))
                    # Converged: |gamma| <= 2^-200 sqrt(alpha beta) (relative), or
                    # |gamma| <= 2^-300 ||W||_F^2 (absolute; fixed-point noise of a zero column).
                    if (alpha == 0 or beta == 0 or (gamma * gamma) << 400 <= alpha * beta
                            or abs(gamma) << 300 <= fro2):
                        continue
                    zeta = (Decimal(beta) - Decimal(alpha)) / (2 * Decimal(gamma))
                    sg = -1 if zeta < 0 else 1
                    t = sg / (abs(zeta) + (1 + zeta * zeta).sqrt())
                    cd = 1 / (1 + t * t).sqrt()
                    cs, sn = dround(cd * one), dround(cd * t * one)
                    w[p] = [(cs * x - sn * y) >> FB for x, y in zip(ap, aq)]
                    w[qq] = [(sn * x + cs * y) >> FB for x, y in zip(ap, aq)]
                    sq[p] = sum(x * x for x in w[p])
                    sq[qq] = sum(x * x for x in w[qq])
                    rotated = True
            if not rotated:
                break
        else:
            raise AssertionError("oracle did not converge")
        unit = Decimal(2) ** (e - FB)
        return sorted((Decimal(x).sqrt() * unit for x in sq), reverse=True)


def mgs(cols):
    """Modified Gram-Schmidt on fixed-point column vectors (scaled by 2^FB)."""
    out = []
    for col in cols:
        x = list(col)
        for qv in out:
            d = sum(a * b for a, b in zip(qv, x)) >> FB
            x = [a - ((d * b) >> FB) for a, b in zip(x, qv)]
        nn = math.isqrt(sum(a * a for a in x))
        assert nn > (1 << (FB - 1)), "model V is far from orthogonal"
        out.append([(a << FB) // nn for a in x])
    return out


def check_sigma(m, n, model, ref, tag):
    """REQ-005 singular-value bound in 70-digit decimal; returns worst error / bound."""
    with decimal.localcontext(CTX):
        bound = 10 * n * dec(U) * ref[0]
        worst = Decimal(0)
        for s, rv in zip(model.sigma, ref):
            err = abs(dec(s) - rv)
            assert err <= bound, (tag, "sigma", s, rv)
            if bound:
                worst = max(worst, err / bound)
        return float(worst)


def check_recon(m, n, a, model, tag):
    """Exact ||A - U S V^T||_F <= 10 n u ||A||_F; returns error / bound."""
    k = model.k
    ui, eu = qrf.to_ints(model.u)
    si, es = qrf.to_ints(model.sigma)
    vi, ev = qrf.to_ints(model.v)
    us = [ui[i * k + j] * si[j] for i in range(m) for j in range(k)]  # 2^(eu + es)
    vt = [vi[i * k + j] for j in range(k) for i in range(n)]  # k x n, 2^ev
    usv = qrf.int_matmul(us, vt, m, k, n)  # 2^(eu + es + ev)
    ai, ea = qrf.to_ints(a)
    e0 = min(eu + es + ev, ea)
    sp, sa = eu + es + ev - e0, ea - e0
    err2 = sum(((x << sa) - (y << sp)) ** 2 for x, y in zip(ai, usv))
    a2 = sum((x << sa) ** 2 for x in ai)
    assert err2 << 106 <= 100 * n * n * a2, (tag, "reconstruction")
    return math.sqrt(err2 / a2) / (10 * n * U) if a2 else 0.0


# ---------------------------------------------------------------- shared input generator


GRADE = 50  # 2^-50 ~ 8.9e-16: singular values from about 1 down to about 1e-15
KINDS = ("general", "graded", "hadamard", "rankdef", "general", "scaled")


def had(i, k):
    """Sylvester-Hadamard entry (-1)^popcount(i & k)."""
    return -1 if bin(i & k).count("1") & 1 else 1


def gen_matrix(seed, kind, m, n):
    """The matrix the Rust test rebuilds (see the fixture header); also the Hadamard d."""
    g = Gen(seed)
    d = None
    if kind == "general":
        a = [g.general() for _ in range(m * n)]
    elif kind == "graded":
        a = [g.general() for _ in range(m * n)]
        a = [a[i * n + j] * math.ldexp(1.0, -qrf.grade_exp(j, n, GRADE)) for i in range(m) for j in range(n)]
    elif kind == "hadamard":
        kk = min(m, n)
        d = [qrf.g_int(g, -3, 3) for _ in range(kk)]
        a = [float(sum(had(i, k) * d[k] * had(j, k) for k in range(kk))) for i in range(m) for j in range(n)]
    elif kind == "rankdef":
        r = (n + 1) // 2
        cols = [[float(qrf.g_int(g, -8, 8)) for _ in range(m)] for _ in range(r)]
        for j in range(r, n):
            c1, c2 = float(qrf.g_int(g, -3, 3)), float(qrf.g_int(g, -3, 3))
            p, q = cols[j % r], cols[(j + 1) % r]
            cols.append([p[i] * c1 + q[i] * c2 for i in range(m)])
        a = [cols[j][i] for i in range(m) for j in range(n)]
    elif kind == "scaled":
        e = 400 if g.next() & 1 else -400
        a = [g.general() * math.ldexp(1.0, e) for _ in range(m * n)]
    else:
        raise ValueError(kind)
    return a, d


# ---------------------------------------------------------------- hashing


def put(h, vals):
    for v in vals:
        h.update(struct.pack("<Q", bits(v)))


def put_ints(h, vals):
    for v in vals:
        h.update(struct.pack("<Q", v))


def svd_words(h, s):
    put(h, s.u)
    put(h, s.sigma)
    put(h, s.v)
    put_ints(h, [s.rank])
    put(h, [s.cond])
    put_ints(h, [s.sweeps, 1 if s.singular() else 0])


# ---------------------------------------------------------------- svd-v1


SEED0 = 0x4C41_5356_4400_0000
POW2 = (1, 2, 4, 8, 16, 32)


def svd_sizes():
    rng = random.Random(20261121)
    fixed = [(200, 200, "graded"), (200, 60, "general"), (64, 64, "hadamard"), (40, 200, "general"),
             (80, 80, "rankdef"), (200, 3, "general"), (3, 200, "graded"), (60, 60, "scaled"),
             (64, 16, "hadamard"), (16, 64, "hadamard"), (1, 1, "general"), (1, 7, "general"), (7, 1, "rankdef"),
             (50, 50, "graded")]
    out = list(fixed)
    i = 0
    while len(out) < 300:
        kind = KINDS[i % len(KINDS)]
        i += 1
        if kind == "hadamard":
            m, n = rng.choice(POW2), rng.choice(POW2)
        else:
            r = rng.random()
            if r < 0.15:
                m, n = rng.randint(1, 4), rng.randint(1, 4)
            elif r < 0.9:
                m, n = rng.randint(1, 24), rng.randint(1, 24)
            else:
                m, n = rng.randint(25, 48), rng.randint(1, 48)
            if rng.random() < 0.7 and m < n:
                m, n = n, m
        out.append((m, n, kind))
    return out


def tol_for(idx):
    return (1e-12, 0.0, 1e-8, 1e-14)[idx % 4]


def sci(x, smax):
    """50 significant digits of a decimal oracle value; 0 below the oracle's 1e-40 smax floor."""
    with decimal.localcontext(CTX):
        if x == 0 or x < Decimal(10) ** -40 * smax:
            return "0"
        return f"{+x:.49e}"


def make_svd():
    out = [
        "# KRN.NUM.LA.SVD AC-1, AC-5: 300 seeded matrices (1x1 .. 200x200, tall and wide), CC-BY-4.0.",
        "# Written by make_svd_fixtures.py; do not edit. Inputs are rebuilt from `seed` by the splitmix64",
        "# stream of mat-ops-v1.txt (general(u), and int(lo, hi) = lo + u mod (hi - lo + 1)), by kind:",
        "#  general: A (m x n row-major) = general draws.",
        "#  graded: general draws, entry (i, j) times 2^-g with g = round(50 j / (n - 1)) rounded half up",
        "#    in integers (0 for a single column): singular values from about 1 down to about 1e-15.",
        "#  hadamard: k = min(m, n) (m, n powers of two), d = k int(-3, 3) draws, and",
        "#    A[i][j] = sum_l h(i, l) d_l h(j, l) over l < k, h(i, l) = (-1)^popcount(i & l), in integers;",
        "#    the exact singular values are sqrt(m n) |d_l| (repeated and zero values).",
        "#  rankdef: r = ceil(n / 2) columns of m int(-8, 8) draws (column by column); then for",
        "#    j = r .. n-1: c1 = int(-3, 3), c2 = int(-3, 3), column j = col[j mod r] * c1 + col[(j+1) mod r] * c2.",
        "#  scaled: one draw u, e = 400 if u is odd else -400; A = general draws times 2^e.",
        "# tol = rel_tol. Per case `svd` is the first 16 hex digits of SHA-256 over little-endian 8-byte",
        "# words: U (row-major), sigma, V (row-major), rank, cond bits, sweeps, singular (0/1).",
        "# `digest` = SHA-256 of all those words in case order. The next line `ref` holds the oracle",
        "# singular values (50 significant digits, descending; fixed-point one-sided Jacobi, absolute error",
        "# below 1e-40 sigma_max, values below that written as 0).",
        "# The script checks every case: |sigma_k - ref_k| <= 10 n u sigma_max (decimal) and",
        "# ||A - U diag(sigma) V^T||_F <= 10 n u ||A||_F (exact integers), u = 2^-53, n = columns of A.",
    ]
    body = []
    h = hashlib.sha256()
    worst = [0.0, 0.0]
    for idx, (m, n, kind) in enumerate(svd_sizes()):
        seed = SEED0 + idx
        tol = tol_for(idx)
        a, d = gen_matrix(seed, kind, m, n)
        s = svd(m, n, a, tol)
        ref = oracle_sigma(m, n, a, s)
        if d is not None:
            with decimal.localcontext(CTX):
                exact = sorted((Decimal(m * n).sqrt() * abs(x) for x in d), reverse=True)
                for x, y in zip(exact, ref):
                    assert abs(x - y) <= Decimal(10) ** -45 * (exact[0] + 1), (idx, "hadamard oracle")
                ref = exact
        ws = check_sigma(m, n, s, ref, idx)
        wr = check_recon(m, n, a, s, idx)
        worst = [max(worst[0], ws), max(worst[1], wr)]
        svd_words(h, s)
        hc = hashlib.sha256()
        svd_words(hc, s)
        body.append(f"case {idx} seed {seed:016x} kind {kind} m {m} n {n} tol {hx(tol)} sweeps {s.sweeps} "
                    f"rank {s.rank} svd {hc.hexdigest()[:16]}")
        body.append("ref " + " ".join(sci(x, ref[0]) for x in ref))
    out.append(f"# worst error / bound: sigma {worst[0]:.4f}, reconstruction {worst[1]:.4f}")
    out.append(f"digest sha256:{h.hexdigest()}")
    return "\n".join(out + body) + "\n"


# ---------------------------------------------------------------- svd-edge-v1


def edge_cases():
    c = []

    def add(name, a, tol=1e-12, limit=MAX_SWEEPS):
        c.append((name, a, tol, limit))

    nan = math.nan
    add("ac2-diag", (3, 3, [1.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 2.0]))
    add("ac2-diag-neg", (3, 3, [-1.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, -2.0]))
    ac3 = [1.0, 2.0, 0.5, -3.0, 4.0, 1.0, 0.25, -1.0, 2.0, 7.0, -0.5, 1.5]
    add("ac3-a-tall", (4, 3, ac3))
    add("ac3-neg-tall", (4, 3, [-x for x in ac3]))
    add("ac3-a-wide", (3, 4, ac3))
    add("ac3-neg-wide", (3, 4, [-x for x in ac3]))
    g = Gen(0x4C41_5356_4445_4447)
    big = [g.general() for _ in range(50 * 50)]
    add("ac4-limit-1", (50, 50, big), 1e-12, 1)
    add("ac4-limit-0", (2, 2, [1.0, 2.0, 3.0, 4.0]), 1e-12, 0)
    add("limit-1-diagonal", (2, 2, [2.0, 0.0, 0.0, 1.0]), 1e-12, 1)
    add("one-pos", (1, 1, [2.0]))
    add("one-neg", (1, 1, [-3.0]))
    add("one-zero", (1, 1, [0.0]))
    add("one-neg-zero", (1, 1, [-0.0]))
    add("zero-matrix", (3, 2, [0.0, -0.0, 0.0, 0.0, -0.0, 0.0]))
    add("zero-matrix-wide", (2, 3, [0.0] * 6))
    add("row", (1, 3, [3.0, -4.0, 0.0]))
    add("col", (3, 1, [3.0, -4.0, 0.0]))
    add("equal-columns", (2, 2, [1.0, 1.0, 1.0, 1.0]))
    add("equal-norms-45deg", (2, 2, [1.0, 1.0, -1.0, 1.0]))
    add("rotation-zeta-zero", (2, 2, [1.0, 0.5, 0.5, 1.0]))
    add("rankdef-wide", (2, 3, [1.0, 2.0, 3.0, 2.0, 4.0, 6.0]))
    add("rankdef-tall", (3, 2, [1.0, 2.0, 2.0, 4.0, 3.0, 6.0]))
    add("zero-column", (3, 3, [1.0, 0.0, 2.0, 3.0, 0.0, 4.0, 5.0, -0.0, 6.0]))
    add("zero-row-wide", (2, 3, [0.0, 0.0, 0.0, 1.0, 2.0, 3.0]))
    add("tied-sigma", (2, 2, [3.0, 0.0, 0.0, 3.0]))
    add("tied-sigma-perm", (3, 3, [0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0]))
    add("sign-tie", (2, 2, [1.0, 1.0, -1.0, 1.0]))
    add("hilbert-6", (6, 6, [1.0 / (i + j + 1) for i in range(6) for j in range(6)]), 1e-16)
    add("near-max", (2, 2, [MAX, MAX, MAX, MAX]))
    add("near-max-diag", (2, 2, [MAX, 0.0, 0.0, MAX / 4]))
    add("exponent-1023", (2, 2, [MAX, 1e-300, 3.0, 1.0]))
    add("smallest-normal-scale", (2, 1, [MIN_NORMAL, MIN_NORMAL]))
    add("subnormal-entry-normal-scale", (2, 2, [1.0, 5e-324, 2.0 ** -1060, 1.0]))
    add("subnormal-matrix", (2, 1, [1e-310, -2e-310]))
    add("cond-beyond-range", (2, 2, [1e300, 0.0, 0.0, 1e-300]), 0.0)
    add("graded-diag", (3, 3, [1.0, 0.0, 0.0, 0.0, 1e-8, 0.0, 0.0, 0.0, 1e-16]), 1e-12)
    add("tol-one", (2, 2, [3.0, 0.0, 0.0, 2.0]), 1.0)
    add("tol-equal-threshold", (2, 2, [4.0, 0.0, 0.0, 1.0]), 0.25)
    add("tol-just-below", (2, 2, [4.0, 0.0, 0.0, 1.0]), 0.24999999999999997)
    add("tol-nan", (1, 1, [1.0]), nan)
    add("tol-inf", (1, 1, [1.0]), INF)
    add("tol-neg", (1, 1, [1.0]), -1e-12)
    add("tol-neg-zero", (2, 1, [3.0, 4.0]), -0.0)
    add("tol-before-subnormal", (1, 1, [1e-310]), nan)
    add("wide-1x2", (1, 2, [0.0, -2.0]))
    add("tall-neg-first", (3, 2, [-5.0, 1.0, 0.0, 2.0, 0.0, -3.0]))
    return c


def expect_str(s):
    return (f"ok u {la.vec_str(s.u)} sigma {la.vec_str(s.sigma)} v {la.vec_str(s.v)} rank {s.rank} "
            f"cond {hx(s.cond)} sweeps {s.sweeps} singular {1 if s.singular() else 0}")


def make_edge():
    out = [
        "# KRN.NUM.LA.SVD edge and error cases, CC-BY-4.0.",
        "# Written by make_svd_fixtures.py; do not edit. Per case: `case <name>`, `a <m> <n> <hex...>`",
        "# (row-major), `tol <hex>`, `limit <sweeps>` (the sweep limit; 60 is plain `svd`), then `expect`:",
        "# `ok u <hex...> sigma <hex...> v <hex...> rank <r> cond <hex> sweeps <s> singular <0|1>` or",
        "# `err <Kind> <index|none|detail|sweeps>`.",
    ]
    for name, a, tol, limit in edge_cases():
        try:
            s = expect_str(svd(a[0], a[1], a[2], tol, limit))
        except Fail as f:
            s = f"err {f.kind} {'none' if f.info is None else f.info}"
        out.append(f"case {name}")
        out.append(f"a {a[0]} {a[1]} {la.vec_str(a[2])}")
        out.append(f"tol {hx(tol)}")
        out.append(f"limit {limit}")
        out.append("expect " + s)
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------- main


def self_test():
    # The specialised dot is la.dot bit for bit (signed zeros included).
    g = Gen(7)
    for n in (1, 2, 3, 17, 255, 256):
        for _ in range(20):
            a = [g.general() * (2.0 ** -60 if g.next() & 3 == 0 else 1.0) for _ in range(n)]
            b = [g.general() for _ in range(n)]
            assert bits(fdot(a, b)) == bits(la.dot(a, b)), n
    for a, b in (([-0.0], [1.0]), ([0.0, -0.0], [1.0, 1.0]), ([-0.0, -0.0], [1.0, 1.0])):
        assert bits(fdot(a, b)) == bits(la.dot(a, b)), (a, b)
    # AC-2 (hand-labelled): diag(1, 3, 2): no pair rotates, sigma = [3, 2, 1], V = U = the
    # permutation with columns e1, e2, e0; one sweep.
    s = svd(3, 3, [1.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 2.0], 1e-12)
    assert s.sigma == [3.0, 2.0, 1.0] and s.sweeps == 1
    perm = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    assert s.v == perm and s.u == perm, (s.u, s.v)
    # 1 x 1 [-3]: sigma 3, V = [1] (positive), U = [-1].
    s = svd(1, 1, [-3.0], 0.0)
    assert (s.u, s.sigma, s.v) == ([-1.0], [3.0], [1.0])
    # Zero matrix: sigma 0, U zero, V identity, rank 0, cond infinite, singular.
    s = svd(2, 2, [0.0] * 4, 1e-12)
    assert s.sigma == [0.0, 0.0] and s.u == [0.0] * 4 and s.v == [1.0, 0.0, 0.0, 1.0]
    assert s.rank == 0 and s.cond == INF and s.singular()
    # Decimal oracle against a closed form: [[3, 0], [4, 5]] has A^T A = [[25, 20], [20, 25]]: sigma = sqrt(45), sqrt(5).
    m = svd(2, 2, [3.0, 0.0, 4.0, 5.0], 0.0)
    ref = oracle_sigma(2, 2, [3.0, 0.0, 4.0, 5.0], m)
    with decimal.localcontext(CTX):
        want = [Decimal(45).sqrt(), Decimal(5).sqrt()]
        assert all(abs(x - y) < Decimal(10) ** -60 for x, y in zip(ref, want)), ref


def main():
    self_test()
    files = {"svd-edge-v1.txt": make_edge, "svd-v1.txt": make_svd}
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
