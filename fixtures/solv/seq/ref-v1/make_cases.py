#!/usr/bin/env python3
"""Independent reference for KRN.SOLV.SEQ (CC-BY-4.0 project data).

Writes fixtures/solv/seq/ref-v1/cases.txt. Every expected value is decided here with exact
rational arithmetic (fractions.Fraction) or with the stdlib decimal module at 90 significant
digits (software arithmetic, correctly rounded square roots); nothing here runs or reads the
Rust code or any solver, and nothing uses the platform libm. The only float operations are
float(Fraction) (correctly rounded by integer arithmetic) and bit manipulation. Point
coordinates are generated in Decimal (sin and cos by Taylor series, pi by Machin's formula),
rounded to the nearest double, and from then on taken as the exact rationals of those doubles.

Families (20 problems each). Row labels are (point, side); rows are listed point by point,
side 0 then side 1 where a point has two rows.

- mzc (minimum-zone circle, CertKind::Zone): parameters p = (cx, cy, r_in, r_out); rows
  (i, 0): g = r_in - d_i <= 0 and (i, 1): g = d_i - r_out <= 0, with d_i the distance of point i
  to (cx, cy); f = r_out - r_in. 10 points on a circle with planted form error. The optimum is
  found by exhaustive contact-set search: every 2+2, 3+1 and 1+3 split of 4 points into inner
  and outer contacts gives the centre exactly (intersection of perpendicular bisectors, a
  rational linear solve), the radii squared exactly, and feasibility by exact comparison of
  squared distances. The global optimum is the feasible candidate of least width (widths in
  Decimal). It is the expected value and its contacts the expected active set.
- mic (maximum-inscribed circle, CertKind::Envelope): p = (cx, cy, r); rows (i, 0):
  g = r - d_i <= 0; f = -r (the value of record is -r). 10 points on a ring. Exhaustive search
  over triples: circumcentre exact, empty circle by exact comparison, kept only when the
  centre lies strictly inside the contact triangle (positive multipliers); the largest r wins.
  The reference is the largest strict local maximum with its centre inside the contact
  triangle, not a claimed global maximum.
- mzcyl (minimum-zone cylinder axis, CertKind::Zone): p = (x0, y0, a, b, r_in, r_out), the axis
  through (x0, y0, 0) with direction (a, b, 1); rows as for mzc with d_i the distance of point i
  to the axis. 12 points on a cylinder with planted form error (heights centred on z = 0):
  3 outer contacts at +h, 3 inner contacts at -h, every other point within 0.4 h. The planted
  contact set is polished by Newton's method in Decimal (residual below 1e-75) and kept only
  if it is a strict local minimum: every multiplier mu (G^T mu = -grad f) >= 1e-3 and every
  other row g <= -margin. With 6 independent active rows and positive multipliers that is a
  proven strict local optimum (the critical cone is {0}). A problem is also skipped when a
  neighbouring contact set (one row exchanged) is a strict local minimum too, so the reference
  has no rival optimum nearby. The reference is not proven global.

Every kept problem also satisfies: each multiplier >= 1e-3, each non-contact row
g <= -1e-3 * width (so the classification has no near-active row), and (mzc, mic) the second
best feasible candidate is worse by more than 1e-9 relative.

Least-squares start: for mzc and mic the algebraic (Kasa) circle fit, an exact rational linear
least-squares solve; r_in = min d_i, r_out = max d_i (mic: r = min d_i) at that centre. For mzcyl
eight Gauss-Newton steps of the geometric fit (residual d_i - r) in Decimal from the planted
axis; then r_in = min d_i, r_out = max d_i. All rounded to the nearest double.

Special problems:

- twin (AC-5): points on the intersection of two cylinders of radius R through the origin with
  directions (1, 0, 1) and (-1, 0, 1) (the curves x = 0, y^2 + z^2/2 = R^2 and z = 0,
  x^2/2 + y^2 = R^2), displaced along the radial direction of the first axis (6 planted
  contacts at +-h, others within 0.3 h), plus their mirror images x -> -x (exact in double).
  Both axes are verified strict local minima as above (the second is the mirror of the
  first), so seeded starts on either side reach different certified results.

Output lines (hex = 16 lowercase hex digits of an IEEE-754 double):
  case <name> family <mzc|mic|mzcyl|twin> n <points>
  points <x y [z] per point, hex>
  start <hex per parameter>
  set <point:side ...>             expected active set, sorted
  params <hex per parameter>       optimum, nearest double
  value <hex> <50 significant digits>
  (twin only) set2 / params2 / value2 for the mirror optimum
  end
"""

