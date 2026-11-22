#!/usr/bin/env python3
"""Independent reference for KRN.NUM.LA.EIG (CC-BY-4.0 project data).

Writes
  fixtures/num/la/eig-v1.txt       300 seeded symmetric matrices (2x2 .. 200x200: general,
                                   Hadamard with exact repeated/zero eigenvalues, clustered,
                                   graded 1 .. 1e-15, 3x3 near-planar scatter matrices, and
                                   scaled by 2^+-400) with 50-digit oracle eigenvalues (AC-1, AC-5)
  fixtures/num/la/eig-edge-v1.txt  hand-chosen edge and error cases (AC-2, AC-3, AC-4 and more)

Two independent parts, neither of which runs the Rust code (stdlib only):

1. An oracle that knows nothing of the binary64 operation order:
   - eigenvalues by cyclic Jacobi in 400-bit fixed point (rotation parameters in 70-digit
     `decimal`) on B = Q^T A Q, where Q is the model's V re-orthonormalised by modified
     Gram-Schmidt in the same fixed point; Q only speeds convergence (Q^T A Q has the exact
     same spectrum as A for any orthogonal Q, so the oracle does not depend on the model being
     right). For the Hadamard kind the exact eigenvalues n * d_k are known in closed form and
     must agree too;
   - residual ||A V - V Lambda||_F <= 10 n u ||A||_F and orthogonality ||V^T V - I||_F <= 10 n u
     are checked in exact integers (every binary64 value is a dyadic rational);
   - |lambda_k - oracle_k| <= 10 n u ||A||_2 is checked in 70-digit decimal (REQ-006).
2. A bit-exact Python model of the rules pinned in the Rust module documentation
   (`crates/fid-num/src/la/eig.rs`): checks, power-of-two scaling, symmetry check, upper
   triangle copy, cyclic threshold Jacobi with Rutishauser updates, stopping criterion,
   unscaling, total-order ascending sort, eigenvector sign rule and diagnostics. Reuses the
   reviewed KRN.NUM.RED / KRN.NUM.LA.MAT model of `make_la_fixtures.py`.

Run:  python fixtures/num/la/make_eig_fixtures.py          (writes the fixtures)
      python fixtures/num/la/make_eig_fixtures.py --check  (fails if a fixture is stale)
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
import make_qr_fixtures as qrf  # noqa: E402  (exact integer helpers)

MAX = sys.float_info.max
MIN_NORMAL = 2.0 ** -1022
INF = math.inf
U = 2.0 ** -53
MAX_SWEEPS = 50
Z_BIG = 2.0 ** 256
DEFLATE = 2.0 ** -600
SMALL_RATIO = 0.01 * U
bits, hx, finite, Gen = la.bits, la.hx, la.finite, la.Gen
Fail = qrf.Fail

CTX = decimal.Context(prec=70)


# ---------------------------------------------------------------- 2. model: KRN.NUM.LA.EIG


def pow2(k):
    return math.ldexp(1.0, k)


def total_key(x):
    """Key matching IEEE-754 f64::total_cmp for finite binary64 values."""
    b = bits(x)
    if b >> 63:
        return (0, ~b & 0xFFFF_FFFF_FFFF_FFFF)
    return (1, b)


def total_min(a, b):
    return b if total_key(a) > total_key(b) else a


def total_max(a, b):
    return b if total_key(a) < total_key(b) else a


def largest(x):
    best = 0
    for i, v in enumerate(x):
        if abs(v) > abs(x[best]):
            best = i
    return best


def off_fro(rows, n, sqrt2):
    upper = [rows[i][j] for i in range(n) for j in range(i + 1, n)]
    if not upper:
        return 0.0
    u_norm = la.norm2(upper)
    off = u_norm * sqrt2
    if not finite(off):
        raise Fail("Infinite", None)
    return off


def jacobi(n, rows, v, max_sweeps):
    fro_a = la.norm2([x for r in rows for x in r])
    tol_off = (float(n) * U) * fro_a
    floor = fro_a * DEFLATE
    sqrt2 = math.sqrt(2.0)
    cur_off = off_fro(rows, n, sqrt2)

    for sweep in range(1, max_sweeps + 1):
        thresh = cur_off / (5.0 * float(n)) if sweep <= 3 else 0.0
        for p in range(n):
            rp = rows[p]
            for q in range(p + 1, n):
                apq = rp[q]
                apq_abs = abs(apq)
                if apq_abs == 0.0 or apq_abs <= floor:
                    continue
                if sweep <= 3 and apq_abs < thresh:
                    continue
                rq = rows[q]
                app = rp[p]
                aqq = rq[q]
                if sweep > 3:
                    min_diag = total_min(abs(app), abs(aqq))
                    if apq_abs <= SMALL_RATIO * min_diag:
                        continue
                diff = aqq - app
                theta = diff / (2.0 * apq)
                if not finite(theta):
                    t = apq / diff
                elif abs(theta) >= Z_BIG:
                    t = 0.5 / theta
                else:
                    sg = -1.0 if theta < 0.0 else 1.0
                    t = sg / (abs(theta) + math.sqrt(1.0 + theta * theta))
                cs = 1.0 / math.sqrt(1.0 + t * t)
                sn = cs * t
                tau = sn / (1.0 + cs)

                h = t * apq
                new_rp = [g - sn * (hk + tau * g) for g, hk in zip(rp, rq)]
                new_rq = [hk + sn * (g - tau * hk) for g, hk in zip(rp, rq)]
                new_rp[p] = app - h
                new_rq[q] = aqq + h
                new_rp[q] = 0.0
                new_rq[p] = 0.0
                rows[p] = rp = new_rp
                rows[q] = new_rq
                for k in range(n):
                    if k != p and k != q:
                        rk = rows[k]
                        rk[p] = new_rp[k]
                        rk[q] = new_rq[k]

                vp = v[p]
                vq = v[q]
                v[p] = [a_ - sn * (b_ + tau * a_) for a_, b_ in zip(vp, vq)]
                v[q] = [b_ + sn * (a_ - tau * b_) for a_, b_ in zip(vp, vq)]

        cur_off = off_fro(rows, n, sqrt2)
        if cur_off <= tol_off:
            return sweep
    raise Fail("NoConvergence", max_sweeps)


class SymEig:
    def __init__(self, n, values, vectors, rank, cond, sweeps):
        self.n = n
        self.values = values
        self.vectors = vectors
        self.rank = rank
        self.cond = cond
        self.sweeps = sweeps


def sym_eig(m, n, a, tol, max_sweeps=MAX_SWEEPS):
    """a: row-major list of length m * n. Returns SymEig or raises Fail."""
    if m != n:
        raise Fail("Domain", "bad_shape")
    qrf.check_tol(tol)

    mx = 0.0
    for x in a:
        mx = total_max(mx, abs(x))
    if mx != 0.0 and mx < MIN_NORMAL:
        raise Fail("Domain", "subnormal_input")

    if mx == 0.0:
        e, d1, d2 = 0, 1.0, 1.0
    else:
        e = la.exponent(mx)
        d1, d2 = la.scale_factors(e)

    mx_s = (mx * d1) * d2
    sym_tol = (float(n) * U) * mx_s
    for i in range(n):
        for j in range(i + 1, n):
            sij = (a[i * n + j] * d1) * d2
            sji = (a[j * n + i] * d1) * d2
            if abs(sij - sji) > sym_tol:
                raise Fail("NotSymmetric", f"{i} {j}")

    rows = [[0.0] * n for _ in range(n)]
    for i in range(n):
        rows[i][i] = (a[i * n + i] * d1) * d2
        for j in range(i + 1, n):
            val = (a[i * n + j] * d1) * d2
            rows[i][j] = val
            rows[j][i] = val

    v = [[1.0 if i == j else 0.0 for i in range(n)] for j in range(n)]

    sweeps = jacobi(n, rows, v, max_sweeps)

    scale = pow2(e)
    raw = []
    for j in range(n):
        val = rows[j][j] * scale
        if not finite(val):
            raise Fail("Infinite", j)
        raw.append(val)

    order = sorted(range(n), key=lambda j: (total_key(raw[j]), j))
    values = []
    vcols = []
    for j in order:
        values.append(raw[j])
        col = list(v[j])
        if col[largest(col)] < 0.0:
            col = [-x for x in col]
        vcols.append(col)

    vectors = [vcols[j][i] for i in range(n) for j in range(n)]
    abs_desc = sorted((abs(x) for x in values), key=total_key, reverse=True)
    rank = la.m_rank(abs_desc, tol)[1]
    min_abs = abs_desc[-1]
    if min_abs == 0.0:
        cond = INF
    else:
        q = abs_desc[0] / min_abs
        cond = q if finite(q) else INF
    return SymEig(n, values, vectors, rank, cond, sweeps)


# ---------------------------------------------------------------- 1. oracle


def dec(x):
    return Decimal(x)


FB = 400


def fix(x, e):
    """round(x * 2^(FB - e)) for a binary64 x, exactly."""
    mt, ex = la.ratio(x)
    sh = ex + FB - e
    if sh >= 0:
        return mt << sh
    return (mt + (1 << (-sh - 1))) >> -sh


def dround(x):
    return int(x.to_integral_value(rounding=decimal.ROUND_HALF_EVEN))


def mgs(cols):
    """Modified Gram-Schmidt on fixed-point column vectors (scaled by 2^FB)."""
    out = []
    mul = int.__mul__
    for col in cols:
        x = list(col)
        for qv in out:
            d = sum(map(mul, qv, x)) >> FB
            x = [a - ((d * b) >> FB) for a, b in zip(x, qv)]
        nn = math.isqrt(sum(map(mul, x, x)))
        assert nn > (1 << (FB - 1)), "model V is far from orthogonal"
        out.append([(a << FB) // nn for a in x])
    return out


def oracle_eig(n, a, model):
    """Eigenvalues of symmetric A (from upper triangle), ascending, as Decimals.

    Uses 400-bit fixed-point cyclic Jacobi on B = Q^T A Q where Q = MGS(model.vectors).
    Since Q^T A Q is orthogonally similar to A for any orthogonal Q, the oracle is
    independent of model accuracy. Absolute error of every eigenvalue is below 1e-40 ||A||_2.
    """
    mx = max(abs(x) for x in a)
    if mx == 0.0:
        return [Decimal(0)] * n
    e = la.exponent(mx)
    one = 1 << FB
    mul = int.__mul__
    # Exact symmetric fixed-point A (from upper triangle).
    af = [[0] * n for _ in range(n)]
    for i in range(n):
        af[i][i] = fix(a[i * n + i], e)
        for j in range(i + 1, n):
            v = fix(a[i * n + j], e)
            af[i][j] = v
            af[j][i] = v

    # Precondition with orthogonal Q = MGS(model.vectors).
    qsrc = [[fix(model.vectors[i * n + j], 0) for i in range(n)] for j in range(n)]
    q = mgs(qsrc)  # list of n column vectors

    # T = A Q (n columns of length n).
    aq = [[sum(map(mul, row, qj)) >> FB for row in af] for qj in q]

    # B = Q^T (A Q), symmetrised from upper triangle.
    b = [[0] * n for _ in range(n)]
    for i in range(n):
        qi = q[i]
        bi = b[i]
        for j in range(i, n):
            val = sum(map(mul, qi, aq[j])) >> FB
            bi[j] = val
            b[j][i] = val

    fro = math.isqrt(sum(sum(map(mul, row, row)) for row in b))
    stop = max(1, fro >> 220)

    with decimal.localcontext(CTX):
        for _ in range(30):
            rotated = False
            for p in range(n):
                bp = b[p]
                for qq in range(p + 1, n):
                    bpq = bp[qq]
                    if abs(bpq) <= stop:
                        continue
                    bq = b[qq]
                    bpp, bqq = bp[p], bq[qq]
                    theta = (Decimal(bqq) - Decimal(bpp)) / (2 * Decimal(bpq))
                    sg = -1 if theta < 0 else 1
                    t = sg / (abs(theta) + (1 + theta * theta).sqrt())
                    cd = 1 / (1 + t * t).sqrt()
                    sd = cd * t
                    td = sd / (1 + cd)
                    sn, tau = dround(sd * one), dround(td * one)
                    h = dround(t * Decimal(bpq))
                    new_bp = [g - ((sn * (hk + ((tau * g) >> FB))) >> FB) for g, hk in zip(bp, bq)]
                    new_bq = [hk + ((sn * (g - ((tau * hk) >> FB))) >> FB) for g, hk in zip(bp, bq)]
                    new_bp[p] = bpp - h
                    new_bq[qq] = bqq + h
                    new_bp[qq] = 0
                    new_bq[p] = 0
                    b[p] = bp = new_bp
                    b[qq] = new_bq
                    for k in range(n):
                        if k != p and k != qq:
                            bk = b[k]
                            bk[p] = new_bp[k]
                            bk[qq] = new_bq[k]
                    rotated = True
            if not rotated:
                break
        else:
            raise AssertionError("oracle did not converge")
        unit = Decimal(2) ** (e - FB)
        return sorted(Decimal(b[i][i]) * unit for i in range(n))


def check_eigs(n, model, ref, tag):
    """REQ-006 eigenvalue bound in 70-digit decimal; returns worst error / bound."""
    with decimal.localcontext(CTX):
        norm2 = max(abs(rv) for rv in ref)
        bound = 10 * n * dec(U) * norm2
        worst = Decimal(0)
        for lam, rv in zip(model.values, ref):
            err = abs(dec(lam) - rv)
            assert err <= bound, (tag, "eig", lam, rv)
            if bound:
                worst = max(worst, err / bound)
        return float(worst)


def check_orth_fro(n, d, tag):
    """Exact ||V^T V - I||_F <= 10 n u for row-major n x n V; returns err / bound."""
    vi, e = qrf.to_ints(d)
    assert e < 0, (tag, "unexpected eigenvector exponent")
    one = 1 << (-2 * e)
    cols = [vi[j::n] for j in range(n)]
    err2 = 0
    for i in range(n):
        for j in range(n):
            g = sum(map(int.__mul__, cols[i], cols[j])) - (one if i == j else 0)
            err2 += g * g
    # ||V^T V - I||_F <= 10 n u  <=>  err2 * 2^106 <= 100 n^2 one^2
    assert err2 << 106 <= 100 * n * n * one * one, (tag, "orthonormality", math.sqrt(err2) / one)
    return (math.sqrt(err2) / one) / (10 * n * U)


def check_residual(n, a, model, tag):
    """Exact ||A V - V Lambda||_F <= 10 n u ||A||_F; returns err / bound."""
    # Mirror upper triangle of A to match REQ-001.
    asym = [a[min(i, j) * n + max(i, j)] for i in range(n) for j in range(n)]
    ai, ea = qrf.to_ints(asym)
    vi, ev = qrf.to_ints(model.vectors)
    li, el = qrf.to_ints(model.values)
    av = qrf.int_matmul(ai, vi, n, n, n)  # 2^(ea + ev)
    vl = [vi[i * n + j] * li[j] for i in range(n) for j in range(n)]  # 2^(ev + el)
    e0 = min(ea + ev, ev + el)
    sav, svl = (ea + ev) - e0, (ev + el) - e0
    err2 = sum(((x << sav) - (y << svl)) ** 2 for x, y in zip(av, vl))
    a2 = sum(x * x for x in ai)  # 2^(2 ea)
    # err2 * 2^(2 e0) <= 100 n^2 u^2 * a2 * 2^(2 ea)
    # <=> err2 * 2^(106 + 2(e0 - ea)) <= 100 n^2 a2
    shift = 106 + 2 * (e0 - ea)
    if shift >= 0:
        assert err2 << shift <= 100 * n * n * a2, (tag, "residual")
    else:
        assert err2 <= (100 * n * n * a2) << (-shift), (tag, "residual")
    if a2 == 0:
        return 0.0
    ratio = math.sqrt(err2 / a2) * (2.0 ** (e0 - ea))
    return ratio / (10 * n * U)


# ---------------------------------------------------------------- shared input generator


GRADE = 50  # 2^-50 ~ 8.9e-16
KINDS = ("general", "hadamard", "clustered", "graded", "scatter3", "scaled")


def had(i, k):
    return -1 if bin(i & k).count("1") & 1 else 1


def gen_matrix(seed, kind, n):
    """Symmetric n x n matrix rebuilt identically by the Rust test; also Hadamard d."""
    g = Gen(seed)
    d = None
    if kind == "general":
        a = [0.0] * (n * n)
        for i in range(n):
            a[i * n + i] = g.general()
            for j in range(i + 1, n):
                v = g.general()
                a[i * n + j] = a[j * n + i] = v
    elif kind == "hadamard":
        d = [qrf.g_int(g, -3, 3) for _ in range(n)]
        a = [float(sum(had(i, k) * d[k] * had(j, k) for k in range(n))) for i in range(n) for j in range(n)]
    elif kind == "clustered":
        # Clustered eigenvalues conjugated by deterministic exact Pythagorean Givens rotations.
        a = [0.0] * (n * n)
        for i in range(n):
            center = float(qrf.g_int(g, -3, 3))
            eps = float(qrf.g_int(g, -8, 8)) * pow2(-24)
            a[i * n + i] = center + eps
        steps = 3 * n
        for s in range(steps):
            p = int(g.next() % n)
            q = int(g.next() % (n - 1))
            if q >= p:
                q += 1
            if p > q:
                p, q = q, p
            cs, sn = (0.6, 0.8) if (s & 1) == 0 else (0.28, -0.96)
            # Left multiply rows p, q by G^T
            for j in range(n):
                ap = a[p * n + j]
                aq = a[q * n + j]
                a[p * n + j] = cs * ap - sn * aq
                a[q * n + j] = sn * ap + cs * aq
            # Right multiply cols p, q by G
            for i in range(n):
                ap = a[i * n + p]
                aq = a[i * n + q]
                a[i * n + p] = cs * ap - sn * aq
                a[i * n + q] = sn * ap + cs * aq
        # Symmetrise exactly from upper triangle.
        for i in range(n):
            for j in range(i + 1, n):
                a[j * n + i] = a[i * n + j]
    elif kind == "graded":
        a = [0.0] * (n * n)
        for i in range(n):
            gi = qrf.grade_exp(i, n, GRADE)
            a[i * n + i] = g.general() * pow2(-gi)
            for j in range(i + 1, n):
                gj = qrf.grade_exp(j, n, GRADE)
                v = g.general() * pow2(-((gi + gj) // 2))
                a[i * n + j] = a[j * n + i] = v
    elif kind == "scatter3":
        assert n == 3
        # 3x3 scatter matrix of 24 points on a near-planar patch (random basis + tiny normal noise).
        e1 = [g.general(), g.general(), g.general()]
        e2 = [g.general(), g.general(), g.general()]
        e3 = [g.general(), g.general(), g.general()]
        noise_exp = -24 if (g.next() & 1) == 0 else -40
        noise_scale = pow2(noise_exp)
        pts = []
        for _ in range(24):
            u = g.general()
            v = g.general()
            w = g.general() * noise_scale
            pts.append([u * e1[c] + v * e2[c] + w * e3[c] for c in range(3)])
        a = [0.0] * 9
        for i in range(3):
            for j in range(i, 3):
                s = 0.0
                for pt in pts:
                    s = s + pt[i] * pt[j]
                a[i * 3 + j] = a[j * 3 + i] = s
    elif kind == "scaled":
        e = 400 if (g.next() & 1) else -400
        sc = pow2(e)
        a = [0.0] * (n * n)
        for i in range(n):
            a[i * n + i] = g.general() * sc
            for j in range(i + 1, n):
                v = g.general() * sc
                a[i * n + j] = a[j * n + i] = v
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


def eig_words(h, s):
    put(h, s.values)
    put(h, s.vectors)
    put_ints(h, [s.rank])
    put(h, [s.cond])
    put_ints(h, [s.sweeps])


# ---------------------------------------------------------------- eig-v1


SEED0 = 0x4C41_4549_4700_0000
POW2 = (2, 4, 8, 16, 32, 64)


def eig_sizes():
    rng = random.Random(20261122)
    fixed = [
        (200, "general"),
        (100, "clustered"),
        (64, "hadamard"),
        (80, "graded"),
        (60, "scaled"),
        (64, "hadamard"),
        (50, "clustered"),
        (50, "general"),
        (3, "scatter3"),
        (3, "scatter3"),
        (2, "general"),
        (2, "clustered"),
    ]
    out = list(fixed)
    i = 0
    while len(out) < 300:
        kind = KINDS[i % len(KINDS)]
        i += 1
        if kind == "scatter3":
            n = 3
        elif kind == "hadamard":
            n = rng.choice(POW2)
        else:
            r = rng.random()
            if r < 0.2:
                n = rng.randint(2, 4)
            elif r < 0.88:
                n = rng.randint(5, 24)
            else:
                n = rng.randint(25, 48)
        out.append((n, kind))
    return out


def tol_for(idx):
    return (1e-12, 0.0, 1e-8, 1e-14)[idx % 4]


def sci(x, norm2):
    """50 significant digits of a decimal oracle value; 0 below the oracle's 1e-40 ||A||_2 floor."""
    with decimal.localcontext(CTX):
        if x == 0 or abs(x) < Decimal(10) ** -40 * norm2:
            return "0"
        return f"{+x:.49e}"


