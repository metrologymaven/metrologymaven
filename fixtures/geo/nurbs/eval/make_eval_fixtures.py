#!/usr/bin/env python3
"""Independent reference for GEO.NURBS.EVAL (CC-BY-4.0 project data).

Writes fixtures/geo/nurbs/eval/digests-v1.txt: the SHA-256 of the evaluation results of every
entity of fixtures/geo/nurbs/ref_set_v1.json (GEO.NURBS.REF) under the order of operations
pinned in spec/geo/nurbs/eval/NODE.md (Design), one line per entity plus a total (AC-4).

Run:  python fixtures/geo/nurbs/eval/make_eval_fixtures.py          (writes the fixture)
      python fixtures/geo/nurbs/eval/make_eval_fixtures.py --check  (fails if it is stale)

Stdlib only (json, struct, hashlib, fractions, decimal); shares no code with any Rust crate and
never runs the Rust code. Two independent parts:

1. A bit-exact binary64 model of the pinned algorithm (span search, Cox-de Boor triangle, basis
   derivatives by the degree-lowering recurrence, homogeneous accumulation, quotient rule,
   normal, fundamental forms, principal curvatures and directions). CPython floats are IEEE 754
   binary64 with round-to-nearest-even, never fuse a multiply-add, and math.sqrt is correctly
   rounded, so the model's bit patterns are the Rust results on every target. A domain width
   or a knot window (U[s+p] - U[s-p]) that overflows is NonFinite, and a weight below
   f64::MIN_POSITIVE used at the parameter is SubnormalWeight, as in the Rust code. The digests are
   computed from these bits.
2. Accuracy checks of that model before anything is written, against values that do not come
   from it:
   - the exact-rational GEO.NURBS.REF oracle values stored in ref_set_v1.json (positions within
     1e-12 m, derivatives within 1e-9 relative, normals within 1e-9, curvatures within 1e-9
     relative to max(1, |k1|, |k2|), curve curvature likewise, regularity flags equal);
   - the domain checks stored in ref_set_v1.json (outside_domain or clamp to the end);
   - principal directions recomputed here at 60 decimal digits from the oracle's derivatives
     (Fraction -> Decimal): the model's d1 must be within 1e-6 of +-the reference wherever the
     principal curvatures are separated by more than 1e-3 x max(1, |k1|, |k2|);
   - the degeneracy decision recomputed exactly in Fraction from the model's own derivatives:
     the model's f64 test (c2 < 1e-28 x (a2 x b2) or a zero) must agree with the exact test
     |Su x Sv|^2 < 1e-28 |Su|^2 |Sv|^2 everywhere in the set.

Digest text (pinned; one line per evaluation row of ref_set_v1, in file order; every f64 is its
binary64 bit pattern as 16 lowercase hex digits; a vector is three of them separated by spaces;
"-" stands for an absent value):

  curve:    c <t> <p> <d1> <d2> <R|D> <tangent|-> <curvature|->
  surface:  s <u> <v> <p> <su> <sv> <suu> <suv> <svv> <R|D> then, when regular,
            <normal> <k1> <k2> <gaussian> <mean> <d1|-> <d2|->, and "-" when degenerate
  domain:   x <c|u|v> <t> O                          (outside_domain)
            x <c|u|v> <t> C <clamped t> <p>          (clamped; p evaluated at order 0)

Domain lines follow the entity's eval lines, in the order of its domain_checks (curves), or u
checks then v checks (surfaces, the other parameter at its domain start). The text of an entity
is its lines each followed by "\n"; its digest is SHA-256 of that UTF-8 text; the total is
SHA-256 of all entity texts concatenated in file order.
"""

import hashlib
import json
import math
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
REF = HERE.parent / "ref_set_v1.json"
OUT = HERE / "digests-v1.txt"

MAX_DEGREE = 25
CLAMP_REL = 1e-14      # REQ-GEO.NURBS.EVAL-002
DEGEN2 = 1e-28         # (1e-14)^2, REQ-GEO.NURBS.EVAL-003 (f64 literal)
UMBILIC_REL = 1e-8     # NODE.md Design: no principal directions when r <= 1e-8 |H|
POS_TOL = 1e-12
DER_REL = 1e-9

