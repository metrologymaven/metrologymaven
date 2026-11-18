#!/usr/bin/env python3
"""Independent oracle for GEO.DIST.FACE.REF: closest points on trimmed faces and bodies.

Written for this project from the spec (spec/geo/dist/face/ref/NODE.md); it shares no
code with any Rust crate. Python standard library only: `decimal` at 60 working digits
(answers are claimed to 50) stands in for mpmath, and `fractions` gives exact
point-in-loop tests on the pcurves. Output data is CC-BY-4.0.

Per face: a G x G parameter grid seeds Newton refinement of |S(u,v) - p|^2 at 60 digits;
a refined point counts only if an exact rational point-in-loop test puts it inside or on
the trim. Per boundary edge (edge geometry = owner face surface o pcurve): sampled seeds
plus 1D Newton, and the vertices. Body answer: minimum over faces, ties within 1e-15 m go
to the lowest face ID (REQ-GEO.DIST.FACE-004). Analytic bodies are also solved in closed
form and the two answers must agree within 1e-15 m (AC-2), or the oracle aborts.

Fixture format (fid-face-ref/1), one JSON file per body plus manifest.json:
- "body": builder description (provisional "fid-brep-builder/0"; GEO.CAD.TOPO.MODEL has no
  JSON schema on main yet): vertices, edges {start, end, curve | null, param_range,
  degenerate?}, faces {surface, sense, loops [{outer, coedges [{edge, sense, pcurve}]}]},
  shells. Entities are referenced by label; labels are the imported TopologyIds.
- Surfaces, with placement (origin O, axis Z, ref_dir X, Y = Z x X):
  plane O + uX + vY; cylinder O + r(cos u X + sin u Y) + vZ; cone O + (r + v tan a)(cos u X
  + sin u Y) + vZ; sphere O + r cos v (cos u X + sin u Y) + r sin v Z; torus O + (R + r cos v)
  (cos u X + sin u Y) + r sin v Z; nurbs: degree_u/v, knots_u/v, ctrl row-major (n_u x n_v),
  weights or null.
- Pcurves run in the edge's direction (pcurve start = edge start vertex); coedge sense says
  how the loop traverses the edge. line2 {from, to}, t in [0, 1]; circle2 {center, radius,
  orient}: (cu + r cos t, cv + orient r sin t), t in [0, 2 pi].
- An edge used twice by one face is a seam, and an edge marked degenerate is a pole; neither
  is a boundary, so a foot on them is "interior". A closed edge (start == end) has no corner:
  a foot on it is reported as the edge, never as its vertex.
- Each query: p, face, feature {kind: interior | edge | vertex, id}, foot, distance (all f64
  as {hex, dec}, same bits), alternatives (other answers within 1e-12 m: equidistant faces,
  shared edges, symmetric minima), edge_distance (p to the nearest boundary edge),
  foot_edge_distance, untrimmed (trim-ignoring answer: infinite analytic surfaces, or the
  whole NURBS domain patch), gen (generator) and tags (trimmed_away, near_edge, equidistant,
  tie; measured, not assumed).

Run:   python tools/fixtures/face_proj_oracle.py --out fixtures/geo/dist/face/ref_v1 --seed 31
Check: python tools/fixtures/face_proj_oracle.py --out fixtures/geo/dist/face/ref_v1 --seed 31 --check
"""

import argparse
import hashlib
import json
import math
import random
import sys
from decimal import Decimal, ROUND_HALF_EVEN, getcontext, localcontext
from fractions import Fraction
from pathlib import Path

ORACLE_VERSION = "face_proj_oracle 1.0.0"
FORMAT = "fid-face-ref/1"
PREC = 60
getcontext().prec = PREC
D = Decimal
ZERO, ONE, TWO = D(0), D(1), D(2)

GRID = 64              # interior seed grid per face (G x G)
EDGE_SAMPLES = 64      # seed samples per edge
TIE_FACE = D("1e-15")  # REQ-GEO.DIST.FACE-004
ALT_WINDOW = D("1e-12")  # other answers listed when within this of the minimum distance
SELF_TOL = D("1e-15")  # AC-2
TRIM_MARGIN = D("1e-9")  # trimmed_away: trim-ignoring answer shorter by more than this
EDGE_NEAR = D("1e-6")  # REQ-GEO.DIST.FACE.REF-003
STEP_EPS = D("1e-52")
PICK_EPS2 = D("1e-40")
RANK = {"vertex": 0, "edge": 1, "interior": 2}

P2 = 2 * math.pi       # f64 values written into pcurves
PH = math.pi / 2

STATS = {"selfcheck": 0, "selfcheck_max_dev": ZERO, "clamp_checks": 0}


class Reject(Exception):
    """Query is degenerate for the closed form (point on an axis or centre); resample."""


def die(msg):
    sys.stderr.write("face_proj_oracle: " + msg + "\n")
    sys.exit(2)


# ---------------------------------------------------------------- 60-digit functions

def _atan_small(x):
    x2, term, s, k = x * x, x, x, 1
    eps = D(10) ** -(getcontext().prec + 4)
    while True:
        term = -term * x2
        k += 2
        t = term / k
        if abs(t) < eps:
            return s
        s += t


def _pi():
    with localcontext() as c:
        c.prec = PREC + 10
        v = 16 * _atan_small(ONE / 5) - 4 * _atan_small(ONE / 239)
    return +v


PI = _pi()
TWO_PI = 2 * PI
HALF_PI = PI / 2
_SC = {}


def sin_cos(x):
    r = _SC.get(x)
    if r is not None:
        return r
    k = (x / TWO_PI).to_integral_value(rounding=ROUND_HALF_EVEN)
    y = x - k * TWO_PI
    eps = D(10) ** -(PREC + 3)
    s = c = ZERO
    term, n = ONE, 0
    while True:
        m = n % 4
        if m == 0:
            c += term
        elif m == 1:
            s += term
        elif m == 2:
            c -= term
        else:
            s -= term
        n += 1
        term = term * y / n
        if n > 3 and abs(term) < eps:
            break
    r = (+s, +c)
    if len(_SC) < 300000:
        _SC[x] = r
    return r


def tan(x):
    s, c = sin_cos(x)
    return s / c


def atan(x):
    if x < 0:
        return -atan(-x)
    if x > 1:
        return HALF_PI - atan(ONE / x)
    m = 0
    while x > D("0.2"):
        x = x / (1 + (1 + x * x).sqrt())
        m += 1
    return _atan_small(x) * (2 ** m)


def atan2(y, x):
    if x > 0:
        return atan(y / x)
    if x < 0:
        return atan(y / x) + (PI if y >= 0 else -PI)
    if y > 0:
        return HALF_PI
    if y < 0:
        return -HALF_PI
    raise Reject("atan2(0, 0)")


def wrap(x, lo):
    r = (x - lo) % TWO_PI
    if r < 0:
        r += TWO_PI
    return lo + r


# ---------------------------------------------------------------- vectors

def fin(x, what):
    if not isinstance(x, (int, float)) or isinstance(x, bool) or not math.isfinite(x):
        die(f"non-finite or non-numeric input in {what}: {x!r}")
    return x


def dec3(v, what="vector"):
    if len(v) != 3:
        die(f"{what}: expected 3 components")
    return tuple(D(fin(x, what)) for x in v)


