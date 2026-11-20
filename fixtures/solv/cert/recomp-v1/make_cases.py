#!/usr/bin/env python3
"""Independent reference for KRN.SOLV.CERT.RECOMP (CC-BY-4.0 project data).

Writes fixtures/solv/cert/recomp-v1/cases.txt. Every expected value is decided here with
exact rational arithmetic (fractions.Fraction of exact dyadic data) or, for the square roots
of the circle radii, with integer square roots giving 60 correct digits. Nothing here runs or
reads the Rust code, any solver or the platform libm: the only float operations are
float(Fraction) (correctly rounded by integer arithmetic) and float(str) of a 60-digit
decimal (correctly rounded by the parser).

Problems (all data dyadic, so the f64 data equal the rational data):

- straightness (10): points (x_i, y_i), parameters p = (a, b, t) of the zone
  |y - a x - b| <= t/2. Row (i, 1) is the upper bound, (i, 0) the lower bound.
- flatness (10): points (x_i, y_i, z_i), p = (a, b, c, t) of |z - a x - b y - c| <= t/2.
- circle (10): points (x_i, y_i), p = (cx, cy, r_in, r_out) of
  r_in <= |p_i - c| <= r_out. Row (i, 0) is the inner bound, (i, 1) the outer.

For each problem the minimum-zone optimum is found by brute force, independently of any
active-set logic:

- straightness and flatness are LPs in (p): every basis of 3 (4) rows of distinct points is
  solved exactly; the feasible basic solution with the least t is the optimum (an LP optimum
  is attained at a vertex). The script asserts it is unique and nondegenerate (no other row
  tight), so its basis is the active set.
- circle: the minimum-zone centre is a vertex of the overlay of the farthest- and nearest-point
  Voronoi diagrams, so it is the circumcentre of three points or the intersection of the
  perpendicular bisectors of two disjoint point pairs. Every such centre is formed exactly;
  for each, the zone sqrt(max d^2) - sqrt(min d^2) is evaluated to 60 digits and the least is
  taken. The script asserts a unique minimiser with exactly four extreme points (2 outer and 2
  inner, or 3 and 1), which form the active set. The centre is rational (exact), the radii are
  square roots of rationals (60 digits).

`params` / `value` are the exact optimum rounded to the nearest f64; `exact` gives each
parameter as a fraction (linear) or a 50-digit decimal (circle radii and value).

Error cases (AC-3), each decided exactly:
- rank-collinear: three points on one vertical line x = 2 (collinear along the deviation
  axis) in a straightness zone: the active-set matrix rows (-x, -1, -+1/2) have exact rank 2.
- ill-near-degenerate: three points at x = 0, 2^-46, 2^-45: full exact rank, exact
  infinity-norm condition number about 1e14 (recorded), above the 1e12 threshold by a wide
  margin, so any condition estimate within a factor 10 of it exceeds the threshold.
"""

import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from itertools import combinations
from math import isqrt
from pathlib import Path
import struct

OUT = Path(__file__).with_name("cases.txt")
getcontext().prec = 80
DIGITS = 60

MASK = (1 << 64) - 1


class SplitMix:
    def __init__(self, seed):
        self.s = seed & MASK

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def dyadic(self, lo, hi, bits):
        """A dyadic rational k / 2^bits uniformly in [lo, hi] (lo, hi integers)."""
        span = (hi - lo) << bits
        return Fraction(lo) + Fraction(self.next() % (span + 1), 1 << bits)


def hexf(x):
    """Hex bits of the nearest double (float() of a Fraction is correctly rounded)."""
    return struct.pack(">d", float(x)).hex()


def exact_hex(x):
    """Hex of a Fraction that must be exactly representable."""
    f = float(x)
    assert Fraction(f) == x, x
    return struct.pack(">d", f).hex()


