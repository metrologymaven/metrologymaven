#!/usr/bin/env python3
"""Independent oracle for GEO.NURBS.REF: reference data for NURBS and analytic evaluation and inversion.

Written for this project from the spec (spec/geo/nurbs/ref/NODE.md and the EVAL, INVERT and
ANALYTIC leaves it serves); it shares no code with any Rust crate. Python standard library only:
`fractions.Fraction` gives exact rational evaluation, `decimal` at 60 working digits (answers
claimed to 50) stands in for mpmath. Output data is CC-BY-4.0 (project-generated, D-001).

What is exact and what is high precision:
- NURBS positions AND first and second derivatives are computed exactly in rational arithmetic
  from the exact values of the f64 inputs (every f64 is a dyadic rational) and rounded once to
  f64 (Cox-de Boor triangle for the basis, the standard derivative recurrence for basis
  derivatives, homogeneous coordinates and the quotient rule for rational forms).
- Normals, principal curvatures (need square roots) and every analytic-surface value (need sin,
  cos) are computed at 60 digits and rounded once. The degeneracy test of REQ-GEO.NURBS.EVAL-003
  (|Su x Sv| < 1e-14 |Su| |Sv|, or a zero derivative) is decided exactly.
- Inversion (global closest point on an untrimmed curve or surface, including its boundary): a
  dense f64 grid over every non-empty knot span selects seeds (every grid local minimum within
  the grid-resolution bound of the best grid point, the boundary-edge local minima and the
  corners); each seed is refined by damped Newton at 60 digits (2D inside, 1D on edges). Knot
  lines of multiplicity >= degree (C0: the distance has a kink there) split the domain and are
  searched like edges, with one-sided optimality tests. The smallest distance wins, and candidates within 1e-15 m are tied and resolved to the
  lexicographically smallest parameters (REQ-GEO.NURBS.INVERT-004).

Conventions written into the fixtures (`conventions` key of each file):
- Span rule: right-continuous; at the domain end the last non-empty span (REQ-GEO.NURBS.EVAL-006).
- Domain [U[p], U[n]] with n control points; parameters outside by more than 1e-14 x width are an
  error (`outside_domain`), within it they clamp (REQ-GEO.NURBS.EVAL-002).
- Normal n = Su x Sv / |Su x Sv|; curvatures are signed against n (II = <S_xy, n>, k = II/I):
  a sphere with outward n has k1 = k2 = -1/r. k1 >= k2, gaussian = k1 k2, mean = (k1 + k2)/2.
- Analytic placement: origin O, axis Z, ref_dir X, Y = Z x X (computed from the f64 values).
  line O + tX; circle O + r(cos t X + sin t Y); ellipse O + a cos t X + b sin t Y; plane O + uX +
  vY; cylinder O + r(cos u X + sin u Y) + vZ; cone O + (r + v tan a)(cos u X + sin u Y) + vZ;
  sphere O + r cos v (cos u X + sin u Y) + r sin v Z; torus O + (R + r cos v)(cos u X + sin u Y)
  + r sin v Z. Angles of inversion results are reported in [0, 2 pi) (sphere v in [-pi/2, pi/2]);
  when the closest point is not unique the canonical answer has u = 0 (sphere centre: v = 0;
  ellipse centre: the smallest t) and `non_unique` is true (REQ-GEO.NURBS.ANALYTIC-003).

Every f64 is written as {"hex": IEEE 754 binary64 bit pattern "0x" + 16 hex digits, "dec": the
shortest round-trip decimal}, with the same nesting in both members.

Outputs (in --out): ref_set_v1.json, analytic_set.json, multi_minima.json, manifest.json. The
GEO.NURBS.DATA fixtures valid_set.json and invalid_set.json (fixtures/geo/nurbs/
make_data_fixtures.py, seed 41) are part of the reference set; they are not rewritten here, but
the manifest lists them with their SHA-256 so the whole set is pinned.

Run:   python tools/fixtures/nurbs_oracle.py --out fixtures/geo/nurbs --seed 40
Check: python tools/fixtures/nurbs_oracle.py --out fixtures/geo/nurbs --seed 40 --check
"""

import argparse
import bisect
import hashlib
import json
import math
import random
import struct
import sys
from decimal import Decimal, getcontext, localcontext
from fractions import Fraction
from pathlib import Path

ORACLE_VERSION = "nurbs_oracle 1.0.0"
FORMAT = "fid-nurbs-ref/1"
PREC = 60
getcontext().prec = PREC
D = Decimal
DZERO, DONE = D(0), D(1)
FZERO, FONE = Fraction(0), Fraction(1)

POS_TOL = 1e-12        # REQ-GEO.NURBS.REF-005 / REQ-GEO.NURBS-003
DER_TOL_REL = 1e-9     # GEO.NURBS.EVAL AC-1
ANALYTIC_TOL = 1e-13   # REQ-GEO.NURBS.ANALYTIC-004
TIE = D("1e-15")       # REQ-GEO.NURBS.INVERT-004
NEAR_TIE = D("1e-9")
DEGEN2 = Fraction(1, 10 ** 28)  # (1e-14)^2, REQ-GEO.NURBS.EVAL-003
STEP_EPS = D("1e-45")
CLAMP_MARGIN = 1e-14   # REQ-GEO.NURBS.EVAL-002
OUT_MARGIN = 1e-6      # GEO.NURBS.EVAL AC-5

ADOPTED = [("valid_set.json", 41), ("invalid_set.json", 41)]
ADOPTED_GENERATOR = "fixtures/geo/nurbs/make_data_fixtures.py (GEO.NURBS.DATA)"

CONVENTIONS = {
    "span": "right-continuous; at the domain end the last non-empty span (REQ-GEO.NURBS.EVAL-006)",
    "domain": "[U[p], U[n]] for n control points; outside by more than 1e-14 x width is outside_domain, "
              "within it clamps (REQ-GEO.NURBS.EVAL-002)",
    "surface_net": "row-major: control point (i, j) at i * n_v + j",
    "domain_checks": "t just outside an end: outside_domain (1e-6 x width out) or clamp (0.25e-14 x width out, "
                     "evaluates as the end); for surfaces the other parameter is at its domain start",
    "derivatives": "exact rational, rounded once; surfaces su, sv, suu, suv, svv; curves d1, d2",
    "normal": "Su x Sv / |Su x Sv|; null when degenerate (|Su x Sv| < 1e-14 |Su||Sv| or a zero derivative)",
    "curvature": "signed against the normal, k = II/I, II = <S_xy, n>; k1 >= k2; gaussian k1 k2; mean (k1+k2)/2; "
                 "curve curvature |C' x C''| / |C'|^3",
    "inversion": "global closest point on the untrimmed entity including its boundary; ties within 1e-15 m "
                 "resolve to the lexicographically smallest parameters (REQ-GEO.NURBS.INVERT-004); "
                 "alternatives lists other local minima within 1e-9 m",
    "float": "every f64 as {hex: binary64 bit pattern, dec: shortest round-trip decimal}",
}


def die(msg):
    sys.stderr.write("nurbs_oracle: " + msg + "\n")
    sys.exit(2)


# ---------------------------------------------------------------- f64 encoding

def bits(x):
    x = float(x)
    if not math.isfinite(x):
        die("non-finite value " + repr(x))
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", x))[0]


def _map(x, f):
    if isinstance(x, (list, tuple)):
        return [_map(y, f) for y in x]
    return f(x)


def enc(x):
    """f64 (or nested lists of them; Fraction/Decimal are rounded once) as {hex, dec}."""
    fl = _map(x, float)
    return {"hex": _map(fl, bits), "dec": fl}


def check_finite(values, what):
    for v in values:
        if not math.isfinite(v):
            die(f"{what}: non-finite input {v!r}")


# ---------------------------------------------------------------- 60-digit elementary functions

def _atan_taylor(x):
    x2, term, s, k = x * x, x, x, 1
    eps = D(10) ** -(getcontext().prec + 5)
    while True:
        term = -term * x2
        k += 2
        t = term / k
        if abs(t) < eps:
            return s
        s += t


def _compute_pi():
    with localcontext() as c:
        c.prec = PREC + 15
        v = 16 * _atan_taylor(DONE / 5) - 4 * _atan_taylor(DONE / 239)
    return +v


PI = _compute_pi()
TWO_PI = 2 * PI
HALF_PI = PI / 2


def sin_cos(x):
    """sin and cos of a Decimal at PREC digits (argument reduction, halving, Taylor, doubling)."""
    with localcontext() as c:
        c.prec = PREC + 25
        tp = 2 * PI
        k = (x / tp).to_integral_value()
        y = x - k * tp
        m = 10
        y = y / (2 ** m)
        y2 = y * y
        s = term = y
        n = 1
        eps = D(10) ** -(c.prec + 2)
        while True:
            term = -term * y2 / ((n + 1) * (n + 2))
            n += 2
            s += term
            if abs(term) < eps:
                break
        co = term = DONE
        n = 0
        while True:
            term = -term * y2 / ((n + 1) * (n + 2))
            n += 2
            co += term
            if abs(term) < eps:
                break
        for _ in range(m):
            s, co = 2 * s * co, co * co - s * s
    return +s, +co


