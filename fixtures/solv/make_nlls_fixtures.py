#!/usr/bin/env python3
"""Independent reference for KRN.SOLV.NLLS (CC-BY-4.0 project data).

Writes
  fixtures/solv/strd-models-synth-v1.txt  seeded problems on the NIST StRD nonlinear model forms
  fixtures/solv/circle-synth-v1.txt       seeded 2D circle, 50 points, noise 1e-6 m

Nothing here runs or reads the Rust code. Two independent parts:

1. A 50-significant-digit reference solution of every problem, computed with the `decimal`
   module by Gauss-Newton on the normal equations (Gaussian elimination with partial pivoting)
   until the step is below 1e-40 relative. It knows nothing about the driver's algorithm
   (normal equations are fine here: this is a test oracle, not kernel code). The Rust test
   compares the driver's p with it (AC-1: at least 6 significant digits; AC-2: 1e-12 m).

2. A bit-exact model of the driver as pinned in spec/krn/solv/nlls/NODE.md (Design), on top
   of the existing bit-exact models of fid-num: the RED / LA.MAT model in
   fixtures/num/la/make_la_fixtures.py (dot, norm2, back substitution, condition estimator)
   and the vendored-function model in fixtures/num/make_fn_fixtures.py (exp, ln, atan).
   CPython floats are IEEE-754 binary64 with round-to-nearest-even and no FMA, so a match in
   CI shows the Rust code runs exactly the documented algorithm on every target (AC-3). Its
   outputs (p, cost, iterations, evaluations) and their SHA-256 are the golden values.

The data are synthetic: the NIST StRD data files are third-party reference data whose
redistribution terms are open in decision D-038, so they are not committed. Only the model
forms (mathematics) are reused; x grids, true parameters, noise and the two start points are
project choices. See fixtures/solv/nist-strd-nls-v1/convert.py for local runs on the
real NIST files.

Run:  python fixtures/solv/make_nlls_fixtures.py [--check]
"""

import hashlib
import importlib.util
import math
import struct
import sys
from decimal import Decimal, getcontext, localcontext
from pathlib import Path

HERE = Path(__file__).parent
ROOT = HERE.parent.parent


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


LA = _load("make_la_fixtures", "fixtures/num/la/make_la_fixtures.py")
FN = _load("make_fn_fixtures", "fixtures/num/make_fn_fixtures.py")

dot = LA.dot
norm2 = LA.norm2
Overflow = LA.Overflow


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return f"{bits(x):016x}"


def finite(x):
    return not (math.isnan(x) or math.isinf(x))


# ---------------------------------------------------------------- fid-num function model


class EvalError(Exception):
    """The callback returned Err (a fid-num function rejected its argument or result)."""


def f_exp(x):
    y = FN.m_exp(x)
    if not finite(y):
        raise EvalError("f.exp")
    return y


def f_ln(x):
    if x <= 0:
        raise EvalError("f.ln")
    return FN.m_log(x)


def f_atan(x):
    return FN.m_atan(x)


def f_sqrt(x):
    if x < 0:
        raise EvalError("f.sqrt")
    return math.sqrt(x)


def sum_sq(xs):
    return LA.fixed_sum([x * x for x in xs])


# ---------------------------------------------------------------- driver model (NODE.md Design)

MAX_TRIALS = 64
ETA = 1e-4
LAMBDA_MIN = 1e-200
LAMBDA_MAX = 1e200

DEFAULT = dict(method="lm", max_iter=200, tol_grad=1e-10, tol_step=1e-12, tol_cost=1e-14,
               cond_limit=1e12, rank_tol=1e-12, lm_tau=1e-3)


class SolverError(Exception):
    def __init__(self, kind, **kw):
        super().__init__(kind)
        self.kind = kind
        self.kw = kw