def make_eig():
    out = [
        "# KRN.NUM.LA.EIG AC-1, AC-5: 300 seeded symmetric matrices (2x2 .. 200x200), CC-BY-4.0.",
        "# Written by make_eig_fixtures.py; do not edit. Inputs are rebuilt from `seed` by the splitmix64",
        "# stream of mat-ops-v1.txt (general(u), and int(lo, hi) = lo + u mod (hi - lo + 1)), by kind:",
        "#  general: upper triangle i <= j filled by general draws (row-major), mirrored to lower.",
        "#  hadamard: power-of-two n, d = n int(-3, 3) draws, A[i][j] = sum_l h(i, l) d_l h(j, l);",
        "#    exact eigenvalues are n * d_l (repeated and zero values).",
        "#  clustered: diagonal entries int(-3, 3) + int(-8, 8) * 2^-24 conjugated by 3n exact",
        "#    Pythagorean Givens rotations (0.6, 0.8) / (0.28, -0.96), symmetrised from upper.",
        "#  graded: upper triangle general draws scaled by 2^-((g_i + g_j) // 2), g_i = round(50 i / (n-1)).",
        "#  scatter3: 3x3 scatter matrix of 24 points u*e1 + v*e2 + w*2^noise*e3 (noise in {-24, -40}).",
        "#  scaled: one draw u, e = 400 if u is odd else -400; symmetric general draws times 2^e.",
        "# tol = rel_tol. Per case `eig` is the first 16 hex digits of SHA-256 over little-endian 8-byte",
        "# words: values, vectors (row-major), rank, cond bits, sweeps.",
        "# `digest` = SHA-256 of all those words in case order. The next line `ref` holds the oracle",
        "# eigenvalues (50 significant digits, ascending; fixed-point cyclic Jacobi, absolute error below",
        "# 1e-40 ||A||_2, values below that written as 0).",
        "# The script checks every case: |lambda_k - ref_k| <= 10 n u ||A||_2 (decimal),",
        "# ||A V - V Lambda||_F <= 10 n u ||A||_F and ||V^T V - I||_F <= 10 n u (exact integers), u = 2^-53.",
    ]
    body = []
    h = hashlib.sha256()
    worst = [0.0, 0.0, 0.0]
    for idx, (n, kind) in enumerate(eig_sizes()):
        seed = SEED0 + idx
        tol = tol_for(idx)
        a, d = gen_matrix(seed, kind, n)
        s = sym_eig(n, n, a, tol)
        ref = oracle_eig(n, a, s)
        if d is not None:
            with decimal.localcontext(CTX):
                exact = sorted(Decimal(n * x) for x in d)
                nrm = max((abs(x) for x in exact), default=Decimal(0))
                for x, y in zip(exact, ref):
                    assert abs(x - y) <= Decimal(10) ** -45 * (nrm + 1), (idx, "hadamard oracle")
                ref = exact
        we = check_eigs(n, s, ref, idx)
        wr = check_residual(n, a, s, idx)
        wo = check_orth_fro(n, s.vectors, idx)
        worst = [max(worst[0], we), max(worst[1], wr), max(worst[2], wo)]
        eig_words(h, s)
        hc = hashlib.sha256()
        eig_words(hc, s)
        body.append(f"case {idx} seed {seed:016x} kind {kind} n {n} tol {hx(tol)} sweeps {s.sweeps} "
                    f"rank {s.rank} eig {hc.hexdigest()[:16]}")
        norm2 = max((abs(x) for x in ref), default=Decimal(0))
        body.append("ref " + " ".join(sci(x, norm2) for x in ref))
    out.append(f"# worst error / bound: eig {worst[0]:.4f}, residual {worst[1]:.4f}, "
               f"orthonormality {worst[2]:.4f}")
    out.append(f"digest sha256:{h.hexdigest()}")
    return "\n".join(out + body) + "\n"