def atan(x):
    with localcontext() as c:
        c.prec = PREC + 15
        if x < 0:
            return -atan(-x)
        inv = x > 1
        if inv:
            x = DONE / x
        for _ in range(3):  # atan x = 2 atan(x / (1 + sqrt(1 + x^2)))
            x = x / (1 + (1 + x * x).sqrt())
        r = 8 * _atan_taylor(x)
        if inv:
            r = PI / 2 - r
    return +r


def atan2(y, x):
    if x > 0:
        return atan(y / x)
    if x < 0:
        return atan(y / x) + (PI if y >= 0 else -PI)
    if y > 0:
        return HALF_PI
    if y < 0:
        return -HALF_PI
    return DZERO


def wrap_2pi(a):
    while a < 0:
        a += TWO_PI
    while a >= TWO_PI:
        a -= TWO_PI
    return a


def dec_of(x):
    """Exact Fraction or float to a Decimal at working precision."""
    if isinstance(x, Fraction):
        return D(x.numerator) / D(x.denominator)
    if isinstance(x, Decimal):
        return +x
    return +D(x)


# ---------------------------------------------------------------- vector helpers (any numeric type)

def vadd(a, b):
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def vsub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def vscale(s, a):
    return [s * a[0], s * a[1], s * a[2]]


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dnorm(a):
    return dot(a, a).sqrt()


# ---------------------------------------------------------------- B-spline kernel (generic: Fraction, Decimal or float)

def find_span(U, p, n, t, left=False):
    """Span s with U[s] <= t < U[s+1] (right-continuous); at t == U[n] the last non-empty span.
    left=True gives the left limit instead (U[s] < t <= U[s+1]; at t == U[p] the first span); it is
    used only by the inversion to test one-sided optimality at C0 knot lines."""
    if t < U[p] or t > U[n]:
        raise ValueError("parameter outside the domain")
    if left and t > U[p]:
        return bisect.bisect_left(U, t) - 1
    if t == U[n]:
        s = n - 1
        while U[s] == U[s + 1]:
            s -= 1
        return s
    return bisect.bisect_right(U, t) - 1


def basis_ders(U, p, s, t, nd, zero, one):
    """ders[d][j] = d-th derivative of N_{s-p+j, p} at t, d = 0..nd, j = 0..p.

    The degree-k values come from the Cox-de Boor triangle (0/0 := 0); derivatives use
    N'_{i,k} = k (N_{i,k-1} / (U[i+k] - U[i]) - N_{i+1,k-1} / (U[i+k+1] - U[i+1])), x/0 := 0,
    applied to the derivative table of the degree below.
    """
    levels = [[one]]
    for k in range(1, p + 1):
        prev = levels[-1]
        cur = []
        for j in range(k + 1):
            i = s - k + j
            v = zero
            if j >= 1:
                den = U[i + k] - U[i]
                if den != 0:
                    v = v + (t - U[i]) / den * prev[j - 1]
            if j <= k - 1:
                den = U[i + k + 1] - U[i + 1]
                if den != 0:
                    v = v + (U[i + k + 1] - t) / den * prev[j]
            cur.append(v)
        levels.append(cur)
    # table[d][k][j]: d-th derivative of N_{s-k+j, k}
    table = [levels]
    for d in range(1, nd + 1):
        lower = table[-1]
        cur_d = [[zero]]
        for k in range(1, p + 1):
            prev = lower[k - 1]
            row = []
            for j in range(k + 1):
                i = s - k + j
                v = zero
                if j >= 1:
                    den = U[i + k] - U[i]
                    if den != 0:
                        v = v + prev[j - 1] / den
                if j <= k - 1:
                    den = U[i + k + 1] - U[i + 1]
                    if den != 0:
                        v = v - prev[j] / den
                row.append(k * v)
            cur_d.append(row)
        table.append(cur_d)
    return [table[d][p] for d in range(nd + 1)]


NUMS = {
    "frac": (Fraction, FZERO, FONE),
    "dec": (lambda x: +D(x), DZERO, DONE),
    "float": (float, 0.0, 1.0),
}


class Spline:
    """A curve (n_v == 0) or a tensor-product surface; inputs are f64, kept exact."""

    def __init__(self, ident, name, degree, knots, ctrl, weights, degree_v=None, knots_v=None, n_u=None, n_v=None,
                 tags=()):
        self.id, self.name = ident, name
        self.surface = degree_v is not None
        self.p, self.U = degree, list(knots)
        self.q, self.V = degree_v, (list(knots_v) if knots_v is not None else None)
        self.ctrl = [list(map(float, c)) for c in ctrl]
        self.w = list(map(float, weights)) if weights is not None else None
        self.n_u = n_u if self.surface else len(ctrl)
        self.n_v = n_v
        self.tags = list(tags)
        check_finite(self.U + (self.V or []) + [x for c in self.ctrl for x in c] + (self.w or []), ident)
        if self.w is not None and any(w <= 0 for w in self.w):
            die(f"{ident}: non-positive weight")
        n = self.n_u
        if len(self.U) != n + self.p + 1:
            die(f"{ident}: knot count")
        if self.surface and (len(self.V) != self.n_v + self.q + 1 or len(self.ctrl) != self.n_u * self.n_v):
            die(f"{ident}: surface shape")
        self._cache = {}

    def num(self, kind):
        if kind not in self._cache:
            conv = NUMS[kind][0]
            U = [conv(x) for x in self.U]
            V = [conv(x) for x in self.V] if self.surface else None
            Pw = []
            for i, c in enumerate(self.ctrl):
                w = conv(self.w[i]) if self.w is not None else conv(1.0)
                Pw.append([w * conv(c[0]), w * conv(c[1]), w * conv(c[2]), w])
            self._cache[kind] = (U, V, Pw)
        return self._cache[kind]

    def domain_u(self):
        return self.U[self.p], self.U[self.n_u]

    def domain_v(self):
        return self.V[self.q], self.V[self.n_v]

    # -- curves
    def curve_ders(self, t, nd, kind, left=False):
        U, _, Pw = self.num(kind)
        _, zero, one = NUMS[kind]
        p, n = self.p, self.n_u
        s = find_span(U, p, n, t, left)
        N = basis_ders(U, p, s, t, nd, zero, one)
        A = []
        for d in range(nd + 1):
            acc = [zero, zero, zero, zero]
            for j in range(p + 1):
                b, pw = N[d][j], Pw[s - p + j]
                acc = [acc[0] + b * pw[0], acc[1] + b * pw[1], acc[2] + b * pw[2], acc[3] + b * pw[3]]
            A.append(acc)
        w0 = A[0][3]
        C0 = [A[0][k] / w0 for k in range(3)]
        out = [C0]
        if nd >= 1:
            w1 = A[1][3]
            C1 = [(A[1][k] - w1 * C0[k]) / w0 for k in range(3)]
            out.append(C1)
        if nd >= 2:
            w2 = A[2][3]
            C2 = [(A[2][k] - 2 * w1 * C1[k] - w2 * C0[k]) / w0 for k in range(3)]
            out.append(C2)
        return out

    # -- surfaces
    def surf_ders(self, u, v, nd, kind, left_u=False, left_v=False):
        """{(k, l): vector} for k + l <= nd (nd <= 2)."""
        U, V, Pw = self.num(kind)
        _, zero, one = NUMS[kind]
        p, q, nu, nv = self.p, self.q, self.n_u, self.n_v
        su = find_span(U, p, nu, u, left_u)
        sv = find_span(V, q, nv, v, left_v)
        Nu = basis_ders(U, p, su, u, nd, zero, one)
        Nv = basis_ders(V, q, sv, v, nd, zero, one)
        tmp = {}
        for l in range(nd + 1):
            rows = []
            for i in range(p + 1):
                base = (su - p + i) * nv + (sv - q)
                acc = [zero, zero, zero, zero]
                for j in range(q + 1):
                    b, pw = Nv[l][j], Pw[base + j]
                    acc = [acc[0] + b * pw[0], acc[1] + b * pw[1], acc[2] + b * pw[2], acc[3] + b * pw[3]]
                rows.append(acc)
            tmp[l] = rows
        A = {}
        for k in range(nd + 1):
            for l in range(nd + 1 - k):
                acc = [zero, zero, zero, zero]
                for i in range(p + 1):
                    b, r = Nu[k][i], tmp[l][i]
                    acc = [acc[0] + b * r[0], acc[1] + b * r[1], acc[2] + b * r[2], acc[3] + b * r[3]]
                A[(k, l)] = acc

        def w(k, l):
            return A[(k, l)][3]

        def a(k, l):
            return A[(k, l)][:3]

        w0 = w(0, 0)
        S = {(0, 0): [x / w0 for x in a(0, 0)]}
        if nd >= 1:
            for kl in ((1, 0), (0, 1)):
                S[kl] = [(a(*kl)[c] - w(*kl) * S[(0, 0)][c]) / w0 for c in range(3)]
        if nd >= 2:
            S00, S10, S01 = S[(0, 0)], S[(1, 0)], S[(0, 1)]
            S[(2, 0)] = [(a(2, 0)[c] - 2 * w(1, 0) * S10[c] - w(2, 0) * S00[c]) / w0 for c in range(3)]
            S[(0, 2)] = [(a(0, 2)[c] - 2 * w(0, 1) * S01[c] - w(0, 2) * S00[c]) / w0 for c in range(3)]
            S[(1, 1)] = [(a(1, 1)[c] - w(1, 0) * S01[c] - w(0, 1) * S10[c] - w(1, 1) * S00[c]) / w0
                         for c in range(3)]
        return S

    # -- f64 grid for seeding
    def grid_params(self, U, p, n, per_span):
        out = []
        b = U[n]
        for s in range(p, n):
            if U[s] == U[s + 1]:
                continue
            for k in range(per_span):
                out.append(U[s] + (U[s + 1] - U[s]) * k / per_span)
        out.append(b)
        return sorted(set(out))

    def json_geometry(self):
        g = {"id": self.id, "name": self.name, "kind": "surface" if self.surface else "curve", "tags": self.tags}
        if self.surface:
            g.update({"degree_u": self.p, "degree_v": self.q, "n_u": self.n_u, "n_v": self.n_v,
                      "knots_u": enc(self.U), "knots_v": enc(self.V), "ctrl": enc(self.ctrl),
                      "weights": enc(self.w) if self.w is not None else None,
                      "periodic_u": False, "periodic_v": False,
                      "domain_u": enc(list(self.domain_u())), "domain_v": enc(list(self.domain_v()))})
        else:
            g.update({"degree": self.p, "knots": enc(self.U), "ctrl": enc(self.ctrl),
                      "weights": enc(self.w) if self.w is not None else None, "periodic": False,
                      "domain": enc(list(self.domain_u()))})
        return g