def qr_solve(cols, rows, b):
    """Pivoted Householder QR of the column-major `cols` (n columns of length `rows`) applied
    to `b`. Returns (R row-major n*n, perm, qtb). `cols` and `b` are consumed."""
    n = len(cols)
    perm = list(range(n))
    for k in range(n):
        best = k
        bestnorm = norm2(cols[k][k:])
        for j in range(k + 1, n):
            c = norm2(cols[j][k:])
            if c > bestnorm:
                best, bestnorm = j, c
        if best != k:
            cols[k], cols[best] = cols[best], cols[k]
            perm[k], perm[best] = perm[best], perm[k]
        x = cols[k]
        alpha = x[k]
        xnorm = bestnorm
        if xnorm == 0:
            continue
        beta = -xnorm if alpha > 0 else xnorm
        u0 = alpha - beta
        v = [1.0] + [xi / u0 for xi in x[k + 1:]]
        tau = (beta - alpha) / beta
        x[k] = beta
        for i in range(k + 1, rows):
            x[i] = 0.0
        for j in range(k + 1, n):
            c = cols[j]
            s = tau * dot(v, c[k:])
            for i in range(k, rows):
                c[i] = c[i] - s * v[i - k]
        s = tau * dot(v, b[k:])
        for i in range(k, rows):
            b[i] = b[i] - s * v[i - k]
    r = [0.0] * (n * n)
    for i in range(n):
        for j in range(i, n):
            r[i * n + j] = cols[j][i]
    return r, perm, b[:n]


def back_solve(n, r, qtb, perm):
    y, bad = LA.back_sub(n, r, qtb)
    if y is None or LA.singular_scan(n, r, 0.0) is not None:
        raise SolverError("NonFinite", stage="step")
    z = [0.0] * n
    for i in range(n):
        z[perm[i]] = y[i]
    return z


def col(j_mat, m, n, c):
    return [j_mat[i * n + c] for i in range(m)]


def assess(m, n, jac, r, cfg):
    d = []
    for c in range(n):
        dc = norm2(col(jac, m, n, c))
        d.append(1.0 if dc == 0 else dc)
    js = [jac[i * n + c] / d[c] for i in range(m) for c in range(n)]
    cols = [col(js, m, n, c) for c in range(n)]
    rr, perm, qtb = qr_solve(cols, m, [-x for x in r])
    diag = [abs(rr[k * n + k]) for k in range(n)]
    mx = 0.0
    for v in diag:
        mx = v if v > mx else mx
    thr = cfg["rank_tol"] * mx
    rank = n
    for k in range(n):
        if diag[k] <= thr:
            rank = k
            break
    c = LA.m_cond((n, n, rr), 0.0)
    cond = c[1][0] if c[0] == "ok" else None
    gs = [dot(col(js, m, n, c_), r) for c_ in range(n)]
    gu = [dot(col(jac, m, n, c_), r) for c_ in range(n)]
    grad_norm = norm2(gu)
    rnorm = norm2(r)
    return dict(d=d, js=js, rr=rr, perm=perm, qtb=qtb, rank=rank, cond=cond, gs=gs,
                grad_norm=grad_norm, rnorm=rnorm)


def evaluate(prob, p):
    try:
        r, jac = prob.eval(p)
    except EvalError as e:
        raise SolverError("Eval", what=str(e))
    if not all(finite(x) for x in r):
        raise SolverError("NonFinite", stage="residual")
    if not all(finite(x) for x in jac):
        raise SolverError("NonFinite", stage="jacobian")
    return r, jac