def vadd(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def vsub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def vmul(a, s):
    return (a[0] * s, a[1] * s, a[2] * s)


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def norm(a):
    return dot(a, a).sqrt()


def lin(base, *terms):
    x, y, z = base
    for s, v in terms:
        x += s * v[0]
        y += s * v[1]
        z += s * v[2]
    return (x, y, z)


def d2(a, b):
    r = vsub(a, b)
    return dot(r, r)


def f3(v):
    return tuple(float(x) for x in v)


Z3 = (ZERO, ZERO, ZERO)


# ---------------------------------------------------------------- surfaces

class Placement:
    def __init__(self, j):
        self.O = dec3(j["origin"], "placement origin")
        self.Z = dec3(j["axis"], "placement axis")
        self.X = dec3(j["ref_dir"], "placement ref_dir")
        self.Y = cross(self.Z, self.X)


class Plane:
    per_u = per_v = False
    pole = False

    def __init__(self, j):
        self.pl = Placement(j["placement"])

    def point(self, u, v):
        pl = self.pl
        return lin(pl.O, (u, pl.X), (v, pl.Y))

    def eval2(self, u, v):
        return self.point(u, v), self.pl.X, self.pl.Y, Z3, Z3, Z3

    def canon(self, u, v):
        return u, v


class Cylinder(Plane):
    per_u = True

    def __init__(self, j):
        self.pl = Placement(j["placement"])
        self.r = D(fin(j["radius"], "radius"))

    def point(self, u, v):
        pl = self.pl
        s, c = sin_cos(u)
        return lin(pl.O, (self.r * c, pl.X), (self.r * s, pl.Y), (v, pl.Z))

    def eval2(self, u, v):
        pl, r = self.pl, self.r
        s, c = sin_cos(u)
        R = lin(Z3, (c, pl.X), (s, pl.Y))
        T = lin(Z3, (-s, pl.X), (c, pl.Y))
        S = lin(pl.O, (r, R), (v, pl.Z))
        return S, vmul(T, r), pl.Z, vmul(R, -r), Z3, Z3


class Cone(Plane):
    per_u = True

    def __init__(self, j):
        self.pl = Placement(j["placement"])
        self.r0 = D(fin(j["radius"], "radius"))
        self.a = D(fin(j["half_angle"], "half_angle"))
        self.k = tan(self.a)

    def point(self, u, v):
        pl = self.pl
        s, c = sin_cos(u)
        rho = self.r0 + v * self.k
        return lin(pl.O, (rho * c, pl.X), (rho * s, pl.Y), (v, pl.Z))

    def eval2(self, u, v):
        pl, k = self.pl, self.k
        s, c = sin_cos(u)
        rho = self.r0 + v * k
        R = lin(Z3, (c, pl.X), (s, pl.Y))
        T = lin(Z3, (-s, pl.X), (c, pl.Y))
        S = lin(pl.O, (rho, R), (v, pl.Z))
        return S, vmul(T, rho), lin(pl.Z, (k, R)), vmul(R, -rho), vmul(T, k), Z3


class Sphere(Plane):
    per_u = True
    pole = True

    def __init__(self, j):
        self.pl = Placement(j["placement"])
        self.r = D(fin(j["radius"], "radius"))

    def point(self, u, v):
        pl, r = self.pl, self.r
        s, c = sin_cos(u)
        sv, cv = sin_cos(v)
        return lin(pl.O, (r * cv * c, pl.X), (r * cv * s, pl.Y), (r * sv, pl.Z))

    def eval2(self, u, v):
        pl, r = self.pl, self.r
        s, c = sin_cos(u)
        sv, cv = sin_cos(v)
        R = lin(Z3, (c, pl.X), (s, pl.Y))
        T = lin(Z3, (-s, pl.X), (c, pl.Y))
        S = lin(pl.O, (r * cv, R), (r * sv, pl.Z))
        Su = vmul(T, r * cv)
        Sv = lin(Z3, (-r * sv, R), (r * cv, pl.Z))
        Suu = vmul(R, -r * cv)
        Suv = vmul(T, -r * sv)
        Svv = lin(Z3, (-r * cv, R), (-r * sv, pl.Z))
        return S, Su, Sv, Suu, Suv, Svv

    def canon(self, u, v):
        if v > HALF_PI:
            return u + PI, PI - v
        if v < -HALF_PI:
            return u + PI, -PI - v
        return u, v


class Torus(Plane):
    per_u = per_v = True

    def __init__(self, j):
        self.pl = Placement(j["placement"])
        self.R = D(fin(j["major_radius"], "major_radius"))
        self.r = D(fin(j["minor_radius"], "minor_radius"))

    def point(self, u, v):
        pl = self.pl
        s, c = sin_cos(u)
        sv, cv = sin_cos(v)
        rho = self.R + self.r * cv
        return lin(pl.O, (rho * c, pl.X), (rho * s, pl.Y), (self.r * sv, pl.Z))

    def eval2(self, u, v):
        pl, r = self.pl, self.r
        s, c = sin_cos(u)
        sv, cv = sin_cos(v)
        rho = self.R + r * cv
        R = lin(Z3, (c, pl.X), (s, pl.Y))
        T = lin(Z3, (-s, pl.X), (c, pl.Y))
        S = lin(pl.O, (rho, R), (r * sv, pl.Z))
        Su = vmul(T, rho)
        Sv = lin(Z3, (-r * sv, R), (r * cv, pl.Z))
        Suu = vmul(R, -rho)
        Suv = vmul(T, -r * sv)
        Svv = lin(Z3, (-r * cv, R), (-r * sv, pl.Z))
        return S, Su, Sv, Suu, Suv, Svv


def _span(U, p, n, u):
    k = p
    while k < n - 1 and U[k + 1] <= u:
        k += 1
    while k < n - 1 and U[k] == U[k + 1]:
        k += 1
    return k


def basis_ders(U, p, span, u, nd):
    """[[N, N', N''] for N_{span-p+j,p}(u), j = 0..p] (Cox-de Boor, written from the definition)."""
    tab = [[ONE]]
    for k in range(1, p + 1):
        prev, cur = tab[-1], []
        for j in range(k + 1):
            i = span - k + j
            val = ZERO
            if j > 0:
                den = U[i + k] - U[i]
                if den:
                    val += (u - U[i]) / den * prev[j - 1]
            if j < k:
                den = U[i + k + 1] - U[i + 1]
                if den:
                    val += (U[i + k + 1] - u) / den * prev[j]
            cur.append(val)
        tab.append(cur)

    def N(i, k):
        j = i - (span - k)
        return tab[k][j] if 0 <= j <= k else ZERO

    def der(m, i, k):
        if m == 0:
            return N(i, k)
        if k == 0:
            return ZERO
        a, b = U[i + k] - U[i], U[i + k + 1] - U[i + 1]
        r = ZERO
        if a:
            r += der(m - 1, i, k - 1) / a
        if b:
            r -= der(m - 1, i + 1, k - 1) / b
        return k * r

    return [[der(m, span - p + j, p) for m in range(nd + 1)] for j in range(p + 1)]


class Nurbs(Plane):
    def __init__(self, j):
        self.pu, self.pv = int(j["degree_u"]), int(j["degree_v"])
        self.nu, self.nv = int(j["n_u"]), int(j["n_v"])
        self.U = [D(fin(x, "knots_u")) for x in j["knots_u"]]
        self.V = [D(fin(x, "knots_v")) for x in j["knots_v"]]
        if len(self.U) != self.nu + self.pu + 1 or len(self.V) != self.nv + self.pv + 1:
            die("nurbs: knot count mismatch")
        ctrl = [dec3(c, "ctrl") for c in j["ctrl"]]
        if len(ctrl) != self.nu * self.nv:
            die("nurbs: control point count mismatch")
        w = j.get("weights")
        ws = [D(fin(x, "weights")) for x in w] if w is not None else [ONE] * len(ctrl)
        if any(x <= 0 for x in ws):
            die("nurbs: weight not positive")
        self.P = [[ctrl[i * self.nv + k] for k in range(self.nv)] for i in range(self.nu)]
        self.W = [[ws[i * self.nv + k] for k in range(self.nv)] for i in range(self.nu)]
        self.dom = (self.U[self.pu], self.U[self.nu], self.V[self.pv], self.V[self.nv])

    def _ders(self, u, v, nd):
        su = _span(self.U, self.pu, self.nu, u)
        sv = _span(self.V, self.pv, self.nv, v)
        bu = basis_ders(self.U, self.pu, su, u, nd)
        bv = basis_ders(self.V, self.pv, sv, v, nd)
        combos = [(0, 0)] if nd == 0 else [(0, 0), (1, 0), (0, 1), (2, 0), (1, 1), (0, 2)]
        A = {c: [ZERO, ZERO, ZERO] for c in combos}
        Wd = {c: ZERO for c in combos}
        for a in range(self.pu + 1):
            i = su - self.pu + a
            for b in range(self.pv + 1):
                k = sv - self.pv + b
                w, P = self.W[i][k], self.P[i][k]
                for c in combos:
                    coef = bu[a][c[0]] * bv[b][c[1]] * w
                    if coef:
                        Ac = A[c]
                        Ac[0] += coef * P[0]
                        Ac[1] += coef * P[1]
                        Ac[2] += coef * P[2]
                        Wd[c] += coef
        return A, Wd

    def point(self, u, v):
        A, Wd = self._ders(u, v, 0)
        w = Wd[(0, 0)]
        return tuple(x / w for x in A[(0, 0)])

    def eval2(self, u, v):
        A, Wd = self._ders(u, v, 2)
        w = Wd[(0, 0)]
        S = tuple(x / w for x in A[(0, 0)])
        wu, wv = Wd[(1, 0)], Wd[(0, 1)]
        Su = tuple((A[(1, 0)][i] - wu * S[i]) / w for i in range(3))
        Sv = tuple((A[(0, 1)][i] - wv * S[i]) / w for i in range(3))
        Suu = tuple((A[(2, 0)][i] - 2 * wu * Su[i] - Wd[(2, 0)] * S[i]) / w for i in range(3))
        Suv = tuple((A[(1, 1)][i] - wu * Sv[i] - wv * Su[i] - Wd[(1, 1)] * S[i]) / w for i in range(3))
        Svv = tuple((A[(0, 2)][i] - 2 * wv * Sv[i] - Wd[(0, 2)] * S[i]) / w for i in range(3))
        return S, Su, Sv, Suu, Suv, Svv


SURFACES = {"plane": Plane, "cylinder": Cylinder, "cone": Cone, "sphere": Sphere, "torus": Torus, "nurbs": Nurbs}


def make_surface(j):
    cls = SURFACES.get(j.get("type"))
    if cls is None:
        die(f"unknown surface type {j.get('type')!r}")
    return cls(j)


# ---------------------------------------------------------------- pcurves and edges

class Line2:
    closed = False

    def __init__(self, j):
        self.fa = tuple(Fraction(fin(x, "pcurve")) for x in j["from"])
        self.fb = tuple(Fraction(fin(x, "pcurve")) for x in j["to"])
        self.a = tuple(D(x) for x in j["from"])
        self.b = tuple(D(x) for x in j["to"])
        self.d = (self.b[0] - self.a[0], self.b[1] - self.a[1])
        self.t0, self.t1 = ZERO, ONE

    def eval(self, t):
        a, d = self.a, self.d
        return (a[0] + t * d[0], a[1] + t * d[1]), d, (ZERO, ZERO)

    def bbox(self):
        return [min(self.a[0], self.b[0]), max(self.a[0], self.b[0]), min(self.a[1], self.b[1]),
                max(self.a[1], self.b[1])]


class Circle2:
    closed = True

    def __init__(self, j):
        self.c = tuple(D(fin(x, "pcurve")) for x in j["center"])
        self.r = D(fin(j["radius"], "pcurve radius"))
        self.o = int(j["orient"])
        if self.o not in (1, -1):
            die("circle2 orient must be 1 or -1")
        self.fc = tuple(Fraction(x) for x in j["center"])
        self.fr2 = Fraction(j["radius"]) ** 2
        self.t0, self.t1 = ZERO, TWO_PI

    def eval(self, t):
        s, c = sin_cos(t)
        r, o = self.r, self.o
        return ((self.c[0] + r * c, self.c[1] + o * r * s), (-r * s, o * r * c), (-r * c, -o * r * s))

    def bbox(self):
        return [self.c[0] - self.r, self.c[0] + self.r, self.c[1] - self.r, self.c[1] + self.r]


def make_pcurve(j):
    if j.get("type") == "line2":
        return Line2(j)
    if j.get("type") == "circle2":
        return Circle2(j)
    die(f"unknown pcurve type {j.get('type')!r}")


class Edge:
    def __init__(self, label, start, end, surf, pc, real):
        self.label, self.start, self.end, self.surf, self.pc, self.real = label, start, end, surf, pc, real
        self.closed = start == end  # a closed edge (full circle) has no corner at its vertex
        self.t0, self.t1 = pc.t0, pc.t1
        n = EDGE_SAMPLES
        self.ts = [self.t0 + (self.t1 - self.t0) * k / n for k in range(n if self.closed else n + 1)]
        self.pts = [f3(self.point(t)) for t in self.ts]

    def point(self, t):
        uv = self.pc.eval(t)[0]
        return self.surf.point(uv[0], uv[1])

    def eval(self, t):
        (u, v), (du, dv), (ddu, ddv) = self.pc.eval(t)
        S, Su, Sv, Suu, Suv, Svv = self.surf.eval2(u, v)
        C1 = lin(Z3, (du, Su), (dv, Sv))
        C2 = lin(Z3, (du * du, Suu), (2 * du * dv, Suv), (dv * dv, Svv), (ddu, Su), (ddv, Sv))
        return S, C1, C2

    def norm_t(self, t):
        if self.closed:
            span = self.t1 - self.t0
            r = (t - self.t0) % span
            if r < 0:
                r += span
            return self.t0 + r
        return t


class Cand:
    __slots__ = ("d2", "foot", "feat", "face")

    def __init__(self, dd, foot, feat, face=None):
        self.d2, self.foot, self.feat, self.face = dd, foot, feat, face


def newton1(edge, p, t):
    for _ in range(100):
        C, C1, C2 = edge.eval(t)
        r = vsub(C, p)
        g = dot(r, C1)
        h = dot(C1, C1) + dot(r, C2)
        if h > 0:
            dt = -g / h
        else:
            c11 = dot(C1, C1)
            if c11 == 0:
                return t
            dt = -g / c11
        if dt == 0:
            return t
        f0 = dot(r, r)
        s = ONE
        for _ in range(100):
            tn = t + s * dt
            if not edge.closed:
                tn = min(max(tn, edge.t0), edge.t1)
            if d2(edge.point(tn), p) <= f0:
                break
            s /= 2
        else:
            return t
        if abs(tn - t) < STEP_EPS:
            return tn
        t = tn
    return t


def edge_candidates(edge, p, pf):
    dd = [(x - pf[0]) ** 2 + (y - pf[1]) ** 2 + (z - pf[2]) ** 2 for x, y, z in edge.pts]
    n = len(dd)
    loc = []
    for k in range(n):
        if edge.closed:
            l, r = dd[(k - 1) % n], dd[(k + 1) % n]
        else:
            l = dd[k - 1] if k > 0 else math.inf
            r = dd[k + 1] if k < n - 1 else math.inf
        if dd[k] <= l and dd[k] <= r:
            loc.append((dd[k], k))
    loc.sort()
    ts = [newton1(edge, p, edge.ts[k]) for _, k in loc[:3]]
    if not edge.closed:
        ts += [edge.t0, edge.t1]
    out = []
    for t in ts:
        t = edge.norm_t(t)
        P = edge.point(t)
        if not edge.closed and t == edge.t0:
            feat = ("vertex", edge.start)
        elif not edge.closed and t == edge.t1:
            feat = ("vertex", edge.end)
        else:
            feat = ("edge", edge.label)
        out.append(Cand(d2(P, p), P, feat))
    return out


# ---------------------------------------------------------------- faces and trims

def pip_poly(segs, x, y):
    inside = False
    for (x1, y1), (x2, y2) in segs:
        cr = (x2 - x1) * (y - y1) - (y2 - y1) * (x - x1)
        if cr == 0 and min(x1, x2) <= x <= max(x1, x2) and min(y1, y2) <= y <= max(y1, y2):
            return "on"
        if (y1 > y) != (y2 > y):
            xi = x1 + (y - y1) * (x2 - x1) / (y2 - y1)
            if x < xi:
                inside = not inside
    return "in" if inside else "out"


class Face:
    def __init__(self, label, surf, loops, edges):
        """loops: list of (outer, [(edge_label, pcurve)])."""
        self.label, self.surf = label, surf
        self.loops = []
        bb = None
        seen = []
        for outer, coedges in loops:
            pcs = [pc for _, pc in coedges]
            if all(isinstance(pc, Circle2) for pc in pcs):
                if len(pcs) != 1:
                    die(f"{label}: a circle loop must have exactly one coedge")
                self.loops.append((outer, "circle", (pcs[0].fc, pcs[0].fr2)))
            elif all(isinstance(pc, Line2) for pc in pcs):
                self.loops.append((outer, "poly", [(pc.fa, pc.fb) for pc in pcs]))
            else:
                die(f"{label}: mixed line2/circle2 loops are not supported")
            for el, pc in coedges:
                b = pc.bbox()
                bb = b if bb is None else [min(bb[0], b[0]), max(bb[1], b[1]), min(bb[2], b[2]), max(bb[3], b[3])]
                if edges[el].real and el not in seen:
                    seen.append(el)
        self.edges = [edges[e] for e in seen]
        self.bb = bb
        self.full_u = surf.per_u and abs(bb[1] - bb[0] - TWO_PI) < D("1e-12")
        self.full_v = surf.per_v and abs(bb[3] - bb[2] - TWO_PI) < D("1e-12")
        self.umid, self.vmid = (bb[0] + bb[1]) / 2, (bb[2] + bb[3]) / 2

    def classify(self, u, v):
        u, v = self.surf.canon(u, v)
        bb = self.bb
        if self.surf.per_u:
            u = wrap(u, bb[0])
        if self.surf.per_v:
            v = wrap(v, bb[2])
        if self.full_u:
            u = self.umid
        if self.full_v:
            v = self.vmid
        if self.surf.pole and bb[3] < v <= bb[3] + D("1e-12"):
            v = bb[3]
        U, V = Fraction(u), Fraction(v)
        for outer, kind, data in self.loops:
            if kind == "circle":
                (cu, cv), r2 = data
                q = (U - cu) ** 2 + (V - cv) ** 2
                c = "on" if q == r2 else ("in" if q < r2 else "out")
            else:
                c = pip_poly(data, U, V)
            if c == "on":
                return "on"
            if outer and c == "out":
                return "out"
            if not outer and c == "in":
                return "out"
        return "in"

    def prepare(self, G):
        bb = self.bb
        self.gu = [bb[0] + (bb[1] - bb[0]) * i / (G - 1) for i in range(G)]
        self.gv = [bb[2] + (bb[3] - bb[2]) * k / (G - 1) for k in range(G)]
        self.gp = [[f3(self.surf.point(u, v)) for v in self.gv] for u in self.gu]
        self.mask = [[self.classify(u, v) != "out" for v in self.gv] for u in self.gu]

    def grid_d2(self, pf):
        return [[(x - pf[0]) ** 2 + (y - pf[1]) ** 2 + (z - pf[2]) ** 2 for x, y, z in row] for row in self.gp]

    def seeds(self, pf, K=4):
        g = self.grid_d2(pf)
        G1, G2 = len(g), len(g[0])
        loc = []
        for i in range(G1):
            for k in range(G2):
                x = g[i][k]
                if ((i == 0 or x <= g[i - 1][k]) and (i == G1 - 1 or x <= g[i + 1][k])
                        and (k == 0 or x <= g[i][k - 1]) and (k == G2 - 1 or x <= g[i][k + 1])):
                    loc.append((x, i, k))
        loc.sort()
        return [(self.gu[i], self.gv[k]) for _, i, k in loc[:K]]

    def newton2(self, p, u, v):
        bb = self.bb
        wu, wv = bb[1] - bb[0], bb[3] - bb[2]
        surf = self.surf
        for _ in range(80):
            S, Su, Sv, Suu, Suv, Svv = surf.eval2(u, v)
            r = vsub(S, p)
            gu, gv = dot(r, Su), dot(r, Sv)
            a = dot(Su, Su) + dot(r, Suu)
            b = dot(Su, Sv) + dot(r, Suv)
            c = dot(Sv, Sv) + dot(r, Svv)
            det = a * c - b * b
            if a > 0 and det > 0:
                du = -(c * gu - b * gv) / det
                dv = -(a * gv - b * gu) / det
            else:
                su, sv = dot(Su, Su), dot(Sv, Sv)
                du = -gu / su if su > 0 else ZERO
                dv = -gv / sv if sv > 0 else ZERO
            if du == 0 and dv == 0:
                return u, v
            f0 = dot(r, r)
            s = ONE
            for _ in range(100):
                un, vn = u + s * du, v + s * dv
                if d2(surf.point(un, vn), p) <= f0:
                    break
                s /= 2
            else:
                return u, v
            step = abs(un - u) + abs(vn - v)
            u, v = un, vn
            if not surf.per_u and not (bb[0] - wu <= u <= bb[1] + wu):
                return None
            if not surf.per_v and not surf.pole and not (bb[2] - wv <= v <= bb[3] + wv):
                return None
            if step < STEP_EPS:
                return u, v
        return u, v

    def candidates(self, p, pf, ecache):
        out = []
        for u, v in self.seeds(pf):
            r = self.newton2(p, u, v)
            if r is None:
                continue
            if self.classify(r[0], r[1]) == "out":
                continue
            S = self.surf.point(r[0], r[1])
            out.append(Cand(d2(S, p), S, ("interior", None), self.label))
        for e in self.edges:
            if e.label not in ecache:
                ecache[e.label] = edge_candidates(e, p, pf)
            for c in ecache[e.label]:
                out.append(Cand(c.d2, c.foot, c.feat, self.label))
        return out


def pick(cands):
    m = min(c.d2 for c in cands)
    close = [c for c in cands if c.d2 - m <= PICK_EPS2]
    return min(close, key=lambda c: (RANK[c.feat[0]], c.d2))


# ---------------------------------------------------------------- bodies

class Body:
    def __init__(self, j, cf, untrimmed_cf):
        self.j, self.cf, self.ucf = j, cf, untrimmed_cf
        self.name = j["body_id"]
        self.vpts = {v["label"]: dec3(v["point"], "vertex") for v in j["vertices"]}
        uses = {}
        for f in j["faces"]:
            for L in f["loops"]:
                for c in L["coedges"]:
                    uses.setdefault(c["edge"], []).append((f["label"], c))
        self.surfs = {f["label"]: make_surface(f["surface"]) for f in j["faces"]}
        self.edges = {}
        for e in j["edges"]:
            for x in (e.get("param_range") or []):
                fin(x, "param_range")
            us = uses.get(e["label"])
            if not us:
                die(f"edge {e['label']} has no coedge")
            seam = len(us) == 2 and us[0][0] == us[1][0]
            real = not seam and not e.get("degenerate", False)
            fl0, c0 = us[0]
            self.edges[e["label"]] = Edge(e["label"], e["start"], e["end"], self.surfs[fl0],
                                          make_pcurve(c0["pcurve"]), real)
        self.faces = []
        for f in sorted(j["faces"], key=lambda f: f["label"].encode()):
            loops = [(L["outer"], [(c["edge"], make_pcurve(c["pcurve"])) for c in L["coedges"]]) for L in f["loops"]]
            face = Face(f["label"], self.surfs[f["label"]], loops, self.edges)
            face.prepare(GRID)
            self.faces.append(face)
        self.check_consistency(uses, j)
        # trim-ignoring twins of NURBS faces (domain rectangle as the only loop)
        self.virtual = []
        if self.ucf is None:
            for f in self.faces:
                u0, u1, v0, v1 = f.surf.dom
                corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)]
                loop, vedges = [], {}
                for k in range(4):
                    a, b = corners[k], corners[(k + 1) % 4]
                    lab = f"{f.label}~b{k}"
                    pc = Line2({"from": [float(a[0]), float(a[1])], "to": [float(b[0]), float(b[1])]})
                    vedges[lab] = Edge(lab, f"{f.label}~c{k}", f"{f.label}~c{(k + 1) % 4}", f.surf, pc, True)
                    loop.append((lab, pc))
                vf = Face(f.label, f.surf, [(True, loop)], vedges)
                vf.prepare(GRID)
                self.virtual.append(vf)
        self.real_edges = [e for e in self.edges.values() if e.real]
        allp = [pt for f in self.faces for row in f.gp for pt in row]
        self.lo = [min(pt[i] for pt in allp) for i in range(3)]
        self.hi = [max(pt[i] for pt in allp) for i in range(3)]

    def check_consistency(self, uses, j):
        """Coedge ends meet their vertices, and the coedges of a shared edge trace one curve."""
        tol = D("1e-14")
        ej = {e["label"]: e for e in j["edges"]}
        for el, us in uses.items():
            e = ej[el]
            mids = []
            for fl_, c in us:
                pc = make_pcurve(c["pcurve"])
                S = self.surfs[fl_]
                a = S.point(*pc.eval(pc.t0)[0])
                b = S.point(*pc.eval(pc.t1)[0])
                if norm(vsub(a, self.vpts[e["start"]])) > tol or norm(vsub(b, self.vpts[e["end"]])) > tol:
                    die(f"{self.name}: coedge {c['label']} does not meet the vertices of {el}")
                mids.append([S.point(*pc.eval(pc.t0 + (pc.t1 - pc.t0) * k / 4)[0]) for k in (1, 2, 3)])
            for m in mids[1:]:
                for x, y in zip(mids[0], m):
                    if norm(vsub(x, y)) > tol:
                        die(f"{self.name}: coedges of {el} disagree")

    def face_best(self, face, p, pf, ecache):
        return pick(face.candidates(p, pf, ecache))

    def solve(self, pf, faces=None):
        p = tuple(D(x) for x in pf)
        faces = self.faces if faces is None else faces
        ecache = {}
        allc, best = [], []
        for f in faces:
            cs = f.candidates(p, pf, ecache)
            if not cs:
                die(f"{self.name}: face {f.label} gave no candidate")
            allc.extend(cs)
            best.append(pick(cs))
        dmin = min(b.d2 for b in best).sqrt()
        chosen = next(b for b in best if b.d2.sqrt() <= dmin + TIE_FACE)
        alts = []
        for f in faces:
            fc = [c for c in allc if c.face == f.label and c.d2.sqrt() <= dmin + ALT_WINDOW]
            fc.sort(key=lambda c: (RANK[c.feat[0]], c.d2))
            for c in fc:
                if any(a.face == c.face and d2(a.foot, c.foot) <= D("1e-40") for a in alts):
                    continue
                alts.append(c)
        alts = [a for a in alts if not (a.face == chosen.face and d2(a.foot, chosen.foot) <= D("1e-40"))]
        edist = None
        for el, cs in ecache.items():
            for c in cs:
                if edist is None or c.d2 < edist:
                    edist = c.d2
        return {"p": p, "chosen": chosen, "alts": alts, "dist": chosen.d2.sqrt(),
                "edge_dist": edist.sqrt() if edist is not None else None}

    def foot_edge_distance(self, ans):
        c = ans["chosen"]
        if c.feat[0] != "interior":
            return ZERO
        face = next(f for f in self.faces if f.label == c.face)
        if not face.edges:
            return None
        ff = f3(c.foot)
        best = None
        for e in face.edges:
            for x in edge_candidates(e, c.foot, ff):
                if best is None or x.d2 < best:
                    best = x.d2
        return best.sqrt()

    def untrimmed(self, pf):
        p = tuple(D(x) for x in pf)
        if self.ucf is not None:
            res = [(lab, fn(p)) for lab, fn in self.ucf]
            res.sort(key=lambda r: r[0].encode())
            dmin = min(r[1][0] for r in res).sqrt()
            lab, (dd, foot) = next(r for r in res if r[1][0].sqrt() <= dmin + TIE_FACE)
            return lab, foot, dd.sqrt()
        a = self.solve(pf, self.virtual)
        return a["chosen"].face, a["chosen"].foot, a["dist"]

    def closed_form(self, p):
        res = []
        for f in self.faces:
            dd, foot, feat = self.cf[f.label](p)
            res.append((f.label, dd, foot, feat))
        dmin = min(r[1] for r in res).sqrt()
        return next(r for r in res if r[1].sqrt() <= dmin + TIE_FACE)