getcontext().prec = 60


class OutsideDomain(Exception):
    pass


class NonFinite(Exception):
    pass


class SubnormalWeight(Exception):
    pass


MIN_POSITIVE = sys.float_info.min  # 2^-1022, f64::MIN_POSITIVE


def check_weight(weights, i):
    """NODE.md item 5: a weight below f64::MIN_POSITIVE used at this parameter is refused."""
    if weights is not None and weights[i] < MIN_POSITIVE:
        raise SubnormalWeight(i)


def hx(x):
    return "%016x" % struct.unpack(">Q", struct.pack(">d", x))[0]


def unhex(s):
    return struct.unpack(">d", struct.pack(">Q", int(s, 16)))[0]


def jf(j):
    """{hex, dec} -> float (from the bit pattern), recursively for lists; None stays None."""
    if j is None:
        return None
    h = j["hex"]

    def conv(x):
        return [conv(y) for y in x] if isinstance(x, list) else unhex(x)

    return conv(h)


# ---------------------------------------------------------------- the binary64 model (pinned order)

def dot(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def clamp_param(t, lo, hi):
    if t != t:
        raise ValueError("NaN")
    if t == math.inf or t == -math.inf:
        raise OutsideDomain()
    width = hi - lo
    if not math.isfinite(width):
        raise NonFinite()
    margin = CLAMP_REL * width
    if t < lo:
        if lo - t <= margin:
            return lo
        raise OutsideDomain()
    if t > hi:
        if t - hi <= margin:
            return hi
        raise OutsideDomain()
    return t


def find_span(U, p, n, t):
    if t >= U[n]:
        s = n - 1
        while s > p and U[s] == U[s + 1]:
            s -= 1
        return s
    lo, hi = p, n
    while hi - lo > 1:
        mid = lo + (hi - lo) // 2
        if t < U[mid]:
            hi = mid
        else:
            lo = mid
    return lo


def basis(U, p, s, t, nd):
    """N[d][j]: d-th derivative of N_{s-p+j,p}(t), d <= nd, j <= p."""
    if not math.isfinite(U[s + p] - U[s - p]):
        raise NonFinite()
    levels = [[1.0]]
    for k in range(1, p + 1):
        prev = levels[k - 1]
        cur = [0.0] * (k + 1)
        saved = 0.0
        for r in range(k):
            den = U[s + r + 1] - U[s + r + 1 - k]
            temp = prev[r] / den
            cur[r] = saved + (U[s + r + 1] - t) * temp
            saved = (t - U[s + r + 1 - k]) * temp
        cur[k] = saved
        levels.append(cur)

    def deriv(prev, k):
        kf = float(k)
        out = [0.0] * (k + 1)
        for j in range(k + 1):
            a = prev[j - 1] / (U[s + j] - U[s - k + j]) if j >= 1 else 0.0
            b = prev[j] / (U[s + j + 1] - U[s - k + j + 1]) if j <= k - 1 else 0.0
            out[j] = kf * (a - b)
        return out

    out = [levels[p]]
    if nd >= 1:
        out.append(deriv(levels[p - 1], p))
    if nd >= 2:
        lower = deriv(levels[p - 2], p - 1) if p >= 2 else [0.0]
        out.append(deriv(lower, p))
    return out


def homogeneous(ctrl, weights):
    if weights is None:
        return None
    return [[c[0] * w, c[1] * w, c[2] * w, w] for c, w in zip(ctrl, weights)]


def quotient_curve(A, nd):
    w0 = A[0][3]
    C0 = [A[0][c] / w0 for c in range(3)]
    out = [C0]
    if nd >= 1:
        w1 = A[1][3]
        C1 = [(A[1][c] - w1 * C0[c]) / w0 for c in range(3)]
        out.append(C1)
    if nd >= 2:
        w2 = A[2][3]
        C2 = [((A[2][c] - (2.0 * w1) * C1[c]) - w2 * C0[c]) / w0 for c in range(3)]
        out.append(C2)
    return out


class Model:
    def __init__(self, e):
        self.surface = e["kind"] == "surface"
        self.ctrl = jf(e["ctrl"])
        self.w = jf(e["weights"])
        self.pw = homogeneous(self.ctrl, self.w)
        if self.surface:
            self.p, self.q = e["degree_u"], e["degree_v"]
            self.U, self.V = jf(e["knots_u"]), jf(e["knots_v"])
            self.nu, self.nv = e["n_u"], e["n_v"]
        else:
            self.p = e["degree"]
            self.U = jf(e["knots"])
            self.nu = len(self.ctrl)

    def dom_u(self):
        return self.U[self.p], self.U[self.nu]

    def dom_v(self):
        return self.V[self.q], self.V[self.nv]

    def curve(self, t, nd):
        lo, hi = self.dom_u()
        t = clamp_param(t, lo, hi)
        p = self.p
        s = find_span(self.U, p, self.nu, t)
        N = basis(self.U, p, s, t, nd)
        if self.pw is None:
            out = []
            for d in range(nd + 1):
                acc = [0.0, 0.0, 0.0]
                for j in range(p + 1):
                    b, c = N[d][j], self.ctrl[s - p + j]
                    acc = [acc[0] + b * c[0], acc[1] + b * c[1], acc[2] + b * c[2]]
                out.append(acc)
            return t, out
        A = []
        for d in range(nd + 1):
            acc = [0.0, 0.0, 0.0, 0.0]
            for j in range(p + 1):
                check_weight(self.w, s - p + j)
                b, c = N[d][j], self.pw[s - p + j]
                acc = [acc[0] + b * c[0], acc[1] + b * c[1], acc[2] + b * c[2], acc[3] + b * c[3]]
            A.append(acc)
        return t, quotient_curve(A, nd)

    def surf(self, u, v, nd):
        (ulo, uhi), (vlo, vhi) = self.dom_u(), self.dom_v()
        u = clamp_param(u, ulo, uhi)
        v = clamp_param(v, vlo, vhi)
        p, q, nv = self.p, self.q, self.nv
        su = find_span(self.U, p, self.nu, u)
        sv = find_span(self.V, q, nv, v)
        Nu = basis(self.U, p, su, u, nd)
        Nv = basis(self.V, q, sv, v, nd)
        rational = self.pw is not None
        pts = self.pw if rational else self.ctrl
        dim = 4 if rational else 3
        tmp = []
        for l in range(nd + 1):
            rows = []
            for i in range(p + 1):
                base = (su - p + i) * nv + (sv - q)
                acc = [0.0] * dim
                for j in range(q + 1):
                    check_weight(self.w, base + j)
                    b, c = Nv[l][j], pts[base + j]
                    acc = [acc[x] + b * c[x] for x in range(dim)]
                rows.append(acc)
            tmp.append(rows)
        A = {}
        for k in range(nd + 1):
            for l in range(nd + 1 - k):
                acc = [0.0] * dim
                for i in range(p + 1):
                    b, r = Nu[k][i], tmp[l][i]
                    acc = [acc[x] + b * r[x] for x in range(dim)]
                A[(k, l)] = acc
        if not rational:
            return (u, v), A
        w00 = A[(0, 0)][3]
        S = {(0, 0): [A[(0, 0)][c] / w00 for c in range(3)]}
        if nd >= 1:
            w10, w01 = A[(1, 0)][3], A[(0, 1)][3]
            S[(1, 0)] = [(A[(1, 0)][c] - w10 * S[(0, 0)][c]) / w00 for c in range(3)]
            S[(0, 1)] = [(A[(0, 1)][c] - w01 * S[(0, 0)][c]) / w00 for c in range(3)]
        if nd >= 2:
            S00, S10, S01 = S[(0, 0)], S[(1, 0)], S[(0, 1)]
            w20, w11, w02 = A[(2, 0)][3], A[(1, 1)][3], A[(0, 2)][3]
            S[(2, 0)] = [((A[(2, 0)][c] - (2.0 * w10) * S10[c]) - w20 * S00[c]) / w00 for c in range(3)]
            S[(1, 1)] = [(((A[(1, 1)][c] - w10 * S01[c]) - w01 * S10[c]) - w11 * S00[c]) / w00 for c in range(3)]
            S[(0, 2)] = [((A[(0, 2)][c] - (2.0 * w01) * S01[c]) - w02 * S00[c]) / w00 for c in range(3)]
        return (u, v), S


def curve_geometry(c1, c2):
    """(regular, tangent, curvature)."""
    cl = math.sqrt(dot(c1, c1))
    den = (cl * cl) * cl
    if den == 0.0:
        return False, None, None
    tan = [c1[0] / cl, c1[1] / cl, c1[2] / cl]
    x = cross(c1, c2)
    return True, tan, math.sqrt(dot(x, x)) / den


def surface_degenerate(su, sv):
    a2, b2 = dot(su, su), dot(sv, sv)
    cr = cross(su, sv)
    c2 = dot(cr, cr)
    return a2 == 0.0 or b2 == 0.0 or c2 == 0.0 or c2 < DEGEN2 * (a2 * b2)


def surface_geometry(S):
    """None when degenerate, else (normal, k1, k2, K, H, d1, d2)."""
    su, sv = S[(1, 0)], S[(0, 1)]
    if surface_degenerate(su, sv):
        return None
    cr = cross(su, sv)
    ln = math.sqrt(dot(cr, cr))
    n = [cr[0] / ln, cr[1] / ln, cr[2] / ln]
    a = math.sqrt(dot(su, su))
    e1 = [su[0] / a, su[1] / a, su[2] / a]
    e2 = cross(n, e1)
    b, c = dot(sv, e1), dot(sv, e2)
    L, M, N = dot(S[(2, 0)], n), dot(S[(1, 1)], n), dot(S[(0, 2)], n)
    t = b / a
    s11 = (L / a) / a
    s12 = ((M - t * L) / a) / c
    s22 = ((N - (2.0 * t) * M) + (t * t) * L) / (c * c)
    H = 0.5 * (s11 + s22)
    h = 0.5 * (s11 - s22)
    r = math.sqrt(h * h + s12 * s12)
    k1, k2 = H + r, H - r
    K = k1 * k2
    d1 = d2 = None
    if not r <= UMBILIC_REL * abs(H):
        A, C = s11 - k1, s22 - k1
        if abs(A) >= abs(C):
            x, y = -s12, A
        else:
            x, y = C, -s12
        d = [x * e1[0] + y * e2[0], x * e1[1] + y * e2[1], x * e1[2] + y * e2[2]]
        dl = math.sqrt(dot(d, d))
        if dl != 0.0:
            d1 = [d[0] / dl, d[1] / dl, d[2] / dl]
            d2 = cross(n, d1)
    return n, k1, k2, K, H, d1, d2


# ---------------------------------------------------------------- digest text

def vx(v):
    return "-" if v is None else " ".join(hx(x) for x in v)


def curve_line(m, t):
    t, (P, D1, D2) = m.curve(t, 2)
    reg, tan, k = curve_geometry(D1, D2)
    return "c %s %s %s %s %s %s %s" % (hx(t), vx(P), vx(D1), vx(D2), "R" if reg else "D", vx(tan),
                                         "-" if k is None else hx(k))


def surface_line(m, u, v):
    (u, v), S = m.surf(u, v, 2)
    g = surface_geometry(S)
    head = "s %s %s %s %s %s %s %s %s" % (hx(u), hx(v), vx(S[(0, 0)]), vx(S[(1, 0)]), vx(S[(0, 1)]),
                                           vx(S[(2, 0)]), vx(S[(1, 1)]), vx(S[(0, 2)]))
    if g is None:
        return head + " D -"
    n, k1, k2, K, H, d1, d2 = g
    return head + " R %s %s %s %s %s %s %s" % (vx(n), hx(k1), hx(k2), hx(K), hx(H), vx(d1), vx(d2))


def domain_line(m, tag, t, other):
    try:
        if tag == "c":
            tc, (P,) = m.curve(t, 0)
        elif tag == "u":
            (tc, _), A = m.surf(t, other, 0)
            P = A[(0, 0)]
        else:
            (_, tc), A = m.surf(other, t, 0)
            P = A[(0, 0)]
    except OutsideDomain:
        return "x %s %s O" % (tag, hx(t))
    return "x %s %s C %s %s" % (tag, hx(t), hx(tc), vx(P))


def entity_text(e, m):
    lines = []
    for r in e["eval"]:
        if m.surface:
            lines.append(surface_line(m, jf(r["u"]), jf(r["v"])))
        else:
            lines.append(curve_line(m, jf(r["t"])))
    if m.surface:
        for tag, other in (("u", m.dom_v()[0]), ("v", m.dom_u()[0])):
            for dc in e["domain_checks"][tag]:
                lines.append(domain_line(m, tag, jf(dc["t"]), other))
    else:
        for dc in e["domain_checks"]:
            lines.append(domain_line(m, "c", jf(dc["t"]), None))
    return "".join(x + "\n" for x in lines)


# ---------------------------------------------------------------- checks against independent values

def inf_err(a, b):
    return max(abs(x - y) for x, y in zip(a, b))


def inf_norm(a):
    return max(abs(x) for x in a)


def check_vec(got, want, rel, what):
    tol = rel * max(1.0, inf_norm(want))
    err = inf_err(got, want)
    if not err <= tol:
        raise SystemExit(f"{what}: error {err:e} above {tol:e}")
    return err / max(1.0, inf_norm(want))


def fr(x):
    return Fraction(x)


def exact_degenerate(su, sv):
    su, sv = [fr(x) for x in su], [fr(x) for x in sv]
    a2 = sum(x * x for x in su)
    b2 = sum(x * x for x in sv)
    cr = [su[1] * sv[2] - su[2] * sv[1], su[2] * sv[0] - su[0] * sv[2], su[0] * sv[1] - su[1] * sv[0]]
    c2 = sum(x * x for x in cr)
    return a2 == 0 or b2 == 0 or c2 < Fraction(1, 10 ** 28) * a2 * b2


def ref_direction(rec):
    """60-digit principal direction for k1 from the oracle's derivatives (Fraction -> Decimal)."""
    D = lambda x: Decimal(fr(x).numerator) / Decimal(fr(x).denominator)  # noqa: E731
    su, sv = [D(x) for x in jf(rec["su"])], [D(x) for x in jf(rec["sv"])]
    suu, suv, svv = ([D(x) for x in jf(rec[k])] for k in ("suu", "suv", "svv"))

    def ddot(a, b):
        return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]

    cr = [su[1] * sv[2] - su[2] * sv[1], su[2] * sv[0] - su[0] * sv[2], su[0] * sv[1] - su[1] * sv[0]]
    ln = ddot(cr, cr).sqrt()
    n = [x / ln for x in cr]
    E, F, G = ddot(su, su), ddot(su, sv), ddot(sv, sv)
    L, M, N = ddot(suu, n), ddot(suv, n), ddot(svv, n)
    den = E * G - F * F
    K = (L * N - M * M) / den
    H = (E * N - 2 * F * M + G * L) / (2 * den)
    disc = max(H * H - K, Decimal(0))
    k1 = H + disc.sqrt()
    A, B, C = L - k1 * E, M - k1 * F, N - k1 * G
    a, b = (-B, A) if abs(A) >= abs(C) else (C, -B)
    d = [a * su[i] + b * sv[i] for i in range(3)]
    dl = ddot(d, d).sqrt()
    return [float(x / dl) for x in d]