# ---------------------------------------------------------------- exact evaluation records

def curvature_curve(C1, C2):
    d1 = [dec_of(x) for x in C1]
    d2 = [dec_of(x) for x in C2]
    n1 = dnorm(d1)
    if n1 == 0:
        return None
    return dnorm(cross(d1, d2)) / (n1 * n1 * n1)


def surface_geometry(S):
    """Regularity (decided exactly when S holds Fractions), normal and curvatures at 60 digits."""
    su, sv = S[(1, 0)], S[(0, 1)]
    cr = cross(su, sv)
    a2, b2, c2 = dot(su, su), dot(sv, sv), dot(cr, cr)
    if isinstance(a2, Fraction):
        degenerate = a2 == 0 or b2 == 0 or c2 < DEGEN2 * a2 * b2
    else:
        degenerate = a2 == 0 or b2 == 0 or c2 < D("1e-28") * a2 * b2
    if degenerate:
        return {"regularity": "degenerate", "normal": None, "k1": None, "k2": None, "gaussian": None, "mean": None}
    dsu, dsv = [dec_of(x) for x in su], [dec_of(x) for x in sv]
    dcr = cross(dsu, dsv)
    n = vscale(DONE / dnorm(dcr), dcr)
    E, F, G = dot(dsu, dsu), dot(dsu, dsv), dot(dsv, dsv)
    L = dot([dec_of(x) for x in S[(2, 0)]], n)
    M = dot([dec_of(x) for x in S[(1, 1)]], n)
    N = dot([dec_of(x) for x in S[(0, 2)]], n)
    den = E * G - F * F
    K = (L * N - M * M) / den
    H = (E * N - 2 * F * M + G * L) / (2 * den)
    disc = H * H - K
    if disc < 0:
        disc = DZERO
    r = disc.sqrt()
    return {"regularity": "regular", "normal": enc(n), "k1": enc(H + r), "k2": enc(H - r), "gaussian": enc(K),
            "mean": enc(H)}


def eval_curve_record(c, t, tag=None):
    tf = Fraction(t)
    C = c.curve_ders(tf, 2, "frac")
    k = curvature_curve(C[1], C[2])
    rec = {"t": enc(t), "p": enc(C[0]), "d1": enc(C[1]), "d2": enc(C[2]), "curvature": enc(k) if k is not None else None}
    if tag:
        rec["tag"] = tag
    return rec, C


def eval_surface_record(s, u, v, tag=None):
    S = s.surf_ders(Fraction(u), Fraction(v), 2, "frac")
    rec = {"u": enc(u), "v": enc(v), "p": enc(S[(0, 0)]), "su": enc(S[(1, 0)]), "sv": enc(S[(0, 1)]),
           "suu": enc(S[(2, 0)]), "suv": enc(S[(1, 1)]), "svv": enc(S[(0, 2)])}
    rec.update(surface_geometry(S))
    if tag:
        rec["tag"] = tag
    return rec, S


def domain_checks(lo, hi, surface):
    """Parameters just outside the domain: clamp (within 1e-14 x width) or outside_domain."""
    w = hi - lo
    out = []
    for end, sign in ((lo, -1.0), (hi, 1.0)):
        t_out = end + sign * OUT_MARGIN * w
        t_in = end + sign * 0.25 * CLAMP_MARGIN * w
        out.append({"t": enc(t_out), "expect": "outside_domain"})
        if t_in != end:
            out.append({"t": enc(t_in), "expect": "clamp", "to": enc(end)})
    return out


# ---------------------------------------------------------------- inversion

def solve_pd(H, g):
    """Solve H x = -g for symmetric positive-definite H (1x1 or 2x2); None if not PD."""
    if len(g) == 1:
        if H[0][0] <= 0:
            return None
        return [-g[0] / H[0][0]]
    a, b, c = H[0][0], H[0][1], H[1][1]
    det = a * c - b * b
    if a <= 0 or det <= 0:
        return None
    return [(-g[0] * c + g[1] * b) / det, (g[0] * b - g[1] * a) / det]


def minimize(fgh, x0, lo, hi):
    """Damped Newton (Levenberg-style) on f(x) inside the box [lo, hi]. Returns (x, f, is_min) or
    None if it leaves the box or does not settle. Every returned x is a real point of the entity,
    so a spurious candidate can never make the minimum wrong; is_min marks a positive-definite
    Hessian at convergence (a true local minimum)."""
    x = list(x0)
    f, g, H = fgh(x)
    width = [hi[i] - lo[i] for i in range(len(x))]
    lam = DZERO
    last_small = False
    for _ in range(120):
        d0 = solve_pd(H, g)
        if d0 is not None and all(abs(d0[i]) <= STEP_EPS * width[i] for i in range(len(x))):
            return x, f, True
        if last_small:
            return x, f, False
        accepted = False
        for _ in range(80):
            scale = max(abs(H[i][i]) for i in range(len(x))) or DONE
            Hl = [[H[i][j] + (lam * scale if i == j else DZERO) for j in range(len(x))] for i in range(len(x))]
            d = solve_pd(Hl, g)
            if d is not None:
                xn = [x[i] + d[i] for i in range(len(x))]
                if all(lo[i] <= xn[i] <= hi[i] for i in range(len(x))):
                    fn, gn, Hn = fgh(xn)
                    if fn <= f:
                        last_small = all(abs(d[i]) <= STEP_EPS * width[i] for i in range(len(x)))
                        x, f, g, H = xn, fn, gn, Hn
                        accepted = True
                        break
            lam = D("1e-8") if lam == 0 else lam * 10
            if lam > D("1e40"):
                break
        if not accepted:
            return None
        lam = DZERO if lam < D("1e-6") else lam / 100
    return None


def surface_fgh(s, P, mode, fixed):
    def fgh(x):
        if mode == "uv":
            u, v = x
        elif mode == "u":
            u, v = x[0], fixed
        else:
            u, v = fixed, x[0]
        S = s.surf_ders(u, v, 2, "dec")
        r = vsub(S[(0, 0)], P)
        f = dot(r, r)
        gu, gv = dot(S[(1, 0)], r), dot(S[(0, 1)], r)
        huu = dot(S[(1, 0)], S[(1, 0)]) + dot(S[(2, 0)], r)
        huv = dot(S[(1, 0)], S[(0, 1)]) + dot(S[(1, 1)], r)
        hvv = dot(S[(0, 1)], S[(0, 1)]) + dot(S[(0, 2)], r)
        if mode == "uv":
            return f, [gu, gv], [[huu, huv], [huv, hvv]]
        if mode == "u":
            return f, [gu], [[huu]]
        return f, [gv], [[hvv]]
    return fgh


def curve_fgh(c, P):
    def fgh(x):
        C = c.curve_ders(x[0], 2, "dec")
        r = vsub(C[0], P)
        return dot(r, r), [dot(C[1], r)], [[dot(C[1], C[1]) + dot(C[2], r)]]
    return fgh