import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from itertools import combinations
from pathlib import Path
from random import Random

OUT = Path(__file__).with_name("cases.txt")
getcontext().prec = 90
COUNT = 20


# ------------------------------------------------------------------ number helpers


def dec(x):
    if isinstance(x, Decimal):
        return x
    x = Fraction(x)
    return Decimal(x.numerator) / Decimal(x.denominator)


def fl(x):
    """Nearest double of a Fraction or Decimal (float(Fraction) is correctly rounded)."""
    return float(Fraction(x))


def hx(f):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", f))[0]


def digits50(d):
    return "{:.49e}".format(dec(d))


def machin_pi():
    def atan_inv(k):
        x = Decimal(1) / k
        x2 = x * x
        term, total, n = x, x, 1
        while True:
            term = -term * x2
            n += 2
            t = term / n
            if abs(t) < Decimal(10) ** -95:
                return total
            total += t

    return 16 * atan_inv(5) - 4 * atan_inv(239)


PI = machin_pi()


def dsin_cos(t):
    t = dec(t)
    two_pi = 2 * PI
    t = t - two_pi * (t / two_pi).to_integral_value()
    s, c = Decimal(0), Decimal(0)
    term = Decimal(1)
    k = 0
    while True:
        if k % 2 == 0:
            c += term if k % 4 == 0 else -term
        else:
            s += term if k % 4 == 1 else -term
        k += 1
        term = term * t / k
        if abs(term) < Decimal(10) ** -95:
            return s, c


def solve(m, rhs):
    """Gaussian elimination with partial pivoting (Decimal or Fraction entries)."""
    n = len(rhs)
    a = [list(r) + [v] for r, v in zip(m, rhs)]
    for col in range(n):
        piv = max(range(col, n), key=lambda r: abs(a[r][col]))
        if a[piv][col] == 0:
            return None
        a[col], a[piv] = a[piv], a[col]
        for r in range(col + 1, n):
            f = a[r][col] / a[col][col]
            if f:
                for c in range(col, n + 1):
                    a[r][c] -= f * a[col][c]
    x = [0] * n
    for r in range(n - 1, -1, -1):
        s = a[r][n] - sum(a[r][c] * x[c] for c in range(r + 1, n))
        x[r] = s / a[r][r]
    return x


def rnd(rng):
    """A uniform number in [0, 1) as an exact Fraction."""
    return Fraction(rng.random())


# ------------------------------------------------------------------ circles


def ring_points(rng, n, centre, radius, errors):
    pts = []
    for k in range(n):
        theta = 2 * PI * (k + dec(rnd(rng) * Fraction(3, 5) - Fraction(3, 10))) / n
        s, c = dsin_cos(theta)
        r = dec(radius + errors[k])
        pts.append((fl(dec(centre[0]) + r * c), fl(dec(centre[1]) + r * s)))
    return pts


def kasa(pts):
    """Exact algebraic circle fit: min sum (x^2 + y^2 + D x + E y + F)^2; centre (-D/2, -E/2)."""
    rows = [(Fraction(x), Fraction(y), Fraction(1)) for x, y in pts]
    rhs = [-(Fraction(x) ** 2 + Fraction(y) ** 2) for x, y in pts]
    ata = [[sum(r[i] * r[j] for r in rows) for j in range(3)] for i in range(3)]
    atb = [sum(r[i] * v for r, v in zip(rows, rhs)) for i in range(3)]
    d, e, _ = solve(ata, atb)
    return (-d / 2, -e / 2)


def d2(p, c):
    return (Fraction(p[0]) - c[0]) ** 2 + (Fraction(p[1]) - c[1]) ** 2


def bisector(p, q):
    p = (Fraction(p[0]), Fraction(p[1]))
    q = (Fraction(q[0]), Fraction(q[1]))
    return [2 * (q[0] - p[0]), 2 * (q[1] - p[1])], q[0] ** 2 + q[1] ** 2 - p[0] ** 2 - p[1] ** 2


def centre_of(pairs):
    (r1, b1), (r2, b2) = pairs
    det = r1[0] * r2[1] - r1[1] * r2[0]
    if det == 0:
        return None
    return ((b1 * r2[1] - b2 * r1[1]) / det, (r1[0] * b2 - r2[0] * b1) / det)