# ---------------------------------------------------------------- eig-edge-v1


def edge_cases():
    c = []

    def add(name, a, tol=1e-12, limit=MAX_SWEEPS):
        c.append((name, a, tol, limit))

    nan = math.nan
    add("ac4-identity-3", (3, 3, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]))
    add("ac3-not-sym", (2, 2, [1.0, 1.0, 1.1, 2.0]))
    add("not-sym-3x3-first", (3, 3, [1.0, 2.0, 3.0, 2.0, 4.0, 5.0, 9.0, 8.0, 6.0]))
    add("not-sym-within-tol", (2, 2, [1.0, 1.0, 1.0 + 2.0 * U, 2.0]))
    add("not-sym-just-over-tol", (2, 2, [1.0, 1.0, 1.0 + 4.0 * U, 2.0]))
    add("not-sym-max-opp", (2, 2, [0.0, MAX, -MAX, 0.0]))
    add("upper-only-a", (3, 3, [2.0, 1.0, 0.5, 1.0 + 2.0 * U, 3.0, -0.25, 0.5 - 2.0 * U, -0.25, 4.0]))
    add("upper-only-b", (3, 3, [2.0, 1.0, 0.5, 1.0, 3.0, -0.25, 0.5, -0.25, 4.0]))
    add("non-square-tall", (3, 2, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]))
    add("non-square-wide", (2, 3, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]))
    add("one-pos", (1, 1, [2.5]))
    add("one-neg", (1, 1, [-3.5]))
    add("one-zero", (1, 1, [0.0]))
    add("one-neg-zero", (1, 1, [-0.0]))
    add("zero-3x3", (3, 3, [0.0, -0.0, 0.0, 0.0, -0.0, 0.0, 0.0, 0.0, -0.0]))
    add("diag-unsorted", (3, 3, [3.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 1.0]))
    add("diag-neg-zero-tie", (2, 2, [0.0, 0.0, 0.0, -0.0]))
    add("sym-2x2-1-2", (2, 2, [1.0, 2.0, 2.0, 1.0]))
    add("sym-2x2-neg", (2, 2, [-1.0, -2.0, -2.0, -1.0]))
    add("sym-2x2-equal-diag", (2, 2, [5.0, 1e-12, 1e-12, 5.0]))
    add("sym-2x2-tiny-off", (2, 2, [1.0, 1e-300, 1e-300, 2.0]))
    add("sym-2x2-sign-tie", (2, 2, [0.0, 1.0, 1.0, 0.0]))
    add("sym-3x3-rank1", (3, 3, [1.0, 2.0, 3.0, 2.0, 4.0, 6.0, 3.0, 6.0, 9.0]))
    add("hilbert-6", (6, 6, [1.0 / (i + j + 1) for i in range(6) for j in range(6)]), 1e-16)
    add("near-max-overflow", (2, 2, [MAX, MAX, MAX, MAX]))
    add("near-max-diag", (2, 2, [MAX, 0.0, 0.0, -MAX / 4.0]))
    add("smallest-normal", (2, 2, [MIN_NORMAL, MIN_NORMAL, MIN_NORMAL, MIN_NORMAL]))
    add("subnormal-matrix", (2, 2, [1e-310, 0.0, 0.0, 2e-310]))
    add("subnormal-offdiag-normal-diag", (2, 2, [1.0, 5e-324, 5e-324, 2.0]))
    add("cond-beyond-range", (2, 2, [1e300, 0.0, 0.0, -1e-300]), 0.0)
    add("graded-diag", (3, 3, [1.0, 0.0, 0.0, 0.0, -1e-8, 0.0, 0.0, 0.0, 1e-16]), 1e-12)
    add("tol-one", (2, 2, [3.0, 0.0, 0.0, -2.0]), 1.0)
    add("tol-equal-threshold", (2, 2, [-4.0, 0.0, 0.0, 1.0]), 0.25)
    add("tol-just-below", (2, 2, [-4.0, 0.0, 0.0, 1.0]), 0.24999999999999997)
    add("tol-nan", (1, 1, [1.0]), nan)
    add("tol-inf", (1, 1, [1.0]), INF)
    add("tol-neg", (1, 1, [1.0]), -1e-12)
    add("tol-neg-zero", (2, 2, [3.0, 0.0, 0.0, -4.0]), -0.0)
    add("shape-before-tol", (1, 2, [1.0, 2.0]), nan)
    add("tol-before-subnormal", (1, 1, [1e-310]), nan)
    add("limit-0", (2, 2, [1.0, 0.5, 0.5, 2.0]), 1e-12, 0)
    add("limit-1-diag", (2, 2, [2.0, 0.0, 0.0, 1.0]), 1e-12, 1)
    g = Gen(0x4C41_4549_4745_4447)
    big = [0.0] * (50 * 50)
    for i in range(50):
        big[i * 50 + i] = g.general()
        for j in range(i + 1, 50):
            v = g.general()
            big[i * 50 + j] = big[j * 50 + i] = v
    add("limit-1-50x50", (50, 50, big), 1e-12, 1)
    return c