class Grid:
    """f64 samples of a curve or surface for seeding the inversion."""

    def __init__(self, e, per_span):
        self.e = e
        U, V, Pw = e.num("float")
        self.us = e.grid_params(U, e.p, e.n_u, per_span)
        if not e.surface:
            self.pts = []
            for t in self.us:
                s = find_span(U, e.p, e.n_u, t)
                N = basis_ders(U, e.p, s, t, 0, 0.0, 1.0)[0]
                acc = [0.0, 0.0, 0.0, 0.0]
                for j in range(e.p + 1):
                    pw = Pw[s - e.p + j]
                    acc = [acc[k] + N[j] * pw[k] for k in range(4)]
                self.pts.append((acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3]))
            self.h = max(math.dist(self.pts[i], self.pts[i + 1]) for i in range(len(self.pts) - 1))
            return
        self.vs = e.grid_params(V, e.q, e.n_v, per_span)
        nu, nv, p, q = e.n_u, e.n_v, e.p, e.q
        bu = []
        for u in self.us:
            s = find_span(U, p, nu, u)
            bu.append((s, basis_ders(U, p, s, u, 0, 0.0, 1.0)[0]))
        cols = []
        for v in self.vs:
            s = find_span(V, q, nv, v)
            N = basis_ders(V, q, s, v, 0, 0.0, 1.0)[0]
            rows = []
            for i in range(nu):
                acc = [0.0, 0.0, 0.0, 0.0]
                for j in range(q + 1):
                    pw = Pw[i * nv + s - q + j]
                    acc = [acc[k] + N[j] * pw[k] for k in range(4)]
                rows.append(acc)
            cols.append(rows)
        self.pts = []  # row-major over (iu, iv)
        for (s, N) in bu:
            for rows in cols:
                acc = [0.0, 0.0, 0.0, 0.0]
                for i in range(p + 1):
                    r = rows[s - p + i]
                    acc = [acc[k] + N[i] * r[k] for k in range(4)]
                self.pts.append((acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3]))
        m = len(self.vs)
        h = 0.0
        for iu in range(len(self.us) - 1):
            for iv in range(m - 1):
                a = self.pts[iu * m + iv]
                h = max(h, math.dist(a, self.pts[(iu + 1) * m + iv + 1]), math.dist(self.pts[(iu + 1) * m + iv],
                                                                                    self.pts[iu * m + iv + 1]))
        self.h = h


MAX_SEEDS = 16
KKT_EPS = D("1e-40")
SAME = D("1e-20")


def breaks(K, p, n):
    """Domain ends plus interior knots of multiplicity >= p (C0 or worse: the distance function has
    a kink there, so they are treated as internal boundaries of the search)."""
    lo, hi = K[p], K[n]
    out = [lo]
    for k in sorted(set(K)):
        if lo < k < hi and K.count(k) >= p:
            out.append(k)
    out.append(hi)
    return out


def one_sided_ok(g_right, g_left, x, lo, hi):
    """First-order test that f does not decrease when leaving x along this parameter."""
    ok = True
    if x < hi:
        ok = ok and g_right >= -KKT_EPS
    if x > lo:
        ok = ok and g_left <= KKT_EPS
    return ok


def local_minima_1d(vals, thr):
    n = len(vals)
    return [i for i in range(n) if vals[i] <= thr and (i == 0 or vals[i] <= vals[i - 1]) and
            (i == n - 1 or vals[i] <= vals[i + 1])]


def invert(e, grid, Pf, rng_tag=None):
    """Global closest point of f64 point Pf on entity e. Returns a query record (without id)."""
    P = [D(x) for x in Pf]
    d2 = [(a - Pf[0]) ** 2 + (b - Pf[1]) ** 2 + (c - Pf[2]) ** 2 for (a, b, c) in grid.pts]
    dmin = math.sqrt(min(d2))
    thr = (dmin + 2.0 * grid.h + 1e-12) ** 2
    cands = []  # (params, f, on_domain_boundary, is_local_min)
    U = e.num("dec")[0]
    ulo, uhi = U[e.p], U[e.n_u]
    bu = breaks(U, e.p, e.n_u)
    gu = [+D(x) for x in grid.us]
    if not e.surface:
        fgh = curve_fgh(e, P)
        seeds = sorted(local_minima_1d(d2, thr), key=lambda i: (d2[i], i))[:MAX_SEEDS]
        for i in seeds:
            for a, b in zip(bu, bu[1:]):
                if a <= gu[i] <= b:
                    r = minimize(fgh, [gu[i]], [a], [b])
                    if r is not None:
                        cands.append(((r[0][0],), r[1], False, r[2]))
        for t in bu:
            C = e.curve_ders(t, 1, "dec")
            Cl = e.curve_ders(t, 1, "dec", left=True)
            r = vsub(C[0], P)
            local = one_sided_ok(dot(C[1], r), dot(Cl[1], r), t, ulo, uhi)
            cands.append(((t,), dot(r, r), t in (ulo, uhi), local))
        widths = [uhi - ulo]
    else:
        V = e.num("dec")[1]
        vlo, vhi = V[e.q], V[e.n_v]
        bv = breaks(V, e.q, e.n_v)
        gv = [+D(x) for x in grid.vs]
        nu, nv = len(gu), len(gv)

        def at(iu, iv):
            return d2[iu * nv + iv]

        seeds = []
        for idx, val in enumerate(d2):
            if val > thr:
                continue
            iu, iv = divmod(idx, nv)
            ok = True
            for du in (-1, 0, 1):
                for dv in (-1, 0, 1):
                    ju, jv = iu + du, iv + dv
                    if (du or dv) and 0 <= ju < nu and 0 <= jv < nv and at(ju, jv) < val:
                        ok = False
            if ok:
                seeds.append(idx)
        seeds.sort(key=lambda i: (d2[i], i))
        fuv = surface_fgh(e, P, "uv", None)
        for idx in seeds[:MAX_SEEDS]:
            iu, iv = divmod(idx, nv)
            u0, v0 = gu[iu], gv[iv]
            for a, b in zip(bu, bu[1:]):
                if not a <= u0 <= b:
                    continue
                for c, d in zip(bv, bv[1:]):
                    if not c <= v0 <= d:
                        continue
                    r = minimize(fuv, [u0, v0], [a, c], [b, d])
                    if r is not None:
                        cands.append(((r[0][0], r[0][1]), r[1], False, r[2]))
        # break lines: u fixed (move v inside each v piece), v fixed (move u inside each u piece)
        line_pts = []
        for uu in bu:
            iu = gu.index(uu)
            line = [at(iu, iv) for iv in range(nv)]
            fg = surface_fgh(e, P, "v", uu)
            for iv in sorted(local_minima_1d(line, thr), key=lambda i: (line[i], i))[:4]:
                for c, d in zip(bv, bv[1:]):
                    if c <= gv[iv] <= d:
                        r = minimize(fg, [gv[iv]], [c], [d])
                        if r is not None:
                            line_pts.append((uu, r[0][0]))
        for vv in bv:
            iv = gv.index(vv)
            line = [at(iu, iv) for iu in range(nu)]
            fg = surface_fgh(e, P, "u", vv)
            for iu in sorted(local_minima_1d(line, thr), key=lambda i: (line[i], i))[:4]:
                for a, b in zip(bu, bu[1:]):
                    if a <= gu[iu] <= b:
                        r = minimize(fg, [gu[iu]], [a], [b])
                        if r is not None:
                            line_pts.append((r[0][0], vv))
        line_pts += [(uu, vv) for uu in bu for vv in bv]
        for (uu, vv) in line_pts:
            S = e.surf_ders(uu, vv, 1, "dec")
            r = vsub(S[(0, 0)], P)
            Sl = e.surf_ders(uu, vv, 1, "dec", left_u=True, left_v=True)
            local = (one_sided_ok(dot(S[(1, 0)], r), dot(Sl[(1, 0)], r), uu, ulo, uhi) and
                     one_sided_ok(dot(S[(0, 1)], r), dot(Sl[(0, 1)], r), vv, vlo, vhi))
            onb = uu in (ulo, uhi) or vv in (vlo, vhi)
            cands.append(((uu, vv), dot(r, r), onb, local))
        widths = [uhi - ulo, vhi - vlo]
    full = sorted(((c[0], c[1].sqrt(), c[2], c[3]) for c in cands), key=lambda c: (c[1], c[0]))
    dbest = full[0][1]
    best = sorted((c for c in full if c[1] - dbest <= TIE), key=lambda c: c[0])[0]

    def same(a, b):
        return all(abs(a[0][i] - b[0][i]) <= SAME * widths[i] for i in range(len(widths)))

    distinct = []
    for c in full:
        if c[3] and not any(same(c, o) for o in distinct):
            distinct.append(c)
    if not any(same(best, o) for o in distinct):
        distinct.append(best)
    others = [o for o in distinct if not same(best, o)]
    alts = sorted([o for o in others if o[1] - dbest <= NEAR_TIE], key=lambda o: (o[1], o[0]))
    tags = []
    if any(o[1] - dbest <= TIE for o in others):
        tags.append("tie")
    elif alts:
        tags.append("near_tie")
    if len(distinct) >= 2:
        tags.append("multi_local")
    if best[2]:
        tags.append("boundary")
    if rng_tag:
        tags.append(rng_tag)
    if e.surface:
        foot = e.surf_ders(best[0][0], best[0][1], 0, "dec")[(0, 0)]
        rec = {"q": enc(Pf), "u": enc(best[0][0]), "v": enc(best[0][1])}
    else:
        foot = e.curve_ders(best[0][0], 0, "dec")[0]
        rec = {"q": enc(Pf), "t": enc(best[0][0])}
    rec.update({"foot": enc(foot), "distance": enc(best[1]), "on_boundary": bool(best[2]),
                "local_minima": len(distinct), "tags": tags,
                "alternatives": [{"params": enc(list(o[0])), "distance": enc(o[1])} for o in alts]})
    return rec