# ---------------------------------------------------------------- closed forms (AC-2)

def circle_closest(p, c, n, r):
    q = vsub(p, vmul(n, dot(vsub(p, c), n)))
    w = vsub(q, c)
    lw = norm(w)
    if lw < D("1e-20"):
        raise Reject("point on a circle axis")
    return lin(c, (r / lw, w))


def cf_plane_poly(O, n, segs):
    O, n = dec3(O), dec3(n)
    segs = [(e, dec3(A), dec3(B), va, vb) for e, A, B, va, vb in segs]

    def f(p):
        q = vsub(p, vmul(n, dot(vsub(p, O), n)))
        sg = [dot(cross(vsub(B, A), vsub(q, A)), n) for _, A, B, _, _ in segs]
        if all(s >= 0 for s in sg) or all(s <= 0 for s in sg):
            return d2(p, q), q, ("interior", None)
        best = None
        for e, A, B, va, vb in segs:
            AB = vsub(B, A)
            t = dot(vsub(p, A), AB) / dot(AB, AB)
            if t <= 0:
                foot, feat = A, ("vertex", va)
            elif t >= 1:
                foot, feat = B, ("vertex", vb)
            else:
                foot, feat = lin(A, (t, AB)), ("edge", e)
            c = (d2(p, foot), foot, feat)
            if best is None or c[0] < best[0]:
                best = c
        return best
    return f


