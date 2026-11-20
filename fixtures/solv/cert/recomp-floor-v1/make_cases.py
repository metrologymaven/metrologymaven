#!/usr/bin/env python3
"""Independent reference for the KRN.SOLV.CERT.RECOMP convergence rule (CC-BY-4.0 project data).

Writes fixtures/solv/cert/recomp-floor-v1/cases.txt: nonlinear minimum-zone problems whose
Gauss-Newton steps, with residuals evaluated in plain f64, stall at the rounding floor above
4 u ||p|| (partial arcs, long cylinders, parts far from the origin), and one problem that has
not converged after its step budget and must still be refused.

Every expected value is decided here with exact rational arithmetic (fractions.Fraction of the
exact dyadic data) or with the stdlib decimal module at 90 significant digits (software
arithmetic, correctly rounded square roots; sin and cos by Taylor series, pi by Machin's
formula). Nothing here runs or reads the Rust code or any solver, and nothing uses the
platform libm. The only float operations are float(Fraction) (correctly rounded by integer
arithmetic) and bit manipulation. Point coordinates are generated in Decimal, rounded to the
nearest double and from then on taken as the exact rationals of those doubles.

Problems. Rows (i, 0): g = r_in - d_i <= 0 (inner) and (i, 1): g = d_i - r_out <= 0 (outer);
f = r_out - r_in; rows are listed point by point, side 0 then side 1.

- arc (minimum-zone circle on a partial arc): p = (cx, cy, r_in, r_out), d_i the distance of
  point i to (cx, cy). 12 points on an arc of 3 to 10 degrees (radius 10 to 50) with planted
  form error h of 0.05 % to 0.2 % of the radius; the contacts alternate outer, inner, outer,
  inner along the arc at +-h, every other point within 0.4 h. `arc-*` have the centre within
  5 mm of the origin, `arc-far-*` about 300 mm from it.
- cyl (minimum-zone cylinder axis): p = (x0, y0, a, b, r_in, r_out), the axis through
  (x0, y0, 0) with direction (a, b, 1), |a|, |b| <= 0.3; d_i the distance of point i to the
  axis (formulas as in fixtures/solv/seq/ref-v1/make_cases.py). 14 points with planted form
  error: 3 outer contacts at +h near 0, 120, 240 degrees, 3 inner at -h near 60, 180, 300
  degrees, every other point within 0.4 h. `cyl-long-*`: radius 2 to 5, length 60 to 120 radii,
  heights z in [0, L] (all on one side of the z = 0 plane the axis parameters refer to).
  `cyl-far-*`: radius 2 to 5, length 1.5 to 3 radii, axis about 300 mm from the origin in x and
  y, heights z in [300, 300 + L].

Each planted contact set is polished by Newton's method in Decimal (residual below 1e-75) and
kept only if it is a strict local minimum: every multiplier mu (G^T mu = -grad f) >= 1e-3 and
every other row g <= -width / 1000. With n independent active rows and positive multipliers
that is a proven strict local optimum (the critical cone is {0}), so the set is the active set
and the polished root is the exact optimum of the f64 data.

`start` (cylinders): the planted axis rounded to the nearest double, with r_in and r_out the
distances of the set's first inner and first outer contact to it. Problem data, not a solver
iterate. Arcs carry no start: the Rust test derives it from the set (perpendicular bisectors).

The must-fail case `cyl-unconv` is a long cylinder whose start axis is moved by R / 200 and
tilted by 0.0005, run with a budget of `gn_steps 2`. Two exact Newton steps from the start are
taken here: the last step s, and the Newton correction c at the final point (to first order the
distance to the root, and the centre of any Krawczyk box around it). The script asserts
s > 1000 * (4 u ||p|| + 2 ||c||), so the convergence rule must refuse it whatever the proof's
overestimate within that factor; rounding of the f64 iteration (about 1e-15 relative) is far
below these magnitudes.

Output lines (hex = 16 lowercase hex digits of an IEEE-754 double):
  case <name> kind <arc|cyl>
  points <x y [z] per point, hex>
  start <hex per parameter>        (cylinders only)
  set <point:side ...>             the active set, sorted
  params <hex per parameter>       exact optimum, nearest double
  value <hex> <50 significant digits>
  expect certify | no_convergence
  gn_steps <n>                     (no_convergence only)
  end
"""