def gen_queries(e, grid, rng, n, start=0):
    """Query points around the entity: near (1e-7..1e-3 m), mid, far, off-boundary, on-surface."""
    out = []
    ulo, uhi = e.domain_u()
    if e.surface:
        vlo, vhi = e.domain_v()
    size = max(max(abs(x) for x in c) for c in e.ctrl) or 1.0
    k = 0
    while len(out) < n:
        k += 1
        r = rng.random()
        u = ulo + (uhi - ulo) * rng.random()
        v = (vlo + (vhi - vlo) * rng.random()) if e.surface else None
        kind = "random"
        if r < 0.2:
            kind = "off_boundary"
            side = int(rng.random() * (4 if e.surface else 2))
            if side == 0:
                u = ulo
            elif side == 1:
                u = uhi
            elif side == 2:
                v = vlo
            else:
                v = vhi
        if e.surface:
            S = e.surf_ders(Fraction(u), Fraction(v), 1, "frac")
            base = [float(x) for x in S[(0, 0)]]
        else:
            C = e.curve_ders(Fraction(u), 1, "frac")
            base = [float(x) for x in C[0]]
        dv = [rng.gauss(0.0, 1.0) for _ in range(3)]
        nn = math.sqrt(sum(x * x for x in dv)) or 1.0
        dv = [x / nn for x in dv]
        if kind == "off_boundary":
            mag = size * (0.01 + 0.3 * rng.random())
        elif r < 0.45:
            kind = "near"
            mag = 10.0 ** (-7 + 4 * rng.random())
        elif r < 0.85:
            mag = size * (0.02 + 0.3 * rng.random())
        elif r < 0.95:
            kind = "far"
            mag = size * (0.5 + 1.5 * rng.random())
        else:
            kind = "on_entity"
            mag = 0.0
        Pf = [base[i] + mag * dv[i] for i in range(3)]
        rec = invert(e, grid, Pf, kind)
        rec = {"id": f"{e.id}-q{start + len(out):04d}", **rec}
        out.append(rec)
    return out


# ---------------------------------------------------------------- entity builders

def bezier_knots(p, a=0.0, b=1.0):
    return [a] * (p + 1) + [b] * (p + 1)


def random_knots(rng, p, n, a=0.0, b=1.0, repeat=0, clamped=True):
    """Knot vector for n control points: clamped ends, sorted random interior knots; `repeat`
    raises one interior knot to that multiplicity (<= p)."""
    m = n - p - 1
    interior = []
    if m > 0:
        if repeat >= 2 and m >= repeat:
            distinct = m - repeat + 1
        else:
            repeat, distinct = 0, m
        vals = sorted(a + (b - a) * (0.08 + 0.84 * rng.random()) for _ in range(distinct))
        for i in range(1, len(vals)):  # keep spans non-trivial
            if vals[i] <= vals[i - 1]:
                vals[i] = math.nextafter(vals[i - 1], math.inf)
        if repeat:
            pick = int(rng.random() * distinct)
            vals = vals[:pick] + [vals[pick]] * (repeat - 1) + vals[pick:]
        interior = vals
    if clamped:
        return [a] * (p + 1) + interior + [b] * (p + 1)
    step = (b - a) / 4.0
    return [a - step * (p - i) for i in range(p)] + [a] + interior + [b] + [b + step * (i + 1) for i in range(p)]


def random_ctrl(rng, n, spread=0.5):
    pts = []
    for i in range(n):
        t = i / max(n - 1, 1)
        pts.append([-spread + 2 * spread * t + 0.1 * (rng.random() - 0.5),
                    spread * (rng.random() - 0.5) * 1.6,
                    spread * (rng.random() - 0.5) * 1.6])
    return pts


def random_weights(rng, n):
    return [0.5 + 1.5 * rng.random() for _ in range(n)]


def height_net(rng, nu, nv, amp=0.15, size=1.0):
    pts = []
    for i in range(nu):
        for j in range(nv):
            x = size * (i / (nu - 1) - 0.5) + 0.02 * (rng.random() - 0.5)
            y = size * (j / (nv - 1) - 0.5) + 0.02 * (rng.random() - 0.5)
            z = amp * (2 * rng.random() - 1)
            pts.append([x, y, z])
    return pts


S2 = math.sqrt(0.5)


def circle_curve(ident, r=1.0):
    """The 9-point rational quadratic unit circle (repeated interior knots)."""
    pts = [[r, 0, 0], [r, r, 0], [0, r, 0], [-r, r, 0], [-r, 0, 0], [-r, -r, 0], [0, -r, 0], [r, -r, 0], [r, 0, 0]]
    w = [1, S2, 1, S2, 1, S2, 1, S2, 1]
    U = [0, 0, 0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1, 1, 1]
    return Spline(ident, "NURBS unit circle (9-point quadratic)", 2, U, pts, w, tags=["named:circle"])


def sphere_surface(ident, r):
    """Revolution of a rational quadratic semicircle (meridian, v) about z by the 9-point circle (u)."""
    circ = [[1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0], [-1, -1], [0, -1], [1, -1], [1, 0]]
    cw = [1, S2, 1, S2, 1, S2, 1, S2, 1]
    mer = [[0, -r], [r, -r], [r, 0], [r, r], [0, r]]  # (radius, z) from south to north pole
    mw = [1, S2, 1, S2, 1]
    pts, w = [], []
    for i, (cx, cy) in enumerate(circ):
        for j, (rr, z) in enumerate(mer):
            pts.append([cx * rr, cy * rr, z])
            w.append(cw[i] * mw[j])
    U = [0, 0, 0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1, 1, 1]
    V = [0, 0, 0, 0.5, 0.5, 1, 1, 1]
    return Spline(ident, f"NURBS sphere radius {r} m (poles at v = 0 and v = 1)", 2, U, pts, w, degree_v=2,
                  knots_v=V, n_u=9, n_v=5, tags=["named:sphere"])


def cylinder_surface(ident, r, h):
    circ = [[1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0], [-1, -1], [0, -1], [1, -1], [1, 0]]
    cw = [1, S2, 1, S2, 1, S2, 1, S2, 1]
    pts, w = [], []
    for i, (cx, cy) in enumerate(circ):
        for z in (0.0, h):
            pts.append([cx * r, cy * r, z])
            w.append(cw[i])
    U = [0, 0, 0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1, 1, 1]
    return Spline(ident, f"NURBS cylinder radius {r} m, height {h} m", 2, U, pts, w, degree_v=1,
                  knots_v=[0, 0, 1, 1], n_u=9, n_v=2, tags=["named:cylinder"])


def build_ref_entities(rng):
    curves, surfaces = [], []
    k = 0
    for p in range(1, 26):
        for rational in (False, True):
            k += 1
            m = int(rng.random() * 4) if p < 20 else int(rng.random() * 2)
            n = p + 1 + m
            repeat = 0
            tags = []
            if p >= 2 and (p + int(rational)) % 3 == 0:
                m = max(m, p)
                n = p + 1 + m
                repeat = p if p <= 5 else 2
            if p == 1 and rational:
                n = p + 1 + 3
                repeat = 0
            a, b = (0.0, 1.0)
            if k % 5 == 0:
                a = -2.0 + rng.random()
                b = a + 0.5 + 3 * rng.random()
            clamped = not (k in (7, 30))
            U = random_knots(rng, p, n, a, b, repeat, clamped)
            if not clamped:
                tags.append("unclamped")
            c = Spline(f"c{p:02d}{'r' if rational else 'n'}", f"degree {p} {'rational' if rational else 'B-spline'} "
                       f"curve", p, U, random_ctrl(rng, n), random_weights(rng, n) if rational else None, tags=tags)
            curves.append(c)
    curves.append(circle_curve("circle"))
    specs = [
        ("s0101n", 1, 1, 2, 2, False, 0, 0),
        ("s0202r", 2, 2, 5, 4, True, 0, 0),
        ("s0303n", 3, 3, 7, 6, False, 3, 2),
        ("s0302r", 3, 2, 6, 5, True, 2, 0),
        ("s0504n", 5, 4, 7, 6, False, 0, 0),
        ("s0207r", 2, 7, 5, 9, True, 2, 0),
        ("s2502n", 25, 2, 26, 4, False, 0, 0),
        ("s0425r", 4, 25, 5, 26, True, 0, 0),
        ("s1210n", 12, 10, 14, 11, False, 0, 0),
    ]
    for ident, p, q, nu, nv, rational, rep_u, rep_v in specs:
        U = random_knots(rng, p, nu, 0.0, 1.0, rep_u)
        V = random_knots(rng, q, nv, 0.0, 1.0, rep_v)
        n = nu * nv
        surfaces.append(Spline(ident, f"degree ({p}, {q}) {'rational' if rational else 'B-spline'} surface", p, U,
                               height_net(rng, nu, nv), random_weights(rng, n) if rational else None, degree_v=q,
                               knots_v=V, n_u=nu, n_v=nv))
    surfaces.append(sphere_surface("sphere", 0.5))
    surfaces.append(cylinder_surface("cylinder", 0.01, 0.05))
    return curves, surfaces