def cf_plane_annulus(c, n, r_out, e_out, r_in, e_in):
    c, n, r_out = dec3(c), dec3(n), D(r_out)
    r_in = D(r_in) if r_in is not None else None

    def f(p):
        q = vsub(p, vmul(n, dot(vsub(p, c), n)))
        rho = norm(vsub(q, c))
        if rho > r_out:
            foot, feat = circle_closest(p, c, n, r_out), ("edge", e_out)
        elif r_in is not None and rho < r_in:
            foot, feat = circle_closest(p, c, n, r_in), ("edge", e_in)
        else:
            foot, feat = q, ("interior", None)
        return d2(p, foot), foot, feat
    return f


def _axial(p, O, Z):
    w = vsub(p, O)
    z = dot(w, Z)
    rad = vsub(w, vmul(Z, z))
    rho = norm(rad)
    if rho < D("1e-20"):
        raise Reject("point on an axis")
    return z, rho, vmul(rad, ONE / rho)


def cf_cyl_band(O, Z, r, v0, v1, e0, e1):
    O, Z, r, v0, v1 = dec3(O), dec3(Z), D(r), D(v0), D(v1)

    def f(p):
        z, rho, Rh = _axial(p, O, Z)
        zf, feat = (v0, ("edge", e0)) if z < v0 else ((v1, ("edge", e1)) if z > v1 else (z, ("interior", None)))
        foot = lin(O, (zf, Z), (r, Rh))
        return d2(p, foot), foot, feat
    return f