def check_entity(e, m):
    worst = {"p": 0.0, "der": 0.0}
    eid = e["id"]
    for r in e["eval"]:
        if m.surface:
            (u, v), S = m.surf(jf(r["u"]), jf(r["v"]), 2)
            ctx = f"{eid} u={u!r} v={v!r}"
            if not inf_err(S[(0, 0)], jf(r["p"])) <= POS_TOL:
                raise SystemExit(f"{ctx}: position")
            worst["p"] = max(worst["p"], inf_err(S[(0, 0)], jf(r["p"])))
            for key, kl in (("su", (1, 0)), ("sv", (0, 1)), ("suu", (2, 0)), ("suv", (1, 1)), ("svv", (0, 2))):
                worst["der"] = max(worst["der"], check_vec(S[kl], jf(r[key]), DER_REL, f"{ctx} {key}"))
            g = surface_geometry(S)
            if exact_degenerate(S[(1, 0)], S[(0, 1)]) != (g is None):
                raise SystemExit(f"{ctx}: f64 degeneracy test disagrees with the exact test")
            if (g is None) != (r["regularity"] == "degenerate"):
                raise SystemExit(f"{ctx}: regularity")
            if g is None:
                continue
            n, k1, k2, K, H, d1, d2 = g
            check_vec(n, jf(r["normal"]), DER_REL, f"{ctx} normal")
            scale = max(1.0, abs(jf(r["k1"])), abs(jf(r["k2"])))
            for got, key in ((k1, "k1"), (k2, "k2"), (K, "gaussian"), (H, "mean")):
                tol = DER_REL * (scale * scale if key == "gaussian" else scale)
                if not abs(got - jf(r[key])) <= tol:
                    raise SystemExit(f"{ctx} {key}: {got!r} vs {jf(r[key])!r}")
            if jf(r["k1"]) - jf(r["k2"]) > 1e-3 * scale:
                if d1 is None:
                    raise SystemExit(f"{ctx}: no principal direction at a non-umbilic point")
                want = ref_direction(r)
                err = min(inf_err(d1, want), inf_err(d1, [-x for x in want]))
                if not err <= 1e-6:
                    raise SystemExit(f"{ctx}: principal direction off by {err:e}")
                if abs(dot(d1, d2)) > 1e-12 or abs(dot(d2, n)) > 1e-12:
                    raise SystemExit(f"{ctx}: directions not orthonormal")
        else:
            t, (P, D1, D2) = m.curve(jf(r["t"]), 2)
            ctx = f"{eid} t={t!r}"
            if not inf_err(P, jf(r["p"])) <= POS_TOL:
                raise SystemExit(f"{ctx}: position {inf_err(P, jf(r['p']))}")
            worst["p"] = max(worst["p"], inf_err(P, jf(r["p"])))
            worst["der"] = max(worst["der"], check_vec(D1, jf(r["d1"]), DER_REL, f"{ctx} d1"))
            worst["der"] = max(worst["der"], check_vec(D2, jf(r["d2"]), DER_REL, f"{ctx} d2"))
            reg, tan, k = curve_geometry(D1, D2)
            want = jf(r["curvature"])
            if (k is None) != (want is None):
                raise SystemExit(f"{ctx}: curve regularity")
            if k is not None and not abs(k - want) <= DER_REL * max(1.0, abs(want)):
                raise SystemExit(f"{ctx}: curvature {k!r} vs {want!r}")
    # domain checks against the oracle's labels
    checks = ([("c", dc, None) for dc in e["domain_checks"]] if not m.surface else
              [("u", dc, m.dom_v()[0]) for dc in e["domain_checks"]["u"]]
              + [("v", dc, m.dom_u()[0]) for dc in e["domain_checks"]["v"]])
    for tag, dc, other in checks:
        line = domain_line(m, tag, jf(dc["t"]), other)
        if dc["expect"] == "outside_domain":
            if not line.endswith(" O"):
                raise SystemExit(f"{eid}: {tag} {jf(dc['t'])!r} should be outside the domain")
        elif line.split(" ")[4] != hx(jf(dc["to"])):
            raise SystemExit(f"{eid}: {tag} {jf(dc['t'])!r} should clamp to {jf(dc['to'])!r}")
    return worst


