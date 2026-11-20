#!/usr/bin/env python3
"""Global-minimum certificate and audit for the GEO.NURBS.REF inversion references.

Independent of the reference search in `nurbs_oracle.py` (it shares no code with it and reads the
geometry from the JSON fixtures, exactly, from the IEEE hex fields). Python standard library only.
Output data of the oracle is CC-BY-4.0 (D-001); this script writes nothing.

Method (pinned in spec/geo/nurbs/ref/NODE.md, Design, "Global certificate"):

1. Bezier extraction: every NURBS curve or surface is split into rational Bezier pieces by exact
   knot insertion (Boehm) in `fractions.Fraction`, until every breakpoint of the domain
   [U[p], U[n]] has multiplicity p (in u and in v for surfaces). Homogeneous control points
   (w x, w y, w z, w) are kept exactly.
2. For a query q and a claimed distance d, let c = (d - delta)^2 with delta = 1e-15 m (the tie
   tolerance of REQ-GEO.NURBS.INVERT-004). On each piece the polynomial
       G = |A - w q|^2 - c w^2      (A = homogeneous xyz, w = weight; degree 2p, or (2p, 2q))
   is formed exactly in Bernstein form (Bernstein products). G > 0 at a parameter means the
   entity point there is farther than d - delta from q (w > 0).
3. Branch and bound: a piece (or sub-piece) whose Bernstein coefficients are all > 0 is excluded
   (G > 0 on all of it, convex-hull property). A sub-piece whose corner value (an exact point
   value of G) is <= 0 is a witness: a point at least delta closer than d exists, so d is not
   the global minimum. Otherwise the sub-piece is halved exactly (integer de Casteljau at 1/2,
   surfaces in the direction of larger coefficient variation) and both halves are examined.
   If every piece is excluded, no point of the entity is closer than d - delta: d is global.
4. Attainment: the claimed parameters are evaluated exactly (de Casteljau on the Bezier piece)
   and must give (d - delta)^2 <= |S - q|^2 <= (d + delta)^2, and the stored foot must be
   within 1e-14 m (each coordinate) of the exact point there (the foot is rounded once from the
   unrounded parameters; rounding the parameters to f64 moves the point by |S'| ulp(t)).

Together, 2 to 4 prove the reference distance is the global minimum within delta. Analytic
entities (analytic_set.json) are audited against closed forms written here (line, plane, circle,
cylinder, cone with both nappes, sphere, torus); the ellipse uses the same Bernstein certificate on
its exact rational quadratic parametrisation (two halves).

The sphere, cylinder and circle of the reference set are additionally checked against their closed
forms (|q| - r, radial projection clamped to the height, and the circle torus distance).

Run:  python tools/fixtures/nurbs_cert.py fixtures/geo/nurbs            (audit every reference file)
      python tools/fixtures/nurbs_cert.py fixtures/geo/nurbs --file ref_set_v1.json --report
Exit status 0 when every query is certified, 1 when any query fails.
"""

import argparse
import bisect
import json
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from math import comb
from pathlib import Path

getcontext().prec = 60
DELTA = Fraction(1, 10 ** 15)
FOOT_TOL = Fraction(1, 10 ** 14)
MAX_NODES = 400000
MAX_DEPTH = 400


# ---------------------------------------------------------------- reading the fixtures exactly

def hexf(h):
    """IEEE hex bit pattern (or nested lists of them) to exact Fractions."""
    if isinstance(h, list):
        return [hexf(x) for x in h]
    return Fraction(struct.unpack(">d", int(h, 16).to_bytes(8, "big"))[0])


def val(rec):
    return hexf(rec["hex"])


# ---------------------------------------------------------------- exact Bezier extraction

def insert_knot(U, P, p, t):
    """Boehm insertion of t once into knot vector U (degree p) with control points P (lists of
    Fraction tuples); returns (U', P')."""
    if not U[0] <= t < U[-1]:
        raise ValueError("knot insertion outside the knot vector")
    k = bisect.bisect_right(U, t) - 1
    s = sum(1 for x in U if x == t)
    Q = []
    for i in range(len(P) + 1):
        if i <= k - p:
            Q.append(P[i])
        elif i >= k - s + 1:
            Q.append(P[i - 1])
        else:
            a = (t - U[i]) / (U[i + p] - U[i])
            Q.append(tuple((1 - a) * x + a * y for x, y in zip(P[i - 1], P[i])))
    return U[:k + 1] + [t] + U[k + 1:], Q