# ---------------------------------------------------------------- ref_set_v1

INVERT_CURVES = {"c01n": 100, "c02n": 100, "c02r": 100, "c03n": 100, "c03r": 100, "c04r": 100, "c05n": 100,
                 "c06r": 100, "c07n": 100, "c09r": 100, "c10n": 100, "c13r": 100, "c15n": 100, "c20r": 100,
                 "c25n": 100, "c25r": 100, "circle": 200}
INVERT_SURFACES = {"s0101n": 400, "s0202r": 400, "s0303n": 400, "s0302r": 400, "s0504n": 400, "s0207r": 400,
                   "sphere": 400, "cylinder": 400}


def surface_eval_params(s, rng, n):
    ulo, uhi = s.domain_u()
    vlo, vhi = s.domain_v()
    out = [(ulo, vlo, "corner"), (ulo, vhi, "corner"), (uhi, vlo, "corner"), (uhi, vhi, "corner")]
    ku = sorted(set(x for x in s.U if ulo < x < uhi))
    kv = sorted(set(x for x in s.V if vlo < x < vhi))
    for x in ku[:4]:
        out.append((x, vlo + (vhi - vlo) * rng.random(), "knot"))
    for y in kv[:4]:
        out.append((ulo + (uhi - ulo) * rng.random(), y, "knot"))
    while len(out) < n:
        out.append((ulo + (uhi - ulo) * rng.random(), vlo + (vhi - vlo) * rng.random(), None))
    return out


def curve_eval_params(c, rng, n):
    lo, hi = c.domain_u()
    out = [(lo, "end"), (hi, "end")]
    for x in sorted(set(x for x in c.U if lo < x < hi)):
        out.append((x, "knot"))
    while len(out) < n:
        out.append((lo + (hi - lo) * rng.random(), None))
    return out


def build_ref_set(seed, log):
    rng = random.Random(seed)
    curves, surfaces = build_ref_entities(rng)
    entities = []
    n_eval = n_inv = 0
    for c in curves:
        g = c.json_geometry()
        npar = 100 if c.id == "circle" else (30 if c.p <= 15 else 16)
        recs = []
        for t, tag in curve_eval_params(c, rng, npar):
            rec, C = eval_curve_record(c, t, tag)
            recs.append(rec)
            if c.id == "circle":  # AC-2: sanity of the oracle itself
                x, y = float(C[0][0]), float(C[0][1])
                rad = (D(x) ** 2 + D(y) ** 2).sqrt()
                if abs(rad - 1) > D("1e-15") or float(C[0][2]) != 0.0:
                    die(f"circle sanity failed at t={t!r}: radius {rad}")
        g["eval"] = recs
        g["domain_checks"] = domain_checks(*c.domain_u(), False)
        nq = INVERT_CURVES.get(c.id, 0)
        if nq:
            grid = Grid(c, 200)
            g["invert"] = gen_queries(c, grid, rng, nq)
        else:
            g["invert"] = []
        n_eval += len(recs)
        n_inv += nq
        entities.append(g)
        log(f"  {c.id}: {len(recs)} evals, {nq} inversions")
    for s in surfaces:
        g = s.json_geometry()
        npar = 60 if max(s.p, s.q) <= 7 else 14
        recs = []
        for u, v, tag in surface_eval_params(s, rng, npar):
            recs.append(eval_surface_record(s, u, v, tag)[0])
        if s.id == "sphere":
            for u in (0.0, 0.125, 0.3, 0.5):
                for v in (0.0, 1.0):
                    rec = eval_surface_record(s, u, v, "pole")[0]
                    if rec["regularity"] != "degenerate":
                        die("sphere pole not degenerate")
                    recs.append(rec)
        if s.id == "cylinder":
            while len(recs) < 100 + npar:
                rec = eval_surface_record(s, rng.random(), rng.random(), "curvature")[0]
                for kk, want in (("k1", 0.0), ("k2", -100.0)):
                    if abs(rec[kk]["dec"] - want) > 1e-9 * 100:
                        die(f"cylinder curvature {kk} = {rec[kk]['dec']}")
                recs.append(rec)
        g["eval"] = recs
        g["domain_checks"] = {"u": domain_checks(*s.domain_u(), True), "v": domain_checks(*s.domain_v(), True)}
        nq = INVERT_SURFACES.get(s.id, 0)
        if nq:
            grid = Grid(s, 40 if max(s.p, s.q) <= 3 else 24)
            g["invert"] = gen_queries(s, grid, rng, nq)
        else:
            g["invert"] = []
        n_eval += len(recs)
        n_inv += nq
        entities.append(g)
        log(f"  {s.id}: {len(recs)} evals, {nq} inversions")
    head = {"format": FORMAT, "oracle_version": ORACLE_VERSION, "seed": seed, "file_seed": seed,
            "license": "CC-BY-4.0", "unit": "m", "position_tolerance_m": POS_TOL,
            "derivative_tolerance_rel": DER_TOL_REL, "conventions": CONVENTIONS}
    return render(head, entities), seed, {"entities": len(entities), "evals": n_eval, "inversions": n_inv}


# ---------------------------------------------------------------- multi_minima

def wavy_surface():
    """Bicubic height field symmetric under u -> 1 - u (x -> -x): two crests at x = +-0.25, a trough
    at x = 0, so query points on the plane x = 0 above the trough are equidistant to both crests."""
    U = [0, 0, 0, 0, 0.25, 0.5, 0.75, 1, 1, 1, 1]
    V = [0, 0, 0, 0, 0.5, 1, 1, 1, 1]
    xs = [-0.5, -0.375, -0.25, 0.0, 0.25, 0.375, 0.5]
    zs = [-0.0625, 0.25, 0.1875, -0.375, 0.1875, 0.25, -0.0625]
    ys = [-0.5, -0.25, 0.0, 0.25, 0.5]
    wv = [0.0, 0.03125, 0.0625, 0.03125, 0.0]
    pts = [[xs[i], ys[j], zs[i] + wv[j]] for i in range(7) for j in range(5)]
    return Spline("wavy", "wavy bicubic surface, mirror-symmetric in x (two crests)", 3, U, pts, None, degree_v=3,
                  knots_v=V, n_u=7, n_v=5, tags=["multi_minima", "mirror_x"])


def cusp_surface():
    """Cubic Bezier with a cusp at t = 1/2 (control polygon (0,0),(1,1),(0,1),(1,0)) extruded along z:
    S_u = 0 along u = 1/2, cusp tip at (1/2, 3/4)."""
    prof = [[0.0, 0.0], [1.0, 1.0], [0.0, 1.0], [1.0, 0.0]]
    pts = [[x, y, z] for (x, y) in prof for z in (0.0, 0.5)]
    return Spline("cusp", "cubic Bezier cusp (S_u = 0 at u = 1/2) extruded along z", 3, bezier_knots(3), pts, None,
                  degree_v=1, knots_v=[0, 0, 1, 1], n_u=4, n_v=2, tags=["cusp"])


