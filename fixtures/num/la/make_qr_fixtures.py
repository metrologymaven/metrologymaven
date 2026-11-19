#!/usr/bin/env python3
"""Independent reference for KRN.NUM.LA.QR (CC-BY-4.0 project data).

Writes
  fixtures/num/la/qr-v1.txt       300 seeded matrices (1x1 .. 200x200; general, column- and
                                  row-graded, rank-deficient, tied column norms, scaled by
                                  2^+-400): qr, qr_pivoted, thin Q, Q^T b and lstsq (AC-1, AC-5)
  fixtures/num/la/lstsq-v1.txt    synthetic least-squares problems with exact rational reference
                                  solutions (AC-3)
  fixtures/num/la/qr-edge-v1.txt  hand-chosen edge and error cases (AC-2, AC-4 and more)

Two independent parts, neither of which runs the Rust code (stdlib only):

1. An exact oracle that knows nothing of the factorization's operation order:
   - reconstruction ||A P - Q R||_F and orthogonality ||Q^T Q - I||_F of the model's thin Q and
     R are computed exactly in integers (every binary64 value is a dyadic rational, so after a
     common power-of-two scaling every matrix product is an integer product), and the REQ-006
     bounds 10 n u ||A||_F and 10 n u (u = 2^-53) are checked exactly;
   - least-squares reference solutions are the exact rational solutions of the normal
     equations A^T A x = A^T b (`fractions.Fraction`, Gaussian elimination), so they carry no
     rounding at all; the 7-significant-digit check of AC-3 is made against them.
2. A bit-exact Python model of the rules pinned in the leaf's Design section and in the Rust
   module documentation (reflector, pivot, update and diagnostics rules). It reuses the
   KRN.NUM.RED and KRN.NUM.LA.MAT model of `make_la_fixtures.py` (block-and-tree Neumaier dot,
   scaled norm2, rank rule, condition estimator). CPython floats are binary64 with
   round-to-nearest-even and never fuse a multiply-add, so the model's bits are the expected
   Rust results on every target.

Run:  python fixtures/num/la/make_qr_fixtures.py          (writes the fixtures)
      python fixtures/num/la/make_qr_fixtures.py --check  (fails if a fixture is stale)
"""

import hashlib
import math
import random
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE))
import make_la_fixtures as la  # noqa: E402  (the reviewed RED / LA.MAT model)

MAX = sys.float_info.max
MIN_NORMAL = 2.0 ** -1022
INF = math.inf
bits, hx, finite, Gen = la.bits, la.hx, la.finite, la.Gen


class Fail(Exception):
    """A typed error of the model: kind, info (index, detail or rank)."""

    def __init__(self, kind, info):
        super().__init__(kind, info)
        self.kind, self.info = kind, info


def ovf(k):
    return Fail("Infinite", k)


# ---------------------------------------------------------------- 2. model: KRN.NUM.LA.QR


def check_tol(t):
    e = la.check_tol(t)
    if e:
        raise Fail(e[1], e[2])


def scan(xs):
    e = la.scan(xs)
    if e:
        raise Fail(e[1], e[2])


def check_scale(xs):
    """A nonzero input whose largest magnitude is subnormal is refused."""
    m = max((abs(x) for x in xs), default=0.0)
    if m != 0 and m < MIN_NORMAL:
        raise Fail("Domain", "subnormal_input")


def norm2_at(xs, k):
    try:
        return la.norm2(xs)
    except la.Overflow:
        raise ovf(k)


def reflect(c, lo, v, t, k):
    """c[lo:] -= (t * dot(v, c[lo:])) * v, in place; every result must be finite."""
    try:
        s = t * la.dot(v, c[lo:])
    except la.Overflow:
        raise ovf(k)
    if not finite(s):
        raise ovf(k)
    for i, vi in enumerate(v):
        y = c[lo + i] - s * vi
        if not finite(y):
            raise ovf(k)
        c[lo + i] = y


def factor(m, n, a, pivot):
    """(columns, tau, perm): column j of the packed factor is columns[j]."""
    w = [[a[i * n + j] for i in range(m)] for j in range(n)]
    tau, perm = [], list(range(n))
    for k in range(n):
        if pivot:
            best, xn = k, norm2_at(w[k][k:], k)
            for j in range(k + 1, n):
                c = norm2_at(w[j][k:], k)
                if c > xn:
                    best, xn = j, c
            if best != k:
                w[k], w[best] = w[best], w[k]
                perm[k], perm[best] = perm[best], perm[k]
        else:
            xn = norm2_at(w[k][k:], k)
        if xn == 0:
            tau.append(0.0)
            continue
        alpha = w[k][k]
        beta = -xn if alpha > 0 else xn
        u0 = alpha - beta
        if not finite(u0):
            raise ovf(k)
        v = [1.0] + [w[k][i] / u0 for i in range(k + 1, m)]
        t = (beta - alpha) / beta
        tau.append(t)
        w[k][k] = beta
        w[k][k + 1:] = v[1:]
        for j in range(k + 1, n):
            reflect(w[j], k, v, t, k)
    return w, tau, perm