def solve(prob, p0, cfg):
    """Returns dict(ok=True, p, cost, iterations, evaluations, reason, rank, cond, grad_norm)
    or dict(ok=False, kind, last) mirroring fid_solv::nlls::solve."""
    n, m = prob.n, prob.m
    st = dict(iterations=0, evaluations=0, cost=None, grad_norm=None, rank=None, cond=None,
              lam=None)

    def fail(kind, **kw):
        return dict(ok=False, kind=kind, last=dict(st), **kw)

    try:
        p = list(p0)
        r, jac = evaluate(prob, p)
        st["evaluations"] = 1
        try:
            cost = sum_sq(r)
        except Overflow:
            raise SolverError("NonFinite", stage="cost")
        st["cost"] = cost
        k = 0
        lam = None
        pending = None
        while True:
            a = assess(m, n, jac, r, cfg)
            st.update(grad_norm=a["grad_norm"], rank=a["rank"], cond=a["cond"])
            if a["rank"] < n:
                return fail("RankDeficient", rank=a["rank"], cond=a["cond"])
            if a["cond"] is None or a["cond"] > cfg["cond_limit"]:
                return fail("IllConditioned", cond=a["cond"])
            if pending is not None:
                return dict(ok=True, p=p, r=r, cost=cost, iterations=k, evaluations=st["evaluations"],
                            reason=pending, rank=a["rank"], cond=a["cond"],
                            grad_norm=a["grad_norm"], lam=lam)
            if cost == 0:
                pending = "zero_residual"
                continue
            gmax = 0.0
            for g in a["gs"]:
                gmax = abs(g) if abs(g) > gmax else gmax
            if gmax <= cfg["tol_grad"] * a["rnorm"]:
                pending = "gradient"
                continue
            if k >= cfg["max_iter"]:
                return fail("IterationLimit")
            d, js = a["d"], a["js"]
            if cfg["method"] == "gn":
                trials = [(0.0, back_solve(n, a["rr"], a["qtb"], a["perm"]))]
            else:
                if lam is None:
                    lam = cfg["lm_tau"]
                trials = None
            t = 0
            while True:
                if cfg["method"] == "gn":
                    if t == 1:
                        break
                    lam_t, z = trials[0]
                else:
                    if t == MAX_TRIALS:
                        break
                    lam_t = lam
                    sl = f_sqrt(lam)
                    cols = [col(js, m, n, c_) + [sl if i == c_ else 0.0 for i in range(n)]
                            for c_ in range(n)]
                    rr, perm, qtb = qr_solve(cols, m + n, [-x for x in r] + [0.0] * n)
                    z = back_solve(n, rr, qtb, perm)
                t += 1
                pt = []
                for j in range(n):
                    v = p[j] + z[j] / d[j]
                    if not finite(v):
                        raise SolverError("NonFinite", stage="step")
                    pt.append(v)
                rt, jt = evaluate(prob, pt)
                st["evaluations"] += 1
                try:
                    cost_t = sum_sq(rt)
                except Overflow:
                    raise SolverError("NonFinite", stage="cost")
                jz = [dot(js[i * n:(i + 1) * n], z) for i in range(m)]
                pred = sum_sq(jz) + (2.0 * lam_t) * sum_sq(z)
                act = cost - cost_t
                tc = cfg["tol_cost"] * cost
                if cfg["method"] == "gn":
                    accepted = True
                else:
                    accepted = False
                    if pred > 0:
                        rho = act / pred
                        accepted = rho > ETA
                        if rho > 0.75:
                            lam = lam / 3.0
                            if lam < LAMBDA_MIN:
                                lam = LAMBDA_MIN
                        elif rho < 0.25:
                            lam = lam * 2.0
                            if lam > LAMBDA_MAX:
                                lam = LAMBDA_MAX
                    st["lam"] = lam
                if accepted:
                    p, r, jac, cost = pt, rt, jt, cost_t
                    k += 1
                    st.update(iterations=k, cost=cost)
                    dpn = norm2([d[j] * p[j] for j in range(n)])
                    if norm2(z) <= cfg["tol_step"] * (dpn + cfg["tol_step"]):
                        pending = "step"
                if pending is None and pred <= tc and abs(act) <= tc:
                    pending = "cost"
                if accepted or pending is not None:
                    break
            if not accepted and pending is None:
                return fail("IterationLimit")
    except SolverError as e:
        return fail(e.kind, **e.kw)
    except Overflow:
        return fail("Numeric", what="overflow")


# ---------------------------------------------------------------- problems (f64 model)


class Problem:
    """Residual r_i = model(x_i, p) - y_i and its Jacobian, evaluated exactly as the Rust test
    problem does (same operations in the same order)."""

    def __init__(self, name, n, xs, ys):
        self.name, self.n, self.xs, self.ys = name, n, xs, ys
        self.m = len(xs)

    def eval(self, p):
        r, jac = [], []
        for x, y in zip(self.xs, self.ys):
            f, g = MODELS[self.name][0](x, p)
            r.append(f - y)
            jac.extend(g)
        return r, jac


INV_PI = 1.0 / math.pi


def misra1a(x, b):
    e = f_exp(-(b[1] * x))
    return b[0] * (1.0 - e), [1.0 - e, (b[0] * x) * e]