def build_multi_minima(seed, log):
    rng = random.Random(seed)
    entities = []
    w = wavy_surface()
    grid = Grid(w, 48)
    g = w.json_geometry()
    g["eval"] = [eval_surface_record(w, u, v, t)[0] for u, v, t in surface_eval_params(w, rng, 20)]
    qs = []
    tries = 0
    while sum("equidistant" in q["tags"] for q in qs) < 60:
        tries += 1
        if tries > 1000:
            die("could not generate equidistant queries")
        y = -0.4 + 0.8 * rng.random()
        h = 0.0 + 0.25 * rng.random()
        rec = invert(w, grid, [0.0, y, h], "equidistant")
        if "tie" not in rec["tags"]:
            continue
        qs.append({"id": f"wavy-q{len(qs):04d}", **rec})
    gen = gen_queries(w, grid, rng, 140, start=len(qs))
    g["invert"] = qs + gen
    entities.append(g)
    log(f"  wavy: {len(qs)} equidistant + {len(gen)} generic queries")
    c = cusp_surface()
    grid = Grid(c, 200)
    g = c.json_geometry()
    g["eval"] = [eval_surface_record(c, u, v, t)[0] for u, v, t in
                 [(0.5, 0.0, "cusp"), (0.5, 0.5, "cusp"), (0.5, 1.0, "cusp")] + surface_eval_params(c, rng, 12)]
    qs = []
    for k in range(20):
        h = 10.0 ** (-6 + 5 * rng.random())
        z = 0.5 * rng.random()
        rec = invert(c, grid, [0.5, 0.75 + h, z], "above_cusp")
        qs.append({"id": f"cusp-q{k:04d}", **rec})
    gen = gen_queries(c, grid, rng, 40, start=len(qs))
    g["invert"] = qs + gen
    entities.append(g)
    log(f"  cusp: {len(qs)} above-tip + {len(gen)} generic queries")
    head = {"format": FORMAT, "oracle_version": ORACLE_VERSION, "seed": seed - 3, "file_seed": seed,
            "license": "CC-BY-4.0", "unit": "m", "position_tolerance_m": POS_TOL, "conventions": CONVENTIONS}
    return render(head, entities), seed, {"entities": len(entities), "inversions": sum(len(e["invert"]) for e in entities)}


# ---------------------------------------------------------------- analytic_set

def rand_frame(rng):
    """Random orthonormal frame at 60 digits, rounded to f64 (orthonormal within ~1e-16)."""
    while True:
        z = [D(rng.gauss(0.0, 1.0)) for _ in range(3)]
        a = [D(rng.gauss(0.0, 1.0)) for _ in range(3)]
        nz = dnorm(z)
        if nz < D("0.1"):
            continue
        z = vscale(DONE / nz, z)
        a = vsub(a, vscale(dot(a, z), z))
        na = dnorm(a)
        if na < D("0.1"):
            continue
        x = vscale(DONE / na, a)
        return [float(v) for v in z], [float(v) for v in x]


class Analytic:
    def __init__(self, ident, typ, origin, axis, ref_dir, params, aligned):
        self.id, self.type, self.aligned = ident, typ, aligned
        self.o, self.z, self.x = origin, axis, ref_dir
        self.params = params  # dict name -> f64
        check_finite(origin + axis + ref_dir + list(params.values()), ident)
        self.O = [D(v) for v in origin]
        self.Z = [D(v) for v in axis]
        self.X = [D(v) for v in ref_dir]
        self.Y = cross(self.Z, self.X)
        self.P = {k: D(v) for k, v in params.items()}
        self.curve = typ in ("line", "circle", "ellipse")

    def json(self):
        return {"id": self.id, "type": self.type, "kind": "curve" if self.curve else "surface",
                "placement": {"origin": enc(self.o), "axis": enc(self.z), "ref_dir": enc(self.x)},
                "params": {k: enc(v) for k, v in self.params.items()}}

    def w(self, a, b, c):
        """O + aX + bY + cZ."""
        return [self.O[i] + a * self.X[i] + b * self.Y[i] + c * self.Z[i] for i in range(3)]

    def lin(self, a, b, c):
        return [a * self.X[i] + b * self.Y[i] + c * self.Z[i] for i in range(3)]

    def curve_ders(self, t):
        P, t = self.P, D(t)
        if self.type == "line":
            return [self.w(t, DZERO, DZERO), self.lin(DONE, DZERO, DZERO), [DZERO] * 3]
        s, c = sin_cos(t)
        a, b = (P["radius"], P["radius"]) if self.type == "circle" else (P["semi_axis_1"], P["semi_axis_2"])
        return [self.w(a * c, b * s, DZERO), self.lin(-a * s, b * c, DZERO), self.lin(-a * c, -b * s, DZERO)]

    def surf_ders(self, u, v):
        P, u, v = self.P, D(u), D(v)
        Z0 = DZERO
        if self.type == "plane":
            return {(0, 0): self.w(u, v, Z0), (1, 0): self.lin(DONE, Z0, Z0), (0, 1): self.lin(Z0, DONE, Z0),
                    (2, 0): [Z0] * 3, (1, 1): [Z0] * 3, (0, 2): [Z0] * 3}
        su, cu = sin_cos(u)
        if self.type == "cylinder":
            r = P["radius"]
            return {(0, 0): self.w(r * cu, r * su, v), (1, 0): self.lin(-r * su, r * cu, Z0),
                    (0, 1): self.lin(Z0, Z0, DONE), (2, 0): self.lin(-r * cu, -r * su, Z0), (1, 1): [Z0] * 3,
                    (0, 2): [Z0] * 3}
        if self.type == "cone":
            sa, ca = sin_cos(P["semi_angle"])
            ta = sa / ca
            rho = P["radius"] + v * ta
            return {(0, 0): self.w(rho * cu, rho * su, v), (1, 0): self.lin(-rho * su, rho * cu, Z0),
                    (0, 1): self.lin(ta * cu, ta * su, DONE), (2, 0): self.lin(-rho * cu, -rho * su, Z0),
                    (1, 1): self.lin(-ta * su, ta * cu, Z0), (0, 2): [Z0] * 3}
        sv, cv = sin_cos(v)
        if self.type == "sphere":
            r = P["radius"]
            return {(0, 0): self.w(r * cv * cu, r * cv * su, r * sv), (1, 0): self.lin(-r * cv * su, r * cv * cu, Z0),
                    (0, 1): self.lin(-r * sv * cu, -r * sv * su, r * cv),
                    (2, 0): self.lin(-r * cv * cu, -r * cv * su, Z0), (1, 1): self.lin(r * sv * su, -r * sv * cu, Z0),
                    (0, 2): self.lin(-r * cv * cu, -r * cv * su, -r * sv)}
        R, r = P["major_radius"], P["minor_radius"]
        rho = R + r * cv
        return {(0, 0): self.w(rho * cu, rho * su, r * sv), (1, 0): self.lin(-rho * su, rho * cu, Z0),
                (0, 1): self.lin(-r * sv * cu, -r * sv * su, r * cv), (2, 0): self.lin(-rho * cu, -rho * su, Z0),
                (1, 1): self.lin(r * sv * su, -r * sv * cu, Z0), (0, 2): self.lin(-r * cv * cu, -r * cv * su, -r * sv)}

    def local(self, Pd):
        d = vsub(Pd, self.O)
        return dot(d, self.X), dot(d, self.Y), dot(d, self.Z)

    def invert(self, Pf):
        """Closed-form closest point (ellipse: grid + Newton). Returns (params, foot, dist, non_unique)."""
        Pd = [D(v) for v in Pf]
        x, y, z = self.local(Pd)
        P = self.P
        nu = False
        if self.type == "line":
            prm = [x]
        elif self.type == "circle":
            if x == 0 and y == 0:
                prm, nu = [DZERO], True
            else:
                prm = [wrap_2pi(atan2(y, x))]
        elif self.type == "ellipse":
            prm, nu = self.ellipse_invert(x, y)
        elif self.type == "plane":
            prm = [x, y]
        else:
            rho = (x * x + y * y).sqrt()
            if rho == 0:
                u, nu = DZERO, True
            else:
                u = wrap_2pi(atan2(y, x))
            if self.type == "cylinder":
                prm = [u, z]
            elif self.type == "sphere":
                if rho == 0 and z == 0:
                    prm = [DZERO, DZERO]  # centre: every point is closest
                else:
                    nu = False  # on the axis the pole is the unique foot; u = 0 by convention
                    prm = [u, atan2(z, rho)]
            elif self.type == "cone":
                sa, ca = sin_cos(P["semi_angle"])
                s = (rho - P["radius"]) * sa + z * ca
                rf = P["radius"] + s * sa
                if rf <= 0:
                    raise ValueError("apex region")
                d_same = ((rho - rf) ** 2 + (z - s * ca) ** 2).sqrt()
                # the other nappe (r + v tan a < 0) seen in this half-plane: line rho = -(r + z tan a)
                s2 = -(rho + P["radius"]) * sa + z * ca
                rf2 = -P["radius"] - s2 * sa
                if rf2 >= 0 and ((rho - rf2) ** 2 + (z - s2 * ca) ** 2).sqrt() <= d_same + D("1e-12"):
                    raise ValueError("other nappe as close")
                prm = [u, s * ca]
            else:  # torus
                mx, mz = rho - P["major_radius"], z
                if mx == 0 and mz == 0:
                    prm, nu = [u, DZERO], True
                else:
                    prm = [u, wrap_2pi(atan2(mz, mx))]
        if self.curve:
            foot = self.curve_ders(prm[0])[0]
        else:
            foot = self.surf_ders(prm[0], prm[1])[(0, 0)]
        dist = dnorm(vsub(foot, Pd))
        return prm, foot, dist, nu

    def ellipse_invert(self, x, y):
        a, b = self.P["semi_axis_1"], self.P["semi_axis_2"]
        n = 1440
        best = []
        fx, fy, fa, fb = float(x), float(y), float(a), float(b)
        vals = [(fa * math.cos(2 * math.pi * k / n) - fx) ** 2 + (fb * math.sin(2 * math.pi * k / n) - fy) ** 2
                for k in range(n)]
        seeds = [k for k in range(n) if vals[k] <= vals[k - 1] and vals[k] <= vals[(k + 1) % n]]

        def fgh(tv):
            s, c = sin_cos(tv[0])
            ex, ey = a * c - x, b * s - y
            f = ex * ex + ey * ey
            g = -a * s * ex + b * c * ey
            h = a * a * s * s + b * b * c * c - a * c * ex - b * s * ey
            return f, [g], [[h]]

        for k in seeds:
            t0 = TWO_PI * k / n
            r = minimize(fgh, [t0], [t0 - PI], [t0 + PI])
            if r is not None:
                best.append((r[1].sqrt(), wrap_2pi(r[0][0])))
        best.sort()
        tied = sorted(t for d, t in best if d - best[0][0] <= TIE)
        distinct = []
        for t in tied:
            if all(abs(t - o) > D("1e-30") for o in distinct):
                distinct.append(t)
        return [distinct[0]], len(distinct) > 1