def upper(w, n):
    return [w[j][i] if j >= i else 0.0 for i in range(n) for j in range(n)]


def diagnostics(w, n, tol):
    r = upper(w, n)
    d = sorted((abs(r[k * n + k]) for k in range(n)), reverse=True)
    rank = la.m_rank(d, tol)[1]
    if rank < n:
        return rank, INF
    c = la.m_cond((n, n, r), tol)
    return rank, (c[1][0] if c[0] == "ok" else INF)


class Qr:
    def __init__(self, m, n, w, tau, perm, rank, cond):
        self.m, self.n, self.w, self.tau, self.perm, self.rank, self.cond = m, n, w, tau, perm, rank, cond

    def packed(self):
        return [self.w[j][i] for i in range(self.m) for j in range(self.n)]

    def r(self):
        return upper(self.w, self.n)

    def reflect_all(self, c, forward):
        order = range(self.n) if forward else range(self.n - 1, -1, -1)
        for k in order:
            if self.tau[k] == 0:
                continue
            v = [1.0] + self.w[k][k + 1:]
            reflect(c, k, v, self.tau[k], k)
        return c

    def apply(self, x, forward):
        if len(x) != self.m:
            raise Fail("Domain", "bad_shape")
        scan(x)
        check_scale(x)
        return self.reflect_all(list(x), forward)

    def thin_q(self):
        cols = []
        for j in range(self.n):
            e = [0.0] * self.m
            e[j] = 1.0
            cols.append(self.reflect_all(e, False))
        return [cols[j][i] for i in range(self.m) for j in range(self.n)]


def qr(m, n, a, tol, pivot):
    if m < n:
        raise Fail("Domain", "bad_shape")
    check_tol(tol)
    check_scale(a)
    w, tau, perm = factor(m, n, a, pivot)
    rank, cond = diagnostics(w, n, tol)
    return Qr(m, n, w, tau, perm, rank, cond)


def lstsq(m, n, a, b, tol):
    if m < n or len(b) != m:
        raise Fail("Domain", "bad_shape")
    scan(b)
    check_tol(tol)
    check_scale(a)
    check_scale(b)
    w, tau, perm = factor(m, n, a, True)
    rank, cond = diagnostics(w, n, tol)
    if rank < n:
        raise Fail("RankDeficient", rank)
    q = Qr(m, n, w, tau, perm, rank, cond)
    c = q.reflect_all(list(b), True)
    y, bad = la.back_sub(n, q.r(), c[:n])
    if y is None:
        raise Fail("Infinite", perm[bad])
    x = [0.0] * n
    for i in range(n):
        x[perm[i]] = y[i]
    try:
        res = la.norm2(c[n:])
    except la.Overflow:
        raise Fail("Infinite", None)
    return x, res, rank, cond


# ---------------------------------------------------------------- 1. exact oracle


def to_ints(vals):
    """(ints, e): vals[i] == ints[i] * 2^e exactly, one common exponent."""
    rs = [la.ratio(v) for v in vals]
    nz = [e for (mt, e) in rs if mt]
    e0 = min(nz) if nz else 0
    return [(mt << (e - e0)) if mt else 0 for (mt, e) in rs], e0


def int_matmul(x, y, r, k, c):
    """x (r x k) times y (k x c), integer row-major lists."""
    cols = [y[j::c] for j in range(c)]
    return [sum(map(int.__mul__, x[i * k:(i + 1) * k], cols[j])) for i in range(r) for j in range(c)]