def solve(a, b):
    """Exact Gaussian elimination; None if singular."""
    n = len(a)
    m = [list(r) + [bb] for r, bb in zip(a, b)]
    for c in range(n):
        piv = next((r for r in range(c, n) if m[r][c] != 0), None)
        if piv is None:
            return None
        m[c], m[piv] = m[piv], m[c]
        for r in range(n):
            if r != c and m[r][c] != 0:
                f = m[r][c] / m[c][c]
                m[r] = [x - f * y for x, y in zip(m[r], m[c])]
    return [m[i][n] / m[i][i] for i in range(n)]


def rank(a):
    m = [list(r) for r in a]
    rk, rows, cols = 0, len(m), len(m[0])
    for c in range(cols):
        piv = next((r for r in range(rk, rows) if m[r][c] != 0), None)
        if piv is None:
            continue
        m[rk], m[piv] = m[piv], m[rk]
        for r in range(rows):
            if r != rk and m[r][c] != 0:
                f = m[r][c] / m[rk][c]
                m[r] = [x - f * y for x, y in zip(m[r], m[rk])]
        rk += 1
    return rk


def inverse(a):
    n = len(a)
    cols = [solve(a, [Fraction(int(i == j)) for i in range(n)]) for j in range(n)]
    return [[cols[j][i] for j in range(n)] for i in range(n)]


def norm_inf(a):
    return max(sum(abs(x) for x in r) for r in a)