import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path
from random import Random

OUT = Path(__file__).with_name("cases.txt")
getcontext().prec = 90
U = Fraction(1, 2**53)
# Candidate indices kept per family (16 generated each). Kept: those whose Gauss-Newton
# iteration with plain-f64 residuals ends with a last step above 1.2 * 4 u ||p||, i.e. that the
# step-only convergence rule refused. That property is a statement about f64 arithmetic, so it
# is not decided here: the Rust test replays the iteration bit for bit and asserts it for every
# kept case. The selection was made once and is recorded, not computed. The expected values
# (optima, active sets, local optimality) are all decided by this script.
KEEP = {
    "arc": [3, 8, 9],
    "arc-far": [7, 9, 11, 14],
    "cyl-long": [2, 6, 7, 8, 10, 13],
    "cyl-far": [0, 1, 2, 4, 6, 7],
}


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
    """Gaussian elimination with partial pivoting (Decimal entries)."""
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


def norm2(v):
    return sum(dec(x) * dec(x) for x in v).sqrt()


# ------------------------------------------------------------------ geometry


def dist_grad(pt, p, geo):
    """Distance of pt to the circle centre (geo 2) or cylinder axis (geo 4) of p, and its gradient
    in the geometric parameters, in Decimal."""
    if geo == 2:
        dx, dy = dec(pt[0]) - p[0], dec(pt[1]) - p[1]
        d = (dx * dx + dy * dy).sqrt()
        return d, [-dx / d, -dy / d]
    x0, y0, a, b = p[0], p[1], p[2], p[3]
    wx, wy, wz = dec(pt[0]) - x0, dec(pt[1]) - y0, dec(pt[2])
    cx, cy, cz = wy - wz * b, wz * a - wx, wx * b - wy * a
    nc = (cx * cx + cy * cy + cz * cz).sqrt()
    vv = a * a + b * b + 1
    nv = vv.sqrt()
    k = nc * nv
    g = [
        (cy - b * cz) / k,
        (-cx + a * cz) / k,
        (wz * cy - wy * cz) / k - nc * a / (nv * vv),
        (-wz * cx + wx * cz) / k - nc * b / (nv * vv),
    ]
    return nc / nv, g


def system(pts, labels, p, geo):
    """Residuals d_i - r_side and Jacobian of the contact rows at p."""
    jac, res = [], []
    for i, side in labels:
        d, g = dist_grad(pts[i], p, geo)
        res.append(d - p[geo + side])
        row = g + [Decimal(0), Decimal(0)]
        row[geo + side] = Decimal(-1)
        jac.append(row)
    return res, jac


def newton_step(pts, labels, p, geo):
    res, jac = system(pts, labels, p, geo)
    return solve(jac, res)


def polish(pts, labels, p0, geo):
    """Newton's method on d_i(p) = r_side for the contact rows (n equations, n unknowns)."""
    p = [dec(v) for v in p0]
    for _ in range(40):
        res, jac = system(pts, labels, p, geo)
        step = solve(jac, res)
        if step is None:
            return None
        p = [x - s for x, s in zip(p, step)]
        if max(abs(r) for r in res) < Decimal(10) ** -75:
            return p
    return None


def verify(pts, labels, p, geo):
    """Strict local minimum check: multipliers >= 1e-3 and every other row inside by margin."""
    n = geo + 2
    width = p[geo + 1] - p[geo]
    if width <= 0:
        return None
    rows = []
    for i, side in labels:
        d, g = dist_grad(pts[i], p, geo)
        if abs(d - p[geo + side]) > Decimal(10) ** -60:
            return None
        if side == 0:
            rows.append([-v for v in g] + [Decimal(1), Decimal(0)])
        else:
            rows.append(g + [Decimal(0), Decimal(-1)])
    gt = [[rows[r][k] for r in range(n)] for k in range(n)]
    mu = solve(gt, [Decimal(0)] * geo + [Decimal(1), Decimal(-1)])
    if mu is None or min(mu) < Decimal("1e-3"):
        return None
    contact = {i for i, _ in labels}
    for i, pt in enumerate(pts):
        if i in contact:
            continue
        d, _ = dist_grad(pt, p, geo)
        if d - p[geo] <= width / 1000 or p[geo + 1] - d <= width / 1000:
            return None
    return width