def check_bounds(m, n, a, q, perm, r, tag):
    """Exact REQ-006 checks; returns (recon / bound, orth / bound) as floats."""
    ap = [a[i * n + perm[j]] for i in range(m) for j in range(n)]
    qi, eq = to_ints(q)
    ri, er = to_ints(r)
    ai, ea = to_ints(ap)
    qr_ = int_matmul(qi, ri, m, n, n)  # scale 2^(eq + er)
    e = min(eq + er, ea)
    sq, sa = eq + er - e, ea - e
    err2 = sum(((x << sa) - (y << sq)) ** 2 for x, y in zip(ai, qr_))  # scale 2^(2e)
    a2 = sum((x << sa) ** 2 for x in ai)
    # ||E||^2 <= (10 n u)^2 ||A||^2  <=>  err2 * 2^106 <= 100 n^2 a2
    assert err2 << 106 <= 100 * n * n * a2, (tag, "reconstruction")
    recon = math.sqrt(err2 / a2) / (10 * n * 2.0 ** -53) if a2 else 0.0
    qt = [qi[i * n + j] for j in range(n) for i in range(m)]
    g = int_matmul(qt, qi, n, m, n)  # scale 2^(2 eq)
    assert eq < 0, (tag, "unexpected Q exponent")
    one = 1 << (-2 * eq)
    orth2 = sum((g[i * n + j] - (one if i == j else 0)) ** 2 for i in range(n) for j in range(n))
    # ||Q^T Q - I||^2 <= (10 n u)^2  <=>  orth2 * 2^106 <= 100 n^2 one^2
    assert orth2 << 106 <= 100 * n * n * one * one, (tag, "orthogonality")
    orth = math.sqrt(orth2 / (one * one)) / (10 * n * 2.0 ** -53)
    return recon, orth


def exact_lstsq(m, n, a, b):
    """Exact least-squares solution by the normal equations in rationals (A full rank)."""
    af = [Fraction(v) for v in a]
    bf = [Fraction(v) for v in b]
    g = [[sum((af[i * n + p] * af[i * n + q] for i in range(m)), Fraction(0)) for q in range(n)] for p in range(n)]
    h = [sum((af[i * n + p] * bf[i] for i in range(m)), Fraction(0)) for p in range(n)]
    for k in range(n):
        piv = next(i for i in range(k, n) if g[i][k] != 0)
        g[k], g[piv], h[k], h[piv] = g[piv], g[k], h[piv], h[k]
        for i in range(k + 1, n):
            f = g[i][k] / g[k][k]
            if f:
                for j in range(k, n):
                    g[i][j] -= f * g[k][j]
                h[i] -= f * h[k]
    x = [Fraction(0)] * n
    for i in range(n - 1, -1, -1):
        x[i] = (h[i] - sum((g[i][j] * x[j] for j in range(i + 1, n)), Fraction(0))) / g[i][i]
    return x


def exact_residual(m, n, a, b, x):
    r2 = sum(((Fraction(b[i]) - sum((Fraction(a[i * n + j]) * x[j] for j in range(n)), Fraction(0))) ** 2
              for i in range(m)), Fraction(0))
    return math.sqrt(float(r2))


def digits(got, ref):
    """Log relative error (LRE) of got against the exact ref, capped at 17."""
    if ref == 0:
        return 17.0 if got == 0 else 0.0
    rel = abs(Fraction(got) - ref) / abs(ref)
    return 17.0 if rel == 0 else min(17.0, -math.log10(float(rel)))


# ---------------------------------------------------------------- shared input generator


def g_int(g, lo, hi):
    """An integer in [lo, hi] from one draw: lo + (u % (hi - lo + 1))."""
    return lo + g.next() % (hi - lo + 1)


def grade_exp(i, cnt, steps):
    """round(steps * i / (cnt - 1)), half up, in integers (0 if cnt == 1)."""
    return (2 * steps * i + (cnt - 1)) // (2 * (cnt - 1)) if cnt > 1 else 0


GRADE = 40
KINDS = ("general", "colgraded", "rowgraded", "rankdef", "tied", "scaled")


def gen_matrix(seed, kind, m, n):
    """The matrix and right-hand side the Rust test rebuilds (see the fixture header)."""
    g = Gen(seed)
    if kind == "general":
        a = [g.general() for _ in range(m * n)]
    elif kind == "colgraded":
        a = [g.general() for _ in range(m * n)]
        a = [a[i * n + j] * math.ldexp(1.0, -grade_exp(j, n, GRADE)) for i in range(m) for j in range(n)]
    elif kind == "rowgraded":
        a = [g.general() for _ in range(m * n)]
        a = [a[i * n + j] * math.ldexp(1.0, -grade_exp(i, m, GRADE)) for i in range(m) for j in range(n)]
    elif kind == "rankdef":
        r = (n + 1) // 2
        cols = [[float(g_int(g, -8, 8)) for _ in range(m)] for _ in range(r)]
        for j in range(r, n):
            c1, c2 = float(g_int(g, -3, 3)), float(g_int(g, -3, 3))
            p, q = cols[j % r], cols[(j + 1) % r]
            cols.append([p[i] * c1 + q[i] * c2 for i in range(m)])
        a = [cols[j][i] for i in range(m) for j in range(n)]
    elif kind == "tied":
        base = [float(g_int(g, -8, 8)) for _ in range(m)]
        base[0] = 9.0  # never an all-zero column
        cols = []
        for j in range(n):
            u = g.next()
            col = [base[(i + j) % m] for i in range(m)]
            cols.append([-v if (u >> (i % 64)) & 1 else v for i, v in enumerate(col)])
        a = [cols[j][i] for i in range(m) for j in range(n)]
    elif kind == "scaled":
        e = 400 if g.next() & 1 else -400
        a = [g.general() * math.ldexp(1.0, e) for _ in range(m * n)]
    else:
        raise ValueError(kind)
    b = [g.general() for _ in range(m)]
    return a, b