def bezier_split(U, P, p):
    """Bezier pieces of a 1-parameter B-spline on its domain [U[p], U[n]]: list of
    ((a, b), [p + 1 control tuples])."""
    n = len(P)
    lo, hi = U[p], U[n]
    for t in sorted(set(x for x in U if lo <= x <= hi)):
        while sum(1 for x in U if x == t) < p:
            U, P = insert_knot(U, P, p, t)
    n = len(P)
    pieces = []
    for i in range(p, n):
        if U[i] < U[i + 1] and lo <= U[i] and U[i + 1] <= hi:
            pieces.append(((U[i], U[i + 1]), P[i - p:i + 1]))
    return pieces


def entity_pieces(e):
    """Rational Bezier pieces of a NURBS entity from fixture JSON. Curve piece:
    (box ((a, b),), coeffs [p+1][4]); surface: (box ((a, b), (c, d)), coeffs [p+1][q+1][4])."""
    ctrl = val(e["ctrl"])
    w = val(e["weights"]) if e.get("weights") is not None else [Fraction(1)] * len(ctrl)
    hom = [(c[0] * wi, c[1] * wi, c[2] * wi, wi) for c, wi in zip(ctrl, w)]
    if e["kind"] == "curve":
        return [((ab,), cps) for ab, cps in bezier_split(val(e["knots"]), hom, e["degree"])]
    p, q, nu, nv = e["degree_u"], e["degree_v"], e["n_u"], e["n_v"]
    U, V = val(e["knots_u"]), val(e["knots_v"])
    # split along v for every u row, then along u for every column of each v piece
    rows = []
    for i in range(nu):
        rows.append(bezier_split(list(V), hom[i * nv:(i + 1) * nv], q))
    out = []
    for k in range(len(rows[0])):
        cd = rows[0][k][0]
        cols = [rows[i][k][1] for i in range(nu)]  # nu x (q+1)
        # flatten each control point column-wise into one tuple of (q+1)*4 entries, split in u
        packed = [tuple(x for cp in cols[i] for x in cp) for i in range(nu)]
        for ab, cps in bezier_split(list(U), packed, p):
            net = [[tuple(cp[4 * j:4 * j + 4]) for j in range(q + 1)] for cp in cps]
            out.append(((ab, cd), net))
    return out


# ---------------------------------------------------------------- Bernstein products (integers)
#
# A Bernstein polynomial is kept in "scaled" form a'_i = C(n, i) a_i (2-D: C(m, i) C(n, j) a_ij);
# then the product is a plain convolution, (ab)'_k = sum a'_i b'_j. The true coefficients are
# (ab)'_k / C(2n, k); multiplying them by L = lcm_k C(2n, k) gives integers with the same signs.

def _zip_add(a, b):
    if isinstance(a[0], list):
        return [[x + y for x, y in zip(r, t)] for r, t in zip(a, b)]
    return [x + y for x, y in zip(a, b)]


def _flatten(a):
    return [x for r in a for x in r] if isinstance(a[0], list) else list(a)


def _lcm(a, b):
    return a * b // _gcd(a, b)


def _scaled1(a):
    n = len(a) - 1
    return [comb(n, i) * x for i, x in enumerate(a)]


def _conv1(a, b):
    out = [0] * (len(a) + len(b) - 1)
    for i, x in enumerate(a):
        if x:
            for j, y in enumerate(b):
                out[i + j] += x * y
    return out