def frame(a, b):
    """Orthonormal vectors e1, e2 perpendicular to (a, b, 1), and the unit axis vector."""
    v = (dec(a), dec(b), Decimal(1))
    nv = (v[0] ** 2 + v[1] ** 2 + 1).sqrt()
    u = tuple(x / nv for x in v)
    e1 = (u[2], Decimal(0), -u[0])
    n1 = (e1[0] ** 2 + e1[2] ** 2).sqrt()
    e1 = (e1[0] / n1, Decimal(0), e1[2] / n1)
    e2 = (u[1] * e1[2] - u[2] * e1[1], u[2] * e1[0] - u[0] * e1[2], u[0] * e1[1] - u[1] * e1[0])
    return u, e1, e2


# ------------------------------------------------------------------ problems


def make_arc(k, far):
    attempt = 0
    while True:
        rng = Random(f"recomp-floor-v1-arc-{'far' if far else 'near'}-{k}-{attempt}")
        attempt += 1
        radius = Fraction(rng.randint(100, 500), 10)
        span = Fraction(rng.randint(3, 10), 360)
        if far:
            cx, cy = Fraction(rng.randint(2900, 3100), 10), Fraction(rng.randint(-3100, -2900), 10)
        else:
            cx, cy = Fraction(rng.randint(-50, 50), 10), Fraction(rng.randint(-50, 50), 10)
        h = radius * Fraction(rng.randint(5, 20), 10000)
        phase = rnd(rng)
        n = 12
        plan = []
        for j in range(4):
            at = Fraction(j, 3) + (rnd(rng) - Fraction(1, 2)) / 20
            at = min(max(at, Fraction(0)), Fraction(1))
            plan.append((at, h if j % 2 == 0 else -h))
        for _ in range(n - 4):
            plan.append((Fraction(1, 20) + rnd(rng) * Fraction(9, 10), Fraction(2, 5) * h * (2 * rnd(rng) - 1)))
        order = list(range(n))
        rng.shuffle(order)
        pts = [None] * n
        for slot, (at, err) in zip(order, plan):
            s, c = dsin_cos(2 * PI * dec(phase + span * at))
            r = dec(radius + err)
            pts[slot] = (fl(dec(cx) + r * c), fl(dec(cy) + r * s))
        labels = sorted((order[j], 1 if j % 2 == 0 else 0) for j in range(4))
        p = polish(pts, labels, [cx, cy, radius - h, radius + h], 2)
        if p is None:
            continue
        width = verify(pts, labels, p, 2)
        if width is None:
            continue
        return pts, labels, p, width


def cyl_points(rng, n, radius, h, height_lo, length, x0, y0, a, b):
    u, e1, e2 = frame(a, b)
    plan = []
    for j in range(6):
        deg = 60 * j + rng.randint(-20, 20)
        plan.append((Fraction(deg, 360), h if j % 2 == 0 else -h, height_lo + length * rnd(rng)))
    for _ in range(n - 6):
        plan.append((rnd(rng), Fraction(2, 5) * h * (2 * rnd(rng) - 1), height_lo + length * rnd(rng)))
    order = list(range(n))
    rng.shuffle(order)
    pts = [None] * n
    for slot, (turn, err, t) in zip(order, plan):
        s, c = dsin_cos(2 * PI * dec(turn))
        r = dec(radius + err)
        base = [
            dec(x0) + r * (c * e1[0] + s * e2[0]),
            dec(y0) + r * (c * e1[1] + s * e2[1]),
            r * (c * e1[2] + s * e2[2]),
        ]
        tt = (dec(t) - base[2]) / u[2]
        pts[slot] = tuple(fl(base[i] + tt * u[i]) for i in range(3))
    labels = sorted((order[j], 1 if j % 2 == 0 else 0) for j in range(6))
    return pts, labels


def cyl_start(pts, labels, axis):
    p = [dec(fl(v)) for v in axis]
    inner = next(i for i, s in labels if s == 0)
    outer = next(i for i, s in labels if s == 1)
    return [fl(v) for v in p] + [fl(dist_grad(pts[inner], p, 4)[0]), fl(dist_grad(pts[outer], p, 4)[0])]