# ---------------------------------------------------------------- hashing


def put(h, vals):
    for v in vals:
        h.update(struct.pack("<Q", bits(v)))


def put_ints(h, vals):
    for v in vals:
        h.update(struct.pack("<Q", v))


def qr_bytes(q, thin):
    h = hashlib.sha256()
    put(h, q.packed())
    put(h, q.tau)
    put_ints(h, q.perm)
    put_ints(h, [q.rank])
    put(h, [q.cond])
    put(h, thin)
    return h


# ---------------------------------------------------------------- qr-v1


SEED0 = 0x4C41_5152_0000_0000


def qr_sizes():
    rng = random.Random(20261119)
    fixed = [(200, 200, "general"), (200, 6, "general"), (200, 120, "colgraded"), (150, 150, "rankdef"),
             (100, 100, "tied"), (120, 60, "rowgraded"), (60, 60, "scaled"), (1, 1, "general"),
             (5, 1, "tied"), (4, 3, "tied"), (3, 3, "rankdef"), (2, 2, "scaled")]
    out = list(fixed)
    i = 0
    while len(out) < 300:
        kind = KINDS[i % len(KINDS)]
        i += 1
        r = rng.random()
        if r < 0.15:
            m = rng.randint(1, 4)
        elif r < 0.9:
            m = rng.randint(1, 24)
        else:
            m = rng.randint(25, 60)
        n = rng.randint(1, m)
        out.append((m, n, kind))
    return out


def tol_for(idx):
    return (1e-12, 0.0, 1e-8, 1e-14)[idx % 4]


def make_qr():
    out = [
        "# KRN.NUM.LA.QR AC-1, AC-5: 300 seeded matrices (1x1 .. 200x200), CC-BY-4.0.",
        "# Written by make_qr_fixtures.py; do not edit. Inputs are rebuilt from `seed` by the splitmix64",
        "# stream of mat-ops-v1.txt (general(u), and int(lo, hi) = lo + u mod (hi - lo + 1)), by kind:",
        "#  general: A (m x n row-major) = general draws.",
        "#  colgraded / rowgraded: general draws, entry (i, j) times 2^-g with g = round(40 j / (n - 1))",
        "#    (resp. round(40 i / (m - 1))) rounded half up in integers, 0 for a single column (row).",
        "#  rankdef: r = ceil(n / 2) columns of int(-8, 8) draws (column by column); then for",
        "#    j = r .. n-1: c1 = int(-3, 3), c2 = int(-3, 3), column j = col[j mod r] * c1 + col[(j+1) mod r] * c2.",
        "#  tied: base = m int(-8, 8) draws, base[0] = 9; column j: one draw u, entry i = base[(i + j) mod m],",
        "#    negated iff bit (i mod 64) of u is set (all column norms are equal and exact).",
        "#  scaled: one draw u, e = 400 if u is odd else -400; A = general draws times 2^e.",
        "# then b (m) = general draws. tol = rel_tol. Results per case, each the first 16 hex digits of",
        "# SHA-256 over little-endian 8-byte words: `qr` and `qrp` hash packed (row-major), tau, perm,",
        "# rank, cond bits and thin Q (row-major) of qr / qr_pivoted; `qt` hashes apply_qt(b) then",
        "# apply_q(apply_qt(b)) of qr_pivoted; `lstsq` hashes x, residual_norm, rank, cond, or is",
        "# `rankdef:<rank>`. `digest` = SHA-256 of all those words in that order, case by case.",
        "# The script checks every factor exactly: ||A P - Q R||_F <= 10 n u ||A||_F and",
        "# ||Q^T Q - I||_F <= 10 n u (u = 2^-53), for qr and qr_pivoted, full rank or not.",
    ]
    body = []
    h = hashlib.sha256()
    worst = [0.0, 0.0]
    for idx, (m, n, kind) in enumerate(qr_sizes()):
        seed = SEED0 + idx
        tol = tol_for(idx)
        a, b = gen_matrix(seed, kind, m, n)
        fields = []
        ranks = []
        for name, piv in (("qr", False), ("qrp", True)):
            q = qr(m, n, a, tol, piv)
            thin = q.thin_q()
            rec, orth = check_bounds(m, n, a, thin, q.perm, q.r(), (idx, name))
            worst = [max(worst[0], rec), max(worst[1], orth)]
            put(h, q.packed())
            put(h, q.tau)
            put_ints(h, q.perm)
            put_ints(h, [q.rank])
            put(h, [q.cond])
            put(h, thin)
            fields.append(f"{name} {qr_bytes(q, thin).hexdigest()[:16]}")
            ranks.append(q.rank)
            if piv:
                qp = q
        qt = qp.apply(b, True)
        back = qp.apply(qt, False)
        hq = hashlib.sha256()
        for hh in (h, hq):
            put(hh, qt)
            put(hh, back)
        fields.append(f"qt {hq.hexdigest()[:16]}")
        try:
            x, res, rank, cond = lstsq(m, n, a, b, tol)
            hl = hashlib.sha256()
            for hh in (h, hl):
                put(hh, x)
                put(hh, [res])
                put_ints(hh, [rank])
                put(hh, [cond])
            fields.append(f"lstsq {hl.hexdigest()[:16]}")
        except Fail as f:
            assert f.kind == "RankDeficient", (idx, f.kind, f.info)
            put_ints(h, [f.info])
            fields.append(f"lstsq rankdef:{f.info}")
        body.append(f"case {idx} seed {seed:016x} kind {kind} m {m} n {n} tol {hx(tol)} rank {ranks[0]} {ranks[1]} "
                    + " ".join(fields))
    out.append(f"# worst error / bound: reconstruction {worst[0]:.4f}, orthogonality {worst[1]:.4f}")
    out.append(f"digest sha256:{h.hexdigest()}")
    return "\n".join(out + body) + "\n"