def circle_rows(pts, c, labels):
    """Constraint gradients of circle rows at centre c (Decimal); side 0: (u, 1, 0) etc."""
    out = []
    for i, side in labels:
        dx, dy = dec(Fraction(pts[i][0]) - c[0]), dec(Fraction(pts[i][1]) - c[1])
        d = (dx * dx + dy * dy).sqrt()
        ux, uy = dx / d, dy / d
        out.append((ux, uy, side))
    return out


def mz_circle(pts):
    n = len(pts)
    cands = []
    for quad in combinations(range(n), 4):
        for inner_count in (1, 2, 3):
            for inner in combinations(quad, inner_count):
                outer = tuple(i for i in quad if i not in inner)
                big = inner if len(inner) >= len(outer) else outer
                small = outer if big is inner else inner
                if len(big) == 2:
                    c = centre_of([bisector(pts[big[0]], pts[big[1]]), bisector(pts[small[0]], pts[small[1]])])
                else:
                    c = centre_of([bisector(pts[big[0]], pts[big[1]]), bisector(pts[big[0]], pts[big[2]])])
                if c is None:
                    continue
                rin2, rout2 = d2(pts[inner[0]], c), d2(pts[outer[0]], c)
                if rin2 >= rout2:
                    continue
                if all(rin2 <= d2(p, c) <= rout2 for p in pts):
                    width = dec(rout2).sqrt() - dec(rin2).sqrt()
                    labels = sorted([(i, 0) for i in inner] + [(i, 1) for i in outer])
                    cands.append((width, c, rin2, rout2, labels))
    cands.sort(key=lambda t: t[0])
    best = cands[0]
    if len(cands) > 1 and cands[1][0] - best[0] <= best[0] * Decimal("1e-9"):
        return None
    width, c, rin2, rout2, labels = best
    rin, rout = dec(rin2).sqrt(), dec(rout2).sqrt()
    # Margins of the non-contact rows.
    contact = {i for i, _ in labels}
    for i, p in enumerate(pts):
        if i in contact:
            continue
        d = dec(d2(p, c)).sqrt()
        if d - rin <= width / 1000 or rout - d <= width / 1000:
            return None
    # Multipliers: G^T mu = -grad f = (0, 0, 1, -1).
    rows = []
    for ux, uy, side in circle_rows(pts, c, labels):
        rows.append([ux, uy, Decimal(1), Decimal(0)] if side == 0 else [-ux, -uy, Decimal(0), Decimal(-1)])
    gt = [[rows[r][k] for r in range(4)] for k in range(4)]
    mu = solve(gt, [Decimal(0), Decimal(0), Decimal(1), Decimal(-1)])
    if mu is None or min(mu) < Decimal("1e-3"):
        return None
    return labels, [dec(c[0]), dec(c[1]), rin, rout], width


def mic_circle(pts):
    n = len(pts)
    best = None
    rs = []
    for tri in combinations(range(n), 3):
        c = centre_of([bisector(pts[tri[0]], pts[tri[1]]), bisector(pts[tri[0]], pts[tri[2]])])
        if c is None:
            continue
        r2 = d2(pts[tri[0]], c)
        if not all(d2(p, c) >= r2 for p in pts):
            continue
        # Centre strictly inside the contact triangle (exact barycentric signs).
        (x1, y1), (x2, y2), (x3, y3) = [(Fraction(pts[i][0]), Fraction(pts[i][1])) for i in tri]

        def side(ax, ay, bx, by):
            return (bx - ax) * (c[1] - ay) - (by - ay) * (c[0] - ax)

        s = [side(x1, y1, x2, y2), side(x2, y2, x3, y3), side(x3, y3, x1, y1)]
        if not (all(v > 0 for v in s) or all(v < 0 for v in s)):
            continue
        rs.append(dec(r2).sqrt())
        if best is None or r2 > best[1]:
            best = (tri, r2, c)
    if best is None:
        return None
    tri, r2, c = best
    r = dec(r2).sqrt()
    rs.sort(reverse=True)
    if len(rs) > 1 and r - rs[1] <= r * Decimal("1e-9"):
        return None
    for i, p in enumerate(pts):
        if i not in tri and dec(d2(p, c)).sqrt() - r <= r / 1000:
            return None
    labels = [(i, 0) for i in tri]
    rows = [[ux, uy, Decimal(1)] for ux, uy, _ in circle_rows(pts, c, labels)]
    gt = [[rows[r_][k] for r_ in range(3)] for k in range(3)]
    mu = solve(gt, [Decimal(0), Decimal(0), Decimal(1)])
    if mu is None or min(mu) < Decimal("1e-3"):
        return None
    return labels, [dec(c[0]), dec(c[1]), r], -r