def sqrt_dec(q):
    """sqrt of a nonnegative Fraction to DIGITS significant digits (truncated, then the
    Decimal carries the extra digits so rounding to 50 or to a double is exact enough)."""
    scale = 10 ** (2 * (DIGITS + 10))
    v = isqrt(q.numerator * scale // q.denominator)
    return Decimal(v) / Decimal(10 ** (DIGITS + 10))


def fmt_dec(d):
    getcontext().prec = 50
    s = str(+d)
    getcontext().prec = 80
    return s


def frac_str(q):
    return f"{q.numerator}/{q.denominator}"


# ---------------------------------------------------------------- linear zones


def lin_rows(pt, side, dim):
    """Row of the active-set system for point pt and side (1 upper, 0 lower): the residual
    is z - sum(coef * coord) - c - sgn t/2 = J p + z with J = (-coords, -1, -sgn/2)."""
    sgn = 1 if side == 1 else -1
    return [-c for c in pt[:dim]] + [Fraction(-1), Fraction(-sgn, 2)], pt[dim]


def lin_optimum(pts, dim):
    """Brute-force minimum zone of an LP zone in dim coordinates (dim 1: straightness)."""
    n = dim + 2
    labels = [(i, s) for i in range(len(pts)) for s in (0, 1)]
    best = []
    for pts_idx in combinations(range(len(pts)), n):
        for sides in range(1 << n):
            rows = [(i, (sides >> k) & 1) for k, i in enumerate(pts_idx)]
            a, b = [], []
            for i, s in rows:
                r, z = lin_rows(pts[i], s, dim)
                a.append(r)
                b.append(-z)
            p = solve(a, b)
            if p is None or p[-1] < 0:
                continue
            ok = True
            for q in pts:
                dev = q[dim] - sum(c * x for c, x in zip(p[:dim], q[:dim])) - p[dim]
                if abs(dev) > p[-1] / 2:
                    ok = False
                    break
            if ok:
                best.append((p[-1], rows, p))
    t = min(b[0] for b in best)
    winners = [b for b in best if b[0] == t]
    assert len(winners) == 1, "optimum not unique"
    _, rows, p = winners[0]
    tight = []
    for i, q in enumerate(pts):
        dev = q[dim] - sum(c * x for c, x in zip(p[:dim], q[:dim])) - p[dim]
        for s, sgn in ((0, -1), (1, 1)):
            if dev == sgn * p[-1] / 2:
                tight.append((i, s))
    assert sorted(tight) == sorted(rows), "degenerate optimum"
    assert all(l in labels for l in rows)
    return sorted(rows), p


def gen_straightness(rng):
    while True:
        n = 7
        x0 = rng.dyadic(-50, 50, 6)
        step = rng.dyadic(1, 8, 2)
        a0 = rng.dyadic(-2, 2, 8)
        b0 = rng.dyadic(-20, 20, 8)
        amp = [Fraction(1, 64), Fraction(1, 8), Fraction(1)][rng.next() % 3]
        pts = []
        for i in range(n):
            x = x0 + i * step
            e = (rng.dyadic(-1, 1, 10)) * amp
            pts.append((x, a0 * x + b0 + e))
        try:
            rows, p = lin_optimum(pts, 1)
        except AssertionError:
            continue
        return pts, rows, p


def gen_flatness(rng):
    while True:
        n = 8
        a0 = rng.dyadic(-1, 1, 8)
        b0 = rng.dyadic(-1, 1, 8)
        c0 = rng.dyadic(-30, 30, 6)
        amp = [Fraction(1, 32), Fraction(1, 4)][rng.next() % 2]
        pts = []
        for _ in range(n):
            x = rng.dyadic(-40, 40, 4)
            y = rng.dyadic(-40, 40, 4)
            e = rng.dyadic(-1, 1, 10) * amp
            pts.append((x, y, a0 * x + b0 * y + c0 + e))
        try:
            rows, p = lin_optimum(pts, 2)
        except AssertionError:
            continue
        return pts, rows, p


# ---------------------------------------------------------------- circle


def d2(p, c):
    return (p[0] - c[0]) ** 2 + (p[1] - c[1]) ** 2


def bisector(p, q):
    """Row (A, B), rhs C of 2 c . (q - p) = |q|^2 - |p|^2."""
    return [2 * (q[0] - p[0]), 2 * (q[1] - p[1])], q[0] ** 2 + q[1] ** 2 - p[0] ** 2 - p[1] ** 2


def centre_of(eqs):
    a = [e[0] for e in eqs]
    b = [e[1] for e in eqs]
    s = solve(a, b)
    return None if s is None else (s[0], s[1])


def circle_optimum(pts):
    n = len(pts)
    centres = set()
    for t in combinations(range(n), 3):
        c = centre_of([bisector(pts[t[0]], pts[t[1]]), bisector(pts[t[0]], pts[t[2]])])
        if c is not None:
            centres.add(c)
    for o in combinations(range(n), 2):
        rest = [i for i in range(n) if i not in o]
        for i in combinations(rest, 2):
            c = centre_of([bisector(pts[o[0]], pts[o[1]]), bisector(pts[i[0]], pts[i[1]])])
            if c is not None:
                centres.add(c)
    scored = []
    for c in sorted(centres):
        ds = [d2(p, c) for p in pts]
        hi, lo = max(ds), min(ds)
        zone = sqrt_dec(hi) - sqrt_dec(lo)
        scored.append((zone, c, ds, hi, lo))
    scored.sort(key=lambda s: s[0])
    best, second = scored[0], scored[1]
    assert second[0] - best[0] > Decimal(10) ** -40, "circle optimum not unique"
    zone, c, ds, hi, lo = best
    outer = [i for i, d in enumerate(ds) if d == hi]
    inner = [i for i, d in enumerate(ds) if d == lo]
    assert len(outer) + len(inner) == 4 and len(outer) >= 1 and len(inner) >= 1, "degenerate circle"
    rows = sorted([(i, 1) for i in outer] + [(i, 0) for i in inner])
    return rows, c, hi, lo


def gen_circle(rng):
    while True:
        n = 9
        cx = rng.dyadic(-60, 60, 4)
        cy = rng.dyadic(-60, 60, 4)
        r = rng.dyadic(5, 40, 2)
        amp = [Fraction(1, 128), Fraction(1, 16)][rng.next() % 2]
        pts = []
        for k in range(n):
            # rational point on the unit circle from the parameter t, angles spread over the
            # whole circle (t = tan of half the angle), then a radial perturbation, snapped
            # to the dyadic grid 2^-12 exactly.
            t = Fraction(-3) + Fraction(6 * k, n - 1) + rng.dyadic(0, 1, 6) / 8
            u = ((1 - t * t) / (1 + t * t), 2 * t / (1 + t * t))
            rr = r + rng.dyadic(-1, 1, 10) * amp
            g = 1 << 12
            x = Fraction(round((cx + rr * u[0]) * g), g)
            y = Fraction(round((cy + rr * u[1]) * g), g)
            pts.append((x, y))
        if len(set(pts)) != n:
            continue
        try:
            rows, c, hi, lo = circle_optimum(pts)
        except AssertionError:
            continue
        return pts, rows, c, hi, lo


# ---------------------------------------------------------------- output


def labels(rows):
    return " ".join(f"{i}:{s}" for i, s in rows)


def points_line(pts):
    return " ".join(exact_hex(v) for p in pts for v in p)


def render():
    out = [
        "# KRN.SOLV.CERT.RECOMP fixture recomp-v1 (CC-BY-4.0). Generated by make_cases.py; do not edit.",
        "# Minimum-zone straightness, flatness (exact rational optima) and circle (60-digit radii)",
        "# sets with their exact optima, and the AC-3 error cases. See make_cases.py.",
    ]
    rng = SplitMix(0x5EC7_2EC0_11_2026)
    for k in range(10):
        pts, rows, p = gen_straightness(rng)
        out += [f"case strt-{k:02d} kind straightness", f"points {points_line(pts)}", f"set {labels(rows)}"]
        out.append("exact " + " ".join(frac_str(v) for v in p))
        out.append("params " + " ".join(hexf(v) for v in p))
        out.append(f"value {hexf(p[-1])}")
        out.append("end")
    for k in range(10):
        pts, rows, p = gen_flatness(rng)
        out += [f"case flat-{k:02d} kind flatness", f"points {points_line(pts)}", f"set {labels(rows)}"]
        out.append("exact " + " ".join(frac_str(v) for v in p))
        out.append("params " + " ".join(hexf(v) for v in p))
        out.append(f"value {hexf(p[-1])}")
        out.append("end")
    for k in range(10):
        pts, rows, c, hi, lo = gen_circle(rng)
        r_out, r_in = sqrt_dec(hi), sqrt_dec(lo)
        out += [f"case circ-{k:02d} kind circle", f"points {points_line(pts)}", f"set {labels(rows)}"]
        out.append(f"exact {frac_str(c[0])} {frac_str(c[1])} {fmt_dec(r_in)} {fmt_dec(r_out)}")
        out.append(f"exact_value {fmt_dec(r_out - r_in)}")
        out.append("params " + " ".join(hexf(v) for v in [c[0], c[1]]) + " " + " ".join(hexf(float(str(v))) for v in [r_in, r_out]))
        out.append(f"value {hexf(float(str(r_out - r_in)))}")
        out.append("end")

    # AC-3 error cases (straightness problems, hand-built, decided exactly).
    pts = [(Fraction(2), Fraction(1, 2)), (Fraction(2), Fraction(-1, 4)), (Fraction(2), Fraction(1)),
           (Fraction(5), Fraction(3, 8)), (Fraction(7), Fraction(-1, 8))]
    rows = [(0, 1), (1, 0), (2, 1)]
    a = [lin_rows(pts[i], s, 1)[0] for i, s in rows]
    rk = rank(a)
    assert rk == 2
    out += ["case rank-collinear kind error", f"points {points_line(pts)}", f"set {labels(rows)}",
            f"expect RankDeficient {rk}", "end"]

    d = Fraction(1, 1 << 46)
    pts = [(Fraction(0), Fraction(1, 4)), (d, Fraction(-1, 4)), (2 * d, Fraction(1, 4)),
           (Fraction(1), Fraction(0)), (Fraction(3), Fraction(1, 8))]
    rows = [(0, 1), (1, 0), (2, 1)]
    a = [lin_rows(pts[i], s, 1)[0] for i, s in rows]
    assert rank(a) == 3
    kappa = norm_inf(a) * norm_inf(inverse(a))
    assert Fraction(10) ** 13 < kappa < Fraction(10) ** 16, float(kappa)
    out += ["case ill-near-degenerate kind error", f"points {points_line(pts)}", f"set {labels(rows)}",
            "expect IllConditioned", f"kappa_inf {float(kappa):.6e}", "end"]
    return "\n".join(out) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        if OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT} is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