def expect_str(s):
    return (f"ok values {la.vec_str(s.values)} vectors {la.vec_str(s.vectors)} "
            f"rank {s.rank} cond {hx(s.cond)} sweeps {s.sweeps}")


def make_edge():
    out = [
        "# KRN.NUM.LA.EIG edge and error cases, CC-BY-4.0.",
        "# Written by make_eig_fixtures.py; do not edit. Per case: `case <name>`, `a <m> <n> <hex...>`",
        "# (row-major), `tol <hex>`, `limit <sweeps>` (the sweep limit; 50 is plain `sym_eig`), then",
        "# `expect`: `ok values <hex...> vectors <hex...> rank <r> cond <hex> sweeps <s>` or",
        "# `err <Kind> <info...>`.",
    ]
    for name, a, tol, limit in edge_cases():
        try:
            r = sym_eig(a[0], a[1], a[2], tol, limit)
            check_orth_fro(a[0], r.vectors, name)
            check_residual(a[0], a[2], r, name)
            s = expect_str(r)
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
    # AC-4: identity 3x3 -> values [1, 1, 1], vectors = I_3, sweeps = 1.
    s = sym_eig(3, 3, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], 1e-12)
    assert s.values == [1.0, 1.0, 1.0] and s.sweeps == 1
    assert s.vectors == [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    assert s.rank == 3 and s.cond == 1.0
    # 2x2 [[1, 2], [2, 1]] -> exact eigenvalues [-1, 3].
    s = sym_eig(2, 2, [1.0, 2.0, 2.0, 1.0], 1e-12)
    assert abs(s.values[0] - (-1.0)) < 1e-15 and abs(s.values[1] - 3.0) < 1e-15
    ref = oracle_eig(2, [1.0, 2.0, 2.0, 1.0], s)
    with decimal.localcontext(CTX):
        assert abs(ref[0] - Decimal(-1)) < Decimal(10) ** -60
        assert abs(ref[1] - Decimal(3)) < Decimal(10) ** -60
    # Upper-triangle-only invariance: two matrices differing within sym_tol in lower triangle
    # produce bit-identical outputs.
    a1 = [2.0, 1.0, 0.5, 1.0, 3.0, -0.25, 0.5, -0.25, 4.0]
    a2 = [2.0, 1.0, 0.5, 1.0 + 2.0 * U, 3.0, -0.25, 0.5 - 2.0 * U, -0.25, 4.0]
    r1 = sym_eig(3, 3, a1, 1e-12)
    r2 = sym_eig(3, 3, a2, 1e-12)
    assert [bits(x) for x in r1.values] == [bits(x) for x in r2.values]
    assert [bits(x) for x in r1.vectors] == [bits(x) for x in r2.vectors]


def main():
    self_test()
    files = {"eig-edge-v1.txt": make_edge, "eig-v1.txt": make_eig}
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