def circle_start(pts, mic):
    c = kasa(pts)
    ds = [dec(d2(p, c)).sqrt() for p in pts]
    if mic:
        return [fl(c[0]), fl(c[1]), fl(min(ds))]
    return [fl(c[0]), fl(c[1]), fl(min(ds)), fl(max(ds))]


def make_circle(k, mic):
    attempt = 0
    while True:
        rng = Random(f"seq-ref-v1-{'mic' if mic else 'mzc'}-{k}-{attempt}")
        attempt += 1
        radius = Fraction(rng.randint(50, 200), 10)
        centre = (Fraction(rng.randint(-100, 100), 10), Fraction(rng.randint(-100, 100), 10))
        h = radius * Fraction(rng.randint(5, 30), 1000)
        errors = [h * (2 * rnd(rng) - 1) for _ in range(10)]
        pts = ring_points(rng, 10, centre, radius, errors)
        res = mic_circle(pts) if mic else mz_circle(pts)
        if res is None:
            continue
        labels, params, value = res
        return pts, circle_start(pts, mic), labels, params, value


# ------------------------------------------------------------------ cylinders


def cyl_dist_grad(pt, p):
    """Distance of pt to the axis of p = (x0, y0, a, b, ...) and its gradient in (x0, y0, a, b),
    in Decimal: c = w x v, d = |c| / |v| (see the Rust test for the same formulas)."""
    x0, y0, a, b = p[0], p[1], p[2], p[3]
    wx, wy, wz = dec(pt[0]) - x0, dec(pt[1]) - y0, dec(pt[2])
    cx, cy, cz = wy - wz * b, wz * a - wx, wx * b - wy * a
    nc = (cx * cx + cy * cy + cz * cz).sqrt()
    vv = a * a + b * b + 1
    nv = vv.sqrt()
    d = nc / nv
    k = nc * nv
    g = [
        (cy - b * cz) / k,
        (-cx + a * cz) / k,
        (wz * cy - wy * cz) / k - nc * a / (nv * vv),
        (-wz * cx + wx * cz) / k - nc * b / (nv * vv),
    ]
    return d, g


def frame(a, b):
    """Orthonormal vectors e1, e2 perpendicular to (a, b, 1), and the unit axis vector."""
    v = (dec(a), dec(b), Decimal(1))
    nv = (v[0] ** 2 + v[1] ** 2 + 1).sqrt()
    u = tuple(x / nv for x in v)
    e1 = (u[2], Decimal(0), -u[0])  # u x (0, 1, 0), up to sign
    n1 = (e1[0] ** 2 + e1[2] ** 2).sqrt()
    e1 = (e1[0] / n1, Decimal(0), e1[2] / n1)
    e2 = (u[1] * e1[2] - u[2] * e1[1], u[2] * e1[0] - u[0] * e1[2], u[0] * e1[1] - u[1] * e1[0])
    return u, e1, e2


def polish(pts, labels, p0):
    """Newton's method on d_i(p) = r_side for the contact rows (6 equations, 6 unknowns)."""
    p = [dec(v) for v in p0]
    for _ in range(40):
        jac, res = [], []
        for i, side in labels:
            d, g = cyl_dist_grad(pts[i], p)
            res.append(d - p[4 + side])
            row = g + [Decimal(0), Decimal(0)]
            row[4 + side] = Decimal(-1)
            jac.append(row)
        step = solve(jac, res)
        if step is None:
            return None
        p = [x - s for x, s in zip(p, step)]
        if max(abs(r) for r in res) < Decimal(10) ** -75:
            return p
    return None


def verify_cyl(pts, labels, p):
    """Strict local minimum check: multipliers >= 1e-3 and every other row inside by margin."""
    width = p[5] - p[4]
    if width <= 0:
        return None
    rows = []
    for i, side in labels:
        d, g = cyl_dist_grad(pts[i], p)
        if abs(d - p[4 + side]) > Decimal(10) ** -60:
            return None
        rows.append([-v for v in g] + [Decimal(1), Decimal(0)] if side == 0 else g + [Decimal(0), Decimal(-1)])
    gt = [[rows[r][k] for r in range(6)] for k in range(6)]
    mu = solve(gt, [Decimal(0)] * 4 + [Decimal(1), Decimal(-1)])
    if mu is None or min(mu) < Decimal("1e-3"):
        return None
    contact = {i for i, _ in labels}
    for i, pt in enumerate(pts):
        if i in contact:
            continue
        d, _ = cyl_dist_grad(pt, p)
        if d - p[4] <= width / 1000 or p[5] - d <= width / 1000:
            return None
    return width