def cf_cone_band(O, Z, r0, a, v0, v1, e0, e1):
    O, Z, r0, v0, v1 = dec3(O), dec3(Z), D(r0), D(v0), D(v1)
    k = tan(D(a))

    def f(p):
        z, rho, Rh = _axial(p, O, Z)
        A = (r0 + k * v0, v0)
        B = (r0 + k * v1, v1)
        ab = (B[0] - A[0], B[1] - A[1])
        t = ((rho - A[0]) * ab[0] + (z - A[1]) * ab[1]) / (ab[0] ** 2 + ab[1] ** 2)
        if t <= 0:
            rf, zf, feat = A[0], A[1], ("edge", e0)
        elif t >= 1:
            rf, zf, feat = B[0], B[1], ("edge", e1)
        else:
            rf, zf, feat = A[0] + t * ab[0], A[1] + t * ab[1], ("interior", None)
        foot = lin(O, (zf, Z), (rf, Rh))
        return d2(p, foot), foot, feat
    return f


def cf_sphere_cap(C, Z, R, v0, e):
    C, Z, R, v0 = dec3(C), dec3(Z), D(R), D(v0)
    s0, c0 = sin_cos(v0)

    def f(p):
        w = vsub(p, C)
        lw = norm(w)
        if lw < D("1e-20"):
            raise Reject("sphere centre")
        if dot(w, Z) >= lw * s0:
            foot, feat = lin(C, (R / lw, w)), ("interior", None)
        else:
            foot, feat = circle_closest(p, lin(C, (R * s0, Z)), Z, R * c0), ("edge", e)
        return d2(p, foot), foot, feat
    return f


def cf_torus_seg(O, Z, X, R, r, u1, e0, e1):
    O, Z, X, R, r, u1 = dec3(O), dec3(Z), dec3(X), D(R), D(r), D(u1)
    Y = cross(Z, X)

    def tube(u):
        s, c = sin_cos(u)
        return lin(O, (R * c, X), (R * s, Y)), lin(Z3, (-s, X), (c, Y))

    def f(p):
        w = vsub(p, O)
        phi = atan2(dot(w, Y), dot(w, X))
        cands = []
        for psi in (wrap(phi, ZERO), wrap(phi + PI, ZERO)):
            if psi <= u1:
                c, _ = tube(psi)
                q = vsub(p, c)
                lq = norm(q)
                if lq < D("1e-20"):
                    raise Reject("tube centre")
                foot = lin(c, (r / lq, q))
                cands.append((d2(p, foot), foot, ("interior", None)))
        for u, e in ((ZERO, e0), (u1, e1)):
            c, n = tube(u)
            foot = circle_closest(p, c, n, r)
            cands.append((d2(p, foot), foot, ("edge", e)))
        return min(cands, key=lambda x: (x[0], RANK[x[2][0]]))
    return f


# trim-ignoring (untrimmed, infinite) analytic surfaces

def uf_plane(O, n):
    O, n = dec3(O), dec3(n)

    def f(p):
        q = vsub(p, vmul(n, dot(vsub(p, O), n)))
        return d2(p, q), q
    return f


def uf_cylinder(O, Z, r):
    O, Z, r = dec3(O), dec3(Z), D(r)

    def f(p):
        z, rho, Rh = _axial(p, O, Z)
        foot = lin(O, (z, Z), (r, Rh))
        return d2(p, foot), foot
    return f


def uf_cone(O, Z, r0, a):
    O, Z, r0 = dec3(O), dec3(Z), D(r0)
    k = tan(D(a))

    def f(p):
        z, rho, Rh = _axial(p, O, Z)
        best = None
        for sgn in (ONE, -ONE):  # the two generator lines of the double cone in the meridian plane
            A = (sgn * r0, ZERO)
            dvec = (sgn * k, ONE)
            t = ((rho - A[0]) * dvec[0] + z * dvec[1]) / (dvec[0] ** 2 + 1)
            rf, zf = A[0] + t * dvec[0], t
            foot = lin(O, (zf, Z), (rf, Rh))
            c = (d2(p, foot), foot)
            if best is None or c[0] < best[0]:
                best = c
        return best
    return f


def uf_sphere(C, R):
    C, R = dec3(C), D(R)

    def f(p):
        w = vsub(p, C)
        lw = norm(w)
        if lw < D("1e-20"):
            raise Reject("sphere centre")
        foot = lin(C, (R / lw, w))
        return d2(p, foot), foot
    return f


def uf_torus(O, Z, R, r):
    O, Z, R, r = dec3(O), dec3(Z), D(R), D(r)

    def f(p):
        z, rho, Rh = _axial(p, O, Z)
        c = lin(O, (R, Rh))
        q = vsub(p, c)
        lq = norm(q)
        if lq < D("1e-20"):
            raise Reject("tube centre")
        foot = lin(c, (r / lq, q))
        return d2(p, foot), foot
    return f


# ---------------------------------------------------------------- body descriptions

def placement(o, z, x):
    return {"origin": [float(c) for c in o], "axis": [float(c) for c in z], "ref_dir": [float(c) for c in x]}


def line2(a, b):
    return {"type": "line2", "from": [float(a[0]), float(a[1])], "to": [float(b[0]), float(b[1])]}


def circle2(c, r, orient):
    return {"type": "circle2", "center": [float(c[0]), float(c[1])], "radius": float(r), "orient": orient}


def fcross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


class Builder:
    def __init__(self, name, description):
        self.name, self.description = name, description
        self.vertices, self.edges, self.faces = [], [], []
        self.cf, self.ucf = {}, []

    def vertex(self, label, pt):
        self.vertices.append({"label": label, "point": [float(c) for c in pt]})
        return label

    def edge(self, label, start, end, curve, rng, degenerate=False):
        e = {"label": label, "start": start, "end": end, "curve": curve,
             "param_range": [float(x) for x in rng] if rng is not None else None}
        if degenerate:
            e["degenerate"] = True
        self.edges.append(e)

    def face(self, label, surface, sense, loops, cf=None, ucf=None):
        """loops: list of (outer, [(edge, sense, pcurve)])."""
        js = []
        for li, (outer, coedges) in enumerate(loops, 1):
            js.append({"label": f"{label}L{li}", "outer": outer,
                       "coedges": [{"label": f"{label}L{li}C{ci}", "edge": e, "sense": s, "pcurve": pc}
                                   for ci, (e, s, pc) in enumerate(coedges, 1)]})
        self.faces.append({"label": label, "surface": surface, "sense": sense, "loops": js})
        if cf is not None:
            self.cf[label] = cf
        if ucf is not None:
            self.ucf.append((label, ucf))

    def json(self):
        return {"format": "fid-brep-builder/0", "body_id": self.name, "unit": "m",
                "description": self.description, "vertices": self.vertices, "edges": self.edges,
                "faces": self.faces, "shells": [{"label": "S1", "faces": [f["label"] for f in self.faces]}]}


def build_cube():
    b = Builder("cube", "Axis-aligned cube, side 0.125 m: 6 planar faces, 12 line edges, 8 vertices.")
    c0, a = (0.0625, -0.03125, 0.015625), 0.125

    def vp(idx):
        return tuple(c0[i] + a * idx[i] for i in range(3))
    vl = {}
    for k in (0, 1):
        for j in (0, 1):
            for i in (0, 1):
                vl[(i, j, k)] = b.vertex(f"V{1 + i + 2 * j + 4 * k}", vp((i, j, k)))
    emap, n = {}, 0
    for ax in range(3):
        o = [d for d in range(3) if d != ax]
        for s1 in (0, 1):
            for s2 in (0, 1):
                i0 = [0, 0, 0]
                i0[o[0]], i0[o[1]] = s1, s2
                i1 = list(i0)
                i1[ax] = 1
                n += 1
                d = [0.0, 0.0, 0.0]
                d[ax] = 1.0
                lab = f"E{n}"
                b.edge(lab, vl[tuple(i0)], vl[tuple(i1)], {"type": "line", "origin": list(vp(i0)), "direction": d},
                       (0.0, a))
                emap[frozenset((tuple(i0), tuple(i1)))] = (lab, tuple(i0), tuple(i1))
    fi = 0
    for ax in range(3):
        for side in (0, 1):
            fi += 1
            Zv = [0.0, 0.0, 0.0]
            Zv[ax] = 1.0 if side else -1.0
            o1, o2 = [d for d in range(3) if d != ax]
            Xv = [0.0, 0.0, 0.0]
            Xv[o1] = 1.0
            ys = fcross(Zv, Xv)[o2]

            def corner(uu, vv, ax=ax, side=side, o1=o1, o2=o2, ys=ys):
                idx = [0, 0, 0]
                idx[ax], idx[o1], idx[o2] = side, uu, (vv if ys > 0 else 1 - vv)
                return tuple(idx)
            sq = [(0, 0), (1, 0), (1, 1), (0, 1)]
            coedges, segs = [], []
            for k in range(4):
                A, B = sq[k], sq[(k + 1) % 4]
                ca, cb = corner(*A), corner(*B)
                lab, st, en = emap[frozenset((ca, cb))]
                same = st == ca
                ua = (A[0] * a, A[1] * a)
                ub = (B[0] * a, B[1] * a)
                coedges.append((lab, "same" if same else "reversed", line2(ua, ub) if same else line2(ub, ua)))
                segs.append((lab, vp(ca), vp(cb), vl[ca], vl[cb]))  # loop order, for the inside test
            origin = vp(corner(0, 0))
            b.face(f"F{fi}", {"type": "plane", "placement": placement(origin, Zv, Xv)}, "same", [(True, coedges)],
                   cf=cf_plane_poly(origin, Zv, segs), ucf=uf_plane(origin, Zv))
    return b, (c0, a)


