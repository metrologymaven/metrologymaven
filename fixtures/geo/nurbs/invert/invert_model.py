"""Bit-exact binary64 model of GEO.NURBS.INVERT (spec/geo/nurbs/invert/NODE.md, Design).

Stdlib only; shares no code with any Rust crate and never runs the Rust code. Position and
derivative evaluation is the bit-exact GEO.NURBS.EVAL model in
fixtures/geo/nurbs/eval/make_eval_fixtures.py (imported, not copied). CPython floats are IEEE 754
binary64 with round-to-nearest-even, never fuse a multiply-add, and math.sqrt is correctly
rounded, so every bit pattern computed here is the Rust result on every target as long as the
order of operations below is the one pinned in NODE.md. Comparisons only (no min/max on floats
with NaN semantics), fixed loop orders, a binary heap keyed by (lower bound, sequence number).
"""

import heapq
import importlib.util
import math
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("nurbs_eval_model", HERE.parent / "eval" / "make_eval_fixtures.py")
EM = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(EM)

# ---------------------------------------------------------------- pinned constants (NODE.md Design)
MAX_ITER = 50            # REQ-003: Newton iterations per run
MAX_HALVINGS = 30        # step halvings per Newton iteration
TIE = 1e-15              # REQ-004, metres
REL = 1e-12              # REQ-002 relative orthogonality
FLOOR = 1e-9             # REQ-002 distance floor, metres
NOISE = 2.0 ** -46       # evaluation-noise floor, times the coordinate scale M
SING2 = 2.0 ** -52       # singular direction: |S_x|^2 W_x^2 <= SING2 * L2
GAMMA = 2.0 ** -36       # Bernstein sign margin, times the magnitude polynomial
PRUNE_REL = 2.0 ** -48   # box lower bound safety, times M
DEGEN_REL = 2.0 ** -40   # degenerate (pole) line, times the largest box extent
FOOT_SEP = 1e-9          # distinct feet, metres (infinity norm)
MAX_DEPTH = 60           # binary subdivisions below a Bezier piece
BUDGET = 1024            # subdivisions per inversion
MAX_BISECT = 80          # bisection steps in a unique one-parameter node
MAX_BINOM = 75           # 3 * MAX_DEGREE

BINOM = [[1.0]]
for _n in range(1, MAX_BINOM + 1):
    _prev = BINOM[-1]
    _row = [1.0] * (_n + 1)
    for _k in range(1, _n):
        _row[_k] = _prev[_k - 1] + _prev[_k]
    BINOM.append(_row)


class NonFiniteError(Exception):
    pass


def sqrt(x):
    return math.sqrt(x)