def rival_nearby(pts, labels, p):
    """Whether a neighbouring contact set (one contact row exchanged for another row) is also a
    strict local minimum. Such problems have competing optima near the reference and are
    skipped, so the reference is the only strict local minimum within one exchange."""
    rows = [(i, s) for i in range(len(pts)) for s in (0, 1)]
    for k in range(len(labels)):
        for row in rows:
            if row in labels or any(row[0] == i for i, _ in labels if (i, _) != labels[k]):
                continue
            cand = sorted(labels[:k] + labels[k + 1 :] + [row])
            if not any(s == 0 for _, s in cand) or not any(s == 1 for _, s in cand):
                continue
            q = polish(pts, cand, p)
            if q is not None and verify_cyl(pts, cand, q) is not None:
                return True
    return False


def lsq_cylinder(pts, axis, radius):
    """Eight Gauss-Newton steps of the geometric least-squares cylinder (residual d_i - r)."""
    p = [dec(v) for v in axis] + [dec(radius)]
    for _ in range(8):
        jac, res = [], []
        for pt in pts:
            d, g = cyl_dist_grad(pt, p)
            res.append(d - p[4])
            jac.append(g + [Decimal(-1)])
        jtj = [[sum(r[i] * r[j] for r in jac) for j in range(5)] for i in range(5)]
        jtr = [sum(r[i] * v for r, v in zip(jac, res)) for i in range(5)]
        step = solve(jtj, jtr)
        p = [x - s for x, s in zip(p, step)]
    ds = [cyl_dist_grad(pt, p)[0] for pt in pts]
    return [fl(p[0]), fl(p[1]), fl(p[2]), fl(p[3]), fl(min(ds)), fl(max(ds))]


def make_cylinder(k):
    attempt = 0
    while True:
        rng = Random(f"seq-ref-v1-mzcyl-{k}-{attempt}")
        attempt += 1
        radius = Fraction(rng.randint(50, 150), 10)
        length = radius * Fraction(rng.randint(10, 30), 10)
        x0, y0 = Fraction(rng.randint(-50, 50), 10), Fraction(rng.randint(-50, 50), 10)
        a, b = Fraction(rng.randint(-50, 50), 1000), Fraction(rng.randint(-50, 50), 1000)
        h = radius * Fraction(rng.randint(5, 20), 1000)
        u, e1, e2 = frame(a, b)
        n = 12
        # Contacts: outer near 0, 120, 240 degrees, inner near 60, 180, 300, random heights.
        plan = []
        for j in range(6):
            deg = 60 * j + rng.randint(-20, 20)
            plan.append((Fraction(deg, 360), h if j % 2 == 0 else -h, length * (rnd(rng) - Fraction(1, 2))))
        for _ in range(n - 6):
            plan.append((rnd(rng), Fraction(2, 5) * h * (2 * rnd(rng) - 1), length * (rnd(rng) - Fraction(1, 2))))
        order = list(range(n))
        rng.shuffle(order)
        pts = [None] * n
        for slot, (turn, err, t) in zip(order, plan):
            s, c = dsin_cos(2 * PI * dec(turn))
            r = dec(radius + err)
            # Point = (x0, y0, 0) + t' u + r (c e1 + s e2), with t' chosen so that z = t.
            base = [
                dec(x0) + r * (c * e1[0] + s * e2[0]),
                dec(y0) + r * (c * e1[1] + s * e2[1]),
                r * (c * e1[2] + s * e2[2]),
            ]
            tt = (dec(t) - base[2]) / u[2]
            pts[slot] = tuple(fl(base[i] + tt * u[i]) for i in range(3))
        labels = sorted((order[j], 1 if j % 2 == 0 else 0) for j in range(6))
        p = polish(pts, labels, [x0, y0, a, b, radius - h, radius + h])
        if p is None:
            continue
        width = verify_cyl(pts, labels, p)
        if width is None or rival_nearby(pts, labels, p):
            continue
        start = lsq_cylinder(pts, (x0, y0, a, b), radius)
        return pts, start, labels, p, width