# ---------------------------------------------------------------- lstsq-v1


def plain_rhs(m, n, a, xs, noise):
    """b_i = (((a_i0 x_0 + a_i1 x_1) + ...) + noise_i), plain binary64 operations left to right."""
    b = []
    for i in range(m):
        s = 0.0
        for j in range(n):
            s = s + a[i * n + j] * xs[j]
        b.append(s + noise[i])
    return b


def lstsq_cases():
    """(name, m, n, a, b, tol, certify): certify = 1 if AC-3's 7 digits are asserted."""
    g = Gen(0x4C53_5153_0000_0000)
    cases = []

    def xstar(n):
        return [float(g_int(g, 1, 9) * (1 if g.next() & 1 else -1)) for _ in range(n)]

    # Polynomial fits in integer abscissae: exact integer design matrices.
    for deg, m in ((1, 10), (2, 15), (3, 21), (4, 31), (5, 41), (6, 41)):
        xs = [float(i - (m - 1) // 2) for i in range(m)]
        n = deg + 1
        a = [xs[i] ** j for i in range(m) for j in range(n)]
        noise = [g_int(g, -64, 64) * 2.0 ** -10 for _ in range(m)]
        cases.append((f"poly-deg{deg}", m, n, a, plain_rhs(m, n, a, xstar(n), noise), 1e-14, 1))
    # Longley-like: an intercept, a year column and near-collinear columns with large offsets.
    for v in range(4):
        m, n = 16, 7
        rows = []
        for i in range(m):
            t = 1947.0 + i
            rows += [1.0, t, t + g_int(g, -5, 5), 3.0 * t + g_int(g, -9, 9), 60000.0 + 500.0 * i + g_int(g, -300, 300),
                     float(g_int(g, 1000, 5000)), 100.0 * t + g_int(g, -50, 50)]
        noise = [g_int(g, -64, 64) * 2.0 ** -6 for _ in range(m)]
        cases.append((f"longley-like-{v}", m, n, rows, plain_rhs(m, n, rows, xstar(n), noise), 1e-15, 1))
    # Random tall problems (general draws), sizes up to 60 x 12.
    for v, (m, n) in enumerate(((3, 2), (8, 8), (20, 5), (40, 12), (60, 3), (60, 12), (33, 1), (50, 10))):
        a = [g.general() for _ in range(m * n)]
        noise = [g.general() * 2.0 ** -20 for _ in range(m)]
        cases.append((f"random-{v}", m, n, a, plain_rhs(m, n, a, xstar(n), noise), 1e-13, 1))
    # Column-scaled problems (powers of two over 40 binades). Pivoted QR is not invariant under
    # column scaling, so componentwise digits of the small-scale columns are only normwise
    # accurate: the 12 x 4 case is certified, the 30 x 9 case (about 5.5 digits) is pinned only.
    for v, (m, n) in enumerate(((12, 4), (30, 9))):
        a = [g.general() * math.ldexp(1.0, -grade_exp(j, n, 40)) for _ in range(m) for j in range(n)]
        noise = [g.general() * 2.0 ** -30 for _ in range(m)]
        cases.append((f"colscaled-{v}", m, n, a, plain_rhs(m, n, a, xstar(n), noise), 0.0, 1 - v))
    # Filip-like: degree-10 polynomial on x = -9 + i/8; condition far beyond 1e10. Bits pinned,
    # digits reported but not asserted (the leaf marks Filip's 7 digits [unverified]).
    m, n = 41, 11
    xs = [-9.0 + i / 8.0 for i in range(m)]
    a = []
    for x in xs:
        p = 1.0
        for j in range(n):
            a.append(p)
            p = p * x
    noise = [g_int(g, -64, 64) * 2.0 ** -20 for _ in range(m)]
    cases.append(("filip-like", m, n, a, plain_rhs(m, n, a, xstar(n), noise), 0.0, 0))
    return cases


def make_lstsq():
    out = [
        "# KRN.NUM.LA.QR AC-3: synthetic least-squares problems with exact references, CC-BY-4.0.",
        "# Written by make_qr_fixtures.py; do not edit. NIST StRD Longley and Filip are not",
        "# redistributed (licence not verified); these synthetic problems imitate them. Per case:",
        "# `case <name> m <m> n <n> tol <hex> certify <0|1> lre <min digits>`, then `a` (row-major),",
        "# `b`, `ref` (the exact rational least-squares solution of A^T A x = A^T b, rounded once),",
        "# `res` (exact residual norm, rounded) and `x` / `rnorm` / `rank` / `cond` (the pinned",
        "# lstsq result bits). certify 1: every component of x has >= 7 significant digits",
        "# (log relative error) against ref, asserted here and by the Rust test.",
        "# `digest` = SHA-256 of x, residual_norm, rank (u64), cond of every case in order.",
    ]
    body = []
    h = hashlib.sha256()
    for name, m, n, a, b, tol, cert in lstsq_cases():
        x, res, rank, cond = lstsq(m, n, a, b, tol)
        ref = exact_lstsq(m, n, a, b)
        lre = min(digits(xi, ri) for xi, ri in zip(x, ref))
        if cert:
            assert lre >= 7.0, (name, lre)
        eres = exact_residual(m, n, a, b, ref)
        put(h, x)
        put(h, [res])
        put_ints(h, [rank])
        put(h, [cond])
        body.append(f"case {name} m {m} n {n} tol {hx(tol)} certify {cert} lre {lre:.2f}")
        body.append("a " + la.vec_str(a))
        body.append("b " + la.vec_str(b))
        body.append("ref " + la.vec_str([float(v) for v in ref]))
        body.append(f"res {hx(eres)}")
        body.append("x " + la.vec_str(x))
        body.append(f"rnorm {hx(res)} rank {rank} cond {hx(cond)}")
    out.append(f"digest sha256:{h.hexdigest()}")
    return "\n".join(out + body) + "\n"


# ---------------------------------------------------------------- qr-edge-v1


def edge_cases():
    c = []

    def add(name, op, a, x=None, tol=1e-12):
        c.append((name, op, a, x, tol))

    nan, tiny = math.nan, 5e-324
    tie = (4, 3, [3.0, 0.0, 4.0, 4.0, 0.0, 0.0, 0.0, 4.0, 3.0, 0.0, 3.0, 0.0])
    add("ac2-tie-qrp", "qr_pivoted", tie)
    add("ac2-tie-qr", "qr", tie)
    dup = (3, 3, [1.0, 2.0, 1.0, 3.0, 1.0, 3.0, 2.0, 5.0, 2.0])
    add("ac4-dup-lstsq", "lstsq", dup, [1.0, 2.0, 3.0], 1e-12)
    add("ac4-dup-qrp", "qr_pivoted", dup)
    add("ac4-wide-qr", "qr", (2, 3, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]))
    add("wide-qrp", "qr_pivoted", (2, 3, [1.0] * 6))
    add("wide-lstsq", "lstsq", (2, 3, [1.0] * 6), [1.0, 2.0])
    add("one-pos", "qr", (1, 1, [2.0]))
    add("one-neg", "qr", (1, 1, [-3.0]))
    add("one-zero", "qr", (1, 1, [0.0]))
    add("one-neg-zero", "qr_pivoted", (1, 1, [-0.0]))
    add("alpha-zero", "qr", (2, 2, [0.0, 1.0, 1.0, 0.0]))
    add("alpha-neg-zero", "qr", (2, 2, [-0.0, 1.0, 3.0, 0.0]))
    add("zero-col-first", "qr", (3, 2, [-0.0, 1.0, 0.0, 2.0, -0.0, 3.0]), None, 0.0)
    add("zero-col-pivoted", "qr_pivoted", (3, 2, [-0.0, 1.0, 0.0, 2.0, -0.0, 3.0]), None, 0.0)
    add("zero-matrix", "qr_pivoted", (3, 2, [0.0] * 6))
    add("zero-matrix-lstsq", "lstsq", (3, 2, [0.0] * 6), [1.0, 1.0, 1.0])
    add("tall-col", "qr", (4, 1, [1.0, -2.0, 2.0, 4.0]))
    add("square-exact", "lstsq", (2, 2, [2.0, 1.0, 1.0, 3.0]), [3.0, 5.0], 0.0)
    add("tall-residual", "lstsq", (3, 2, [1.0, 0.0, 0.0, 1.0, 1.0, 1.0]), [1.0, 1.0, 0.0], 1e-12)
    add("tol-one", "qr_pivoted", (2, 2, [3.0, 0.0, 0.0, 2.0]), None, 1.0)
    add("tol-equal-threshold", "qr_pivoted", (2, 2, [4.0, 0.0, 0.0, 1.0]), None, 0.25)
    add("tol-just-below", "qr_pivoted", (2, 2, [4.0, 0.0, 0.0, 1.0]), None, 0.24999999999999997)
    add("tol-nan", "qr", (1, 1, [1.0]), None, nan)
    add("tol-inf", "qr_pivoted", (1, 1, [1.0]), None, INF)
    add("tol-neg", "lstsq", (1, 1, [1.0]), [1.0], -1e-12)
    add("tol-neg-zero", "qr", (2, 1, [3.0, 4.0]), None, -0.0)
    add("shape-before-tol", "qr", (1, 2, [1.0, 1.0]), None, nan)
    add("b-short", "lstsq", (2, 1, [1.0, 1.0]), [1.0], 1e-12)
    add("b-nan", "lstsq", (2, 1, [1.0, 1.0]), [1.0, nan], 1e-12)
    add("b-inf-before-tol", "lstsq", (2, 1, [1.0, 1.0]), [-INF, 1.0], nan)
    add("subnormal-matrix", "qr", (2, 1, [1e-310, -2e-310]))
    add("subnormal-matrix-lstsq", "lstsq", (2, 1, [1e-310, 2e-310]), [1.0, 1.0])
    add("subnormal-b", "lstsq", (2, 1, [1.0, 1.0]), [tiny, -tiny])
    add("subnormal-entry-normal-scale", "qr_pivoted", (2, 2, [1.0, tiny, 2.0 ** -1060, 1.0]))
    add("smallest-normal-scale", "qr", (2, 1, [MIN_NORMAL, MIN_NORMAL]))
    add("zero-b", "lstsq", (2, 1, [1.0, 1.0]), [0.0, -0.0])
    add("overflow-u0", "qr", (2, 1, [1e308, 1e308]))
    add("overflow-update", "qr", (2, 2, [1.0, MAX, 1.0, MAX]))
    add("overflow-norm-pivoted", "qr_pivoted", (2, 2, [1.0, MAX, 1.0, MAX]))
    add("cond-beyond-range", "qr_pivoted", (2, 2, [1e300, 0.0, 0.0, 1e-300]), None, 0.0)
    add("cond-beyond-range-tol", "qr_pivoted", (2, 2, [1e300, 0.0, 0.0, 1e-300]), None, 1e-12)
    add("lstsq-overflow-x", "lstsq", (2, 2, [1e-300, 0.0, 0.0, 1.0]), [1e10, 1.0], 0.0)
    add("pivot-later-column", "qr_pivoted", (3, 3, [1.0, 2.0, 10.0, 0.0, 1.0, 1.0, 0.0, 0.0, 5.0]))
    add("hilbert-5", "qr_pivoted", (5, 5, [1.0 / (i + j + 1) for i in range(5) for j in range(5)]), None, 1e-16)
    add("hilbert-5-lstsq", "lstsq", (5, 5, [1.0 / (i + j + 1) for i in range(5) for j in range(5)]), [1.0] * 5, 1e-16)
    add("apply-qt-short", "apply_qt", (3, 2, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), [1.0, 2.0])
    add("apply-qt-nan", "apply_qt", (3, 2, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), [1.0, 2.0, nan])
    add("apply-qt-subnormal", "apply_qt", (3, 2, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), [tiny, 0.0, 0.0])
    add("apply-qt", "apply_qt", (3, 2, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), [1.0, -1.0, 0.5])
    add("apply-q", "apply_q", (3, 2, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), [1.0, -1.0, 0.5])
    add("apply-qt-neg-zero-skip", "apply_qt", (2, 1, [0.0, 0.0]), [-0.0, 1.0])
    add("thin-q", "thin_q", (3, 2, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]))
    return c