def chwirut(x, b):
    e = f_exp(-(b[0] * x))
    u = b[1] + b[2] * x
    f = e / u
    return f, [-(x * f), -(f / u), -((x * f) / u)]


def danwood(x, b):
    lx = f_ln(x)
    w = f_exp(b[1] * lx)
    return b[0] * w, [w, (b[0] * w) * lx]


def lanczos3(x, b):
    f = 0.0
    g = []
    for i in range(3):
        e = f_exp(-(b[2 * i + 1] * x))
        f = f + b[2 * i] * e
        g.extend([e, -((b[2 * i] * x) * e)])
    return f, g


def kirby2(x, b):
    xx = x * x
    num = (b[0] + b[1] * x) + b[2] * xx
    den = (1.0 + b[3] * x) + b[4] * xx
    f = num / den
    return f, [1.0 / den, x / den, xx / den, -((f * x) / den), -((f * xx) / den)]


def misra1d(x, b):
    u = 1.0 + b[1] * x
    q = x / u
    return (b[0] * b[1]) * q, [b[1] * q, (b[0] * q) / u]


def roszman1(x, b):
    u = x - b[3]
    t = b[2] / u
    w = 1.0 + t * t
    f = (b[0] - b[1] * x) - f_atan(t) * INV_PI
    return f, [1.0, -x, -((1.0 / u) / w) * INV_PI, -((t / u) / w) * INV_PI]


def mgh17(x, b):
    e4 = f_exp(-(x * b[3]))
    e5 = f_exp(-(x * b[4]))
    f = (b[0] + b[1] * e4) + b[2] * e5
    return f, [1.0, e4, e5, -((x * b[1]) * e4), -((x * b[2]) * e5)]


# Decimal versions for the reference solver (same mathematics, 60 digits).

def d_pi():
    getcontext().prec += 5
    def atan_inv(n):
        x = Decimal(1) / n
        x2 = x * x
        s, t, k = x, x, 1
        while True:
            t = -t * x2
            k += 2
            term = t / k
            if abs(term) < Decimal(10) ** (-(getcontext().prec + 2)):
                break
            s += term
        return s
    p = 4 * (4 * atan_inv(5) - atan_inv(239))
    getcontext().prec -= 5
    return +p


def d_atan(x):
    getcontext().prec += 5
    k = 0
    while abs(x) > Decimal("0.1"):
        x = x / (1 + (1 + x * x).sqrt())
        k += 1
    x2 = x * x
    s, t, i = x, x, 1
    while True:
        t = -t * x2
        i += 2
        term = t / i
        if abs(term) < Decimal(10) ** (-(getcontext().prec + 2)):
            break
        s += term
    getcontext().prec -= 5
    return +(s * (2 ** k))


def dm_misra1a(x, b):
    e = (-(b[1] * x)).exp()
    return b[0] * (1 - e), [1 - e, b[0] * x * e]


def dm_chwirut(x, b):
    e = (-(b[0] * x)).exp()
    u = b[1] + b[2] * x
    f = e / u
    return f, [-x * f, -f / u, -x * f / u]


def dm_danwood(x, b):
    lx = x.ln()
    w = (b[1] * lx).exp()
    return b[0] * w, [w, b[0] * w * lx]


def dm_lanczos3(x, b):
    f, g = Decimal(0), []
    for i in range(3):
        e = (-(b[2 * i + 1] * x)).exp()
        f += b[2 * i] * e
        g += [e, -b[2 * i] * x * e]
    return f, g


def dm_kirby2(x, b):
    num = b[0] + b[1] * x + b[2] * x * x
    den = 1 + b[3] * x + b[4] * x * x
    f = num / den
    return f, [1 / den, x / den, x * x / den, -f * x / den, -f * x * x / den]


def dm_misra1d(x, b):
    u = 1 + b[1] * x
    return b[0] * b[1] * x / u, [b[1] * x / u, b[0] * x / (u * u)]


def dm_roszman1(x, b):
    pi = d_pi()
    u = x - b[3]
    t = b[2] / u
    w = 1 + t * t
    return b[0] - b[1] * x - d_atan(t) / pi, [Decimal(1), -x, -1 / (u * w * pi), -t / (u * w * pi)]