def dot(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def sub3(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def lerp(a, b, f):
    if f == 0.0:
        return a
    if f == 1.0:
        return b
    x = a + f * (b - a)
    if x < a:
        return a
    if x > b:
        return b
    return x


def fmax(a, b):
    return b if b > a else a


# ---------------------------------------------------------------- Bezier extraction (knot insertion)

def insert_all(U, p, rows):
    """Insert every distinct knot value of the domain [U[p], U[n]] until its multiplicity is at
    least p (ascending values, one Boehm insertion at a time). `rows` is a list of control
    polygons (lists of 4-vectors) sharing the knot vector; returns (U', rows')."""
    U = list(U)
    rows = [[list(c) for c in r] for r in rows]
    n = len(U) - p - 1
    a, b = U[p], U[n]
    values = []
    for x in U:
        if a <= x <= b and (not values or values[-1] != x):
            values.append(x)
    for x in values:
        while True:
            m = sum(1 for y in U if y == x)
            if m >= p:
                break
            k = max(i for i in range(len(U)) if U[i] <= x)
            new_rows = []
            for r in rows:
                q = []
                for i in range(k - p + 1, k - m + 1):
                    alpha = (x - U[i]) / (U[i + p] - U[i])
                    oma = 1.0 - alpha
                    q.append([alpha * r[i][c] + oma * r[i - 1][c] for c in range(4)])
                new_rows.append(r[:k - p + 1] + q + r[k - m:])
            rows = new_rows
            U = U[:k + 1] + [x] + U[k + 1:]
    return U, rows, values


def segments_of(U, p, values):
    """[(first control index, x0, x1)] for every non-empty span of the domain."""
    out = []
    for i in range(len(values) - 1):
        x0, x1 = values[i], values[i + 1]
        s = max(k for k in range(len(U)) if U[k] == x0)
        out.append((s - p, x0, x1))
    return out


def homogeneous(ctrl, weights):
    if weights is None:
        return [[c[0], c[1], c[2], 1.0] for c in ctrl]
    return [[c[0] * w, c[1] * w, c[2] * w, w] for c, w in zip(ctrl, weights)]


# ---------------------------------------------------------------- Bernstein arrays (rows x cols)

def weight(m, n, i, j):
    return (BINOM[m][i] * BINOM[n][j]) / BINOM[m + n][i + j]


def prod2(a, b):
    m1, n1 = len(a) - 1, len(a[0]) - 1
    m2, n2 = len(b) - 1, len(b[0]) - 1
    out = [[0.0] * (n1 + n2 + 1) for _ in range(m1 + m2 + 1)]
    for k in range(m1 + m2 + 1):
        for l in range(n1 + n2 + 1):
            acc = 0.0
            for i in range(max(0, k - m2), min(m1, k) + 1):
                wu = weight(m1, m2, i, k - i)
                for j in range(max(0, l - n2), min(n1, l) + 1):
                    acc = acc + ((wu * weight(n1, n2, j, l - j)) * a[i][j]) * b[k - i][l - j]
            out[k][l] = acc
    return out


def ew(f, a, b):
    return [[f(x, y) for x, y in zip(ra, rb)] for ra, rb in zip(a, b)]


def absa(a):
    return [[abs(x) for x in r] for r in a]


def diff_s(a):
    return [[a[i + 1][j] - a[i][j] for j in range(len(a[0]))] for i in range(len(a) - 1)]


def diff_t(a):
    return [[a[i][j + 1] - a[i][j] for j in range(len(a[0]) - 1)] for i in range(len(a))]


def mid(x, y):
    if isinstance(x, list):
        return [(a + b) * 0.5 for a, b in zip(x, y)]
    return (x + y) * 0.5


def split_s(a):
    m = len(a) - 1
    work = [list(r) for r in a]
    left = [None] * (m + 1)
    right = [None] * (m + 1)
    left[0] = list(work[0])
    right[m] = list(work[m])
    for r in range(1, m + 1):
        for i in range(m - r + 1):
            work[i] = [mid(x, y) for x, y in zip(work[i], work[i + 1])]
        left[r] = list(work[0])
        right[m - r] = list(work[m - r])
    return left, right


def transpose(a):
    return [list(c) for c in zip(*a)]


def split_t(a):
    l, r = split_s(transpose(a))
    return transpose(l), transpose(r)


def net_comp(net, c):
    return [[pt[c] for pt in row] for row in net]


def g_poly(net, q, axis):
    """G (and its magnitude bound Gm) for axis 's' or 't' of a homogeneous net (rows x cols of
    4-vectors): G = sum_c (dA_c (x) w - A_c (x) dw) (x) (A_c - q_c w)."""
    w = net_comp(net, 3)
    diff = diff_s if axis == "s" else diff_t
    dw = diff(w)
    adw = absa(dw)
    G = Gm = None
    for c in range(3):
        A = net_comp(net, c)
        dA = diff(A)
        N = ew(lambda x, y: x - y, prod2(dA, w), prod2(A, dw))
        Nm = ew(lambda x, y: x + y, prod2(absa(dA), w), prod2(absa(A), adw))
        qc, aq = q[c], abs(q[c])
        R = [[x - qc * y for x, y in zip(ra, rw)] for ra, rw in zip(A, w)]
        Rm = [[abs(x) + aq * y for x, y in zip(ra, rw)] for ra, rw in zip(A, w)]
        P, Pm = prod2(N, R), prod2(Nm, Rm)
        if G is None:
            G, Gm = P, Pm
        else:
            G = ew(lambda x, y: x + y, G, P)
            Gm = ew(lambda x, y: x + y, Gm, Pm)
    for r in G + Gm:
        for x in r:
            if not math.isfinite(x):
                raise NonFiniteError()
    return G, Gm


def signed(G, Gm):
    pos = neg = True
    for r, rm in zip(G, Gm):
        for g, gm in zip(r, rm):
            t = GAMMA * gm
            if g <= t:
                pos = False
            if g >= -t:
                neg = False
    return pos or neg


def monotone_1d(G, Gm):
    """+1 strictly increasing, -1 strictly decreasing, 0 neither (G has one column)."""
    inc = dec = True
    for k in range(len(G) - 1):
        d = G[k + 1][0] - G[k][0]
        t = GAMMA * (Gm[k + 1][0] + Gm[k][0])
        if d <= t:
            inc = False
        if d >= -t:
            dec = False
    if inc:
        return 1
    if dec:
        return -1
    return 0


def unique_2d(Gu, Gum, Gv, Gvm):
    m1, n1 = len(Gu) - 1, len(Gu[0]) - 1
    m2, n2 = len(Gv) - 1, len(Gv[0]) - 1
    a = math.inf
    for k in range(m1):
        for l in range(n1 + 1):
            x = (Gu[k + 1][l] - Gu[k][l]) - GAMMA * (Gum[k + 1][l] + Gum[k][l])
            if x < a:
                a = x
    a = float(m1) * a
    d = math.inf
    for k in range(m2 + 1):
        for l in range(n2):
            x = (Gv[k][l + 1] - Gv[k][l]) - GAMMA * (Gvm[k][l + 1] + Gvm[k][l])
            if x < d:
                d = x
    d = float(n2) * d
    b1 = 0.0
    for k in range(m1 + 1):
        for l in range(n1):
            x = abs(Gu[k][l + 1] - Gu[k][l]) + GAMMA * (Gum[k][l + 1] + Gum[k][l])
            if x > b1:
                b1 = x
    b1 = float(n1) * b1
    b2 = 0.0
    for k in range(m2):
        for l in range(n2 + 1):
            x = abs(Gv[k + 1][l] - Gv[k][l]) + GAMMA * (Gvm[k + 1][l] + Gvm[k][l])
            if x > b2:
                b2 = x
    b2 = float(m2) * b2
    b = (b1 + b2) * 0.5
    return a > 0.0 and d > 0.0 and a * d > b * b


# ---------------------------------------------------------------- nodes

def projected(net):
    return [[[pt[0] / pt[3], pt[1] / pt[3], pt[2] / pt[3]] for pt in row] for row in net]


def box_lb_seed(P, q):
    lo = list(P[0][0])
    hi = list(P[0][0])
    best = None
    bi = bj = 0
    for i, row in enumerate(P):
        for j, x in enumerate(row):
            for c in range(3):
                if x[c] < lo[c]:
                    lo[c] = x[c]
                if x[c] > hi[c]:
                    hi[c] = x[c]
            r = sub3(x, q)
            d2 = dot(r, r)
            if best is None or d2 < best:
                best, bi, bj = d2, i, j
    dl = [0.0, 0.0, 0.0]
    for c in range(3):
        if q[c] < lo[c]:
            dl[c] = lo[c] - q[c]
        elif q[c] > hi[c]:
            dl[c] = q[c] - hi[c]
    lb = sqrt(dot(dl, dl))
    if not math.isfinite(lb):
        raise NonFiniteError()
    return lb, bi, bj


class Node:
    __slots__ = ("net", "root", "s0", "s1", "t0", "t1", "depth", "G", "lb", "si", "sj")


class Ctx:
    def __init__(self, q, M, L2):
        self.q = q
        self.M = M
        self.L2 = L2
        self.best = math.inf
        self.cands = []
        self.splits = 0
        self.unresolved = []  # (lb, projected net)
        self.seq = 0

    def add(self, params, foot, f, pole=False):
        d = sqrt(f)
        self.cands.append((params, foot, d, pole))
        if d < self.best:
            self.best = d

    def add_point(self, params, foot):
        r = sub3(foot, self.q)
        self.add(params, foot, dot(r, r))

    def bound(self, foot):
        """A surface point that only lowers the pruning bound."""
        r = sub3(foot, self.q)
        d = sqrt(dot(r, r))
        if d < self.best:
            self.best = d


def scale(ctrl, q):
    M = 0.0
    lo = list(ctrl[0])
    hi = list(ctrl[0])
    for c in ctrl:
        for k in range(3):
            if abs(c[k]) > M:
                M = abs(c[k])
            if c[k] < lo[k]:
                lo[k] = c[k]
            if c[k] > hi[k]:
                hi[k] = c[k]
    for k in range(3):
        if abs(q[k]) > M:
            M = abs(q[k])
    ext = [hi[k] - lo[k] for k in range(3)]
    L2 = dot(ext, ext)
    X = 0.0
    for k in range(3):
        if ext[k] > X:
            X = ext[k]
    return M, L2, X


# ---------------------------------------------------------------- Newton (box constrained)

def ortho_tol2(e, dist, M):
    inner = REL * fmax(dist, FLOOR)
    T = fmax(inner, NOISE * M)
    return e * (T * T)


def slack(dist, M):
    """Rounding noise of f = |S - q|^2 near a foot: (NOISE M) (2 dist + NOISE M)."""
    n = NOISE * M
    return n * (2.0 * dist + n)


def dir_ok(x, lo, hi, g, tol2):
    if x == lo and g >= 0.0:
        return True
    if x == hi and g <= 0.0:
        return True
    return g * g <= tol2


def clamp(x, lo, hi):
    if x < lo:
        return lo
    if x > hi:
        return hi
    return x




# ---------------------------------------------------------------- Bezier evaluation (segment local)

def casteljau_levels(pts, s, nd):
    """De Casteljau on 4-vectors; returns (A0, A1, A2) of the polynomial in s (derivatives in s,
    A2 only when nd == 2; zero vectors when the degree is too low)."""
    p = len(pts) - 1
    oms = 1.0 - s
    c = [list(x) for x in pts]
    keep1 = keep2 = None
    for r in range(1, p + 1):
        if r == p - 1:
            keep2 = [list(x) for x in c[:3]]
        if r == p:
            keep1 = [list(x) for x in c[:2]]
        c = [[oms * a + s * b for a, b in zip(c[i], c[i + 1])] for i in range(p - r + 1)]
    if p == 1:
        keep1 = [list(x) for x in pts]
    pf = float(p)
    A0 = c[0]
    A1 = [(b - a) * pf for a, b in zip(keep1[0], keep1[1])]
    if nd >= 2 and p >= 2:
        k = pf * (pf - 1.0)
        A2 = [((a - 2.0 * b) + d) * k for a, b, d in zip(keep2[0], keep2[1], keep2[2])]
    else:
        A2 = [0.0, 0.0, 0.0, 0.0]
    return A0, A1, A2


def quotient1(A0, A1, A2):
    w0, w1, w2 = A0[3], A1[3], A2[3]
    C0 = [A0[c] / w0 for c in range(3)]
    C1 = [(A1[c] - w1 * C0[c]) / w0 for c in range(3)]
    C2 = [((A2[c] - (2.0 * w1) * C1[c]) - w2 * C0[c]) / w0 for c in range(3)]
    return C0, C1, C2


def bez1(net, s):
    """Position, first and second derivative in s of a one-column net."""
    A0, A1, A2 = casteljau_levels([row[0] for row in net], s, 2)
    return quotient1(A0, A1, A2)


def bez1_pos(net, s):
    A0, _, _ = casteljau_levels([row[0] for row in net], s, 0)
    w0 = A0[3]
    return [A0[c] / w0 for c in range(3)]


def bez2(net, s, t):
    """(S, S_s, S_t, S_ss, S_st, S_tt) of a tensor net (rows along s, columns along t)."""
    T0, T1, T2 = [], [], []
    for row in net:
        a0, a1, a2 = casteljau_levels(row, t, 2)
        T0.append(a0)
        T1.append(a1)
        T2.append(a2)
    A00, A10, A20 = casteljau_levels(T0, s, 2)
    A01, A11, _ = casteljau_levels(T1, s, 1)
    A02, _, _ = casteljau_levels(T2, s, 0)
    w00 = A00[3]
    S00 = [A00[c] / w00 for c in range(3)]
    w10, w01 = A10[3], A01[3]
    S10 = [(A10[c] - w10 * S00[c]) / w00 for c in range(3)]
    S01 = [(A01[c] - w01 * S00[c]) / w00 for c in range(3)]
    w20, w11, w02 = A20[3], A11[3], A02[3]
    S20 = [((A20[c] - (2.0 * w10) * S10[c]) - w20 * S00[c]) / w00 for c in range(3)]
    S11 = [(((A11[c] - w10 * S01[c]) - w01 * S10[c]) - w11 * S00[c]) / w00 for c in range(3)]
    S02 = [((A02[c] - (2.0 * w01) * S01[c]) - w02 * S00[c]) / w00 for c in range(3)]
    return S00, S10, S01, S20, S11, S02


def bez2_pos(net, s, t):
    T0 = [casteljau_levels(row, t, 0)[0] for row in net]
    A00 = casteljau_levels(T0, s, 0)[0]
    w00 = A00[3]
    return [A00[c] / w00 for c in range(3)]


# ---------------------------------------------------------------- Newton (box constrained, in s/t)

def newton1(net, lo, hi, x0, ctx):
    """Returns (s, certified)."""
    q, M = ctx.q, ctx.M
    x = x0
    it = 0
    polished = False
    while True:
        p, d1, d2 = bez1(net, x)
        r = sub3(p, q)
        f = dot(r, r)
        g = dot(d1, r)
        e = dot(d1, d1)
        dist = sqrt(f)
        ok = dir_ok(x, lo, hi, g, ortho_tol2(e, dist, M))
        if ok:
            if polished:
                return x, True
            polished = True
        if it == MAX_ITER:
            return x, ok
        it += 1
        h = e + dot(d2, r)
        if h > 0.0:
            step = -g / h
        elif e > 0.0:
            step = -g / e
        else:
            return x, ok
        lam = 1.0
        moved = False
        fcap = f + slack(dist, M)
        for _ in range(MAX_HALVINGS + 1):
            xt = clamp(x + lam * step, lo, hi)
            if xt == x:
                return x, ok
            rt = sub3(bez1_pos(net, xt), q)
            if dot(rt, rt) <= fcap:
                moved = True
                break
            lam = lam * 0.5
        if not moved:
            return x, ok
        x = xt


def newton2(net, lo, hi, x0, ctx):
    """Returns ((s, t), certified)."""
    q, M = ctx.q, ctx.M
    u, v = x0
    it = 0
    polished = False
    while True:
        p, su, sv, suu, suv, svv = bez2(net, u, v)
        r = sub3(p, q)
        f = dot(r, r)
        gu = dot(su, r)
        gv = dot(sv, r)
        E = dot(su, su)
        F = dot(su, sv)
        G = dot(sv, sv)
        dist = sqrt(f)
        oku = dir_ok(u, lo[0], hi[0], gu, ortho_tol2(E, dist, M))
        okv = dir_ok(v, lo[1], hi[1], gv, ortho_tol2(G, dist, M))
        ok = oku and okv
        if ok:
            if polished:
                return (u, v), True
            polished = True
        if it == MAX_ITER:
            return (u, v), ok
        it += 1
        huu = E + dot(suu, r)
        huv = F + dot(suv, r)
        hvv = G + dot(svv, r)
        au = (u == lo[0] and gu > 0.0) or (u == hi[0] and gu < 0.0)
        av = (v == lo[1] and gv > 0.0) or (v == hi[1] and gv < 0.0)
        if not au and not av:
            det = huu * hvv - huv * huv
            detI = E * G - F * F
            if huu > 0.0 and det > 0.0:
                du = (huv * gv - hvv * gu) / det
                dv = (huv * gu - huu * gv) / det
            elif E > 0.0 and detI > 0.0:
                du = (F * gv - G * gu) / detI
                dv = (F * gu - E * gv) / detI
            else:
                s = E + G
                if s > 0.0:
                    du = -gu / s
                    dv = -gv / s
                else:
                    return (u, v), ok
        elif not au:
            dv = 0.0
            if huu > 0.0:
                du = -gu / huu
            elif E > 0.0:
                du = -gu / E
            else:
                return (u, v), ok
        elif not av:
            du = 0.0
            if hvv > 0.0:
                dv = -gv / hvv
            elif G > 0.0:
                dv = -gv / G
            else:
                return (u, v), ok
        else:
            return (u, v), ok
        lam = 1.0
        moved = False
        fcap = f + slack(dist, M)
        for _ in range(MAX_HALVINGS + 1):
            ut = clamp(u + lam * du, lo[0], hi[0])
            vt = clamp(v + lam * dv, lo[1], hi[1])
            if ut == u and vt == v:
                return (u, v), ok
            rt = sub3(bez2_pos(net, ut, vt), q)
            if dot(rt, rt) <= fcap:
                moved = True
                break
            lam = lam * 0.5
        if not moved:
            return (u, v), ok
        u, v = ut, vt


# ---------------------------------------------------------------- branch and bound

def make_node(net, root, s0, s1, t0, t1, depth, G, ctx):
    n = Node()
    n.net, n.root, n.s0, n.s1, n.t0, n.t1, n.depth, n.G = net, root, s0, s1, t0, t1, depth, G
    n.lb, n.si, n.sj = box_lb_seed(projected(net), ctx.q)
    return n


def push(heap, nd, ctx):
    """Heap key: lower bound, then depth (wider nodes first), then sequence number."""
    heapq.heappush(heap, (nd.lb, nd.depth, ctx.seq, nd))
    ctx.seq += 1


def g1(net, s, q):
    p, d1, _ = bez1(net, s)
    return dot(d1, sub3(p, q))


def bisect1(net, lo, hi, q):
    """Root of the increasing g = C'(s) . (C(s) - q) in [lo, hi] by bisection on its sign
    (unique 1-D nodes where Newton did not converge): at most MAX_BISECT halvings; the end
    whose |g| is smaller (lo on equality)."""
    glo, ghi = g1(net, lo, q), g1(net, hi, q)
    if glo >= 0.0:
        return lo
    if ghi <= 0.0:
        return hi
    for _ in range(MAX_BISECT):
        mid = (lo + hi) * 0.5
        if mid == lo or mid == hi:
            break
        gm = g1(net, mid, q)
        if gm > 0.0:
            hi, ghi = mid, gm
        else:
            lo, glo = mid, gm
    return lo if -glo <= ghi else hi


def neg_diag(Gu, Gum, Gv, Gvm):
    """dG_s/ds < 0 or dG_t/dt < 0 on the whole node (every coefficient difference below minus
    its margin): any stationary point there has a negative Hessian diagonal, not a minimum."""
    nu = nv = True
    for k in range(len(Gu) - 1):
        for l in range(len(Gu[0])):
            if (Gu[k + 1][l] - Gu[k][l]) >= -(GAMMA * (Gum[k + 1][l] + Gum[k][l])):
                nu = False
    for k in range(len(Gv)):
        for l in range(len(Gv[0]) - 1):
            if (Gv[k][l + 1] - Gv[k][l]) >= -(GAMMA * (Gvm[k][l + 1] + Gvm[k][l])):
                nv = False
    return nu or nv


def prebound(roots, dims, ctx, point):
    """Upper bound before any search: Newton over each Bezier piece (in order) from its closest
    control point; the evaluated point only lowers the pruning bound, it is never an answer."""
    for net, (x0, x1), (y0, y1) in roots:
        _, si, sj = box_lb_seed(projected(net), ctx.q)
        ss = float(si) / float(len(net) - 1)
        if dims == 1:
            x, _ = newton1(net, 0.0, 1.0, ss, ctx)
            ctx.bound(point(lerp(x0, x1, x)))
        else:
            st = float(sj) / float(len(net[0]) - 1)
            (xs, xt), _ = newton2(net, (0.0, 0.0), (1.0, 1.0), (ss, st), ctx)
            ctx.bound(point((lerp(x0, x1, xs), lerp(y0, y1, xt))))


def bnb(roots, dims, ctx, point, lift):
    """roots: [(net, (x0, x1), (y0, y1))] Bezier pieces in order. point(params) -> foot (the
    GEO.NURBS.EVAL position); candidates are added to ctx."""
    heap = []
    for k, (net, _, _) in enumerate(roots):
        push(heap, make_node(net, k, 0.0, 1.0, 0.0, 1.0, 0, None, ctx), ctx)
    q = ctx.q
    while heap:
        lb, _, _, nd = heapq.heappop(heap)
        if lb - PRUNE_REL * ctx.M > ctx.best + TIE:
            break
        rnet, (x0, x1), (y0, y1) = roots[nd.root]

        def at(ss, st):
            return lift(lerp(x0, x1, ss)) if dims == 1 else (lerp(x0, x1, ss), lerp(y0, y1, st))

        if dims == 1:
            G, Gm = g_poly(nd.net, q, "s")
            if signed(G, Gm):
                continue
            mono = monotone_1d(G, Gm)
            if mono == -1:
                continue
            unique = mono == 1
        else:
            Gu, Gum = g_poly(nd.net, q, "s")
            Gv, Gvm = g_poly(nd.net, q, "t")
            if signed(Gu, Gum) or signed(Gv, Gvm) or neg_diag(Gu, Gum, Gv, Gvm):
                continue
            unique = unique_2d(Gu, Gum, Gv, Gvm)
        ss = lerp(nd.s0, nd.s1, float(nd.si) / float(len(nd.net) - 1))
        st = lerp(nd.t0, nd.t1, float(nd.sj) / float(len(nd.net[0]) - 1)) if dims == 2 else 0.0
        reason = "B"
        if unique:
            if dims == 1:
                x, ok = newton1(rnet, nd.s0, nd.s1, ss, ctx)
                if not ok:
                    x = bisect1(rnet, nd.s0, nd.s1, q)
                params = at(x, 0.0)
                ctx.add_point(params, point(params))
                continue
            (xs, xt), ok = newton2(rnet, (nd.s0, nd.t0), (nd.s1, nd.t1), (ss, st), ctx)
            if ok:
                params = at(xs, xt)
                ctx.add_point(params, point(params))
                continue
            reason = "I"
        if nd.depth < MAX_DEPTH and ctx.splits < BUDGET:
            ctx.splits += 1
            push_children(heap, nd, dims, ctx)
            continue
        # unresolved: its seed point bounds the search, it is never an answer
        ctx.unresolved.append((nd.lb, reason))
        ctx.bound(point(at(ss, st)))


def split_dir_t(net):
    """True when the projected net extends further along t than along s (infinity norm of the
    largest control-point difference), so the node is halved along t; ties split along s."""
    P = projected(net)
    ds = dt = 0.0
    for i in range(len(P)):
        for j in range(len(P[0])):
            for c in range(3):
                if i + 1 < len(P):
                    x = abs(P[i + 1][j][c] - P[i][j][c])
                    if x > ds:
                        ds = x
                if j + 1 < len(P[0]):
                    x = abs(P[i][j + 1][c] - P[i][j][c])
                    if x > dt:
                        dt = x
    return dt > ds


def push_children(heap, nd, dims, ctx):
    along_t = dims == 2 and split_dir_t(nd.net)
    if along_t:
        tm = (nd.t0 + nd.t1) * 0.5
        L, R = split_t(nd.net)
        boxes = ((nd.s0, nd.s1, nd.t0, tm), (nd.s0, nd.s1, tm, nd.t1))
    else:
        sm = (nd.s0 + nd.s1) * 0.5
        L, R = split_s(nd.net)
        boxes = ((nd.s0, sm, nd.t0, nd.t1), (sm, nd.s1, nd.t0, nd.t1))
    for side in (0, 1):
        s0, s1, t0, t1 = boxes[side]
        push(heap, make_node((L, R)[side], nd.root, s0, s1, t0, t1, nd.depth + 1, None, ctx), ctx)


# ---------------------------------------------------------------- entities

class Entity:
    def __init__(self, e):
        self.m = EM.Model(e)
        self.e = e
        self.surface = e["kind"] == "surface"
        self.ctrl = self.m.ctrl
        self.pw = homogeneous(self.m.ctrl, self.m.w)
        if self.surface:
            self.p, self.q = self.m.p, self.m.q
            self.U, self.V = self.m.U, self.m.V
            self.nu, self.nv = self.m.nu, self.m.nv
        else:
            self.p = self.m.p
            self.U = self.m.U
            self.nu = self.m.nu

    def c_pos(self, t):
        _, out = self.m.curve(t, 0)
        return out[0]

    def c_d1(self, t):
        _, out = self.m.curve(t, 1)
        return out[1]

    def s_pos(self, u, v):
        _, S = self.m.surf(u, v, 0)
        return S[(0, 0)][:3]

    def s_d1(self, u, v):
        _, S = self.m.surf(u, v, 1)
        return S[(1, 0)][:3], S[(0, 1)][:3]


def crease_values(U, p, values):
    out = []
    for x in values[1:-1]:
        if sum(1 for y in U if y == x) >= p:
            out.append(x)
    return out


def degenerate_line(nets, X):
    P0 = None
    for net in nets:
        for row in projected(net):
            for x in row:
                if P0 is None:
                    P0 = x
                for c in range(3):
                    if abs(x[c] - P0[c]) > DEGEN_REL * X:
                        return False
    return True


def check_query(q):
    for x in q:
        if not math.isfinite(x):
            raise NonFiniteError()


def final_dir_ok(x, lo, hi, g, e, dist, M, L2):
    if x == lo and g >= 0.0:
        return True
    if x == hi and g <= 0.0:
        return True
    if x != lo and x != hi:
        W = hi - lo
        if e * (W * W) <= SING2 * L2:
            return False
    return g * g <= ortho_tol2(e, dist, M)


def select(ctx):
    dmin = math.inf
    for c in ctx.cands:
        if c[2] < dmin:
            dmin = c[2]
    w = None
    for c in ctx.cands:
        if c[2] <= dmin + TIE:
            if w is None or c[0] < w[0]:
                w = c
    # a pole (degenerate line) tied with the winner at the same foot is the canonical answer
    for c in ctx.cands:
        if c[3] and c[2] <= dmin + TIE and inf_dist(c[1], w[1]) <= FOOT_SEP:
            return c
    return w


def inf_dist(a, b):
    m = 0.0
    for c in range(3):
        x = abs(a[c] - b[c])
        if x > m:
            m = x
    return m


def status(ctx, w, cert):
    """"C0" certified, "C1" certified with a tie (a tied candidate's foot more than FOOT_SEP
    away), "UB" / "UI" unresolved because a node that could not be pruned against the winner
    was left by the budget or depth limit (B) or by a non-converging Newton (I), "UC" the
    winner itself is not certified (convergence)."""
    dmin = w[2]
    for lb, reason in ctx.unresolved:
        if lb - PRUNE_REL * ctx.M <= dmin + TIE:
            return "U" + reason
    if not cert:
        return "UC"
    for c in ctx.cands:
        if c[2] <= dmin + TIE and inf_dist(c[1], w[1]) > FOOT_SEP:
            return "C1"
    return "C0"


def add_point(ctx, params, foot, pole=False):
    r = sub3(foot, ctx.q)
    ctx.add(params, foot, dot(r, r), pole)


def invert_curve(ent, q):
    """Returns ((t,), foot, dist, on_boundary, status)."""
    check_query(q)
    M, L2, X = scale(ent.ctrl, q)
    ctx = Ctx(q, M, L2)
    p = ent.p
    U2, (row,), values = insert_all(ent.U, p, [ent.pw])
    segs = segments_of(U2, p, values)
    lo, hi = values[0], values[-1]
    nets = [[[pt] for pt in row[s:s + p + 1]] for s, _, _ in segs]
    degenerate = degenerate_line(nets, X)
    if degenerate:
        add_point(ctx, (lo,), ent.c_pos(lo), True)
    else:
        for x in values:
            add_point(ctx, (x,), ent.c_pos(x))
        roots = [(net, (x0, x1), (0.0, 0.0)) for net, (_, x0, x1) in zip(nets, segs)]
        prebound(roots, 1, ctx, ent.c_pos)
        bnb(roots, 1, ctx, lambda pr: ent.c_pos(pr[0]), lambda t: (t,))
    w = select(ctx)
    params, foot, dist, _ = w
    t = params[0]
    if degenerate or t in crease_values(ent.U, p, values):
        cert = True
    else:
        d1 = ent.c_d1(t)
        r = sub3(foot, q)
        cert = final_dir_ok(t, lo, hi, dot(d1, r), dot(d1, d1), dist, M, L2)
    return (t,), foot, dist, t == lo or t == hi, status(ctx, w, cert)


def invert_surface(ent, q):
    check_query(q)
    M, L2, X = scale(ent.ctrl, q)
    ctx = Ctx(q, M, L2)
    p, qd, nu, nv = ent.p, ent.q, ent.nu, ent.nv
    # u insertion on every column of the net, then v insertion on every row
    cols = [[ent.pw[i * nv + j] for i in range(nu)] for j in range(nv)]
    U2, cols, uvals = insert_all(ent.U, p, cols)
    nu2 = len(cols[0])
    rows = [[cols[j][i] for j in range(nv)] for i in range(nu2)]
    V2, rows, vvals = insert_all(ent.V, qd, rows)
    useg = segments_of(U2, p, uvals)
    vseg = segments_of(V2, qd, vvals)
    patches = []
    for (iu, u0, u1) in useg:
        for (iv, v0, v1) in vseg:
            net = [[rows[iu + i][iv + j] for j in range(qd + 1)] for i in range(p + 1)]
            patches.append((net, (u0, u1), (v0, v1)))
    nvs = len(vseg)
    ulo, uhi = uvals[0], uvals[-1]
    vlo, vhi = vvals[0], vvals[-1]
    ucre = crease_values(ent.U, p, uvals)
    vcre = crease_values(ent.V, qd, vvals)
    degen_u, degen_v = [], []
    prebound(patches, 2, ctx, lambda pr: ent.s_pos(pr[0], pr[1]))
    # lines: u fixed (ascending: start, creases, end), then v fixed
    for along_v in (True, False):
        vals = ([ulo] + ucre + [uhi]) if along_v else ([vlo] + vcre + [vhi])
        for x in vals:
            nets, spans = [], []
            if along_v:
                k = uvals.index(x)
                ku, edge = (k, 0) if k < len(useg) else (k - 1, p)
                for kv in range(nvs):
                    net = patches[ku * nvs + kv][0]
                    nets.append([[pt] for pt in net[edge]])
                    spans.append(vseg[kv][1:])
                bvals = vvals

                def lift(t, x=x):
                    return (x, t)
            else:
                k = vvals.index(x)
                kv, edge = (k, 0) if k < len(vseg) else (k - 1, qd)
                for ku in range(len(useg)):
                    net = patches[ku * nvs + kv][0]
                    nets.append([[r[edge]] for r in net])
                    spans.append(useg[ku][1:])
                bvals = uvals

                def lift(t, x=x):
                    return (t, x)
            if degenerate_line(nets, X):
                (degen_u if along_v else degen_v).append(x)
                pr = lift(bvals[0])
                add_point(ctx, pr, ent.s_pos(*pr), True)
                continue
            for t in bvals:
                pr = lift(t)
                add_point(ctx, pr, ent.s_pos(*pr))
            roots = [(net, sp, (0.0, 0.0)) for net, sp in zip(nets, spans)]
            bnb(roots, 1, ctx, lambda pr: ent.s_pos(pr[0], pr[1]), lift)
    bnb(patches, 2, ctx, lambda pr: ent.s_pos(pr[0], pr[1]), None)
    w = select(ctx)
    (u, v), foot, dist, _ = w
    su, sv = ent.s_d1(u, v)
    r = sub3(foot, q)
    cert = True
    for (x, lo, hi, d1, cre, skip) in ((u, ulo, uhi, su, ucre, v in degen_v), (v, vlo, vhi, sv, vcre, u in degen_u)):
        if skip or x in cre:
            continue
        if not final_dir_ok(x, lo, hi, dot(d1, r), dot(d1, d1), dist, M, L2):
            cert = False
    return (u, v), foot, dist, u == ulo or u == uhi or v == vlo or v == vhi, status(ctx, w, cert)