ANALYTIC_TYPES = ["line", "circle", "ellipse", "plane", "cylinder", "cone", "sphere", "torus"]


def analytic_params(typ, rng):
    s = 0.05 + 0.6 * rng.random()
    if typ in ("circle", "cylinder", "sphere"):
        return {"radius": s}
    if typ == "ellipse":
        b = s * (0.3 + 0.6 * rng.random())
        return {"semi_axis_1": s, "semi_axis_2": b}
    if typ == "cone":
        return {"radius": s, "semi_angle": 0.1 + 1.3 * rng.random()}
    if typ == "torus":
        return {"major_radius": s, "minor_radius": s * (0.1 + 0.7 * rng.random())}
    return {}


def build_analytic(seed, log):
    rng = random.Random(seed)
    ents = []
    n_eval = n_inv = 0
    for typ in ANALYTIC_TYPES:
        for k in range(3):
            aligned = k == 0
            if aligned:
                origin, axis, ref = [0.125, -0.25, 0.0625], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]
            else:
                origin = [0.5 * (rng.random() - 0.5) for _ in range(3)]
                axis, ref = rand_frame(rng)
            a = Analytic(f"{typ}{k}", typ, origin, axis, ref, analytic_params(typ, rng), aligned)
            g = a.json()
            size = max(list(a.params.values()) + [0.3])
            evals = []
            for _ in range(20):
                if a.curve:
                    t = (2 * math.pi * rng.random()) if typ != "line" else size * (2 * rng.random() - 1)
                    C = a.curve_ders(t)
                    kk = curvature_curve(C[1], C[2])
                    evals.append({"t": enc(t), "p": enc(C[0]), "d1": enc(C[1]), "d2": enc(C[2]),
                                  "curvature": enc(kk) if kk is not None else None})
                else:
                    if typ == "plane":
                        u, v = size * (2 * rng.random() - 1), size * (2 * rng.random() - 1)
                    elif typ == "sphere":
                        u, v = 2 * math.pi * rng.random(), math.pi * (rng.random() - 0.5)
                    elif typ == "torus":
                        u, v = 2 * math.pi * rng.random(), 2 * math.pi * rng.random()
                    else:
                        u, v = 2 * math.pi * rng.random(), size * (2 * rng.random() - 1)
                    S = a.surf_ders(u, v)
                    rec = {"u": enc(u), "v": enc(v), "p": enc(S[(0, 0)]), "su": enc(S[(1, 0)]), "sv": enc(S[(0, 1)]),
                           "suu": enc(S[(2, 0)]), "suv": enc(S[(1, 1)]), "svv": enc(S[(0, 2)])}
                    rec.update(surface_geometry(S))
                    evals.append(rec)
            invs = []
            want = 30
            tries = 0
            special = []
            if aligned:
                o = origin
                if typ in ("circle", "ellipse", "sphere"):
                    special.append([o[0], o[1], o[2]])
                if typ in ("circle", "cylinder", "cone", "torus", "sphere"):
                    special.append([o[0], o[1], o[2] + 0.0625])
                    special.append([o[0], o[1], o[2] - 0.03125])
            while len(invs) < want + len(special):
                tries += 1
                if tries > 5000:
                    die(f"{a.id}: could not generate queries")
                if special and len(invs) < len(special):
                    Pf = special[len(invs)]
                else:
                    Pf = [a.o[i] + 1.5 * size * (2 * rng.random() - 1) for i in range(3)]
                try:
                    prm, foot, dist, nu = a.invert(Pf)
                except ValueError:
                    if special and len(invs) < len(special):
                        special.pop(len(invs))
                    continue
                rec = {"id": f"{a.id}-q{len(invs):04d}", "q": enc(Pf), "params": enc(prm), "foot": enc(foot),
                       "distance": enc(dist), "non_unique": nu}
                invs.append(rec)
            g["eval"] = evals
            g["invert"] = invs
            n_eval += len(evals)
            n_inv += len(invs)
            ents.append(g)
        log(f"  {typ}: 3 placements")
    head = {"format": FORMAT, "oracle_version": ORACLE_VERSION, "seed": seed - 2, "file_seed": seed,
            "license": "CC-BY-4.0", "unit": "m", "position_tolerance_m": ANALYTIC_TOL, "conventions": CONVENTIONS}
    return render(head, ents), seed, {"entities": len(ents), "evals": n_eval, "inversions": n_inv}


# ---------------------------------------------------------------- output

ROW_KEYS = ("eval", "domain_checks", "invert")


def render(head, entities):
    text = "{\n"
    for k, v in head.items():
        text += f" {json.dumps(k)}: {json.dumps(v, separators=(',', ':'))},\n"
    text += ' "entities": [\n'
    blocks = []
    for e in entities:
        geo = {k: v for k, v in e.items() if k not in ROW_KEYS}
        s = "  {" + json.dumps(geo, separators=(",", ":"))[1:-1]
        for key in ROW_KEYS:
            if key not in e:
                continue
            rows = e[key]
            if isinstance(rows, dict):
                s += f',\n   "{key}":' + json.dumps(rows, separators=(",", ":"))
            elif not rows:
                s += f',\n   "{key}":[]'
            else:
                s += f',\n   "{key}":[\n' + ",\n".join("    " + json.dumps(r, separators=(",", ":")) for r in rows)
                s += "\n   ]"
        s += "\n  }"
        blocks.append(s)
    text += ",\n".join(blocks) + "\n ]\n}\n"
    return text.encode("utf-8")


def generate(seed, out, log):
    files = {}
    log("ref_set_v1.json")
    data, fseed, counts = build_ref_set(seed, log)
    files["ref_set_v1.json"] = (data, fseed, counts)
    log("analytic_set.json")
    data, fseed, counts = build_analytic(seed + 2, log)
    files["analytic_set.json"] = (data, fseed, counts)
    log("multi_minima.json")
    data, fseed, counts = build_multi_minima(seed + 3, log)
    files["multi_minima.json"] = (data, fseed, counts)
    entries = [{"path": k, "sha256": "sha256:" + hashlib.sha256(v[0]).hexdigest(), "seed": v[1],
                "oracle_version": ORACLE_VERSION, "counts": v[2]} for k, v in sorted(files.items())]
    for name, aseed in ADOPTED:
        f = out / name
        if not f.is_file():
            die(f"adopted fixture {f} missing")
        entries.append({"path": name, "sha256": "sha256:" + hashlib.sha256(f.read_bytes()).hexdigest(), "seed": aseed,
                        "oracle_version": ADOPTED_GENERATOR, "adopted": True})
    man = {"format": FORMAT, "oracle_version": ORACLE_VERSION, "seed": seed, "license": "CC-BY-4.0",
           "generator": "tools/fixtures/nurbs_oracle.py", "files": entries}
    files["manifest.json"] = ((json.dumps(man, indent=1) + "\n").encode("utf-8"), seed, None)
    return files


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", required=True)
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--check", action="store_true", help="regenerate in memory and compare with --out")
    args = ap.parse_args()
    out = Path(args.out)
    files = generate(args.seed, out, lambda m: sys.stderr.write(m + "\n"))
    if args.check:
        bad = [k for k, v in files.items() if not (out / k).is_file() or (out / k).read_bytes() != v[0]]
        if bad:
            die("regenerated bytes differ from " + str(out) + ": " + ", ".join(sorted(bad)))
        sys.stderr.write("check: all files byte-identical\n")
        return
    out.mkdir(parents=True, exist_ok=True)
    for k, v in sorted(files.items()):
        (out / k).write_bytes(v[0])


if __name__ == "__main__":
    main()