def _unscale1(c):
    n = len(c) - 1
    L = 1
    for k in range(n + 1):
        L = _lcm(L, comb(n, k))
    return [x * (L // comb(n, k)) for k, x in enumerate(c)]


def _scaled2(a):
    m, n = len(a) - 1, len(a[0]) - 1
    return [[comb(m, i) * comb(n, j) * x for j, x in enumerate(row)] for i, row in enumerate(a)]


def _conv2(a, b):
    out = [[0] * (len(a[0]) + len(b[0]) - 1) for _ in range(len(a) + len(b) - 1)]
    for i, ra in enumerate(a):
        for j, x in enumerate(ra):
            if not x:
                continue
            for k, rb in enumerate(b):
                o = out[i + k]
                for l, y in enumerate(rb):
                    o[j + l] += x * y
    return out


def _unscale2(c):
    m, n = len(c) - 1, len(c[0]) - 1
    Lu = Lv = 1
    for k in range(m + 1):
        Lu = _lcm(Lu, comb(m, k))
    for k in range(n + 1):
        Lv = _lcm(Lv, comb(n, k))
    return [[x * (Lu // comb(m, i)) * (Lv // comb(n, j)) for j, x in enumerate(row)] for i, row in enumerate(c)]


class Piece:
    """One rational Bezier piece: exact homogeneous control points and their integer images
    (all four coordinates times one common positive denominator)."""

    def __init__(self, box, cps):
        self.box = box
        self.surface = len(box) == 2
        self.cps = cps
        flat = [x for row in cps for cp in row for x in cp] if self.surface else [x for cp in cps for x in cp]
        den = 1
        for x in flat:
            den = _lcm(den, x.denominator)
        if self.surface:
            self.comp = [_scaled2([[int(cp[k] * den) for cp in row] for row in cps]) for k in range(4)]
        else:
            self.comp = [_scaled1([int(cp[k] * den) for cp in cps]) for k in range(4)]

        # query-independent products, integer true-coefficient multiples: sum A_k^2, A_k w, w^2
        conv, unscale = (_conv2, _unscale2) if self.surface else (_conv1, _unscale1)
        aa = None
        for k in range(3):
            t = conv(self.comp[k], self.comp[k])
            aa = t if aa is None else _zip_add(aa, t)
        self.aa = _flatten(unscale(aa))
        self.aw = [_flatten(unscale(conv(self.comp[k], self.comp[3]))) for k in range(3)]
        self.ww = _flatten(unscale(conv(self.comp[3], self.comp[3])))
        self.shape = (2 * (len(cps) - 1) + 1, 2 * (len(cps[0]) - 1) + 1) if self.surface else None

    def g_coeffs(self, q, c):
        """Integer Bernstein coefficients of a positive multiple of G = |A - w q|^2 - c w^2:
        G * S^2 * cd = cd S^2 AA - 2 cd S sum Q_k AW_k + (cd |Q|^2 - cn S^2) WW (q = Q / S, c = cn / cd)."""
        S = 1
        for x in q:
            S = _lcm(S, x.denominator)
        Q = [int(x * S) for x in q]
        cn, cd = c.numerator, c.denominator
        a0 = cd * S * S
        a1 = [-2 * cd * S * Qk for Qk in Q]
        a2 = cd * (Q[0] * Q[0] + Q[1] * Q[1] + Q[2] * Q[2]) - cn * S * S
        g = [a0 * x + a1[0] * y0 + a1[1] * y1 + a1[2] * y2 + a2 * z
             for x, y0, y1, y2, z in zip(self.aa, self.aw[0], self.aw[1], self.aw[2], self.ww)]
        g = _shrink(g)
        if self.surface:
            m = self.shape[1]
            return [g[i * m:(i + 1) * m] for i in range(self.shape[0])]
        return g

    def point(self, params):
        """Exact point of the piece at global parameters (de Casteljau)."""
        loc = [(t - a) / (b - a) for t, (a, b) in zip(params, self.box)]
        if self.surface:
            rows = [_casteljau(row, loc[1]) for row in self.cps]
            h = _casteljau(rows, loc[0])
        else:
            h = _casteljau(self.cps, loc[0])
        return [h[0] / h[3], h[1] / h[3], h[2] / h[3]]


def _gcd(a, b):
    while b:
        a, b = b, a % b
    return a


def _casteljau(cps, t):
    pts = [list(c) for c in cps]
    n = len(pts)
    for r in range(1, n):
        for i in range(n - r):
            pts[i] = [(1 - t) * x + t * y for x, y in zip(pts[i], pts[i + 1])]
    return pts[0]


# ---------------------------------------------------------------- exact integer subdivision

def split1(b):
    """Halves of integer Bernstein coefficients at 1/2, both scaled by 2^n (signs preserved)."""
    n = len(b) - 1
    left, right = [b[0] << n], [b[n] << n]
    work = list(b)
    for r in range(1, n + 1):
        work = [work[i] + work[i + 1] for i in range(len(work) - 1)]
        left.append(work[0] << (n - r))
        right.append(work[-1] << (n - r))
    right.reverse()
    return left, right


def _shrink(rows):
    """Divide every coefficient by the largest common power of two (keeps the integers small)."""
    tz = None
    for x in rows:
        if x:
            z = (x & -x).bit_length() - 1
            tz = z if tz is None else min(tz, z)
            if tz == 0:
                return rows
    if not tz:
        return rows
    return [x >> tz for x in rows]


def certify_g(gs, boxes):
    """Branch and bound on G > 0. gs: list of integer Bernstein arrays (1-D lists or 2-D lists of
    rows), boxes: the matching parameter boxes. Returns ("ok", nodes), ("witness", params, nodes)
    or ("undecided", nodes)."""
    stack = [(g, box, 0) for g, box in zip(gs, boxes)]
    nodes = 0
    while stack:
        g, box, depth = stack.pop()
        nodes += 1
        if nodes > MAX_NODES or depth > MAX_DEPTH:
            return ("undecided", nodes)
        surf = isinstance(g[0], list)
        if surf:
            corners = [(g[0][0], (box[0][0], box[1][0])), (g[0][-1], (box[0][0], box[1][1])),
                       (g[-1][0], (box[0][1], box[1][0])), (g[-1][-1], (box[0][1], box[1][1]))]
            mn = min(min(r) for r in g)
        else:
            corners = [(g[0], (box[0][0],)), (g[-1], (box[0][1],))]
            mn = min(g)
        for v, prm in corners:
            if v <= 0:
                return ("witness", prm, nodes)
        if mn > 0:
            continue
        if not surf:
            a, b = box[0]
            m = (a + b) / 2
            lft, rgt = split1(g)
            stack.append((_shrink(rgt), ((m, b),), depth + 1))
            stack.append((_shrink(lft), ((a, m),), depth + 1))
            continue
        nu, nv = len(g), len(g[0])
        var_u = max((abs(g[i + 1][j] - g[i][j]) for i in range(nu - 1) for j in range(nv)), default=0)
        var_v = max((abs(g[i][j + 1] - g[i][j]) for i in range(nu) for j in range(nv - 1)), default=0)
        if var_u >= var_v and nu > 1:
            cols = [split1([g[i][j] for i in range(nu)]) for j in range(nv)]
            lft = [[cols[j][0][i] for j in range(nv)] for i in range(nu)]
            rgt = [[cols[j][1][i] for j in range(nv)] for i in range(nu)]
            a, b = box[0]
            m = (a + b) / 2
            halves = [(lft, ((a, m), box[1])), (rgt, ((m, b), box[1]))]
        else:
            sp = [split1(r) for r in g]
            c, d = box[1]
            m = (c + d) / 2
            halves = [([s[0] for s in sp], (box[0], (c, m))), ([s[1] for s in sp], (box[0], (m, d)))]
        for h, bx in reversed(halves):
            flat = _shrink([x for r in h for x in r])
            k = len(h[0])
            stack.append(([flat[i * k:(i + 1) * k] for i in range(len(h))], bx, depth + 1))
    return ("ok", nodes)


# ---------------------------------------------------------------- NURBS audit

class EntityCert:
    def __init__(self, e):
        self.e = e
        self.pieces = [Piece(box, cps) for box, cps in entity_pieces(e)]

    def certify(self, q, d):
        """("ok"|"witness"|"undecided", info) for: no point closer than d - DELTA."""
        if d <= DELTA:
            return ("ok", 0)
        c = (d - DELTA) ** 2
        gs = [pc.g_coeffs(q, c) for pc in self.pieces]
        return certify_g(gs, [pc.box for pc in self.pieces])

    def point(self, params):
        for pc in self.pieces:
            if all(a <= t <= b for t, (a, b) in zip(params, pc.box)):
                return pc.point(params)
        raise ValueError("parameters outside every piece")

    def dist2(self, params, q):
        p = self.point(params)
        return sum((p[i] - q[i]) ** 2 for i in range(3))


def closed_form(eid, q):
    """Closed-form distance (Decimal) for the named reference surfaces, or None."""
    x, y, z = (Decimal(v.numerator) / Decimal(v.denominator) for v in q)
    rho = (x * x + y * y).sqrt()
    if eid == "sphere":
        return abs((x * x + y * y + z * z).sqrt() - Decimal("0.5"))
    if eid == "circle":
        return ((rho - 1) ** 2 + z * z).sqrt()
    if eid == "cylinder":
        zc = min(max(z, Decimal(0)), Decimal("0.05"))
        return ((rho - Decimal("0.01")) ** 2 + (z - zc) ** 2).sqrt()
    return None


def audit_nurbs_file(path, report, log, only=None):
    doc = json.loads(path.read_text(encoding="utf-8"))
    fails = []
    for e in doc["entities"]:
        if not e.get("invert") or (only and e["id"] not in only):
            continue
        ec = EntityCert(e)
        surface = e["kind"] == "surface"
        bad = 0
        for r in e["invert"]:
            q = val(r["q"])
            d = val(r["distance"])
            params = (val(r["u"]), val(r["v"])) if surface else (val(r["t"]),)
            problems = []
            f = ec.dist2(params, q)
            lo = max(d - DELTA, Fraction(0))
            if not lo * lo <= f <= (d + DELTA) ** 2:
                problems.append("distance not attained at the parameters")
            foot = ec.point(params)
            if max(abs(a - b) for a, b in zip(foot, val(r["foot"]))) > FOOT_TOL:
                problems.append("foot differs from the exact point at the parameters")
            st = ec.certify(q, d)
            if st[0] == "witness":
                wd = float(ec.dist2(st[1], q)) ** 0.5
                problems.append(f"not global: a point at d = {wd!r} (params {[float(t) for t in st[1]]})")
            elif st[0] == "undecided":
                problems.append(f"certificate undecided after {st[1]} nodes")
            cf = closed_form(e["id"], q)
            if cf is not None:
                dd = Decimal(d.numerator) / Decimal(d.denominator)
                if abs(dd - cf) > Decimal("1e-15"):
                    problems.append(f"closed form {float(cf)!r} differs by {float(dd - cf):.3e}")
            if problems:
                bad += 1
                fails.append((r["id"], float(d), problems))
                if report:
                    log(f"  FAIL {r['id']}: d = {float(d)!r}: " + "; ".join(problems))
        log(f"  {path.name} {e['id']}: {len(e['invert']) - bad}/{len(e['invert'])} certified")
    return fails


# ---------------------------------------------------------------- analytic audit

def _dec(fr):
    return Decimal(fr.numerator) / Decimal(fr.denominator)


def _to_frac(d):
    return Fraction(d)


def bern_from_power(cs):
    """Bernstein coefficients on t in [0, 1] of sum cs[k] s^k with s = 2t - 1 (exact)."""
    n = len(cs) - 1
    # polynomial in t: expand (2t - 1)^k
    pt = [Fraction(0)] * (n + 1)
    for k, ck in enumerate(cs):
        for j in range(k + 1):
            pt[j] += ck * comb(k, j) * (2 ** j) * (-1) ** (k - j)
    # power basis in t to Bernstein: b_i = sum_{j<=i} C(i, j) / C(n, j) a_j
    return [sum(Fraction(comb(i, j), comb(n, j)) * pt[j] for j in range(i + 1)) for i in range(n + 1)]


def _pmul(a, b):
    out = [Fraction(0)] * (len(a) + len(b) - 1)
    for i, x in enumerate(a):
        for j, y in enumerate(b):
            out[i + j] += x * y
    return out


def _padd(*ps):
    n = max(len(p) for p in ps)
    return [sum(p[i] if i < len(p) else 0 for p in ps) for i in range(n)]


def _scale(s, p):
    return [s * x for x in p]


def ellipse_certify(a, b, x, y, z, d):
    """No point of the ellipse (a cos t, b sin t) closer than d - DELTA to local (x, y, z)."""
    if d <= DELTA:
        return ("ok", 0)
    c = (d - DELTA) ** 2 - z * z
    one_s2 = [Fraction(1), Fraction(0), Fraction(1)]
    gs = []
    for sx, sy in ((1, 1), (-1, -1)):
        # point = (sx a (1 - s^2), sy 2 b s) / (1 + s^2)
        ex = _padd(_scale(sx * a, [Fraction(1), Fraction(0), Fraction(-1)]), _scale(-x, one_s2))
        ey = _padd(_scale(sy * 2 * b, [Fraction(0), Fraction(1)]), _scale(-y, one_s2))
        g = _padd(_pmul(ex, ex), _pmul(ey, ey), _scale(-c, _pmul(one_s2, one_s2)))
        bb = bern_from_power(g)
        den = 1
        for v in bb:
            den = den * v.denominator // _gcd(den, v.denominator)
        gs.append([v.numerator * (den // v.denominator) for v in bb])
    return certify_g(gs, [((Fraction(0), Fraction(1)),)] * 2)


def audit_analytic(path, report, log):
    doc = json.loads(path.read_text(encoding="utf-8"))
    fails = []
    for e in doc["entities"]:
        O = [_dec(v) for v in val(e["placement"]["origin"])]
        Z = [_dec(v) for v in val(e["placement"]["axis"])]
        X = [_dec(v) for v in val(e["placement"]["ref_dir"])]
        Y = [Z[1] * X[2] - Z[2] * X[1], Z[2] * X[0] - Z[0] * X[2], Z[0] * X[1] - Z[1] * X[0]]
        P = {k: _dec(val(v)) for k, v in e["params"].items()}
        bad = 0
        for r in e["invert"]:
            qd = [_dec(v) for v in val(r["q"])]
            dv = [qd[i] - O[i] for i in range(3)]
            x = sum(dv[i] * X[i] for i in range(3))
            y = sum(dv[i] * Y[i] for i in range(3))
            z = sum(dv[i] * Z[i] for i in range(3))
            rho = (x * x + y * y).sqrt()
            d = _dec(val(r["distance"]))
            t = e["type"]
            problems = []
            true = None
            if t == "line":
                true = (y * y + z * z).sqrt()
            elif t == "plane":
                true = abs(z)
            elif t == "circle":
                true = ((rho - P["radius"]) ** 2 + z * z).sqrt()
            elif t == "cylinder":
                true = abs(rho - P["radius"])
            elif t == "sphere":
                true = abs((rho * rho + z * z).sqrt() - P["radius"])
            elif t == "torus":
                true = abs(((rho - P["major_radius"]) ** 2 + z * z).sqrt() - P["minor_radius"])
            elif t == "cone":
                # meridian half-plane (signed radius): lines rho = +-(r + z tan a) through the apex
                sa, ca = _sincos(P["semi_angle"])
                r0 = P["radius"]
                # line L1: rho*ca - z*sa - r0*ca = 0 (normalised); L2: rho*ca + z*sa + r0*ca = 0
                d1 = abs(rho * ca - z * sa - r0 * ca)
                d2 = abs(rho * ca + z * sa + r0 * ca)
                true = min(d1, d2)
            elif t == "ellipse":
                st = ellipse_certify(_to_frac(P["semi_axis_1"]), _to_frac(P["semi_axis_2"]), _to_frac(x),
                                     _to_frac(y), _to_frac(z), _to_frac(d))
                if st[0] != "ok":
                    problems.append(f"ellipse certificate: {st[0]}")
                tt = _dec(val(r["params"])[0])
                s, co = _sincos(tt)
                fx, fy = P["semi_axis_1"] * co - x, P["semi_axis_2"] * s - y
                att = (fx * fx + fy * fy + z * z).sqrt()
                if abs(att - d) > Decimal("1e-15"):
                    problems.append(f"distance not attained ({float(att)!r})")
            if true is not None and abs(true - d) > Decimal("1e-15"):
                problems.append(f"closed form {float(true)!r} differs by {float(d - true):.3e}")
            if problems:
                bad += 1
                fails.append((r["id"], float(d), problems))
                if report:
                    log(f"  FAIL {r['id']}: " + "; ".join(problems))
        log(f"  {path.name} {e['id']}: {len(e['invert']) - bad}/{len(e['invert'])} certified")
    return fails


def _sincos(x):
    """60-digit sin and cos (Taylor after halving)."""
    getcontext().prec = 80
    m = 12
    y = x / (2 ** m)
    s = term = y
    n = 1
    while True:
        term = -term * y * y / ((n + 1) * (n + 2))
        n += 2
        s += term
        if abs(term) < Decimal(10) ** -85:
            break
    c = term = Decimal(1)
    n = 0
    while True:
        term = -term * y * y / ((n + 1) * (n + 2))
        n += 2
        c += term
        if abs(term) < Decimal(10) ** -85:
            break
    for _ in range(m):
        s, c = 2 * s * c, c * c - s * s
    getcontext().prec = 60
    return +s, +c


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("dir")
    ap.add_argument("--file", action="append", help="audit only these files (default: all reference files)")
    ap.add_argument("--entity", action="append", help="audit only these entity ids")
    ap.add_argument("--report", action="store_true", help="print every failing query")
    args = ap.parse_args()
    root = Path(args.dir)
    files = args.file or [m["path"] for m in json.loads((root / "manifest.json").read_text())["files"]
                          if not m.get("adopted") and not m.get("superseded")]
    total = []
    for name in files:
        log = lambda m: print(m, flush=True)  # noqa: E731
        if name.startswith("analytic"):
            total += audit_analytic(root / name, args.report, log)
        else:
            total += audit_nurbs_file(root / name, args.report, log, args.entity)
    if total:
        print(f"audit: {len(total)} queries FAILED")
        sys.exit(1)
    print("audit: every query certified")


if __name__ == "__main__":
    main()
