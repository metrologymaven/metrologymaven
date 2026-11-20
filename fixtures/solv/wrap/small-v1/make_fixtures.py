#!/usr/bin/env python3
"""Independent reference for KRN.SOLV.CONIC.WRAP AC-1 (CC-BY-4.0 project data).

Writes fixtures/solv/wrap/small-v1/problems.txt: three small conic problems in the wrapper's
canonical form (minimise 1/2 x'Px + q'x subject to Ax + s = b, s in K) and their optimal
objectives, computed here offline with exact rational arithmetic (Python's fractions) and,
for the one irrational optimum, a 60-digit decimal square root. Nothing here runs or reads
the Rust code or any solver.

Every problem datum is a dyadic rational drawn from a seeded splitmix64 stream, so the f64
values in the fixture equal the rational data exactly and the optima below are exact optima
of the very problems the Rust test solves.

1. straightness-lp (LP, seed 1): minimum-zone straightness of 10 points (x_i = i,
   y_i = k/1024 with k in [-512, 512]). Variables (a, b, t), minimise t subject to
   |y_i - a - b x_i| <= t. Optimum: the Chebyshev (minimax) line. The half range
   h(b) = (max_i(y_i - b x_i) - min_i(y_i - b x_i)) / 2 is convex and piecewise linear in b
   with breakpoints at the slopes of point pairs, so t* = min over pair slopes of h(b).
2. box-qp (QP, seed 5): projection of c (5 components, k/64 in [-4, 4]) onto the box
   [lo, hi] (lo_j = -1 - k/16, hi_j = 1 + k/16, k in [0, 8]). P = I, q = -c, A = [I; -I],
   b = [hi; -lo]. Optimum x* = clip(c, lo, hi), objective 1/2 x*'x* - c'x* (the constant
   1/2 c'c of the distance is not part of the canonical objective).
3. circle-socp (SOCP, seed 3): minimum circumscribed circle of 8 points (coordinates k/256,
   k in [-1024, 1024]). Variables (cx, cy, r), minimise r subject to
   (r, px - cx, py - cy) in SOC(3) for each point. Optimum: the smallest enclosing circle,
   found by brute force over every pair (diameter circle) and every non-collinear triple
   (circumcircle) whose circle contains all points, compared exactly as squared radii.

Format: `problem <name> class <lp|qp|socp>` starts a block, `end` closes it. Floats are
IEEE-754 bit patterns in hex; matrices are compressed sparse column (`*_colptr`,
`*_rowval`, `*_nzval`, sorted rows, explicit zeros omitted); `p none` means no P.
`cones` lists `<zero|nonneg|soc> <dim>` pairs in order. `objective_exact` is the exact
optimum (`p/q` or `sqrt(p/q)`), `objective_decimal` 40 significant digits of it and
`objective_f64` its correctly rounded f64.

Run:  python fixtures/solv/wrap/small-v1/make_fixtures.py [--check]
"""

import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from itertools import combinations
from pathlib import Path

OUT = Path(__file__).with_name("problems.txt")
MASK = (1 << 64) - 1


class SplitMix64:
    def __init__(self, seed):
        self.state = seed & MASK

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def int_in(self, lo, hi):
        """Integer in [lo, hi] (modulo bias is irrelevant for test data)."""
        return lo + self.next() % (hi - lo + 1)


def hexf(v):
    f = float(v)
    if Fraction(f) != Fraction(v):
        raise ValueError(f"{v} is not exactly representable as f64")
    return "%016x" % struct.unpack("<Q", struct.pack("<d", f))[0]


def csc(dense, nrows, ncols):
    colptr, rowval, nzval = [0], [], []
    for j in range(ncols):
        for i in range(nrows):
            if dense[i][j] != 0:
                rowval.append(i)
                nzval.append(dense[i][j])
        colptr.append(len(rowval))
    return colptr, rowval, nzval


def block(name, cls, n, p_dense, q, a_dense, b, cones, exact, decimal_value, f64_value):
    m = len(b)
    lines = [f"problem {name} class {cls}", f"n {n}"]
    if p_dense is None:
        lines.append("p none")
    else:
        cp, rv, nz = csc(p_dense, n, n)
        lines.append("p_colptr " + " ".join(map(str, cp)))
        lines.append("p_rowval " + " ".join(map(str, rv)))
        lines.append("p_nzval " + " ".join(hexf(v) for v in nz))
    lines.append("q " + " ".join(hexf(v) for v in q))
    cp, rv, nz = csc(a_dense, m, n)
    lines.append(f"m {m}")
    lines.append("a_colptr " + " ".join(map(str, cp)))
    lines.append("a_rowval " + " ".join(map(str, rv)))
    lines.append("a_nzval " + " ".join(hexf(v) for v in nz))
    lines.append("b " + " ".join(hexf(v) for v in b))
    lines.append("cones " + " ".join(f"{k} {d}" for k, d in cones))
    lines.append(f"objective_exact {exact}")
    lines.append(f"objective_decimal {decimal_value}")
    lines.append(f"objective_f64 {f64_value}")
    lines.append("end")
    return lines