def run_edge(op, a, x, tol):
    m, n, d = a
    if op == "qr" or op == "qr_pivoted":
        q = qr(m, n, d, tol, op == "qr_pivoted")
        return ("qr", q)
    if op == "lstsq":
        return ("lstsq", lstsq(m, n, d, x, tol))
    q = qr(m, n, d, tol, True)
    if op == "apply_qt":
        return ("vec", q.apply(x, True))
    if op == "apply_q":
        return ("vec", q.apply(x, False))
    if op == "thin_q":
        return ("vec", q.thin_q())
    raise ValueError(op)


def expect_str(r):
    kind, v = r
    if kind == "qr":
        return (f"ok packed {la.vec_str(v.packed())} tau {la.vec_str(v.tau)} perm {' '.join(map(str, v.perm))} "
                f"rank {v.rank} cond {hx(v.cond)}")
    if kind == "lstsq":
        x, res, rank, cond = v
        return f"ok x {la.vec_str(x)} rnorm {hx(res)} rank {rank} cond {hx(cond)}"
    return "ok " + la.vec_str(v)


def make_edge():
    out = [
        "# KRN.NUM.LA.QR edge and error cases, CC-BY-4.0.",
        "# Written by make_qr_fixtures.py; do not edit. Per case: `case <name> <op>`, `a <m> <n> <hex...>`",
        "# (row-major), optional `x <hex...>` (b for lstsq, the vector for apply_qt / apply_q, which",
        "# use qr_pivoted of `a`), `tol <hex>`, then `expect`: for qr / qr_pivoted",
        "# `ok packed <hex...> tau <hex...> perm <ints> rank <r> cond <hex>`; for lstsq",
        "# `ok x <hex...> rnorm <hex> rank <r> cond <hex>`; for apply_qt / apply_q / thin_q `ok <hex...>`;",
        "# or `err <Kind> <index|none|detail|rank>` (Kind RankDeficient carries the rank).",
    ]
    for name, op, a, x, tol in edge_cases():
        try:
            s = expect_str(run_edge(op, a, x, tol))
        except Fail as f:
            s = f"err {f.kind} {'none' if f.info is None else f.info}"
        out.append(f"case {name} {op}")
        out.append(f"a {a[0]} {a[1]} {la.vec_str(a[2])}")
        if x is not None:
            out.append(("x " + la.vec_str(x)).rstrip())
        out.append(f"tol {hx(tol)}")
        out.append("expect " + s)
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------- main