def dm_mgh17(x, b):
    e4 = (-(x * b[3])).exp()
    e5 = (-(x * b[4])).exp()
    return b[0] + b[1] * e4 + b[2] * e5, [Decimal(1), e4, e5, -x * b[1] * e4, -x * b[2] * e5]


MODELS = {
    "misra1a": (misra1a, dm_misra1a),
    "chwirut": (chwirut, dm_chwirut),
    "danwood": (danwood, dm_danwood),
    "lanczos3": (lanczos3, dm_lanczos3),
    "kirby2": (kirby2, dm_kirby2),
    "misra1d": (misra1d, dm_misra1d),
    "roszman1": (roszman1, dm_roszman1),
    "mgh17": (mgh17, dm_mgh17),
}


def true_value(name, x, b):
    """y without noise, at 60 digits, rounded to double (data generation only)."""
    with localcontext() as ctx:
        ctx.prec = 60
        f, _ = MODELS[name][1](Decimal(x), [Decimal(v) for v in b])
        return float(f)


# ---------------------------------------------------------------- reference solver (Decimal)


def ref_solve(name, xs, ys, start):
    with localcontext() as ctx:
        ctx.prec = 60
        X = [Decimal(x) for x in xs]
        Y = [Decimal(y) for y in ys]
        b = [Decimal(v) for v in start]
        n = len(b)
        for it in range(200):
            jtj = [[Decimal(0)] * n for _ in range(n)]
            jtr = [Decimal(0)] * n
            for x, y in zip(X, Y):
                f, g = MODELS[name][1](x, b)
                r = f - y
                for i in range(n):
                    jtr[i] += g[i] * r
                    for j in range(n):
                        jtj[i][j] += g[i] * g[j]
            a = [row[:] + [-jtr[i]] for i, row in enumerate(jtj)]
            for c in range(n):
                piv = max(range(c, n), key=lambda i: abs(a[i][c]))
                a[c], a[piv] = a[piv], a[c]
                for i in range(c + 1, n):
                    fct = a[i][c] / a[c][c]
                    for j in range(c, n + 1):
                        a[i][j] -= fct * a[c][j]
            dx = [Decimal(0)] * n
            for i in range(n - 1, -1, -1):
                s = a[i][n] - sum(a[i][j] * dx[j] for j in range(i + 1, n))
                dx[i] = s / a[i][i]
            b = [bi + di for bi, di in zip(b, dx)]
            if all(abs(di) <= Decimal("1e-40") * abs(bi) for di, bi in zip(dx, b)):
                break
        else:
            raise RuntimeError(f"reference GN did not converge for {name}")
        rss = Decimal(0)
        for x, y in zip(X, Y):
            f, _ = MODELS[name][1](x, b)
            rss += (f - y) * (f - y)
        return [f"{v:.25e}" for v in b], f"{rss:.25e}"


# ---------------------------------------------------------------- seeded data


def splitmix64(state):
    state = (state + 0x9E3779B97F4A7C15) & ((1 << 64) - 1)
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & ((1 << 64) - 1)
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & ((1 << 64) - 1)
    return state, z ^ (z >> 31)


class Rng:
    def __init__(self, seed):
        self.s = seed

    def unit(self):
        """Uniform in [-1, 1)."""
        self.s, z = splitmix64(self.s)
        return 2.0 * ((z >> 11) * 2.0 ** -53) - 1.0


def grid(lo, hi, count):
    return [float(Decimal(lo) + (Decimal(hi) - Decimal(lo)) * i / (count - 1)) for i in range(count)]