def straightness_lp():
    rng = SplitMix64(1)
    xs = [Fraction(i) for i in range(10)]
    ys = [Fraction(rng.int_in(-512, 512), 1024) for _ in range(10)]

    def half_range(slope):
        r = [y - slope * x for x, y in zip(xs, ys)]
        return (max(r) - min(r)) / 2

    slopes = {(ys[j] - ys[i]) / (xs[j] - xs[i]) for i, j in combinations(range(10), 2)}
    t_star = min(half_range(s) for s in sorted(slopes))
    # variables (a, b, t); rows 2i:  -a - b x_i - t <= -y_i ; 2i+1: a + b x_i - t <= y_i
    a_dense, b = [], []
    for x, y in zip(xs, ys):
        a_dense.append([Fraction(-1), -x, Fraction(-1)])
        b.append(-y)
        a_dense.append([Fraction(1), x, Fraction(-1)])
        b.append(y)
    q = [Fraction(0), Fraction(0), Fraction(1)]
    dec = Decimal(t_star.numerator) / Decimal(t_star.denominator)
    return block("straightness-lp", "lp", 3, None, q, a_dense, b, [("nonneg", 20)],
                 f"{t_star.numerator}/{t_star.denominator}", f"{dec:.39e}", hexf_round(t_star))


def box_qp():
    rng = SplitMix64(5)
    n = 5
    c = [Fraction(rng.int_in(-256, 256), 64) for _ in range(n)]
    lo = [Fraction(-1) - Fraction(rng.int_in(0, 8), 16) for _ in range(n)]
    hi = [Fraction(1) + Fraction(rng.int_in(0, 8), 16) for _ in range(n)]
    xs = [min(max(cj, lj), hj) for cj, lj, hj in zip(c, lo, hi)]
    obj = sum(x * x / 2 - cj * x for x, cj in zip(xs, c))
    p_dense = [[Fraction(1) if i == j else Fraction(0) for j in range(n)] for i in range(n)]
    a_dense = [[Fraction(1) if i == j else Fraction(0) for j in range(n)] for i in range(n)]
    a_dense += [[Fraction(-1) if i == j else Fraction(0) for j in range(n)] for i in range(n)]
    b = hi + [-v for v in lo]
    q = [-v for v in c]
    clipped = sum(1 for x, cj in zip(xs, c) if x != cj)
    if clipped == 0 or clipped == n:
        raise ValueError("box-qp seed should clip some but not all components")
    dec = Decimal(obj.numerator) / Decimal(obj.denominator)
    return block("box-qp", "qp", n, p_dense, q, a_dense, b, [("nonneg", 2 * n)],
                 f"{obj.numerator}/{obj.denominator}", f"{dec:.39e}", hexf_round(obj))


def enclosing_circles(pts):
    """Yield (centre, squared radius) of every pair and non-collinear triple circle."""
    for (ax, ay), (bx, by) in combinations(pts, 2):
        cx, cy = (ax + bx) / 2, (ay + by) / 2
        yield (cx, cy), (ax - cx) ** 2 + (ay - cy) ** 2
    for (ax, ay), (bx, by), (cx_, cy_) in combinations(pts, 3):
        d = 2 * (ax * (by - cy_) + bx * (cy_ - ay) + cx_ * (ay - by))
        if d == 0:
            continue
        a2, b2, c2 = ax * ax + ay * ay, bx * bx + by * by, cx_ * cx_ + cy_ * cy_
        ux = (a2 * (by - cy_) + b2 * (cy_ - ay) + c2 * (ay - by)) / d
        uy = (a2 * (cx_ - bx) + b2 * (ax - cx_) + c2 * (bx - ax)) / d
        yield (ux, uy), (ax - ux) ** 2 + (ay - uy) ** 2


def circle_socp():
    rng = SplitMix64(3)
    pts = [(Fraction(rng.int_in(-1024, 1024), 256), Fraction(rng.int_in(-1024, 1024), 256))
           for _ in range(8)]
    best = None
    for (ux, uy), r2 in enclosing_circles(pts):
        if all((px - ux) ** 2 + (py - uy) ** 2 <= r2 for px, py in pts):
            if best is None or r2 < best:
                best = r2
    getcontext().prec = 60
    r = (Decimal(best.numerator) / Decimal(best.denominator)).sqrt()
    # variables (cx, cy, r); per point rows (s0, s1, s2) = (r, px - cx, py - cy)
    a_dense, b = [], []
    for px, py in pts:
        a_dense.append([Fraction(0), Fraction(0), Fraction(-1)])
        b.append(Fraction(0))
        a_dense.append([Fraction(1), Fraction(0), Fraction(0)])
        b.append(px)
        a_dense.append([Fraction(0), Fraction(1), Fraction(0)])
        b.append(py)
    q = [Fraction(0), Fraction(0), Fraction(1)]
    f64 = "%016x" % struct.unpack("<Q", struct.pack("<d", float(r)))[0]
    return block("circle-socp", "socp", 3, None, q, a_dense, b, [("soc", 3)] * 8,
                 f"sqrt({best.numerator}/{best.denominator})", f"{r:.39e}", f64)


def hexf_round(v):
    """Correctly rounded f64 of a rational, as hex bits."""
    return "%016x" % struct.unpack("<Q", struct.pack("<d", float(v)))[0]


def render():
    out = [
        "# KRN.SOLV.CONIC.WRAP AC-1 fixture (CC-BY-4.0), written by make_fixtures.py; do not edit.",
        "# Canonical form: minimise 1/2 x'Px + q'x subject to Ax + s = b, s in K.",
        "# Optima are exact (rational arithmetic; one 60-digit square root); see the script.",
    ]
    for problem in (straightness_lp(), box_qp(), circle_socp()):
        out += problem
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
