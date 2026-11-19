#!/usr/bin/env python3
"""Independent reference for GEO.NURBS.ANALYTIC (CC-BY-4.0 project data).

Writes, next to this script:
  digests-v1.txt   SHA-256 of the pinned evaluation and inversion of every entity of
                   fixtures/geo/nurbs/analytic_set.json, plus one line for fuzz_set.json (AC-4)
  fuzz_set.json    extreme and singular cases with their expected outcome and an independent
                   high-precision reference (construction, evaluation, inversion)

Run:  python fixtures/geo/nurbs/analytic/make_analytic_fixtures.py          (writes both)
      python fixtures/geo/nurbs/analytic/make_analytic_fixtures.py --check  (fails if stale)

Stdlib only; never runs the Rust code. Three parts:

1. A bit-exact binary64 model of crates/fid-geo-nurbs/src/analytic.rs: the order of operations
   written in that file, sin/cos/tan/atan2/hypot from the bit-exact Python model of the
   fid-num vendored libm (fixtures/num/make_fn_fixtures.py, itself checked against fid-num's
   self-test digests), math.sqrt (correctly rounded, as fid_num::f::sqrt), and the normal and
   curvature formulas of the GEO.NURBS.EVAL model (fixtures/geo/nurbs/eval/
   make_eval_fixtures.py). CPython floats are binary64, round-to-nearest-even, never fused,
   so the model's bit patterns are the Rust results on every target. The digests are
   computed from these bits.
2. Accuracy of that model against values that do not come from it, before anything is
   written: the 60-digit GEO.NURBS.REF oracle values in analytic_set.json (positions and feet
   within 1e-13 m, derivatives within 1e-12 x max(1, |reference|), normals within 1e-12,
   curvatures within 1e-9 x max(1, |k1|, |k2|), distances within 1e-13 m, non_unique flags
   equal, parameters equal modulo 2 pi within 1e-12), and the fuzz set below.
3. The fuzz set. Expected outcomes are decided in exact rational arithmetic (fractions) or at
   60 decimal digits with margins of at least 100x around every pinned threshold, so a binary64
   implementation cannot land on the other side by rounding. Reference values: exact rationals
   for local coordinates, lines and planes; 60-digit closed forms (the GEO.NURBS.REF oracle
   tools/fixtures/nurbs_oracle.py, plus a two-nappe cone written here) for the rest; the
   90-digit argument reduction of make_fn_fixtures.py for huge angles. Each ok-case carries
   its own tolerances, derived from the scale S = max(|O|, |q|, sizes) and the conditioning:
     tol_d = 64 eps S + 1e-300                       (distance; plus 2 AXIS_REL dn for a near-
                                                      singular query flagged NonUnique)
     tol_f = tol_d (1 + R / L)  or none              (foot; R the largest size, L the distance
                                                      of the query from the singular set that
                                                      fixes the foot's direction: the axis for
                                                      rotational types, the centre for a sphere,
                                                      the tube circle for a torus, and
                                                      |1 - k d| x size for an ellipse)
   A case marked accept_nonfinite (some magnitude above 1e250) may also be refused with
   NonFinite; an ok answer must still be within tolerance.

Digest text (pinned; every f64 is its binary64 bit pattern as 16 lowercase hex digits, "-"
for an absent value, vectors are three of them separated by spaces):
  eval rows (file order):    the curve and surface lines of GEO.NURBS.EVAL
                             (fixtures/geo/nurbs/eval/make_eval_fixtures.py)
  invert rows (file order):  i <params, 1 or 2> <foot> <distance> <U|N>
  fuzz cases (file order):   k <code|ok>   (construction only)
                             e <code> | e <the eval line>   (evaluation)
                             i <code> | i <the invert line> (inversion)
An entity's text is its lines each followed by "\\n"; its digest is SHA-256 of that text; "all"
is SHA-256 of the analytic_set entity texts concatenated in file order; "fuzz" is SHA-256 of
the fuzz text.
"""

import hashlib
import importlib.util
import json
import math
import random
import struct
import sys
from decimal import Decimal, getcontext, localcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent.parent
SET = HERE.parent / "analytic_set.json"
OUT_DIGESTS = HERE / "digests-v1.txt"
OUT_FUZZ = HERE / "fuzz_set.json"


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


FN = _load("make_fn_fixtures", "fixtures/num/make_fn_fixtures.py")
EV = _load("make_eval_fixtures", "fixtures/geo/nurbs/eval/make_eval_fixtures.py")
OR = _load("nurbs_oracle", "tools/fixtures/nurbs_oracle.py")
getcontext().prec = 60
getcontext().Emax = 10 ** 6
getcontext().Emin = -(10 ** 6)
D = Decimal

FRAME_TOL = 1e-12
SIZE_MIN = 1e-100
SIZE_MAX = 1e100
AXIS_REL = 1e-14
MAX_BISECT = 2200
TAU = 6.283185307179586
FRAC_PI_2 = 1.5707963267948966
EPS = 2.0 ** -52
F64_MAX = sys.float_info.max


class Err(Exception):
    def __init__(self, code, detail=None):
        super().__init__(code)
        self.code = code
        self.detail = detail


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return "%016x" % bits(x)


def unhex(s):
    return struct.unpack(">d", struct.pack(">Q", int(s, 16)))[0]


def jf(j):
    if j is None:
        return None
    h = j["hex"]

    def conv(x):
        return [conv(y) for y in x] if isinstance(x, list) else unhex(x)

    return conv(h)


def h0x(x):
    return "0x%016x" % bits(x)


def isfin(x):
    return not (math.isnan(x) or math.isinf(x))


# ---------------------------------------------------------------- part 1: binary64 model