def self_test():
    # bit-pattern round trip and the hand-checked quadratic Bezier at t = 1/2: (1/4, 1/2, 1/4)
    assert unhex(hx(0.1)) == 0.1
    U = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    N = basis(U, 2, find_span(U, 2, 3, 0.5), 0.5, 2)
    assert N[0] == [0.25, 0.5, 0.25] and N[1] == [-1.0, 0.0, 1.0] and N[2] == [2.0, -4.0, 2.0], N
    assert find_span(U, 2, 3, 1.0) == 2
    U = [0.0, 0.0, 0.5, 0.5, 1.0, 1.0]  # degree 1, repeated interior knot 0.5 (multiplicity 2)
    assert find_span(U, 1, 4, 0.5) == 3 and find_span(U, 1, 4, 0.4999) == 1 and find_span(U, 1, 4, 1.0) == 3
    assert clamp_param(1.0 + 2.0 ** -52, 0.0, 1.0) == 1.0
    try:
        clamp_param(1.000001, 0.0, 1.0)
        raise AssertionError("1.000001 should be outside [0, 1]")
    except OutsideDomain:
        pass
    def enc(x):
        return {"hex": [enc(y)["hex"] for y in x] if isinstance(x, list) else "0x" + hx(x)}

    # subnormal weights (NODE.md item 5): refused, never a silently wrong (0, 1, 0)
    bez = {"kind": "curve", "degree": 2, "knots": enc([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
           "ctrl": enc([[0.3, 0.7, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]])}
    for w, ok in (([5e-324, 1.0, 5e-324], False), ([MIN_POSITIVE, 1.0, MIN_POSITIVE], True)):
        try:
            _, (P,) = Model(dict(bez, weights=enc(w))).curve(0.0, 0)
            # accepted; x * w may still round into the subnormal range (stated in the PR)
            assert ok and max(abs(P[0] - 0.3), abs(P[1] - 0.7), abs(P[2])) <= 1e-15, P
        except SubnormalWeight:
            assert not ok
    for wide, t in (([-1e308, -1e308, 1e308, 1e308], 0.0), ([-1e308, -1e308, 0.0, 1.0, 1e308, 1e308], 0.5)):
        try:
            p = len(wide) // 2 - 1
            n = len(wide) - p - 1
            clamp_param(t, wide[p], wide[n])
            basis(wide, p, find_span(wide, p, n, t), t, 0)
            raise AssertionError("a knot window that overflows should be NonFinite")
        except NonFinite:
            pass


def main():
    check = "--check" in sys.argv[1:]
    self_test()
    doc = json.loads(REF.read_text(encoding="utf-8"))
    out_lines = []
    total = hashlib.sha256()
    worst = {"p": 0.0, "der": 0.0}
    for e in doc["entities"]:
        m = Model(e)
        w = check_entity(e, m)
        worst = {k: max(worst[k], w[k]) for k in worst}
        text = entity_text(e, m).encode("utf-8")
        total.update(text)
        out_lines.append("%s %s" % (e["id"], hashlib.sha256(text).hexdigest()))
    out_lines.append("all %s" % total.hexdigest())
    body = ("# GEO.NURBS.EVAL AC-4: SHA-256 of the pinned evaluation of ref_set_v1.json per entity and in total.\n"
            "# Written by fixtures/geo/nurbs/eval/make_eval_fixtures.py (CC-BY-4.0); do not edit.\n"
            + "".join(x + "\n" for x in out_lines)).encode("utf-8")
    print(f"model vs oracle: worst position {worst['p']:.3e} m, worst derivative {worst['der']:.3e} relative")
    if check:
        if OUT.read_bytes() != body:
            raise SystemExit(f"{OUT} is stale")
        print("digests-v1.txt up to date")
    else:
        OUT.write_bytes(body)
        print(f"wrote {OUT} ({len(out_lines) - 1} entities)")


if __name__ == "__main__":
    main()