def self_test():
    # Hand-labelled from the pinned rules: 1x1 [-3]: beta = +3, u0 = -6, tau = 2, Q = [-1].
    q = qr(1, 1, [-3.0], 0.0, False)
    assert q.packed() == [3.0] and q.tau == [2.0] and q.thin_q() == [-1.0]
    # [2]: beta = -2 (opposite sign of a_kk), tau = 2.
    q = qr(1, 1, [2.0], 0.0, False)
    assert q.packed() == [-2.0] and q.tau == [2.0]
    # AC-2: equal column norms: the first pivot is column 0.
    q = qr(4, 3, [3.0, 0.0, 4.0, 4.0, 0.0, 0.0, 0.0, 4.0, 3.0, 0.0, 3.0, 0.0], 1e-12, True)
    assert q.perm[0] == 0
    # AC-4: two equal columns: rank 2.
    try:
        lstsq(3, 3, [1.0, 2.0, 1.0, 3.0, 1.0, 3.0, 2.0, 5.0, 2.0], [1.0, 2.0, 3.0], 1e-12)
        raise AssertionError("ac4")
    except Fail as f:
        assert (f.kind, f.info) == ("RankDeficient", 2)
    # The exact oracle on a known case: x = (1, 1) solves [[1, 0], [0, 1], [1, 1]] x = (1, 1, 2).
    assert exact_lstsq(3, 2, [1.0, 0.0, 0.0, 1.0, 1.0, 1.0], [1.0, 1.0, 2.0]) == [1, 1]
    ints, e = to_ints([0.5, 3.0, -0.0])
    assert [Fraction(v) * Fraction(2) ** e for v in ints] == [Fraction(1, 2), 3, 0]


def main():
    self_test()
    files = {"qr-edge-v1.txt": make_edge, "lstsq-v1.txt": make_lstsq, "qr-v1.txt": make_qr}
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