def dot(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def fin3(a):
    return all(isfin(x) for x in a)


def check_input(vals):
    if any(math.isnan(x) for x in vals):
        raise Err("NotANumber")
    if any(math.isinf(x) for x in vals):
        raise Err("NonFinite")


def size(name, v):
    check_input([v])
    if SIZE_MIN <= v <= SIZE_MAX:
        return v
    raise Err("BadRadius", name)


def hyp(x, y):
    check_input([x, y])
    r = FN.m_hypot(x, y)
    if not isfin(r):
        raise Err("NonFinite")
    return r


def atan2(y, x):
    check_input([y, x])
    return FN.m_atan2(y, x)


def sin_cos(x):
    return FN.m_sin(x), FN.m_cos(x)


def sqrt(x):
    if math.isnan(x):
        raise Err("NotANumber")
    if math.isinf(x):
        raise Err("NonFinite")
    return math.sqrt(x)


def wrap(a):
    if a == 0.0:
        return 0.0
    if a < 0.0:
        w = a + TAU
        return 0.0 if w >= TAU else w
    return a


class EvalErr(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


def param(t, lo, hi, bounded):
    if math.isnan(t):
        raise EvalErr("NotANumber")
    if math.isinf(t) or (bounded and (t < lo or t > hi)):
        raise EvalErr("OutsideDomain")
    return t


class Placement:
    def __init__(self, o, z, x):
        check_input(o + z + x)
        az = math.sqrt(dot(z, z))
        if abs(az - 1.0) > FRAME_TOL:
            raise Err("BadPlacement", "AxisNotUnit")
        ax = math.sqrt(dot(x, x))
        if abs(ax - 1.0) > FRAME_TOL:
            raise Err("BadPlacement", "RefDirNotUnit")
        if abs(dot(z, x)) > FRAME_TOL:
            raise Err("BadPlacement", "NotOrthogonal")
        self.o, self.z, self.x, self.y = o, z, x, cross(z, x)

    def world(self, l):
        o, x, y, z = self.o, self.x, self.y, self.z
        return [((o[i] + l[0] * x[i]) + l[1] * y[i]) + l[2] * z[i] for i in range(3)]

    def lin(self, l):
        x, y, z = self.x, self.y, self.z
        return [(l[0] * x[i] + l[1] * y[i]) + l[2] * z[i] for i in range(3)]

    def local(self, q):
        check_input(q)
        d = [q[0] - self.o[0], q[1] - self.o[1], q[2] - self.o[2]]
        l = [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
        if not (fin3(d) and fin3(l)):
            raise Err("NonFinite")
        return l


def curve_regular(d1):
    l2 = dot(d1, d1)
    cl = math.sqrt(l2) if isfin(l2) else l2
    den = (cl * cl) * cl
    if not (isfin(l2) and isfin(den)):
        raise EvalErr("NonFinite")
    return den != 0.0


def surface_regular(su, sv):
    a2, b2 = dot(su, su), dot(sv, sv)
    cr = cross(su, sv)
    c2 = dot(cr, cr)
    if not all(isfin(v) for v in (a2, b2, c2, a2 * b2)):
        raise EvalErr("NonFinite")
    return not EV.surface_degenerate(su, sv)


Z3 = [0.0, 0.0, 0.0]
PERIOD = (0.0, TAU)
ALL = (-math.inf, math.inf)
LAT = (-FRAC_PI_2, FRAC_PI_2)


class Model:
    """One analytic entity in binary64 (mirrors analytic.rs)."""

    def __init__(self, typ, o, z, x, prm):
        self.type = typ
        self.curve = typ in ("line", "circle", "ellipse")
        self.pl = Placement(o, z, x)
        if typ in ("circle", "cylinder", "sphere"):
            self.r = size("radius", prm["radius"])
        elif typ == "ellipse":
            self.a = size("semi_axis_1", prm["semi_axis_1"])
            self.b = size("semi_axis_2", prm["semi_axis_2"])
        elif typ == "cone":
            r, ang = prm["radius"], prm["semi_angle"]
            check_input([r, ang])
            self.r0 = 0.0 if r == 0.0 else size("radius", r)
            if ang <= 0.0 or ang >= FRAC_PI_2:
                raise Err("BadAngle")
            self.sa, self.ca, self.ta = FN.m_sin(ang), FN.m_cos(ang), FN.m_tan(ang)
        elif typ == "torus":
            self.R = size("major_radius", prm["major_radius"])
            self.rr = size("minor_radius", prm["minor_radius"])
            if self.rr >= self.R:
                raise Err("BadRadius", "minor_radius")

    # -- evaluation: (params, p, derivatives) like the EVAL model's m.curve / m.surf
    def curve_eval(self, t, order=2):
        lo, hi = ALL if self.type == "line" else PERIOD
        t = param(t, lo, hi, False)
        if self.type == "line":
            l = [[t, 0.0, 0.0], [1.0, 0.0, 0.0], Z3]
        else:
            s, c = sin_cos(t)
            if self.type == "circle":
                rc, rs = self.r * c, self.r * s
                l = [[rc, rs, 0.0], [-rs, rc, 0.0], [-rc, -rs, 0.0]]
            else:
                ac, as_, bc, bs = self.a * c, self.a * s, self.b * c, self.b * s
                l = [[ac, bs, 0.0], [-as_, bc, 0.0], [-ac, -bs, 0.0]]
        p, d1, d2 = self.pl.world(l[0]), self.pl.lin(l[1]), self.pl.lin(l[2])
        if not (fin3(p) and fin3(d1) and fin3(d2)):
            raise EvalErr("NonFinite")
        if order >= 1:
            curve_regular(d1)
        return t, p, d1, d2

    def surf_eval(self, u, v, order=2):
        T = self.type
        doms = {"plane": (ALL, ALL), "cylinder": (PERIOD, ALL), "cone": (PERIOD, ALL),
                "sphere": (PERIOD, LAT), "torus": (PERIOD, PERIOD)}[T]
        u = param(u, doms[0][0], doms[0][1], False)
        v = param(v, doms[1][0], doms[1][1], T == "sphere")
        if T == "plane":
            l = [[u, v, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], Z3, Z3, Z3]
        elif T == "cylinder":
            s, c = sin_cos(u)
            rc, rs = self.r * c, self.r * s
            l = [[rc, rs, v], [-rs, rc, 0.0], [0.0, 0.0, 1.0], [-rc, -rs, 0.0], Z3, Z3]
        elif T == "cone":
            s, c = sin_cos(u)
            rho = self.r0 + v * self.ta
            pc, ps = rho * c, rho * s
            tc, ts = self.ta * c, self.ta * s
            l = [[pc, ps, v], [-ps, pc, 0.0], [tc, ts, 1.0], [-pc, -ps, 0.0], [-ts, tc, 0.0], Z3]
        elif T == "sphere":
            su, cu = sin_cos(u)
            sv, cv = sin_cos(v)
            rcv, rsv = self.r * cv, self.r * sv
            a, b = rcv * cu, rcv * su
            c, d = rsv * cu, rsv * su
            l = [[a, b, rsv], [-b, a, 0.0], [-c, -d, rcv], [-a, -b, 0.0], [d, -c, 0.0], [-a, -b, -rsv]]
        else:
            su, cu = sin_cos(u)
            sv, cv = sin_cos(v)
            rcv, rsv = self.rr * cv, self.rr * sv
            rho = self.R + rcv
            a, b = rho * cu, rho * su
            c, d = rsv * cu, rsv * su
            e, f = rcv * cu, rcv * su
            l = [[a, b, rsv], [-b, a, 0.0], [-c, -d, rcv], [-a, -b, 0.0], [d, -c, 0.0], [-e, -f, -rsv]]
        p = self.pl.world(l[0])
        w = [self.pl.lin(l[k + 1]) for k in range(5)]
        if not (fin3(p) and all(fin3(x) for x in w)):
            raise EvalErr("NonFinite")
        if order >= 1:
            surface_regular(w[0], w[1])
        S = {(0, 0): p, (1, 0): w[0], (0, 1): w[1], (2, 0): w[2], (1, 1): w[3], (0, 2): w[4]}
        return (u, v), S

    # -- inversion
    def polar(self, q):
        x, y, z = self.pl.local(q)
        rho = hyp(x, y)
        dn = hyp(rho, z)
        return x, y, z, rho, dn, rho <= AXIS_REL * dn

    def foot_c(self, q, t, nu):
        try:
            p = self.curve_eval(t, 0)[1]
        except EvalErr as e:
            raise Err("NotANumber" if e.code == "NotANumber" else "NonFinite")
        return [t], p, dist(p, q), nu

    def foot_s(self, q, u, v, nu):
        try:
            p = self.surf_eval(u, v, 0)[1][(0, 0)]
        except EvalErr as e:
            raise Err("NotANumber" if e.code == "NotANumber" else "NonFinite")
        return [u, v], p, dist(p, q), nu

    def invert(self, q):
        T = self.type
        if T == "line":
            l = self.pl.local(q)
            return self.foot_c(q, l[0], False)
        if T == "plane":
            l = self.pl.local(q)
            return self.foot_s(q, l[0], l[1], False)
        x, y, z, rho, dn, on_axis = self.polar(q)
        az = 0.0 if on_axis else wrap(atan2(y, x))
        if T == "circle":
            return self.foot_c(q, az, on_axis)
        if T == "ellipse":
            return self.ellipse_invert(q, x, y, dn, on_axis, az)
        if T == "cylinder":
            return self.foot_s(q, az, z, on_axis)
        if T == "sphere":
            if dn == 0.0:
                return self.foot_s(q, 0.0, 0.0, True)
            v = atan2(z, rho)
            v = 0.0 if v == 0.0 else v
            return self.foot_s(q, az, v, False)
        if T == "cone":
            sa, ca, r0 = self.sa, self.ca, self.r0
            g = z * sa + r0 * ca
            if not isfin(g):
                raise Err("NonFinite")
            tie = abs(g) <= AXIS_REL * (dn + r0)
            if on_axis:
                u, rh, nu = 0.0, rho, not tie
            elif g > 0.0 or tie:
                u, rh, nu = wrap(atan2(y, x)), rho, tie
            else:
                u, rh, nu = wrap(atan2(-y, -x)), -rho, False
            s = (rh - r0) * sa + z * ca
            return self.foot_s(q, u, s * ca, nu)
        # torus
        mx = rho - self.R
        m = hyp(mx, z)
        centre = m <= AXIS_REL * (self.R + dn)
        v = 0.0 if centre else wrap(atan2(z, mx))
        return self.foot_s(q, az, v, on_axis or centre)

    def ellipse_invert(self, q, x, y, dn, on_axis, az):
        a, b = self.a, self.b
        if a == b:
            return self.foot_c(q, az, on_axis)
        swap = b > a
        e0, e1, y0, y1 = (b, a, y, x) if swap else (a, b, x, y)
        ax0, ax1 = abs(y0), abs(y1)
        tol = AXIS_REL * dn
        if ax1 <= tol:
            f0, f1, tie = on_major_axis(e0, e1, ax0)
        elif ax0 == 0.0:
            f0, f1, tie = 0.0, e1, False
        else:
            z0, z1 = ax0 / e0, ax1 / e1
            if z1 == 0.0:
                f0, f1, tie = on_major_axis(e0, e1, ax0)
            elif z0 == 0.0:
                f0, f1, tie = 0.0, e1, False
            else:
                f0, f1 = ellipse_quadrant(e0, e1, ax0, ax1, z0, z1)
                tie = False
        s0 = -f0 if y0 < 0.0 else f0
        s1 = -f1 if y1 < 0.0 else f1

        def t_of(m0, m1):
            lx, ly = (m1, m0) if swap else (m0, m1)
            return wrap(atan2(ly / b, lx / a))

        t = t_of(s0, s1)
        if tie:
            t2 = t_of(s0, -s1)
            if t2 < t:
                t = t2
        return self.foot_c(q, t, tie)


def on_major_axis(e0, e1, ax0):
    numer0 = e0 * ax0
    denom0 = (e0 - e1) * (e0 + e1)
    if not (isfin(numer0) and isfin(denom0)):
        raise Err("NonFinite")
    if numer0 < denom0:
        xde0 = numer0 / denom0
        f0 = e0 * xde0
        f1 = e1 * sqrt(1.0 - xde0 * xde0)
        return f0, f1, f1 != 0.0
    return e0, 0.0, False


def ellipse_quadrant(e0, e1, y0, y1, z0, z1):
    g = (z0 * z0 + z1 * z1) - 1.0
    if not isfin(g):
        raise Err("NonFinite")
    if g == 0.0:
        return y0, y1
    q = e0 / e1
    r0 = q * q
    rm1 = (q - 1.0) * (q + 1.0)
    n0 = r0 * z0
    w0 = z1
    w1 = 1.0 if g < 0.0 else hyp(n0, z1)
    if not (isfin(r0) and isfin(rm1) and isfin(n0)):
        raise Err("NonFinite")
    w = w0
    for _ in range(MAX_BISECT):
        w = 0.5 * w0 + 0.5 * w1
        if w == w0 or w == w1:
            break
        ratio0 = n0 / (w + rm1)
        ratio1 = z1 / w
        f = (ratio0 * ratio0 + ratio1 * ratio1) - 1.0
        if not isfin(f):
            raise Err("NonFinite")
        if f > 0.0:
            w0 = w
        elif f < 0.0:
            w1 = w
        else:
            break
    f0 = r0 * y0 / (w + rm1)
    f1 = y1 / w
    if not (isfin(f0) and isfin(f1)):
        raise Err("NonFinite")
    return f0, f1


def dist(foot, q):
    e = [foot[0] - q[0], foot[1] - q[1], foot[2] - q[2]]
    if not fin3(e):
        raise Err("NonFinite")
    return hyp(hyp(e[0], e[1]), e[2])


# ---------------------------------------------------------------- digest lines

def vx(v):
    return "-" if v is None else " ".join(hx(x) for x in v)


def eval_line(m, a, b=None):
    if m.curve:
        t, p, d1, d2 = m.curve_eval(a, 2)
        reg, tan, k = EV.curve_geometry(d1, d2)
        if reg and not (fin3(tan) and isfin(k)):
            raise EvalErr("NonFinite")
        return "c %s %s %s %s %s %s %s" % (hx(t), vx(p), vx(d1), vx(d2), "R" if reg else "D", vx(tan),
                                             "-" if k is None else hx(k))
    (u, v), S = m.surf_eval(a, b, 2)
    g = EV.surface_geometry(S)
    head = "s %s %s %s %s %s %s %s %s" % (hx(u), hx(v), vx(S[(0, 0)]), vx(S[(1, 0)]), vx(S[(0, 1)]),
                                           vx(S[(2, 0)]), vx(S[(1, 1)]), vx(S[(0, 2)]))
    if g is None:
        return head + " D -"
    n, k1, k2, K, H, d1, d2 = g
    if not (fin3(n) and all(isfin(v) for v in (k1, k2, K, H)) and (d1 is None or fin3(d1) and fin3(d2))):
        raise EvalErr("NonFinite")
    return head + " R %s %s %s %s %s %s %s" % (vx(n), hx(k1), hx(k2), hx(K), hx(H), vx(d1), vx(d2))


def invert_line(m, q):
    prm, foot, d, nu = m.invert(q)
    return "i %s %s %s %s" % (" ".join(hx(x) for x in prm), vx(foot), hx(d), "N" if nu else "U")


def entity_model(e):
    pl = e["placement"]
    prm = {k: jf(v) for k, v in e["params"].items()}
    return Model(e["type"], jf(pl["origin"]), jf(pl["axis"]), jf(pl["ref_dir"]), prm)


def entity_text(e, m):
    lines = []
    for r in e["eval"]:
        lines.append(eval_line(m, jf(r["t"])) if m.curve else eval_line(m, jf(r["u"]), jf(r["v"])))
    for r in e["invert"]:
        lines.append(invert_line(m, jf(r["q"])))
    return "".join(x + "\n" for x in lines)


# ---------------------------------------------------------------- part 2: accuracy vs REF oracle

def inf_err(a, b):
    return max(abs(x - y) for x, y in zip(a, b))


def ang_err(a, b):
    d = abs(a - b) % TAU
    return min(d, TAU - d)


def check_entity(e, m):
    worst = 0.0
    for r in e["eval"]:
        if m.curve:
            t, p, d1, d2 = m.curve_eval(jf(r["t"]), 2)
            assert inf_err(p, jf(r["p"])) <= 1e-13, (e["id"], "p")
            for got, key in ((d1, "d1"), (d2, "d2")):
                want = jf(r[key])
                assert inf_err(got, want) <= 1e-12 * max(1.0, max(abs(x) for x in want)), (e["id"], key)
            reg, tan, k = EV.curve_geometry(d1, d2)
            if r["curvature"] is None:
                assert not reg
            else:
                kw = jf(r["curvature"])
                assert abs(k - kw) <= 1e-9 * max(1.0, abs(kw)), (e["id"], "curvature", k, kw)
            worst = max(worst, inf_err(p, jf(r["p"])))
        else:
            (u, v), S = m.surf_eval(jf(r["u"]), jf(r["v"]), 2)
            assert inf_err(S[(0, 0)], jf(r["p"])) <= 1e-13, (e["id"], "p")
            worst = max(worst, inf_err(S[(0, 0)], jf(r["p"])))
            for key, kk in (("su", (1, 0)), ("sv", (0, 1)), ("suu", (2, 0)), ("suv", (1, 1)), ("svv", (0, 2))):
                want = jf(r[key])
                assert inf_err(S[kk], want) <= 1e-12 * max(1.0, max(abs(x) for x in want)), (e["id"], key)
            g = EV.surface_geometry(S)
            if r["regularity"] == "degenerate":
                assert g is None, e["id"]
                continue
            assert g is not None, e["id"]
            n, k1, k2, K, H, _, _ = g
            assert inf_err(n, jf(r["normal"])) <= 1e-12, (e["id"], "normal")
            ks = max(1.0, abs(jf(r["k1"])), abs(jf(r["k2"])))
            for got, key in ((k1, "k1"), (k2, "k2"), (H, "mean")):
                assert abs(got - jf(r[key])) <= 1e-9 * ks, (e["id"], key, got, jf(r[key]))
            assert abs(K - jf(r["gaussian"])) <= 1e-9 * ks * ks, (e["id"], "gaussian")
    for r in e["invert"]:
        prm, foot, d, nu = m.invert(jf(r["q"]))
        assert nu == r["non_unique"], (r["id"], nu)
        assert inf_err(foot, jf(r["foot"])) <= 1e-13, (r["id"], "foot", inf_err(foot, jf(r["foot"])))
        assert abs(d - jf(r["distance"])) <= 1e-13, (r["id"], "distance")
        want = jf(r["params"])
        periodic = {"line": [False], "circle": [True], "ellipse": [True], "plane": [False, False],
                    "cylinder": [True, False], "cone": [True, False], "sphere": [True, False],
                    "torus": [True, True]}[e["type"]]
        for g, w, per in zip(prm, want, periodic):
            err = ang_err(g, w) if per else abs(g - w)
            assert err <= 1e-12 * max(1.0, abs(w)), (r["id"], "params", prm, want)
        worst = max(worst, inf_err(foot, jf(r["foot"])))
    return worst


# ---------------------------------------------------------------- part 3: fuzz set

def fr(x):
    return Fraction(x)


def frac_dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def frac_cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dec(x):
    """Fraction or float -> Decimal at the current precision."""
    if isinstance(x, Fraction):
        return D(x.numerator) / D(x.denominator)
    return D(x)


def dnorm(v):
    return sum(c * c for c in v).sqrt()


class Exact:
    """Exact / 60-digit reference for one entity (independent of the binary64 model)."""

    def __init__(self, typ, o, z, x, prm):
        self.type, self.prm = typ, prm
        self.O, self.Z, self.X = [fr(c) for c in o], [fr(c) for c in z], [fr(c) for c in x]
        self.Y = frac_cross(self.Z, self.X)
        self.ref = OR.Analytic("fz", typ, o, z, x, dict(prm), False)

    def local(self, q):
        d = [fr(q[i]) - self.O[i] for i in range(3)]
        return frac_dot(d, self.X), frac_dot(d, self.Y), frac_dot(d, self.Z)

    def world(self, a, b, c):
        a, b, c = dec(a), dec(b), dec(c)
        return [dec(self.O[i]) + a * dec(self.X[i]) + b * dec(self.Y[i]) + c * dec(self.Z[i]) for i in range(3)]

    def invert(self, q):
        """(foot as Decimals, distance, non_unique) of the true closest point of the f64 query."""
        T = self.type
        if T == "cone":
            return self.cone_invert(q)
        if T == "ellipse":
            return self.ellipse_invert(q)
        prm, foot, d, nu = self.ref.invert(q)
        return foot, d, nu

    def ellipse_invert(self, q):
        """Global minimum of |E(t) - q| by a 4096-point sign scan of the derivative (binary64,
        normalised by a) and 200 decimal bisection steps per bracket (independent of Eberly's
        root function used by the binary64 code)."""
        x, y, z = (dec(c) for c in self.local(q))
        a, b = D(self.prm["semi_axis_1"]), D(self.prm["semi_axis_2"])
        X, Y, B = x / a, y / a, b / a
        fX, fY, fB = float(X), float(Y), float(B)

        def gf(t):
            c, s = FN.m_cos(t), FN.m_sin(t)
            return -s * (c - fX) + fB * c * (fB * s - fY)

        def gd(t):
            s, c = OR.sin_cos(t)
            return -s * (c - X) + B * c * (B * s - Y)

        def fd(t):
            s, c = OR.sin_cos(t)
            return (c - X) ** 2 + (B * s - Y) ** 2

        n = 4096
        ts = [2 * math.pi * k / n for k in range(n + 1)]
        gs = [gf(t) for t in ts]
        roots = []
        for k in range(n):
            if gs[k] < 0 <= gs[k + 1]:
                lo, hi = D(ts[k]), (OR.TWO_PI if k == n - 1 else D(ts[k + 1]))
                if gd(lo) >= 0:
                    roots.append(lo)
                    continue
                for _ in range(200):
                    mid = (lo + hi) / 2
                    if gd(mid) < 0:
                        lo = mid
                    else:
                        hi = mid
                roots.append((lo + hi) / 2)
        if gs[0] == 0:
            roots.append(D(0))
        vals = sorted((fd(t), t) for t in roots)
        f0 = vals[0][0]
        tied = [t for f, t in vals if f - f0 <= D("1e-40") * (1 + f0)]
        t0 = min(tied)
        s, c = OR.sin_cos(t0)
        foot = self.world(a * c, b * s, 0)
        dd = dnorm([foot[i] - dec(fr(q[i])) for i in range(3)])
        distinct = [t for t in tied if abs(t - t0) > D("1e-30")]
        return foot, dd, len(distinct) > 0

    def cone_invert(self, q):
        x, y, z = (dec(c) for c in self.local(q))
        r0 = D(self.prm["radius"])
        sa, ca = OR.sin_cos(D(self.prm["semi_angle"]))
        rho = (x * x + y * y).sqrt()
        cands = []
        for side in (1, -1):
            rr = side * rho
            s = (rr - r0) * sa + z * ca
            frho, fz = r0 + s * sa, s * ca     # signed radius along side * (x, y) / rho
            if rho == 0:
                ux, uy = D(1), D(0)
                if side < 0:
                    continue
            else:
                ux, uy = side * x / rho, side * y / rho
            foot = self.world(frho * ux, frho * uy, fz)
            dd = dnorm([foot[i] - dec(fr(q[i])) for i in range(3)])
            cands.append((dd, foot))
        cands.sort(key=lambda c: c[0])
        d0, foot = cands[0]
        nu = rho == 0 or (len(cands) > 1 and cands[1][0] - d0 <= D("1e-40") * (d0 + rho + abs(z) + r0))
        return foot, d0, nu


def scale_of(o, q, sizes):
    return max([abs(c) for c in o] + [abs(c) for c in q] + sizes)


def make_frame(rng):
    """Random orthonormal frame from rng.random() only (stable across Python versions and
    platforms): Gram-Schmidt at 60 digits, rounded once to binary64."""
    while True:
        zv = [D(2 * rng.random() - 1) for _ in range(3)]
        av = [D(2 * rng.random() - 1) for _ in range(3)]
        nz = dnorm(zv)
        if nz < D("0.2") or nz > 1:
            continue
        zv = [c / nz for c in zv]
        pa = sum(a * b for a, b in zip(av, zv))
        av = [a - pa * b for a, b in zip(av, zv)]
        na = dnorm(av)
        if na < D("0.2"):
            continue
        return [float(c) for c in zv], [float(c / na) for c in av]


def fh(*xs):
    """sqrt(sum x^2) of binary64 values, at 60 digits, rounded once (no platform libm)."""
    return float(sum(D(x) * D(x) for x in xs).sqrt())


def ellipse_factor(ex, q, prm):
    """|1 - k d| (inside) or 1 + k d (outside) at the exact foot: the planar conditioning."""
    x, y, z = (float(dec(c)) for c in ex.local(q))
    a, b = prm["semi_axis_1"], prm["semi_axis_2"]
    foot, d, _ = ex.invert(q)
    lx = [dec(foot[i]) - dec(ex.O[i]) for i in range(3)]
    cx = float(sum(lx[i] * dec(ex.X[i]) for i in range(3))) / a
    cy = float(sum(lx[i] * dec(ex.Y[i]) for i in range(3))) / b
    sp = fh(a * cy, b * cx)
    kappa = (a / sp) * (b / sp) / sp
    dpl = fh(x - a * cx, y - b * cy)
    inside = (x / a) * (x / a) + (y / b) * (y / b) < 1
    return abs(1 - kappa * dpl) if inside else 1 + kappa * dpl


def foot_extra(typ, ex, q, prm, foot):
    """Foot error from the rounding of the local coordinates (absolute bound
    dl = 4 eps dn + 4 x 2^-1074), propagated through the direction that fixes the foot; a
    singular set the query lies on exactly contributes nothing (both sides take the canonical
    choice)."""
    x, y, z = (float(dec(c)) for c in ex.local(q))
    rho = fh(x, y)
    dn = fh(rho, z)
    dl = 4 * EPS * dn + 4 * 5e-324
    if typ in ("line", "plane"):
        return dl
    lf = [dec(foot[i]) - dec(ex.O[i]) for i in range(3)]
    fx_ = float(sum(lf[i] * dec(ex.X[i]) for i in range(3)))
    fy_ = float(sum(lf[i] * dec(ex.Y[i]) for i in range(3)))
    frad = fh(fx_, fy_)
    if typ == "sphere":
        return prm["radius"] * dl / dn if dn > 0 else 0.0
    if typ == "ellipse":
        return dl / max(ellipse_factor(ex, q, prm), 1e-300)
    az = frad * dl / rho if rho > 0 else 0.0
    if typ == "torus":
        m = fh(rho - prm["major_radius"], z)
        return az + (prm["minor_radius"] * dl / m if m > 0 else 0.0)
    return az + dl


def model_outcome(fn):
    try:
        return "ok", fn()
    except Err as e:
        return "err:" + e.code, None
    except EvalErr as e:
        return "err:" + e.code, None


class Fuzz:
    def __init__(self):
        self.cases = []

    def add(self, case):
        case["id"] = "fz%04d" % len(self.cases)
        self.cases.append(case)


def enc_place(o, z, x):
    return {"origin": [h0x(c) for c in o], "axis": [h0x(c) for c in z], "ref_dir": [h0x(c) for c in x]}


def enc_params(prm):
    return {k: h0x(v) for k, v in prm.items()}


PARAM_NAMES = {"line": [], "plane": [], "circle": ["radius"], "cylinder": ["radius"], "sphere": ["radius"],
               "ellipse": ["semi_axis_1", "semi_axis_2"], "cone": ["radius", "semi_angle"],
               "torus": ["major_radius", "minor_radius"]}
TYPES = ["line", "circle", "ellipse", "plane", "cylinder", "cone", "sphere", "torus"]
ALIGNED = ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0])


def rand_params(typ, rng, k):
    s = k * (0.3 + 0.6 * rng.random())
    if typ in ("circle", "cylinder", "sphere"):
        return {"radius": s}
    if typ == "ellipse":
        return {"semi_axis_1": s, "semi_axis_2": s * (0.2 + 0.7 * rng.random())}
    if typ == "cone":
        return {"radius": s, "semi_angle": 0.15 + 1.2 * rng.random()}
    if typ == "torus":
        return {"major_radius": s, "minor_radius": s * (0.15 + 0.6 * rng.random())}
    return {}


def add_construct(fz, typ, o, z, x, prm, expect, fault=None):
    fz.add({"kind": "construct", "type": typ, "placement": enc_place(o, z, x), "params": enc_params(prm),
            "expect": expect, "fault": fault})


def add_invert(fz, typ, o, z, x, prm, q, expect_nu=None, near_singular=False, canonical=None,
               accept_nonfinite=False, expect=None):
    """An inversion case with its exact reference. expect_nu: designed uniqueness (else exact)."""
    if expect is not None:
        fz.add({"kind": "invert", "type": typ, "placement": enc_place(o, z, x), "params": enc_params(prm),
                "q": [h0x(c) for c in q], "expect": expect})
        return
    ex = Exact(typ, o, z, x, prm)
    foot, d, nu = ex.invert(q)
    if expect_nu is not None:
        nu = expect_nu
    sizes = [v for kk, v in prm.items() if kk != "semi_angle"]
    S = scale_of(o, q, sizes)
    tol_d = 64 * EPS * S + 1e-300
    if near_singular:
        lx, ly, lz = (float(dec(c)) for c in ex.local(q))
        dn = fh(fh(lx, ly), lz)
        tol_d += 2 * AXIS_REL * (dn + max(sizes + [0.0]))
        tol_f = None
    else:
        tol_f = tol_d + 16 * foot_extra(typ, ex, q, prm, foot)
    fz.add({"kind": "invert", "type": typ, "placement": enc_place(o, z, x), "params": enc_params(prm),
            "q": [h0x(c) for c in q], "expect": "ok", "uniqueness": "N" if nu else "U",
            "distance": h0x(float(d)), "foot": [h0x(float(c)) for c in foot] if tol_f is not None else None,
            "tol_d": h0x(tol_d), "tol_f": h0x(tol_f) if tol_f is not None else None,
            "canonical": [h0x(c) for c in canonical] if canonical is not None else None,
            "accept_nonfinite": accept_nonfinite})


def add_eval(fz, typ, o, z, x, prm, args, expect="ok"):
    case = {"kind": "eval", "type": typ, "placement": enc_place(o, z, x), "params": enc_params(prm),
            "args": [h0x(a) for a in args], "expect": expect}
    if expect == "ok":
        p = exact_position(typ, o, z, x, prm, args)
        S = scale_of(o, [0.0, 0.0, 0.0], [abs(float(c)) for c in p] + [abs(a) for a in args if isfin(a)] +
                     [v for k, v in prm.items() if k != "semi_angle"])
        case["p"] = [h0x(float(c)) for c in p]
        case["tol"] = h0x(64 * EPS * S + 1e-300)
    fz.add(case)


def exact_position(typ, o, z, x, prm, args):
    """Position at 60+ digits; sin/cos of possibly huge angles by the 700-digit reduction."""
    ex = Exact(typ, o, z, x, prm)

    def sc(a):
        with localcontext() as c:
            c.prec = 60
            return +FN.d_trig(a, "sin"), +FN.d_trig(a, "cos")

    P = {k: D(v) for k, v in prm.items()}
    if typ == "line":
        return ex.world(fr(args[0]), 0, 0)
    if typ == "plane":
        return ex.world(fr(args[0]), fr(args[1]), 0)
    if typ in ("circle", "ellipse"):
        s, c = sc(args[0])
        a, b = (P["radius"], P["radius"]) if typ == "circle" else (P["semi_axis_1"], P["semi_axis_2"])
        return ex.world(a * c, b * s, 0)
    su, cu = sc(args[0])
    v = D(args[1])
    if typ == "cylinder":
        return ex.world(P["radius"] * cu, P["radius"] * su, v)
    if typ == "cone":
        s, c = sc(prm["semi_angle"])
        rho = P["radius"] + v * s / c
        return ex.world(rho * cu, rho * su, v)
    sv, cv = sc(args[1])
    if typ == "sphere":
        r = P["radius"]
        return ex.world(r * cv * cu, r * cv * su, r * sv)
    rho = P["major_radius"] + P["minor_radius"] * cv
    return ex.world(rho * cu, rho * su, P["minor_radius"] * sv)


def nextdown(x):
    return math.nextafter(x, -math.inf)


def build_fuzz(seed):
    rng = random.Random(seed)
    fz = Fuzz()
    O0 = [0.0, 0.0, 0.0]
    Z, X = ALIGNED
    unit = {"line": {}, "plane": {}, "circle": {"radius": 1.0}, "cylinder": {"radius": 1.0},
            "sphere": {"radius": 1.0}, "ellipse": {"semi_axis_1": 2.0, "semi_axis_2": 1.0},
            "cone": {"radius": 1.0, "semi_angle": 0.5}, "torus": {"major_radius": 2.0, "minor_radius": 0.5}}

    # -- A. construction (decided exactly; margins >= 100x the binary64 rounding of the tests)
    for typ in TYPES:
        add_construct(fz, typ, O0, Z, X, unit[typ], "ok")
        for bad, code in (([math.nan, 0.0, 0.0], "NotANumber"), ([math.inf, 0.0, 0.0], "NonFinite")):
            add_construct(fz, typ, bad, Z, X, unit[typ], "err:" + code)
    for d, code, fault in ((5e-13, "ok", None), (-5e-13, "ok", None), (2e-12, "err:BadPlacement", "AxisNotUnit"),
                           (-2e-12, "err:BadPlacement", "AxisNotUnit")):
        add_construct(fz, "cylinder", O0, [0.0, 0.0, 1.0 + d], X, {"radius": 1.0}, code, fault)
    for d, code, fault in ((5e-13, "ok", None), (2e-12, "err:BadPlacement", "RefDirNotUnit")):
        add_construct(fz, "sphere", O0, Z, [0.6 * (1 + d), 0.8 * (1 + d), 0.0], {"radius": 1.0}, code, fault)
    for d, code, fault in ((5e-13, "ok", None), (3e-12, "err:BadPlacement", "NotOrthogonal")):
        add_construct(fz, "plane", O0, Z, [1.0, 0.0, d], {}, code, fault)
    add_construct(fz, "line", O0, [0.0, 0.0, 1e200], X, {}, "err:BadPlacement", "AxisNotUnit")
    add_construct(fz, "line", O0, [0.0, 0.0, 0.0], X, {}, "err:BadPlacement", "AxisNotUnit")
    add_construct(fz, "line", O0, Z, [math.nan, 0.0, 0.0], {}, "err:NotANumber")
    for r, code in ((0.0, "err:BadRadius"), (-1.0, "err:BadRadius"), (1e-101, "err:BadRadius"),
                    (1e-100, "ok"), (1e100, "ok"), (1.0000001e100, "err:BadRadius"),
                    (sys.float_info.min, "err:BadRadius"), (5e-324, "err:BadRadius"),
                    (math.nan, "err:NotANumber"), (math.inf, "err:NonFinite"), (-math.inf, "err:NonFinite")):
        for typ in ("circle", "cylinder", "sphere"):
            add_construct(fz, typ, O0, Z, X, {"radius": r}, code)
    add_construct(fz, "ellipse", O0, Z, X, {"semi_axis_1": 1.0, "semi_axis_2": 0.0}, "err:BadRadius")
    add_construct(fz, "ellipse", O0, Z, X, {"semi_axis_1": 1e101, "semi_axis_2": 1.0}, "err:BadRadius")
    add_construct(fz, "ellipse", O0, Z, X, {"semi_axis_1": 1e-100, "semi_axis_2": 1e100}, "ok")
    for r, ang, code in ((1.0, 0.0, "err:BadAngle"), (1.0, -0.1, "err:BadAngle"), (1.0, FRAC_PI_2, "err:BadAngle"),
                         (1.0, nextdown(FRAC_PI_2), "ok"), (1.0, 1.6, "err:BadAngle"), (1.0, 1e-300, "ok"),
                         (1.0, 5e-324, "ok"), (1.0, math.nan, "err:NotANumber"), (1.0, math.inf, "err:NonFinite"),
                         (0.0, 0.5, "ok"), (-0.0, 0.5, "ok"), (1e-200, 0.5, "err:BadRadius"),
                         (-1.0, 0.5, "err:BadRadius"), (math.nan, 0.5, "err:NotANumber")):
        add_construct(fz, "cone", O0, Z, X, {"radius": r, "semi_angle": ang}, code)
    for R, r, code in ((1.0, 1.0, "err:BadRadius"), (1.0, 2.0, "err:BadRadius"), (1.0, nextdown(1.0), "ok"),
                       (1e100, 1e-100, "ok"), (1.0, 0.0, "err:BadRadius"), (math.nan, 0.5, "err:NotANumber")):
        add_construct(fz, "torus", O0, Z, X, {"major_radius": R, "minor_radius": r}, code)

    # -- B. evaluation extremes
    for typ, args, expect in (
            ("circle", [1e300], "ok"), ("circle", [1e22], "ok"), ("circle", [-1e15], "ok"),
            ("circle", [math.nan], "err:NotANumber"), ("circle", [math.inf], "err:OutsideDomain"),
            ("line", [1e300], "ok"), ("line", [-math.inf], "err:OutsideDomain"),
            ("ellipse", [7.0e200], "ok"), ("torus", [1e300, -1e300], "ok"),
            ("sphere", [1e10, FRAC_PI_2], "ok"), ("sphere", [0.5, -FRAC_PI_2], "ok"),
            ("sphere", [0.5, math.nextafter(FRAC_PI_2, 2.0)], "err:OutsideDomain"),
            ("sphere", [0.5, -1.6], "err:OutsideDomain"), ("sphere", [math.nan, 0.0], "err:NotANumber"),
            ("cylinder", [0.25, 1e300], "ok"), ("cylinder", [0.25, -1e300], "ok"),
            ("cone", [3.0, 1e300], "ok"), ("cone", [3.0, -2.0], "ok"),
            ("plane", [1e308, 1e308], "ok"), ("plane", [0.0, math.inf], "err:OutsideDomain")):
        add_eval(fz, typ, O0, Z, X, unit[typ], args, expect)
    # overflow: plane with a diagonal frame at u = v = 1.5e308 overflows u X + v Y
    s2 = math.sqrt(0.5)
    add_eval(fz, "plane", O0, Z, [s2, s2, 0.0], {}, [1.5e308, 1.5e308], "err:NonFinite")
    add_eval(fz, "cylinder", [0.0, 0.0, 1.7e308], Z, X, {"radius": 1e100}, [0.0, 1e308], "err:NonFinite")

    # -- C. random inversions across scales, with far-away origins
    for k in (1e-90, 1e-30, 1e-8, 1.0, 1e8, 1e30, 1e90):
        for typ in TYPES:
            for rep in range(6):
                z, x = make_frame(rng)
                prm = rand_params(typ, rng, k)
                off = (1e6 if rep == 5 else 1.0) * k
                o = [off * (rng.random() - 0.5) for _ in range(3)]
                for _ in range(100):
                    q = [o[i] + 2.0 * k * (rng.random() - 0.5) for i in range(3)]
                    if typ == "cone" and not cone_ok(o, z, x, prm, q):
                        continue
                    if typ == "ellipse" and ellipse_factor(Exact(typ, o, z, x, prm), q, prm) < 1e-3:
                        continue
                    break
                add_invert(fz, typ, o, z, x, prm, q)
        # far query: 1e6 sizes away
        for typ in TYPES:
            z, x = make_frame(rng)
            prm = rand_params(typ, rng, k)
            o = [k * (rng.random() - 0.5) for _ in range(3)]
            q = [o[i] + 1e6 * k * (rng.random() - 0.5) for i in range(3)]
            if typ == "cone" and not cone_ok(o, z, x, prm, q):
                continue
            add_invert(fz, typ, o, z, x, prm, q, accept_nonfinite=1e6 * k > 1e250)

    # -- D. singular and near-singular queries (aligned frames: local coordinates exact)
    for k in (1e-80, 1.0, 1e80):
        o = [0.5 * k, -0.25 * k, 0.125 * k]
        P = {t: {n: v * k if n != "semi_angle" else v for n, v in unit[t].items()} for t in TYPES}

        def at(lx, ly, lz):
            return [o[0] + lx * k, o[1] + ly * k, o[2] + lz * k]

        # exactly on the axis
        add_invert(fz, "circle", o, Z, X, P["circle"], at(0, 0, 0.5), expect_nu=True, canonical=[0.0])
        add_invert(fz, "cylinder", o, Z, X, P["cylinder"], at(0, 0, -0.75), expect_nu=True, canonical=[0.0, -0.75 * k])
        add_invert(fz, "torus", o, Z, X, P["torus"], at(0, 0, 0.25), expect_nu=True)
        add_invert(fz, "sphere", o, Z, X, P["sphere"], at(0, 0, 0.5), expect_nu=False,
                   canonical=[0.0, FRAC_PI_2])
        add_invert(fz, "sphere", o, Z, X, P["sphere"], at(0, 0, -3.0), expect_nu=False,
                   canonical=[0.0, -FRAC_PI_2])
        add_invert(fz, "sphere", o, Z, X, P["sphere"], o, expect_nu=True, canonical=[0.0, 0.0])
        add_invert(fz, "cone", o, Z, X, P["cone"], at(0, 0, 0.5), expect_nu=True)
        add_invert(fz, "cone", o, Z, X, P["cone"], at(0, 0, -3.0), expect_nu=True)
        # near the axis: rho / dn = 1e-17 (flagged) and 1e-11 (resolved)
        for frac, nu in ((1e-17, True), (1e-11, False)):
            for typ in ("circle", "cylinder", "torus", "cone"):
                q = at(frac * 0.6, frac * 0.8, 1.0)
                add_invert(fz, typ, o, Z, X, P[typ], q, expect_nu=nu, near_singular=nu)
        # sphere very near the centre: unique and well-conditioned for the given f64 query
        add_invert(fz, "sphere", O0, Z, X, P["sphere"], [3e-300, -4e-300, 1e-300], expect_nu=False)
        # torus tube-circle centre (exact) and near it
        add_invert(fz, "torus", o, Z, X, P["torus"], at(2.0, 0.0, 0.0), expect_nu=True, canonical=[0.0, 0.0])
        add_invert(fz, "torus", o, Z, X, P["torus"], at(2.0 + 1e-17, 0.0, 1e-17), expect_nu=True,
                   near_singular=True)
        add_invert(fz, "torus", o, Z, X, P["torus"], at(2.0 + 1e-9, 0.0, 1e-9), expect_nu=False)
        # ellipse: centre, on the major axis inside / outside the evolute, on the minor axis
        add_invert(fz, "ellipse", o, Z, X, P["ellipse"], at(0, 0, 0.5), expect_nu=True, canonical=[FRAC_PI_2])
        add_invert(fz, "ellipse", o, Z, X, P["ellipse"], at(0.5, 0, 0.0), expect_nu=True)
        add_invert(fz, "ellipse", o, Z, X, P["ellipse"], at(-1.0, 0, 0.25), expect_nu=True)
        add_invert(fz, "ellipse", o, Z, X, P["ellipse"], at(2.5, 0, 0.0), expect_nu=False, canonical=[0.0])
        add_invert(fz, "ellipse", o, Z, X, P["ellipse"], at(0.0, 0.3, 0.0), expect_nu=False,
                   canonical=[FRAC_PI_2])
        add_invert(fz, "ellipse", o, Z, X, P["ellipse"], at(0.0, -3.0, 0.0), expect_nu=False)
        add_invert(fz, "ellipse", o, Z, X, P["ellipse"], at(0.5, 1e-9, 0.0), expect_nu=False)
        add_invert(fz, "ellipse", o, Z, X, P["ellipse"], at(0.5, 1e-17, 0.0), expect_nu=True, near_singular=True)
        # tall ellipse (b > a): centre gives t = 0
        tall = {"semi_axis_1": 1.0 * k, "semi_axis_2": 2.0 * k}
        add_invert(fz, "ellipse", o, Z, X, tall, at(0, 0, 0.0), expect_nu=True, canonical=[0.0])
        add_invert(fz, "ellipse", o, Z, X, tall, at(0, -0.5, 0.0), expect_nu=True)
        # cone with the apex at the origin: apex, the apex plane (tie), the other nappe
        apex = {"radius": 0.0, "semi_angle": 0.5}
        add_invert(fz, "cone", o, Z, X, apex, o, expect_nu=False, canonical=[0.0, 0.0])
        add_invert(fz, "cone", o, Z, X, apex, at(0.6, 0.8, 0.0), expect_nu=True)
        add_invert(fz, "cone", o, Z, X, apex, at(0.6, 0.8, -1.0), expect_nu=False)
        add_invert(fz, "cone", o, Z, X, P["cone"], at(0.6, -0.8, -5.0), expect_nu=False)
        add_invert(fz, "cone", o, Z, X, P["cone"], at(-0.3, 0.1, 2.0), expect_nu=False)

    # -- E. hostile queries
    for typ in TYPES:
        add_invert(fz, typ, O0, Z, X, unit[typ], [math.nan, 0.0, 0.0], expect="err:NotANumber")
        add_invert(fz, typ, O0, Z, X, unit[typ], [0.0, -math.inf, 0.0], expect="err:NonFinite")
        add_invert(fz, typ, [-1.5e308, 0.0, 0.0], Z, X, unit[typ], [1.5e308, 0.0, 0.0], expect="err:NonFinite")
    # subnormal offsets from the origin
    for typ in ("sphere", "cylinder", "circle", "torus", "plane", "line"):
        add_invert(fz, typ, O0, Z, X, unit[typ], [5e-324, 1e-320, 2e-310])
    add_invert(fz, "ellipse", O0, Z, X, {"semi_axis_1": 1e100, "semi_axis_2": 1e-100}, [1e99, 1e-101, 0.0],
               expect_nu=True, near_singular=True, accept_nonfinite=True)
    return fz


def cone_ok(o, z, x, prm, q):
    """Avoid random cone queries within 1e-6 of the apex-plane tie (a designed case covers it)."""
    ex = Exact("cone", o, z, x, prm)
    lx, ly, lz = (dec(c) for c in ex.local(q))
    sa, ca = OR.sin_cos(D(prm["semi_angle"]))
    g = lz * sa + D(prm["radius"]) * ca
    rho = (lx * lx + ly * ly).sqrt()
    dn = (rho * rho + lz * lz).sqrt()
    return abs(g) > D("1e-6") * (dn + D(prm["radius"])) and rho > D("1e-6") * dn


def fuzz_args(c):
    return [unhex(a[2:]) for a in c["args"]]


def fx(s):
    return unhex(s[2:])


def fuzz_model(c):
    pl = c["placement"]
    prm = {k: fx(v) for k, v in c["params"].items()}
    return Model(c["type"], [fx(v) for v in pl["origin"]], [fx(v) for v in pl["axis"]],
                 [fx(v) for v in pl["ref_dir"]], prm)


def fuzz_line(c):
    """Digest line of a fuzz case (the model's outcome), and the outcome for the self-check."""
    try:
        m = fuzz_model(c)
    except Err as e:
        if c["kind"] == "construct":
            return "k " + e.code, "err:" + e.code, e.detail
        return "x " + e.code, "err:" + e.code, e.detail
    if c["kind"] == "construct":
        return "k ok", "ok", None
    if c["kind"] == "eval":
        a = fuzz_args(c)
        try:
            line = "e " + (eval_line(m, a[0]) if m.curve else eval_line(m, a[0], a[1]))
        except EvalErr as e:
            line = "e " + e.code
        try:
            m.curve_eval(a[0], 0) if m.curve else m.surf_eval(a[0], a[1], 0)
        except EvalErr as e:
            return line, "err:" + e.code, None
        return line, "ok", (m, a)
    try:
        q = [fx(v) for v in c["q"]]
        line = invert_line(m, q)
        return line, "ok", m.invert(q)
    except Err as e:
        return "i " + e.code, "err:" + e.code, None


def check_fuzz_case(c, outcome, info):
    exp = c["expect"]
    if c["kind"] == "invert" and exp == "ok" and outcome == "err:NonFinite" and c.get("accept_nonfinite"):
        return
    assert outcome == exp, (c["id"], c["type"], outcome, exp)
    if c["kind"] == "construct" and exp == "err:BadPlacement":
        assert info == c["fault"], (c["id"], info, c["fault"])
    if exp != "ok":
        return
    if c["kind"] == "eval":
        m, a = info
        p = m.curve_eval(a[0], 0)[1] if m.curve else m.surf_eval(a[0], a[1], 0)[1][(0, 0)]
        want = [fx(v) for v in c["p"]]
        assert inf_err(p, want) <= fx(c["tol"]), (c["id"], p, want)
    elif c["kind"] == "invert":
        prm, foot, d, nu = info
        assert ("N" if nu else "U") == c["uniqueness"], (c["id"], c["type"], nu)
        assert abs(d - fx(c["distance"])) <= fx(c["tol_d"]), (c["id"], c["type"], d, fx(c["distance"]))
        if c["foot"] is not None:
            want = [fx(v) for v in c["foot"]]
            assert inf_err(foot, want) <= fx(c["tol_f"]), (c["id"], c["type"], inf_err(foot, want), fx(c["tol_f"]))
        if c["canonical"] is not None:
            want = [fx(v) for v in c["canonical"]]
            assert all(abs(g - w) <= 1e-15 * max(1.0, abs(w)) for g, w in zip(prm, want)), (c["id"], prm, want)


def render_fuzz(cases, seed):
    head = {"format": "fid-nurbs-analytic-fuzz/1", "generator": "fixtures/geo/nurbs/analytic/make_analytic_fixtures.py",
            "seed": seed, "license": "CC-BY-4.0", "unit": "m",
            "conventions": {"float": "every f64 as 0x followed by the 16 hex digits of its binary64 bit pattern",
                            "expect": "ok or err:<AnalyticError or EvalError code>",
                            "accept_nonfinite": "an ok case that may also be refused with NonFinite",
                            "tolerances": "tol_d on the distance, tol_f on the foot (inf norm, null: foot not "
                                          "compared), canonical: exact expected parameters"}}
    text = "{\n"
    for k, v in head.items():
        text += f" {json.dumps(k)}: {json.dumps(v, separators=(',', ':'))},\n"
    text += ' "cases": [\n'
    text += ",\n".join("  " + json.dumps(c, separators=(",", ":")) for c in cases)
    text += "\n ]\n}\n"
    return text


def main():
    check = "--check" in sys.argv[1:]
    seed = 44
    doc = json.loads(SET.read_text(encoding="utf-8"))
    lines = []
    total = hashlib.sha256()
    worst = 0.0
    for e in doc["entities"]:
        m = entity_model(e)
        worst = max(worst, check_entity(e, m))
        text = entity_text(e, m).encode("utf-8")
        total.update(text)
        lines.append("%s %s" % (e["id"], hashlib.sha256(text).hexdigest()))
    lines.append("all %s" % total.hexdigest())
    print(f"model vs GEO.NURBS.REF oracle: worst position/foot error {worst:.3e} m")

    fz = build_fuzz(seed)
    fuzz_text = []
    for c in fz.cases:
        line, outcome, info = fuzz_line(c)
        check_fuzz_case(c, outcome, info)
        fuzz_text.append(line + "\n")
    lines.append("fuzz %s" % hashlib.sha256("".join(fuzz_text).encode("utf-8")).hexdigest())
    print(f"fuzz set: {len(fz.cases)} cases, model passes all")
    body = ("# GEO.NURBS.ANALYTIC AC-4: SHA-256 of the pinned evaluation and inversion of analytic_set.json per\n"
            "# entity and in total, and of the fuzz set. Written by make_analytic_fixtures.py (CC-BY-4.0); do not edit.\n"
            + "".join(x + "\n" for x in lines)).encode("utf-8")
    fuzz_body = render_fuzz(fz.cases, seed).encode("utf-8")
    if check:
        stale = [p.name for p, b in ((OUT_DIGESTS, body), (OUT_FUZZ, fuzz_body)) if p.read_bytes() != b]
        if stale:
            raise SystemExit(f"stale: {stale}")
        print("digests-v1.txt and fuzz_set.json up to date")
    else:
        OUT_DIGESTS.write_bytes(body)
        OUT_FUZZ.write_bytes(fuzz_body)
        print(f"wrote {OUT_DIGESTS.name} and {OUT_FUZZ.name}")


if __name__ == "__main__":
    main()
