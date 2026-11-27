#!/usr/bin/env python3
"""Independent fixture generator and oracle for KRN.ENV.MIC.CIRSPH (maximum inscribed circle
and sphere). CC-BY-4.0 project data.

Writes fixtures/fit/mic-cirsph-v1/cases.txt. With --check, compares instead of writing and exits
1 on any difference. Stdlib only, no shared code with crates/fid-fit, no third-party data;
nothing here runs the Rust code.

# Point generator (pinned; the Rust test regenerates the points and checks their digest)

Every case draws from SplitMix64 (Steele, Lea and Flood 2014) with the case's seed;
`uniform() = (next_u64() >> 11) * 2^-53`. Every value below is one IEEE-754 binary64 operation
in the order written (CPython floats are binary64 and never fused), and `sqrt` is the correctly
rounded square root, so the bytes are the same in any language that follows the rules.

- **Direction** (`dim` 2 or 3): draw `x_j = 2 * uniform() - 1` for each component, `n2 =
  ((x_0 * x_0) + x_1 * x_1) (+ x_2 * x_2)`; reject unless `1/16 <= n2 <= 1`; `s = sqrt(n2)`,
  `u_j = x_j / s`.
- **Lobed form** `sf`: circle `t = u_0 * u_0; sf = 1 + ((4 * t) - 3) * u_0` (that is
  `1 + cos(3 theta)`, in `[0, 2]`, zero at 60, 180 and 300 degrees); sphere `sf = 1 + ((K * u_0)
  * u_1) * u_2` with `K = 5.196152422706632` (about `3 sqrt(3)`, so `sf` is about `[0, 2]`, zero
  at the four tetrahedral directions with `u_0 u_1 u_2 < 0`).
- **Hole / socket point**: `w = uniform()`, `rho = ((rA + A * sf) + g * (1 + w))` with `rA = R
  - A`, point `c_j + rho * u_j` (circle: `z = c_2` for every point).
- **Valleys** (the inscribed contacts): circle directions `(0.5, h)`, `(-1, 0)`, `(0.5, -h)` with
  `h = sqrt(0.75)`; sphere directions `(s, s, -s)`, `(s, -s, s)`, `(-s, s, s)`, `(-s, -s, -s)`
  with `s = 1 / sqrt(3)`; point `c_j + rA * v_j`. Valley `k` of `m` is inserted at index
  `((2 k + 1) * n) // (2 m)`; the other `n - m` points come from the stream in order.
- **Arc** (60 degrees, circle only): directions as above, kept when `u_0 * 0.6 + u_1 * 0.8 >=
  h` (within 30 degrees of the direction (0.6, 0.8)); `rho = R + g * (1 + w)`; no valleys.

`digest` is the SHA-256 of every coordinate's binary64 bits, little endian, in point order
(x, y, z per point).

# Oracle (exact)

Holes and sockets: every non-valley point is at least `rA + g` from the centre `c`, and the
valley points (as rounded) are the only points near the inscribed circle or sphere. The oracle
computes, in exact rational arithmetic (`fractions.Fraction`) on the rounded coordinates, the
circumcentre `c*` of the valley points (circle: in the plane; sphere: of the four), the squared
radius `r*^2 = |v_0 - c*|^2`, checks that every valley is at exactly `r*^2` and every other
point strictly farther, by at least `(g / 2)` in distance (`|p - c*|^2 >= (r* + g / 2)^2` with
`r*` rounded down), and that `c*` lies strictly inside the valleys' triangle or tetrahedron (so
the contacts' multipliers are positive). The expected radius is `sqrt(r*^2)` correctly rounded
(60-digit `decimal`), the centre `c*` correctly rounded; `analytic` is `rA` (the generator's
inscribed radius `R - A`).

Why it is the global optimum over the trust region: with `kappa` the smallest, over directions
`d`, of the largest `v_k . d` (1/2 for the three valleys at 120 degrees, 1/3 for the
tetrahedron), `min_k |c - v_k|^2 <= r^2 - 2 r kappa |c - c*| + |c - c*|^2 < r^2` for `0 <
|c - c*| < 2 kappa r`. Every case has `A <= 0.02 R`, so the least-squares centre is within a
few `A` of `c*` and a trust region of fraction 0.2 lies inside that ball.

Arcs: a 60-degree arc has no bounded inscribed circle: for the centre `c0 - t m` (moving away
from the arc's middle direction `m`) the nearest points are the arc's ends and their distance
grows with `t`. The oracle checks that the smallest point distance from `c0 - t m` increases
strictly over `t = 0, R/40, .., R/4` (60-digit decimal on the rounded points) and labels the case
`unbounded` (AC-3): the conic path must return `unbounded_envelope` and the Voronoi path must
report `on_region_boundary`.

Run:
  python fixtures/fit/make_mic_cirsph_fixtures.py
  python fixtures/fit/make_mic_cirsph_fixtures.py --check
"""

import hashlib
import math
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "mic-cirsph-v1" / "cases.txt"
MASK = 0xFFFFFFFFFFFFFFFF
K3 = 5.196152422706632