def make_twin():
    """AC-5: points on the intersection of two cylinders, planted for the first, mirrored."""
    attempt = 0
    while True:
        rng = Random(f"seq-ref-v1-twin-{attempt}")
        attempt += 1
        radius = Fraction(10)
        h = Fraction(1, 20)
        root2 = Decimal(2).sqrt()
        axis1 = (Decimal(0), Decimal(0), Decimal(1), Decimal(0))
        base = []
        for j in range(10):
            s, c = dsin_cos(2 * PI * dec(rnd(rng)))
            if j % 2 == 0:  # x = 0: y = R sin, z = sqrt(2) R cos
                base.append((Decimal(0), dec(radius) * s, root2 * dec(radius) * c))
            else:  # z = 0: x = sqrt(2) R cos, y = R sin
                base.append((root2 * dec(radius) * c, dec(radius) * s, Decimal(0)))
        contacts = rng.sample(range(10), 6)
        pts = []
        for j, q in enumerate(base):
            if j in contacts:
                err = dec(h) if contacts.index(j) % 2 == 0 else -dec(h)
            else:
                err = dec(Fraction(3, 10) * h * (2 * rnd(rng) - 1))
            # Radial direction of q from axis 1 (direction (1, 0, 1) / sqrt(2), through 0).
            proj = (q[0] + q[2]) / 2
            rad = (q[0] - proj, q[1], q[2] - proj)
            nr = (rad[0] ** 2 + rad[1] ** 2 + rad[2] ** 2).sqrt()
            pts.append(tuple(fl(q[i] + err * rad[i] / nr) for i in range(3)))
        pts = pts + [(-x, y, z) for x, y, z in pts]
        labels = sorted((contacts[j], 1 if j % 2 == 0 else 0) for j in range(6))
        p = polish(pts, labels, list(axis1) + [radius - h, radius + h])
        if p is None:
            continue
        width = verify_cyl(pts, labels, p)
        if width is None:
            continue
        labels2 = sorted((i + 10, s) for i, s in labels)
        p2 = polish(pts, labels2, [-p[0], p[1], -p[2], p[3], p[4], p[5]])
        if p2 is None:
            continue
        width2 = verify_cyl(pts, labels2, p2)
        if width2 is None:
            continue
        return pts, labels, p, width, labels2, p2, width2


# ------------------------------------------------------------------ output


def fmt_labels(labels):
    return " ".join(f"{i}:{s}" for i, s in labels)


def emit(out, name, family, pts, start, labels, params, value, extra=None):
    out.append(f"case {name} family {family} n {len(pts)}")
    out.append("points " + " ".join(hx(c) for p in pts for c in p))
    if start is not None:
        out.append("start " + " ".join(hx(v) for v in start))
    out.append("set " + fmt_labels(labels))
    out.append("params " + " ".join(hx(fl(v)) for v in params))
    out.append(f"value {hx(fl(value))} {digits50(value)}")
    if extra:
        out.extend(extra)
    out.append("end")


def render():
    out = [
        "# KRN.SOLV.SEQ fixture ref-v1 (CC-BY-4.0). Generated by make_cases.py; do not edit.",
        "# 20 minimum-zone circles, 20 maximum-inscribed circles, 20 minimum-zone cylinder axes",
        "# (exact or 90-digit references, see make_cases.py) and the AC-5 twin-optimum cylinder.",
    ]
    for k in range(COUNT):
        pts, start, labels, params, value = make_circle(k, False)
        emit(out, f"mzc-{k:02d}", "mzc", pts, start, labels, params, value)
    for k in range(COUNT):
        pts, start, labels, params, value = make_circle(k, True)
        emit(out, f"mic-{k:02d}", "mic", pts, start, labels, params, value)
    for k in range(COUNT):
        pts, start, labels, params, value = make_cylinder(k)
        emit(out, f"mzcyl-{k:02d}", "mzcyl", pts, start, labels, params, value)
    pts, labels, p, width, labels2, p2, width2 = make_twin()
    extra = [
        "set2 " + fmt_labels(labels2),
        "params2 " + " ".join(hx(fl(v)) for v in p2),
        f"value2 {hx(fl(width2))} {digits50(width2)}",
    ]
    emit(out, "twin", "twin", pts, None, labels, p, width, extra)
    return "\n".join(out) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        if not (OUT.exists() and OUT.read_text(encoding="utf-8") == text):
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT} is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