def build_tube():
    b = Builder("tube", "Cylinder with a coaxial through-hole: outer and inner cylindrical faces, two planar "
                        "annuli (inner loops), four circle edges and two seams.")
    O, Z, X, Yw = (0.015625, -0.03125, 0.0078125), [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]
    ro, ri, h = 0.0625, 0.03125, 0.09375
    Oh = (O[0], O[1], O[2] + h)
    V1 = b.vertex("V1", (O[0] + ro, O[1], O[2]))
    V2 = b.vertex("V2", (O[0] + ro, O[1], O[2] + h))
    V3 = b.vertex("V3", (O[0] + ri, O[1], O[2]))
    V4 = b.vertex("V4", (O[0] + ri, O[1], O[2] + h))
    rng = (0.0, P2)
    b.edge("E1", V1, V1, {"type": "circle", "placement": placement(O, Z, X), "radius": ro}, rng)
    b.edge("E2", V2, V2, {"type": "circle", "placement": placement(Oh, Z, X), "radius": ro}, rng)
    b.edge("E3", V3, V3, {"type": "circle", "placement": placement(O, Z, X), "radius": ri}, rng)
    b.edge("E4", V4, V4, {"type": "circle", "placement": placement(Oh, Z, X), "radius": ri}, rng)
    b.edge("E5", V1, V2, {"type": "line", "origin": [O[0] + ro, O[1], O[2]], "direction": Z}, (0.0, h))
    b.edge("E6", V3, V4, {"type": "line", "origin": [O[0] + ri, O[1], O[2]], "direction": Z}, (0.0, h))
    b.face("F1", {"type": "cylinder", "placement": placement(O, Z, X), "radius": ro}, "same",
           [(True, [("E1", "same", line2((0, 0), (P2, 0))), ("E5", "same", line2((P2, 0), (P2, h))),
                    ("E2", "reversed", line2((0, h), (P2, h))), ("E5", "reversed", line2((0, 0), (0, h)))])],
           cf=cf_cyl_band(O, Z, ro, 0, h, "E1", "E2"), ucf=uf_cylinder(O, Z, ro))
    b.face("F2", {"type": "cylinder", "placement": placement(O, Z, X), "radius": ri}, "reversed",
           [(True, [("E6", "same", line2((0, 0), (0, h))), ("E4", "same", line2((0, h), (P2, h))),
                    ("E6", "reversed", line2((P2, 0), (P2, h))), ("E3", "reversed", line2((0, 0), (P2, 0)))])],
           cf=cf_cyl_band(O, Z, ri, 0, h, "E3", "E4"), ucf=uf_cylinder(O, Z, ri))
    mZ = [0.0, 0.0, -1.0]
    b.face("F3", {"type": "plane", "placement": placement(O, mZ, X)}, "same",
           [(True, [("E1", "same", circle2((0, 0), ro, -1))]), (False, [("E3", "reversed", circle2((0, 0), ri, -1))])],
           cf=cf_plane_annulus(O, mZ, ro, "E1", ri, "E3"), ucf=uf_plane(O, mZ))
    b.face("F4", {"type": "plane", "placement": placement(Oh, Z, X)}, "same",
           [(True, [("E2", "same", circle2((0, 0), ro, 1))]), (False, [("E4", "reversed", circle2((0, 0), ri, 1))])],
           cf=cf_plane_annulus(Oh, Z, ro, "E2", ri, "E4"), ucf=uf_plane(Oh, Z))
    del Yw
    return b


def build_sphere_cap():
    b = Builder("sphere_cap", "Solid spherical cap: the sphere above latitude 0.25 rad plus a planar base disc; "
                              "one circle edge, a seam and a degenerate pole edge.")
    C, Z, X = (-0.0234375, 0.046875, 0.03125), [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]
    R, v0 = 0.0625, 0.25
    s0, c0 = sin_cos(D(v0))
    hb = D(C[2]) + D(R) * s0
    rb = float(D(R) * c0)
    Ob = (C[0], C[1], float(hb))
    V1 = b.vertex("V1", f3((D(C[0]) + D(R) * c0, D(C[1]), hb)))
    V2 = b.vertex("V2", (C[0], C[1], C[2] + R))
    b.edge("E1", V1, V1, {"type": "circle", "placement": placement(Ob, Z, X), "radius": rb}, (0.0, P2))
    b.edge("E2", V1, V2, None, (0.0, 1.0))
    b.edge("E3", V2, V2, None, None, degenerate=True)
    b.face("F1", {"type": "sphere", "placement": placement(C, Z, X), "radius": R}, "same",
           [(True, [("E1", "same", line2((0, v0), (P2, v0))), ("E2", "same", line2((P2, v0), (P2, PH))),
                    ("E3", "same", line2((P2, PH), (0, PH))), ("E2", "reversed", line2((0, v0), (0, PH)))])],
           cf=cf_sphere_cap(C, Z, R, v0, "E1"), ucf=uf_sphere(C, R))
    mZ = [0.0, 0.0, -1.0]
    b.face("F2", {"type": "plane", "placement": placement(Ob, mZ, X)}, "same",
           [(True, [("E1", "same", circle2((0, 0), rb, -1))])],
           cf=cf_plane_annulus(Ob, mZ, rb, "E1", None, None), ucf=uf_plane(Ob, mZ))
    return b


def build_torus_segment():
    b = Builder("torus_segment", "Solid quarter torus segment (u in [0, pi/2]) with two planar end discs; "
                                 "two circle edges and a seam.")
    O, Z, X, Y = (0.03125, 0.015625, -0.046875), [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]
    R, r = 0.09375, 0.03125
    c0 = (O[0] + R, O[1], O[2])
    c1 = (O[0], O[1] + R, O[2])
    V1 = b.vertex("V1", (O[0] + R + r, O[1], O[2]))
    V2 = b.vertex("V2", (O[0], O[1] + R + r, O[2]))
    b.edge("E1", V1, V1, {"type": "circle", "placement": placement(c0, [0.0, -1.0, 0.0], X), "radius": r}, (0.0, P2))
    b.edge("E2", V2, V2, {"type": "circle", "placement": placement(c1, X, Y), "radius": r}, (0.0, P2))
    b.edge("E3", V1, V2, {"type": "circle", "placement": placement(O, Z, X), "radius": R + r}, (0.0, PH))
    b.face("F1", {"type": "torus", "placement": placement(O, Z, X), "major_radius": R, "minor_radius": r}, "same",
           [(True, [("E3", "same", line2((0, 0), (PH, 0))), ("E2", "same", line2((PH, 0), (PH, P2))),
                    ("E3", "reversed", line2((0, P2), (PH, P2))), ("E1", "reversed", line2((0, 0), (0, P2)))])],
           cf=cf_torus_seg(O, Z, X, R, r, PH, "E1", "E2"), ucf=uf_torus(O, Z, R, r))
    mY, mX = [0.0, -1.0, 0.0], [-1.0, 0.0, 0.0]
    b.face("F2", {"type": "plane", "placement": placement(c0, mY, X)}, "same",
           [(True, [("E1", "same", circle2((0, 0), r, 1))])],
           cf=cf_plane_annulus(c0, mY, r, "E1", None, None), ucf=uf_plane(c0, mY))
    b.face("F3", {"type": "plane", "placement": placement(c1, mX, Y)}, "same",
           [(True, [("E2", "same", circle2((0, 0), r, -1))])],
           cf=cf_plane_annulus(c1, mX, r, "E2", None, None), ucf=uf_plane(c1, mX))
    return b


def build_cone_frustum():
    b = Builder("cone_frustum", "Solid cone frustum (half-angle 0.375 rad) along +x with two planar end discs; "
                                "two circle edges and a seam.")
    O, Z, X = (-0.015625, 0.0390625, 0.0234375), [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]
    r0, a, h = 0.046875, 0.375, 0.078125
    rt_exact = D(r0) + D(h) * tan(D(a))
    rt = float(rt_exact)
    Oh = (O[0] + h, O[1], O[2])
    V1 = b.vertex("V1", (O[0], O[1] + r0, O[2]))
    V2 = b.vertex("V2", f3((D(O[0]) + D(h), D(O[1]) + rt_exact, D(O[2]))))
    b.edge("E1", V1, V1, {"type": "circle", "placement": placement(O, Z, X), "radius": r0}, (0.0, P2))
    b.edge("E2", V2, V2, {"type": "circle", "placement": placement(Oh, Z, X), "radius": rt}, (0.0, P2))
    b.edge("E3", V1, V2, None, (0.0, 1.0))
    b.face("F1", {"type": "cone", "placement": placement(O, Z, X), "radius": r0, "half_angle": a}, "same",
           [(True, [("E1", "same", line2((0, 0), (P2, 0))), ("E3", "same", line2((P2, 0), (P2, h))),
                    ("E2", "reversed", line2((0, h), (P2, h))), ("E3", "reversed", line2((0, 0), (0, h)))])],
           cf=cf_cone_band(O, Z, r0, a, 0, h, "E1", "E2"), ucf=uf_cone(O, Z, r0, a))
    mZ = [-1.0, 0.0, 0.0]
    b.face("F2", {"type": "plane", "placement": placement(O, mZ, X)}, "same",
           [(True, [("E1", "same", circle2((0, 0), r0, -1))])],
           cf=cf_plane_annulus(O, mZ, r0, "E1", None, None), ucf=uf_plane(O, mZ))
    b.face("F3", {"type": "plane", "placement": placement(Oh, Z, X)}, "same",
           [(True, [("E2", "same", circle2((0, 0), rt, 1))])],
           cf=cf_plane_annulus(Oh, Z, rt, "E2", None, None), ucf=uf_plane(Oh, Z))
    return b


def nurbs_json(pu, pv, U, V, ctrl, weights=None):
    nu, nv = len(ctrl), len(ctrl[0])
    return {"type": "nurbs", "degree_u": pu, "degree_v": pv, "n_u": nu, "n_v": nv,
            "knots_u": [float(x) for x in U], "knots_v": [float(x) for x in V],
            "ctrl": [[float(c) for c in ctrl[i][k]] for i in range(nu) for k in range(nv)],
            "weights": None if weights is None else [float(weights[i][k]) for i in range(nu) for k in range(nv)]}