# name, x grid, true parameters, noise amplitude, start 1, start 2, seed, NIST difficulty
SPECS = [
    ("misra1a", grid("77.6", "790.32", 14), [240.0, 5.5e-4], 0.1,
     [500.0, 1e-4], [250.0, 5e-4], 101, "lower"),
    ("chwirut", grid("0.5", "6.0", 54), [0.19, 0.0061, 0.0105], 1.0,
     [0.1, 0.01, 0.02], [0.15, 0.008, 0.010], 102, "lower"),
    ("danwood", grid("1.309", "2.274", 6), [0.769, 3.86], 0.005,
     [1.0, 5.0], [0.7, 4.0], 103, "lower"),
    ("lanczos3", grid("0", "1.15", 24), [0.0951, 1.0, 0.8607, 3.0, 1.5576, 5.0], 1e-5,
     [1.2, 0.3, 5.6, 5.5, 6.5, 7.6], [0.5, 0.7, 3.6, 4.2, 4.0, 6.3], 104, "lower"),
    ("kirby2", grid("9.65", "78.08", 40), [1.6745, -0.13927, 0.0025961, -0.0017241, 2.1664e-5], 0.01,
     [2.0, -0.1, 0.003, -0.001, 1e-5], [1.5, -0.15, 0.0025, -0.0015, 2e-5], 105, "average"),
    ("misra1d", grid("77.6", "790.32", 14), [437.37, 3.0228e-4], 0.05,
     [500.0, 1e-4], [450.0, 3e-4], 106, "average"),
    ("roszman1", grid("-4868.68", "-464.0", 25), [0.20197, -6.1954e-6, 1204.46, -181.34], 1e-4,
     [0.1, -1e-5, 1000.0, -100.0], [0.2, -5e-6, 1200.0, -150.0], 107, "average"),
    ("mgh17", grid("0", "320", 33), [0.3754, 1.9358, -1.4647, 0.01287, 0.02212], 1e-3,
     [0.5, 1.5, -1.0, 0.01, 0.02], [0.4, 1.9, -1.4, 0.013, 0.022], 108, "average"),
]


def make_problem(spec):
    name, xs, truth, noise, s1, s2, seed, diff = spec
    rng = Rng(seed)
    ys = [true_value(name, x, truth) + noise * rng.unit() for x in xs]
    return Problem(name, len(truth), xs, ys), s1, s2, diff


def digest(rows):
    h = hashlib.sha256()
    for row in rows:
        for v in row:
            h.update(struct.pack("<d", v))
    return "sha256:" + h.hexdigest()


def run_line(tag, res):
    if not res["ok"]:
        raise RuntimeError(f"{tag}: {res}")
    return (f"{tag} p {' '.join(hx(v) for v in res['p'])} cost {hx(res['cost'])} "
            f"iterations {res['iterations']} evaluations {res['evaluations']} "
            f"reason {res['reason']} rank {res['rank']}")


def build_strd():
    out = ["# KRN.SOLV.NLLS AC-1 / AC-3: seeded problems on NIST StRD nonlinear model forms.",
           "# Generated by fixtures/solv/make_nlls_fixtures.py (CC-BY-4.0). Do not edit.",
           "# Floats are IEEE-754 binary64 bit patterns in hex; ref values are 25-digit decimals.",
           "config " + " ".join(f"{k} {v}" for k, v in DEFAULT.items())]
    rows = []
    for spec in SPECS:
        prob, s1, s2, diff = make_problem(spec)
        ref, rss = ref_solve(prob.name, prob.xs, prob.ys, spec[2])
        out.append(f"problem {prob.name} difficulty {diff} n {prob.n} m {prob.m}")
        out.append("x " + " ".join(hx(v) for v in prob.xs))
        out.append("y " + " ".join(hx(v) for v in prob.ys))
        out.append("ref " + " ".join(ref))
        out.append("ref_rss " + rss)
        for tag, s in (("start1", s1), ("start2", s2)):
            out.append(f"{tag} " + " ".join(hx(v) for v in s))
            res = solve(prob, s, DEFAULT)
            out.append(run_line(f"lm_{tag}", res))
            lre = min(-math.log10(abs((v - float(rv)) / float(rv)) or 1e-17) for v, rv in zip(res["p"], ref))
            print(f"{prob.name} {tag}: iters {res['iterations']} evals {res['evaluations']} "
                  f"reason {res['reason']} cond {res['cond']:.3e} min LRE {lre:.2f}", file=sys.stderr)
            rows.append(res["p"] + [res["cost"], float(res["iterations"]), float(res["evaluations"])])
        if prob.name == "misra1a":
            one = dict(DEFAULT, max_iter=1)
            res = solve(prob, s1, one)
            assert not res["ok"] and res["kind"] == "IterationLimit", res
            out.append(f"iterlimit_start1 cost {hx(res['last']['cost'])} evaluations {res['last']['evaluations']}")
    out.insert(4, "digest " + digest(rows))  # preamble, before the first problem block
    return "\n".join(out) + "\n"