def make_cylinder(tag, k, far):
    attempt = 0
    while True:
        rng = Random(f"recomp-floor-v1-{tag}-{k}-{attempt}")
        attempt += 1
        if far:
            radius = Fraction(rng.randint(20, 50), 10)
            length = radius * Fraction(rng.randint(15, 30), 10)
            x0, y0 = Fraction(rng.randint(2900, 3100), 10), Fraction(rng.randint(-3100, -2900), 10)
            height_lo = Fraction(300)
        else:
            radius = Fraction(rng.randint(20, 50), 10)
            length = radius * Fraction(rng.randint(60, 120))
            x0, y0 = Fraction(rng.randint(-50, 50), 10), Fraction(rng.randint(-50, 50), 10)
            height_lo = Fraction(0)
        a, b = Fraction(rng.randint(-300, 300), 1000), Fraction(rng.randint(-300, 300), 1000)
        h = radius * Fraction(rng.randint(5, 20), 10000)
        pts, labels = cyl_points(rng, 14, radius, h, height_lo, length, x0, y0, a, b)
        p = polish(pts, labels, [x0, y0, a, b, radius - h, radius + h], 4)
        if p is None:
            continue
        width = verify(pts, labels, p, 4)
        if width is None:
            continue
        return pts, cyl_start(pts, labels, (x0, y0, a, b)), labels, p, width


def make_unconverged():
    """A long cylinder with a displaced start and a two-step budget (see the module docs)."""
    pts, start, labels, p, width = make_cylinder("unconv", 0, False)
    radius = (p[4] + p[5]) / 2
    axis = [dec(start[0]) + radius / 200, dec(start[1]), dec(start[2]) + Decimal("0.0005"), dec(start[3])]
    start = cyl_start(pts, labels, axis)
    q = [dec(v) for v in start]
    last = None
    for _ in range(2):
        last = newton_step(pts, labels, q, 4)
        q = [x - s for x, s in zip(q, last)]
    corr = newton_step(pts, labels, q, 4)
    s, c = norm2(last), norm2(corr)
    allowance = 4 * dec(U) * norm2(q) + 2 * c
    assert s > 1000 * allowance, (s, allowance)
    return pts, start, labels, p, width


# ------------------------------------------------------------------ output


def fmt_labels(labels):
    return " ".join(f"{i}:{s}" for i, s in labels)


def emit(out, name, kind, pts, start, labels, params, value, gn_steps=None):
    out.append(f"case {name} kind {kind}")
    out.append("points " + " ".join(hx(c) for p in pts for c in p))
    if start is not None:
        out.append("start " + " ".join(hx(v) for v in start))
    out.append("set " + fmt_labels(labels))
    out.append("params " + " ".join(hx(fl(v)) for v in params))
    out.append(f"value {hx(fl(value))} {digits50(value)}")
    if gn_steps is None:
        out.append("expect certify")
    else:
        out.append("expect no_convergence")
        out.append(f"gn_steps {gn_steps}")
    out.append("end")


def render():
    out = [
        "# KRN.SOLV.CERT.RECOMP fixture recomp-floor-v1 (CC-BY-4.0). Generated by make_cases.py; do not edit.",
        "# Minimum-zone partial arcs and long or far cylinders (90-digit references) whose plain-f64",
        "# Gauss-Newton steps stall at the rounding floor, and one unconverged case. See make_cases.py.",
    ]
    for k in KEEP["arc"]:
        pts, labels, p, width = make_arc(k, False)
        emit(out, f"arc-{k:02d}", "arc", pts, None, labels, p, width)
    for k in KEEP["arc-far"]:
        pts, labels, p, width = make_arc(k, True)
        emit(out, f"arc-far-{k:02d}", "arc", pts, None, labels, p, width)
    for k in KEEP["cyl-long"]:
        pts, start, labels, p, width = make_cylinder("long", k, False)
        emit(out, f"cyl-long-{k:02d}", "cyl", pts, start, labels, p, width)
    for k in KEEP["cyl-far"]:
        pts, start, labels, p, width = make_cylinder("far", k, True)
        emit(out, f"cyl-far-{k:02d}", "cyl", pts, start, labels, p, width)
    pts, start, labels, p, width = make_unconverged()
    emit(out, "cyl-unconv", "cyl", pts, start, labels, p, width, gn_steps=2)
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