def poly_face(b, flabel, surface, loops, vstart, estart):
    """Polygon trim loops on a NURBS face; edges are surface o pcurve (no 3D curve)."""
    surf = make_surface(surface)
    vi, ei, js = vstart, estart, []
    for outer, pts in loops:
        labs = []
        for uv in pts:
            vi += 1
            labs.append(b.vertex(f"V{vi}", f3(surf.point(D(uv[0]), D(uv[1])))))
        co = []
        for k in range(len(pts)):
            ei += 1
            lab = f"E{ei}"
            b.edge(lab, labs[k], labs[(k + 1) % len(pts)], None, (0.0, 1.0))
            co.append((lab, "same", line2(pts[k], pts[(k + 1) % len(pts)])))
        js.append((outer, co))
    b.face(flabel, surface, "same", js)
    return vi, ei


def build_nurbs_bicubic_holed():
    b = Builder("nurbs_bicubic_holed", "Non-rational bicubic B-spline face (5 x 5, one interior knot) trimmed by "
                                       "a quadrilateral outer loop and a triangular inner loop (hole).")
    U = [0, 0, 0, 0, 0.5, 1, 1, 1, 1]
    ctrl = [[(0.0625 * i - 0.015625, 0.0625 * k + 0.0078125, ((i * 7 + k * 13) % 9 - 4) * 0.00390625)
             for k in range(5)] for i in range(5)]
    surf = nurbs_json(3, 3, U, U, ctrl)
    poly_face(b, "F1", surf, [(True, [(0.0625, 0.125), (0.9375, 0.0625), (0.875, 0.9375), (0.125, 0.875)]),
                              (False, [(0.375, 0.4375), (0.5, 0.65625), (0.65625, 0.375)])], 0, 0)
    return b


def build_nurbs_rational_pair():
    b = Builder("nurbs_rational_pair", "Open shell of two trimmed faces: a rational biquadratic patch with a "
                                       "square hole, and a non-rational biquadratic patch above it with a "
                                       "triangular hole.")
    U = [0, 0, 0, 1, 1, 1]
    zi = [0.0, 0.0625, 0.0]
    ctrl1 = [[(0.0625 * i, 0.0625 * k, zi[i] + (0.015625 * k if i == 1 else 0.0)) for k in range(3)]
             for i in range(3)]
    w1 = [[(1.0, 0.75, 1.0)[i] * (1.0, 1.25, 1.0)[k] for k in range(3)] for i in range(3)]
    vi, ei = poly_face(b, "F1", nurbs_json(2, 2, U, U, ctrl1, w1),
                       [(True, [(0.03125, 0.03125), (0.96875, 0.0625), (0.9375, 0.96875), (0.0625, 0.9375)]),
                        (False, [(0.375, 0.375), (0.375, 0.625), (0.625, 0.625), (0.625, 0.375)])], 0, 0)
    ctrl2 = [[(0.0625 * i - 0.03125, 0.0625 * k + 0.015625,
               0.125 + 0.015625 * (i + k) - (0.03125 if (i, k) == (1, 1) else 0.0)) for k in range(3)]
             for i in range(3)]
    poly_face(b, "F2", nurbs_json(2, 2, U, U, ctrl2),
              [(True, [(0.0625, 0.0625), (0.9375, 0.125), (0.875, 0.9375), (0.0625, 0.875)]),
               (False, [(0.4375, 0.40625), (0.65625, 0.5), (0.46875, 0.65625)])], vi, ei)
    return b


BEZ_SQ = (0.25, -0.25, 0.25)  # Bernstein coefficients of (t - 1/2)^2


def build_annulus_trap():
    b = Builder("annulus_trap", "Paraboloid z = 2 r^2 as a non-rational biquadratic patch, trimmed to an annulus "
                                "by two circle pcurves; points above the hole have their untrimmed minimum in "
                                "the hole.")
    x0, y0, z0, s = -0.03125, 0.0234375, 0.0078125, 0.25
    ctrl = [[(x0 + s * (i / 2 - 0.5), y0 + s * (k / 2 - 0.5), z0 + 0.125 * (BEZ_SQ[i] + BEZ_SQ[k]))
             for k in range(3)] for i in range(3)]
    surface = nurbs_json(2, 2, [0, 0, 0, 1, 1, 1], [0, 0, 0, 1, 1, 1], ctrl)
    ro, ri = 0.4375, 0.1875
    Z, X = [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]
    rr_o, rr_i = s * ro, s * ri
    V1 = b.vertex("V1", (x0 + rr_o, y0, z0 + 2 * rr_o * rr_o))
    V2 = b.vertex("V2", (x0 + rr_i, y0, z0 + 2 * rr_i * rr_i))
    b.edge("E1", V1, V1, {"type": "circle", "placement": placement((x0, y0, z0 + 2 * rr_o * rr_o), Z, X),
                          "radius": rr_o}, (0.0, P2))
    b.edge("E2", V2, V2, {"type": "circle", "placement": placement((x0, y0, z0 + 2 * rr_i * rr_i), Z, X),
                          "radius": rr_i}, (0.0, P2))
    b.face("F1", surface, "same", [(True, [("E1", "same", circle2((0.5, 0.5), ro, 1))]),
                                   (False, [("E2", "reversed", circle2((0.5, 0.5), ri, 1))])])
    b.trap = (x0, y0, z0)
    return b


def build_saddle():
    b = Builder("saddle_two_minima", "Saddle z = 8 (x^2 - y^2) as a non-rational biquadratic patch; points above "
                                     "the centre have two symmetric minima, and a hole trims one of them away "
                                     "for heights in about [0.08, 0.13] m.")
    x0, y0, z0, s = 0.015625, -0.0078125, 0.0, 0.25
    ctrl = [[(x0 + s * (i / 2 - 0.5), y0 + s * (k / 2 - 0.5), z0 + 0.5 * (BEZ_SQ[i] - BEZ_SQ[k]))
             for k in range(3)] for i in range(3)]
    surface = nurbs_json(2, 2, [0, 0, 0, 1, 1, 1], [0, 0, 0, 1, 1, 1], ctrl)
    poly_face(b, "F1", surface,
              [(True, [(0.03125, 0.03125), (0.96875, 0.03125), (0.96875, 0.96875), (0.03125, 0.96875)]),
               (False, [(0.6875, 0.4375), (0.6875, 0.5625), (0.875, 0.5625), (0.875, 0.4375)])], 0, 0)
    b.trap = (x0, y0, z0)
    return b


BODIES = [
    ("cube.json", build_cube),
    ("tube.json", build_tube),
    ("sphere_cap.json", build_sphere_cap),
    ("torus_segment.json", build_torus_segment),
    ("cone_frustum.json", build_cone_frustum),
    ("nurbs_bicubic_holed.json", build_nurbs_bicubic_holed),
    ("nurbs_rational_pair.json", build_nurbs_rational_pair),
    ("annulus_trap.json", build_annulus_trap),
    ("saddle_two_minima.json", build_saddle),
]


# ---------------------------------------------------------------- query generation

def hexdec(x):
    f = float(x)
    return {"hex": f.hex(), "dec": f}


def vec(v):
    fs = [float(x) for x in v]
    return {"hex": [f.hex() for f in fs], "dec": fs}


def feat_json(feat):
    return {"kind": feat[0]} if feat[0] == "interior" else {"kind": feat[0], "id": feat[1]}


def unit_dir(rng):
    while True:
        v = [2 * rng.random() - 1 for _ in range(3)]
        n2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
        if 1e-6 < n2 <= 1:
            n = math.sqrt(n2)
            return [x / n for x in v]


def log_uniform(rng, lo_exp, hi_exp):
    """10^U(lo_exp, hi_exp) through Decimal so the bytes do not depend on the platform libm."""
    e = D(lo_exp) + (D(hi_exp) - D(lo_exp)) * D(rng.random())
    return float(D(10) ** e)