class Circle:
    def __init__(self, xs, ys):
        self.xs, self.ys = xs, ys
        self.n, self.m = 3, len(xs)

    def eval(self, p):
        r, jac = [], []
        for x, y in zip(self.xs, self.ys):
            dx = x - p[0]
            dy = y - p[1]
            d = f_sqrt(dx * dx + dy * dy)
            r.append(d - p[2])
            jac.extend([-(dx / d), -(dy / d), -1.0])
        return r, jac


def circle_ref(xs, ys, start):
    with localcontext() as ctx:
        ctx.prec = 60
        X = [Decimal(x) for x in xs]
        Y = [Decimal(y) for y in ys]
        b = [Decimal(v) for v in start]
        for it in range(200):
            jtj = [[Decimal(0)] * 3 for _ in range(3)]
            jtr = [Decimal(0)] * 3
            for x, y in zip(X, Y):
                dx, dy = x - b[0], y - b[1]
                d = (dx * dx + dy * dy).sqrt()
                r = d - b[2]
                g = [-dx / d, -dy / d, Decimal(-1)]
                for i in range(3):
                    jtr[i] += g[i] * r
                    for j in range(3):
                        jtj[i][j] += g[i] * g[j]
            a = [row[:] + [-jtr[i]] for i, row in enumerate(jtj)]
            for c in range(3):
                piv = max(range(c, 3), key=lambda i: abs(a[i][c]))
                a[c], a[piv] = a[piv], a[c]
                for i in range(c + 1, 3):
                    fct = a[i][c] / a[c][c]
                    for j in range(c, 4):
                        a[i][j] -= fct * a[c][j]
            dx_ = [Decimal(0)] * 3
            for i in range(2, -1, -1):
                s = a[i][3] - sum(a[i][j] * dx_[j] for j in range(i + 1, 3))
                dx_[i] = s / a[i][i]
            b = [bi + di for bi, di in zip(b, dx_)]
            if all(abs(di) <= Decimal("1e-45") for di in dx_):
                break
        else:
            raise RuntimeError("circle reference did not converge")
        return [f"{v:.25e}" for v in b]


def build_circle():
    rng = Rng(0xC1C1E)
    a, b, rad = 0.012, -0.034, 0.05
    xs, ys = [], []
    for i in range(50):
        th = 2.0 * math.pi * (i + 0.3 * rng.unit()) / 50.0
        xs.append(a + rad * math.cos(th) + 1e-6 * rng.unit())
        ys.append(b + rad * math.sin(th) + 1e-6 * rng.unit())
    prob = Circle(xs, ys)
    start = [0.0, 0.0, 0.04]
    ref = circle_ref(xs, ys, [a, b, rad])
    out = ["# KRN.SOLV.NLLS AC-2: seeded 2D circle, 50 points, noise 1e-6 m (units: metres).",
           "# Generated by fixtures/solv/make_nlls_fixtures.py (CC-BY-4.0). Do not edit.",
           "x " + " ".join(hx(v) for v in xs),
           "y " + " ".join(hx(v) for v in ys),
           "start " + " ".join(hx(v) for v in start),
           "ref " + " ".join(ref)]
    rows = []
    for method in ("gn", "lm"):
        res = solve(prob, start, dict(DEFAULT, method=method))
        out.append(run_line(method, res))
        err = max(abs(v - float(r)) for v, r in zip(res["p"], ref))
        print(f"circle {method}: iters {res['iterations']} reason {res['reason']} "
              f"max |p - ref| {err:.3e} m", file=sys.stderr)
        rows.append(res["p"] + [res["cost"], float(res["iterations"]), float(res["evaluations"])])
    out.append("digest " + digest(rows))
    return "\n".join(out) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    files = {HERE / "strd-models-synth-v1.txt": build_strd(), HERE / "circle-synth-v1.txt": build_circle()}
    bad = False
    for path, text in files.items():
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"stale: {path}", file=sys.stderr)
                bad = True
        else:
            path.write_bytes(text.encode("utf-8"))
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