getcontext().prec = 60


class SplitMix64:
    def __init__(self, seed):
        self.state = seed & MASK

    def next_u64(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return (z ^ (z >> 31)) & MASK

    def uniform(self):
        return (self.next_u64() >> 11) * (1.0 / (1 << 53))


def hx(x):
    if x == 0.0:
        x = 0.0
    return struct.pack(">d", x).hex()


def direction(rng, dim):
    while True:
        x = [2.0 * rng.uniform() - 1.0 for _ in range(dim)]
        n2 = x[0] * x[0] + x[1] * x[1]
        if dim == 3:
            n2 = n2 + x[2] * x[2]
        if 0.0625 <= n2 <= 1.0:
            s = math.sqrt(n2)
            return [xj / s for xj in x]


def form(u):
    if len(u) == 2:
        t = u[0] * u[0]
        return 1.0 + (4.0 * t - 3.0) * u[0]
    return 1.0 + ((K3 * u[0]) * u[1]) * u[2]


def valleys(dim):
    if dim == 2:
        h = math.sqrt(0.75)
        return [[0.5, h], [-1.0, 0.0], [0.5, -h]]
    s = 1.0 / math.sqrt(3.0)
    return [[s, s, -s], [s, -s, s], [-s, s, s], [-s, -s, -s]]


def point(c, rho, u):
    if len(u) == 2:
        return [c[0] + rho * u[0], c[1] + rho * u[1], c[2]]
    return [c[0] + rho * u[0], c[1] + rho * u[1], c[2] + rho * u[2]]


def gen_lobed(n, seed, dim, R, A, g, c):
    rng = SplitMix64(seed)
    rA = R - A
    vs = valleys(dim)
    m = len(vs)
    at = {((2 * k + 1) * n) // (2 * m): k for k in range(m)}
    pts = []
    vidx = []
    for i in range(n):
        if i in at:
            pts.append(point(c, rA, vs[at[i]]))
            vidx.append(i)
            continue
        u = direction(rng, dim)
        w = rng.uniform()
        rho = (rA + A * form(u)) + g * (1.0 + w)
        pts.append(point(c, rho, u))
    return pts, vidx


def gen_arc(n, seed, R, g, c):
    rng = SplitMix64(seed)
    h = math.sqrt(0.75)
    pts = []
    while len(pts) < n:
        u = direction(rng, 2)
        if u[0] * 0.6 + u[1] * 0.8 >= h:
            w = rng.uniform()
            pts.append(point(c, R + g * (1.0 + w), u))
    return pts


def digest(pts):
    h = hashlib.sha256()
    for p in pts:
        for x in p:
            h.update(struct.pack("<d", x))
    return h.hexdigest()


def solve(m, b):
    """Exact Gaussian elimination on Fractions."""
    n = len(b)
    a = [row[:] + [b[i]] for i, row in enumerate(m)]
    for col in range(n):
        piv = next(r for r in range(col, n) if a[r][col] != 0)
        a[col], a[piv] = a[piv], a[col]
        for r in range(n):
            if r != col and a[r][col] != 0:
                f = a[r][col] / a[col][col]
                a[r] = [x - f * y for x, y in zip(a[r], a[col])]
    return [a[i][n] / a[i][i] for i in range(n)]


def det(m):
    n = len(m)
    a = [row[:] for row in m]
    d = Fraction(1)
    for col in range(n):
        piv = next((r for r in range(col, n) if a[r][col] != 0), None)
        if piv is None:
            return Fraction(0)
        if piv != col:
            a[col], a[piv] = a[piv], a[col]
            d = -d
        d *= a[col][col]
        for r in range(col + 1, n):
            f = a[r][col] / a[col][col]
            a[r] = [x - f * y for x, y in zip(a[r], a[col])]
    return d


def frac_to_float(x):
    """Correctly rounded binary64 of an exact rational."""
    return float(Decimal(x.numerator) / Decimal(x.denominator))


def oracle_lobed(pts, vidx, dim, g):
    V = [[Fraction(pts[i][j]) for j in range(dim)] for i in vidx]
    v0 = V[0]
    m = [[2 * (vk[j] - v0[j]) for j in range(dim)] for vk in V[1:]]
    b = [sum((vk[j] - v0[j]) ** 2 for j in range(dim)) for vk in V[1:]]
    y = solve(m, b)
    c = [v0[j] + y[j] for j in range(dim)]
    r2 = sum((v0[j] - c[j]) ** 2 for j in range(dim))
    for vk in V:
        assert sum((vk[j] - c[j]) ** 2 for j in range(dim)) == r2
    # Strictly inside the valleys' simplex: the barycentric weights of c are all positive.
    base = det([[V[k][j] - V[0][j] for j in range(dim)] for k in range(1, dim + 1)])
    for k in range(dim + 1):
        w = [V[i] if i != k else c for i in range(dim + 1)]
        dk = det([[w[i][j] - w[0][j] for j in range(dim)] for i in range(1, dim + 1)])
        assert dk * base > 0, "centre not strictly inside the contacts"
    r = Decimal(r2.numerator) / Decimal(r2.denominator)
    r = r.sqrt()
    r_low = Fraction(r) - Fraction(1, 10**40)
    sep2 = (r_low + Fraction(g) / 2) ** 2
    cd = [Decimal(x.numerator) / Decimal(x.denominator) for x in c]
    sep2d = Decimal(sep2.numerator) / Decimal(sep2.denominator)
    vset = set(vidx)
    for i, p in enumerate(pts):
        if i in vset:
            continue
        d2 = sum((Decimal(p[j]) - cd[j]) ** 2 for j in range(dim))
        assert d2 > sep2d, f"point {i} within g/2 of the inscribed radius"
    return float(r), [frac_to_float(x) for x in c]


def oracle_arc(pts, R, c0):
    m = (Decimal(0.6), Decimal(0.8))
    prev = None
    for k in range(11):
        t = Decimal(R) * k / 40
        cx = Decimal(c0[0]) - t * m[0]
        cy = Decimal(c0[1]) - t * m[1]
        f = min((Decimal(p[0]) - cx) ** 2 + (Decimal(p[1]) - cy) ** 2 for p in pts)
        assert prev is None or f > prev, "nearest distance does not grow away from the arc"
        prev = f


# id, kind, n, seed, R, A, g, centre
LOBED = [
    ("h01", 2, 50, 0x11, 0.005, 0.0001, 5e-8, [0.0123, -0.0456, 0.003]),
    ("h02", 2, 360, 0x12, 0.010, 0.0002, 1e-7, [-0.25, 0.125, 0.0]),
    ("h03", 2, 2000, 0x13, 0.025, 0.0003, 2.5e-7, [0.4, 0.3, -0.02]),
    ("h04", 2, 20000, 0x14, 0.0125, 0.00015, 1.25e-7, [1.5, -2.25, 0.0]),
    ("h05", 2, 100000, 0x15, 0.010, 0.0002, 1e-7, [0.07, 0.11, 0.0]),
    ("s01", 3, 50, 0x21, 0.006, 0.0001, 6e-8, [0.0123, -0.0456, 0.0789]),
    ("s02", 3, 500, 0x22, 0.012, 0.0002, 1.2e-7, [-0.3, 0.2, 0.1]),
    ("s03", 3, 5000, 0x23, 0.020, 0.0004, 2e-7, [0.5, -0.5, 1.25]),
    ("s04", 3, 20000, 0x24, 0.008, 0.00012, 8e-8, [0.0, 0.0, 0.0]),
]
ARCS = [
    ("a01", 200, 0x31, 0.010, 1e-7, [0.05, -0.02, 0.0]),
    ("a02", 1000, 0x32, 0.020, 2e-7, [-0.3, 0.7, 0.01]),
]


def text():
    out = [
        "# KRN.ENV.MIC.CIRSPH: maximum inscribed circle and sphere (AC-1, AC-2, AC-3).",
        "# Generated by fixtures/fit/make_mic_cirsph_fixtures.py (CC-BY-4.0). Do not edit.",
        "# Floats are binary64 bit patterns (hex), metres; the generator rules are in the script.",
        "# case <id> <circle|sphere> <lobed|arc>",
        "# gen <n> <seed> <R> <A> <g> <cx> <cy> <cz>   lobed (arc: A is 0)",
        "# digest <sha256 of the coordinates, little endian>",
        "# expect inscribed <r> <centre..> analytic <rA> contacts <m> <indices>",
        "# expect unbounded",
    ]
    for cid, dim, n, seed, R, A, g, c in LOBED:
        pts, vidx = gen_lobed(n, seed, dim, R, A, g, c)
        r, cc = oracle_lobed(pts, vidx, dim, g)
        kind = "circle" if dim == 2 else "sphere"
        out.append(f"case {cid} {kind} lobed")
        out.append(f"gen {n} {seed} {hx(R)} {hx(A)} {hx(g)} " + " ".join(hx(x) for x in c))
        out.append(f"digest {digest(pts)}")
        out.append(
            f"expect inscribed {hx(r)} "
            + " ".join(hx(x) for x in cc)
            + f" analytic {hx(R - A)} contacts {len(vidx)} "
            + " ".join(str(i) for i in vidx)
        )
        out.append("end")
    for cid, n, seed, R, g, c in ARCS:
        pts = gen_arc(n, seed, R, g, c)
        oracle_arc(pts, R, c)
        out.append(f"case {cid} circle arc")
        out.append(f"gen {n} {seed} {hx(R)} {hx(0.0)} {hx(g)} " + " ".join(hx(x) for x in c))
        out.append(f"digest {digest(pts)}")
        out.append("expect unbounded")
        out.append("end")
    return "\n".join(out) + "\n"


def main():
    data = text().encode("utf-8")
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_bytes() != data:
            print("stale: fixtures/fit/mic-cirsph-v1/cases.txt")
            sys.exit(1)
        print("ok: fixtures/fit/mic-cirsph-v1/cases.txt")
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(data)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