class Gen:
    def __init__(self, body, builder, rng, cube=None):
        self.body, self.b, self.rng, self.cube = body, builder, rng, cube
        self.queries = []
        self.keys = set()

    def rand_point(self, scale):
        lo, hi = self.body.lo, self.body.hi
        out = []
        for i in range(3):
            c, hw = (lo[i] + hi[i]) / 2, (hi[i] - lo[i]) / 2
            out.append(c + hw * scale * (2 * self.rng.random() - 1))
        return out

    def evaluate(self, pf, gen):
        """Full oracle answer for one query; None if degenerate. Runs the AC-2 self-check."""
        key = tuple(x.hex() for x in pf)
        if key in self.keys:
            return None
        body = self.body
        try:
            ans = body.solve(pf)
            un = body.untrimmed(pf)
            if body.cf:
                self.self_check(ans)
        except Reject:
            return None
        c = ans["chosen"]
        fed = body.foot_edge_distance(ans)
        tags = []
        if un[2] < ans["dist"] - TRIM_MARGIN:
            tags.append("trimmed_away")
        if ans["edge_dist"] is not None and ans["edge_dist"] <= EDGE_NEAR:
            tags.append("near_edge")
        if any(a.face != c.face and norm(vsub(a.foot, c.foot)) > D("1e-9") for a in ans["alts"]):
            tags.append("equidistant")
        if ans["alts"]:
            tags.append("tie")
        q = {"id": f"q{len(self.queries) + 1:04d}", "gen": gen, "tags": tags, "p": vec(pf),
             "face": c.face, "feature": feat_json(c.feat), "foot": vec(c.foot), "distance": hexdec(ans["dist"]),
             "alternatives": [{"face": a.face, "feature": feat_json(a.feat), "foot": vec(a.foot),
                               "distance": hexdec(a.d2.sqrt())} for a in ans["alts"]],
             "edge_distance": hexdec(ans["edge_dist"]) if ans["edge_dist"] is not None else None,
             "foot_edge_distance": hexdec(fed) if fed is not None else None,
             "untrimmed": {"face": un[0], "foot": vec(un[1]), "distance": hexdec(un[2])}}
        self.sanity(ans, pf)
        self.keys.add(key)
        self.queries.append(q)
        return q

    def self_check(self, ans):
        body = self.body
        p = ans["p"]
        lab, dd, foot, feat = body.closed_form(p)
        dc = dd.sqrt()
        dev = abs(dc - ans["dist"])
        ok = False
        for a in [ans["chosen"]] + ans["alts"]:
            if a.face == lab and a.feat == feat and norm(vsub(a.foot, foot)) <= SELF_TOL:
                ok = True
                dev = max(dev, norm(vsub(a.foot, foot)))
        if not ok or abs(dc - ans["dist"]) > SELF_TOL:
            die(f"AC-2 self-check failed on {body.name} at p={f3(p)}: closed form {lab} {feat} d={dc:.20e}, "
                f"oracle {ans['chosen'].face} {ans['chosen'].feat} d={ans['dist']:.20e}")
        STATS["selfcheck"] += 1
        STATS["selfcheck_max_dev"] = max(STATS["selfcheck_max_dev"], dev)
        if self.cube is not None:
            c0, a = self.cube
            lo = [D(x) for x in c0]
            hi = [D(x) + D(a) for x in c0]
            if any(p[i] < lo[i] or p[i] > hi[i] for i in range(3)):
                cl = tuple(min(max(p[i], lo[i]), hi[i]) for i in range(3))
                if norm(vsub(cl, ans["chosen"].foot)) > SELF_TOL:
                    die(f"AC-2 clamp check failed on cube at p={f3(p)}")
                STATS["clamp_checks"] += 1

    def sanity(self, ans, pf):
        """The answer is never worse than any sampled point of the trimmed faces or edges."""
        dd = ans["chosen"].d2
        ddf = float(dd)
        for f in self.body.faces:
            g = f.grid_d2(pf)
            for i, row in enumerate(g):
                for k, x in enumerate(row):
                    if f.mask[i][k] and x < ddf * (1 - 1e-9) - 1e-24:
                        die(f"{self.body.name}: grid point beats the oracle at p={pf}")
        for e in self.body.real_edges:
            for x, y, z in e.pts:
                if (x - pf[0]) ** 2 + (y - pf[1]) ** 2 + (z - pf[2]) ** 2 < ddf * (1 - 1e-9) - 1e-24:
                    die(f"{self.body.name}: edge sample beats the oracle at p={pf}")

    def count(self, tag):
        return sum(1 for q in self.queries if tag in q["tags"])

    def gen_random(self, n):
        for _ in range(n):
            for _ in range(50):
                if self.evaluate(self.rand_point(1.3), "random"):
                    break

    def trim_prefilter(self, pf):
        body = self.body
        p = tuple(D(x) for x in pf)
        if body.cf:
            try:
                un = body.untrimmed(pf)
                cf = body.closed_form(p)
            except Reject:
                return False
            return un[2] < cf[1].sqrt() - D("1e-7")
        un_min = tr_min = math.inf
        for f, vf in zip(body.faces, body.virtual):
            g = f.grid_d2(pf)
            for i, row in enumerate(g):
                for k, x in enumerate(row):
                    if f.mask[i][k]:
                        tr_min = min(tr_min, x)
            for row in vf.grid_d2(pf):
                un_min = min(un_min, min(row))
        return math.sqrt(un_min) < math.sqrt(tr_min) - 1e-4

    def gen_trimmed(self, n, scale=1.6):
        target = self.count("trimmed_away") + n
        tries = 0
        while self.count("trimmed_away") < target:
            tries += 1
            if tries > 40000:
                die(f"{self.body.name}: could not generate enough trimmed-away queries")
            pf = self.rand_point(scale)
            if self.trim_prefilter(pf):
                q = self.evaluate(pf, "trimmed_away")
                if q is not None and "trimmed_away" not in q["tags"]:
                    self.queries.pop()
                    self.keys.discard(tuple(x.hex() for x in pf))

    def gen_near_edge(self, n):
        edges = self.body.real_edges
        done = 0
        while done < n:
            e = edges[int(self.rng.random() * len(edges))]
            t = e.t0 + (e.t1 - e.t0) * D(self.rng.random())
            P = f3(e.point(t))
            L = log_uniform(self.rng, -9, D("-6.05"))
            dv = unit_dir(self.rng)
            pf = [P[i] + L * dv[i] for i in range(3)]
            q = self.evaluate(pf, "near_edge")
            if q is not None:
                if "near_edge" not in q["tags"]:
                    die(f"{self.body.name}: near-edge query is not within 1e-6 m of an edge")
                done += 1

    def gen_near_vertex(self, n):
        vs = [v for v in self.b.vertices]
        done = 0
        while done < n:
            v = vs[done % len(vs)]["point"]
            L = log_uniform(self.rng, -9, -4)
            dv = unit_dir(self.rng)
            if self.evaluate([v[i] + L * dv[i] for i in range(3)], "near_vertex") is not None:
                done += 1

    def gen_equidistant(self, n):
        body = self.body
        if len(body.faces) < 2:
            return
        done, tries = 0, 0
        while done < n:
            tries += 1
            if tries > 400:
                die(f"{body.name}: could not generate equidistant queries")
            a = self.rand_point(1.2)
            try:
                fa = body.solve(a)["chosen"].face
            except Reject:
                continue
            others = [f for f in body.faces if f.label != fa]
            fb = others[int(self.rng.random() * len(others))]
            cells = [(i, k) for i in range(GRID) for k in range(GRID) if fb.mask[i][k]]
            i, k = cells[int(self.rng.random() * len(cells))]
            g = fb.gp[i][k]
            dv = unit_dir(self.rng)
            bpt = [g[j] + 1e-3 * dv[j] for j in range(3)]
            if body.solve(bpt)["chosen"].face != fb.label:
                continue
            FA = next(f for f in body.faces if f.label == fa)

            def gap(s):
                pf = [a[j] + s * (bpt[j] - a[j]) for j in range(3)]
                p = tuple(D(x) for x in pf)
                return body.face_best(FA, p, pf, {}).d2.sqrt() - body.face_best(fb, p, pf, {}).d2.sqrt(), pf
            lo, hi = 0.0, 1.0
            if not (gap(lo)[0] < 0 < gap(hi)[0]):
                continue
            pf = None
            for _ in range(70):
                mid = (lo + hi) / 2
                if mid in (lo, hi):
                    break
                g_, pf_ = gap(mid)
                if g_ < 0:
                    lo = mid
                else:
                    hi = mid
                pf = pf_
            q = self.evaluate(pf, "equidistant")
            if q is None:
                continue
            if "equidistant" not in q["tags"]:
                self.queries.pop()
                self.keys.discard(tuple(x.hex() for x in pf))
                continue
            done += 1

    def gen_trap(self, n):
        x0, y0, z0 = self.b.trap
        for _ in range(n):
            th = TWO_PI * D(self.rng.random())
            s, c = sin_cos(th)
            eps = D(log_uniform(self.rng, -8, -3))
            h = 0.006 + 0.23 * self.rng.random()
            self.evaluate([float(D(x0) + eps * c), float(D(y0) + eps * s), z0 + h], "trap")

    def gen_two_minima(self, n):
        x0, y0, z0 = self.b.trap
        for k in range(n):
            h = 0.07 + 0.09 * self.rng.random()
            dx = 0.0 if k % 2 == 0 else log_uniform(self.rng, -6, -3)
            self.evaluate([x0 + dx, y0, z0 + h], "two_minima")


def generate(seed, log):
    files = {}
    for bi, (fname, fn) in enumerate(BODIES):
        built = fn()
        cube = None
        if isinstance(built, tuple):
            built, cube = built
        bj = built.json()
        body = Body(bj, built.cf if built.cf else None, built.ucf if built.ucf else None)
        rng = random.Random(seed * 1000 + bi)
        g = Gen(body, built, rng, cube)
        g.gen_random(30)
        g.gen_trimmed(55)
        g.gen_near_edge(55)
        g.gen_near_vertex(12)
        g.gen_equidistant(8)
        if fname == "annulus_trap.json":
            g.gen_trap(24)
        if fname == "saddle_two_minima.json":
            g.gen_two_minima(24)
        doc_head = {"format": FORMAT, "oracle_version": ORACLE_VERSION, "seed": seed, "body_seed": seed * 1000 + bi,
                    "license": "CC-BY-4.0", "unit": "m", "foot_tolerance_m": 1e-9,
                    "analytic": bool(built.cf)}
        text = "{\n"
        for k, v in doc_head.items():
            text += f" {json.dumps(k)}: {json.dumps(v)},\n"
        text += ' "body": ' + json.dumps(bj, indent=1).replace("\n", "\n ") + ",\n"
        text += ' "queries": [\n' + ",\n".join("  " + json.dumps(q, separators=(",", ":")) for q in g.queries)
        text += "\n ]\n}\n"
        files[fname] = (text.encode("utf-8"), seed * 1000 + bi, len(g.queries))
        log(f"{fname}: {len(g.queries)} queries, trimmed_away={g.count('trimmed_away')}, "
            f"near_edge={g.count('near_edge')}, equidistant={g.count('equidistant')}, tie={g.count('tie')}")
    man = {"format": FORMAT, "oracle_version": ORACLE_VERSION, "seed": seed, "license": "CC-BY-4.0",
           "generator": "tools/fixtures/face_proj_oracle.py",
           "files": [{"path": k, "sha256": "sha256:" + hashlib.sha256(v[0]).hexdigest(), "seed": v[1],
                      "oracle_version": ORACLE_VERSION, "queries": v[2]} for k, v in sorted(files.items())]}
    files["manifest.json"] = ((json.dumps(man, indent=1) + "\n").encode("utf-8"), seed, None)
    return files


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", required=True)
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--check", action="store_true", help="regenerate in memory and compare with --out")
    args = ap.parse_args()
    files = generate(args.seed, lambda m: sys.stderr.write(m + "\n"))
    sys.stderr.write(f"AC-2 self-checks: {STATS['selfcheck']} (max deviation {STATS['selfcheck_max_dev']:.3e} m), "
                     f"cube clamp checks: {STATS['clamp_checks']}\n")
    out = Path(args.out)
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
