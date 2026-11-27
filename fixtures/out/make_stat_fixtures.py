#!/usr/bin/env python3
"""Independent reference for KRN.OUT.STAT (n-sigma and robust-scale outlier policies)
fixtures (CC-BY-4.0 project data).

Writes  fixtures/out/stat-contam-v1/cases.txt   four seeded contaminated cases (two planes,
                                                two circles), each with its points, its
                                                least-squares residuals, the planted outlier
                                                indices and the expected masks of every
                                                policy and side at k = 3

Nothing here runs the Rust code (stdlib only, no third-party data). It is a second
implementation of spec node KRN.OUT.STAT:

- Points. Each case is seeded (splitmix64) and uses arithmetic only (no libm transcendental
  function), so the output is the same on every platform: Gaussian noise is the Irwin-Hall sum
  of twelve uniforms minus six; circle points use the rational parametrisation
  ((1 - t^2) / (1 + t^2), 2 t / (1 + t^2)). 2 % of the points (seeded indices) are gross
  outliers planted at 50 sigma (random sign) along the plane normal or the circle radius.
- Least-squares residuals, by an independent fit: the plane through the fsum centroid with the
  normal of the smallest eigenvalue of the scatter matrix (cyclic Jacobi), residual the signed
  distance; the circle by Gauss-Newton on the geometric distance from the algebraic (Kasa)
  start, residual rho - r. The Rust test checks that fid-fit's PlaneLs and Circle2Ls give the
  same residuals (to a tolerance; the plane's sign is arbitrary) and then runs the policies on
  the residuals committed here.
- n-sigma (REQ-001): exact rational arithmetic. With the population variance var of the
  residuals (divisor n), a point is beyond the limit when r^2 > k^2 var; side both takes every
  such point, positive those with r > 0, negative those with r < 0. Every residual must lie at
  least a relative 1e-9 from the limit (checked here), so any implementation whose limit is
  within a few ulps of exact gives the same mask.
- robust scale (REQ-002): the pinned float rules, reproduced operation for operation in
  binary64 (Python floats): total-order stable sort, median y[n//2] or y[n//2-1]*0.5 +
  y[n//2]*0.5; MAD scale 1.4826 * median(|r - m|); Qn scale 2.219144465985076 (exact, see QN_FACTOR) * the k-th smallest of all
  n(n-1)/2 differences y[j] - y[i] (k = h(h-1)/2, h = n//2 + 1), found here by listing and
  sorting every difference (the Rust code searches bit patterns instead); limit k * scale;
  both |r - m| > limit, positive r - m > limit, negative r - m < -limit. These values are
  exact, so the expected centre, scale and limit are bit patterns.
- AC-1 is checked here on the model's own masks: robust scale (MAD and Qn) with k = 3, side
  both, excludes every planted outlier and at most 1 % of the clean points.

The Rust test (crates/fid-filter/tests/stat.rs) reads cases.txt.

Run:  python fixtures/out/make_stat_fixtures.py          (writes the fixture)
      python fixtures/out/make_stat_fixtures.py --check  (fails if the fixture is stale)
"""

import math
import re
import statistics
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "stat-contam-v1" / "cases.txt"
MASK64 = 2**64 - 1
K = 3.0
MAD_FACTOR = 1.4826
# The exact Qn normal-consistency constant 1 / (sqrt(2) * Phi^-1(5/8)), not the 2.2219
# printed in Rousseeuw and Croux (1993). The value used below is the correctly rounded
# binary64 literal, so the fixture does not depend on the platform's libm; check_qn_factor
# recomputes it from statistics.NormalDist and requires the Rust literal to be the same one.
QN_FACTOR = 2.219144465985076
RUST_STAT = HERE.parent.parent / "crates" / "fid-filter" / "src" / "stat.rs"
OUTLIER_SIGMA = 50.0
MARGIN = Fraction(1, 10**9)
SIDES = ("both", "positive", "negative")

# name, primitive, point count, seed, noise sigma (m)
CASES = [
    ("plane-a", "plane", 500, 0x7374_6174_2D70_6C61, 5e-6),
    ("plane-b", "plane", 400, 0x7374_6174_2D70_6C62, 2e-6),
    ("circle-a", "circle2", 300, 0x7374_6174_2D63_6972, 3e-6),
    ("circle-b", "circle2", 250, 0x7374_6174_2D63_6973, 1e-6),
]


def fail(msg):
    print(f"make_stat_fixtures: {msg}", file=sys.stderr)
    sys.exit(1)


def bits(x):
    return format(struct.unpack("<Q", struct.pack("<d", x))[0], "016x")


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK64
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
        return z ^ (z >> 31)

    def uniform(self):
        return (self.next() >> 11) * 2.0**-53

    def gauss(self):
        return math.fsum(self.uniform() for _ in range(12)) - 6.0

    def sign(self):
        return 1.0 if self.next() & 1 else -1.0


def planted_indices(rng, n):
    count = n * 2 // 100
    out = set()
    while len(out) < count:
        out.add(rng.next() % n)
    return sorted(out)


def normalise(v):
    s = math.sqrt(math.fsum(c * c for c in v))
    return [c / s for c in v]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def gen_plane(rng, n, sigma, planted):
    c0 = [0.0125, -0.04, 0.3]
    n0 = normalise([0.1, -0.2, 1.0])
    e1 = normalise(cross(n0, [1.0, 0.0, 0.0]))
    e2 = cross(n0, e1)
    pts = []
    for i in range(n):
        u = (rng.uniform() - 0.5) * 0.1
        v = (rng.uniform() - 0.5) * 0.08
        g = rng.gauss()
        h = rng.sign() * OUTLIER_SIGMA * sigma if i in planted else g * sigma
        pts.append([c0[j] + u * e1[j] + v * e2[j] + h * n0[j] for j in range(3)])
    return pts


def gen_circle(rng, n, sigma, planted):
    a, b, r = 0.031, -0.0125, 0.02
    pts = []
    for i in range(n):
        t = rng.uniform() * 2.0 - 1.0
        cx = (1.0 - t * t) / (1.0 + t * t)
        cy = (2.0 * t) / (1.0 + t * t)
        if rng.next() & 1:
            cx, cy = -cx, -cy
        g = rng.gauss()
        h = rng.sign() * OUTLIER_SIGMA * sigma if i in planted else g * sigma
        pts.append([a + (r + h) * cx, b + (r + h) * cy, 0.0])
    return pts


def jacobi_smallest(m):
    """Eigenvector of the smallest eigenvalue of a symmetric 3x3 matrix (cyclic Jacobi)."""
    a = [row[:] for row in m]
    v = [[1.0 if i == j else 0.0 for j in range(3)] for i in range(3)]
    for _ in range(100):
        off = math.fsum(a[i][j] * a[i][j] for i in range(3) for j in range(3) if i != j)
        diag = math.fsum(a[i][i] * a[i][i] for i in range(3))
        if off <= 1e-30 * diag:
            break
        for p in range(3):
            for q in range(p + 1, 3):
                if a[p][q] == 0.0:
                    continue
                theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q])
                t = (1.0 if theta >= 0 else -1.0) / (abs(theta) + math.sqrt(theta * theta + 1.0))
                c = 1.0 / math.sqrt(t * t + 1.0)
                s = t * c
                for k in range(3):
                    akp, akq = a[k][p], a[k][q]
                    a[k][p] = c * akp - s * akq
                    a[k][q] = s * akp + c * akq
                for k in range(3):
                    apk, aqk = a[p][k], a[q][k]
                    a[p][k] = c * apk - s * aqk
                    a[q][k] = s * apk + c * aqk
                for k in range(3):
                    vkp, vkq = v[k][p], v[k][q]
                    v[k][p] = c * vkp - s * vkq
                    v[k][q] = s * vkp + c * vkq
    else:
        fail("jacobi did not converge")
    i = min(range(3), key=lambda j: a[j][j])
    return normalise([v[0][i], v[1][i], v[2][i]])


def fit_plane(pts):
    n = len(pts)
    c = [math.fsum(p[j] for p in pts) / n for j in range(3)]
    q = [[p[j] - c[j] for j in range(3)] for p in pts]
    s = [[math.fsum(r[i] * r[j] for r in q) for j in range(3)] for i in range(3)]
    nrm = jacobi_smallest(s)
    if nrm[2] < 0:
        nrm = [-x for x in nrm]
    return [math.fsum(r[j] * nrm[j] for j in range(3)) for r in q]


def solve3(m, rhs):
    a = [m[i][:] + [rhs[i]] for i in range(3)]
    for col in range(3):
        piv = max(range(col, 3), key=lambda r: abs(a[r][col]))
        a[col], a[piv] = a[piv], a[col]
        if a[col][col] == 0.0:
            fail("singular system")
        for r in range(col + 1, 3):
            f = a[r][col] / a[col][col]
            for k in range(col, 4):
                a[r][k] -= f * a[col][k]
    x = [0.0, 0.0, 0.0]
    for r in (2, 1, 0):
        x[r] = (a[r][3] - math.fsum(a[r][k] * x[k] for k in range(r + 1, 3))) / a[r][r]
    return x


def fit_circle(pts):
    n = len(pts)
    cx = math.fsum(p[0] for p in pts) / n
    cy = math.fsum(p[1] for p in pts) / n
    uv = [(p[0] - cx, p[1] - cy) for p in pts]
    # Kasa: u^2 + v^2 + D u + E v + F = 0, least squares in (D, E, F).
    rows = [(u, v, 1.0, -(u * u + v * v)) for u, v in uv]
    ata = [[math.fsum(r[i] * r[j] for r in rows) for j in range(3)] for i in range(3)]
    atb = [math.fsum(r[i] * r[3] for r in rows) for i in range(3)]
    d, e, f = solve3(ata, atb)
    a, b = -d / 2.0, -e / 2.0
    r = math.sqrt(a * a + b * b - f)
    for _ in range(200):
        res, jac = [], []
        for u, v in uv:
            du, dv = u - a, v - b
            rho = math.sqrt(du * du + dv * dv)
            res.append(rho - r)
            jac.append((-du / rho, -dv / rho, -1.0))
        jtj = [[math.fsum(j[i] * j[k] for j in jac) for k in range(3)] for i in range(3)]
        jtr = [-math.fsum(j[i] * x for j, x in zip(jac, res)) for i in range(3)]
        step = solve3(jtj, jtr)
        a, b, r = a + step[0], b + step[1], r + step[2]
        if max(abs(s) for s in step) <= 1e-17:
            break
    else:
        fail("Gauss-Newton did not converge")
    out = []
    for u, v in uv:
        du, dv = u - a, v - b
        out.append(math.sqrt(du * du + dv * dv) - r)
    return out


def tsort(xs):
    # IEEE-754 totalOrder on finite values (-0.0 before +0.0), stable.
    return sorted(xs, key=lambda x: (x, math.copysign(1.0, x)))


def median_sorted(y):
    n = len(y)
    if n % 2 == 1:
        return y[n // 2]
    return y[n // 2 - 1] * 0.5 + y[n // 2] * 0.5


def qn_raw(xs):
    y = tsort(xs)
    n = len(y)
    h = n // 2 + 1
    k = h * (h - 1) // 2
    diffs = sorted(y[j] - y[i] for i in range(n) for j in range(i + 1, n))
    return diffs[k - 1]


def side_test(side, x, limit):
    if side == "both":
        return abs(x) > limit
    if side == "positive":
        return x > limit
    return x < -limit


def robust(res, estimator, side):
    m = median_sorted(tsort(res))
    if estimator == "mad":
        scale = MAD_FACTOR * median_sorted(tsort([abs(r - m) for r in res]))
    else:
        scale = QN_FACTOR * qn_raw(res)
    limit = K * scale
    if all(r == res[0] for r in res) or scale == 0.0 or limit == 0.0:
        return m, 0.0, 0.0, "zero_scale", []
    mask = [i for i, r in enumerate(res) if side_test(side, r - m, limit)]
    return m, scale, limit, "decided", mask


def nsigma(res, side):
    n = len(res)
    fr = [Fraction(r) for r in res]
    mean = sum(fr) / n
    var = sum((x - mean) ** 2 for x in fr) / n
    if var == 0:
        return 0.0, "zero_scale", []
    lim2 = Fraction(K) ** 2 * var
    for i, x in enumerate(fr):
        if abs(x * x - lim2) <= MARGIN * lim2:
            fail(f"residual {i} within the margin of the n-sigma limit")
    mask = []
    for i, x in enumerate(fr):
        if x * x > lim2 and (side == "both" or (side == "positive" and x > 0) or (side == "negative" and x < 0)):
            mask.append(i)
    return math.sqrt(float(var)), "decided", mask


def check_qn_factor():
    computed = 1.0 / (math.sqrt(2.0) * statistics.NormalDist().inv_cdf(0.625))
    # inv_cdf goes through the platform's log, so allow a few ulps of libm difference.
    if abs(computed - QN_FACTOR) > 4 * math.ulp(QN_FACTOR):
        fail(f"QN_FACTOR {QN_FACTOR!r} is not 1/(sqrt(2) inv_cdf(5/8)) = {computed!r}")
    src = RUST_STAT.read_text(encoding="utf-8")
    m = re.search(r"pub const QN_FACTOR: f64 = ([0-9.eE+-]+);", src)
    if not m or float(m.group(1)) != QN_FACTOR:
        fail(f"QN_FACTOR in {RUST_STAT} does not match {QN_FACTOR!r}")


def build():
    check_qn_factor()
    out = [
        "# KRN.OUT.STAT AC-1, AC-2, AC-3: n-sigma and robust-scale masks on contaminated fits.",
        "# Generated by fixtures/out/make_stat_fixtures.py (CC-BY-4.0). Do not edit.",
        "# f64 values are binary64 bit patterns in hex. Per case: 'p x y z' points and 'r' the",
        "# least-squares residuals (metres, point order), 'planted' the outlier indices,",
        "# 'robust <mad|qn> <side> <centre> <scale> <limit> <status> <indices...>' (exact bit",
        "# patterns) and 'nsigma <side> <sd> <status> <indices...>' (sd approximate).",
        f"k {bits(K)}",
    ]
    for name, prim, n, seed, sigma in CASES:
        rng = Rng(seed)
        planted = planted_indices(rng, n)
        pset = set(planted)
        pts = gen_plane(rng, n, sigma, pset) if prim == "plane" else gen_circle(rng, n, sigma, pset)
        res = fit_plane(pts) if prim == "plane" else fit_circle(pts)
        if not all(math.isfinite(r) for r in res):
            fail(f"{name}: non-finite residual")
        out.append(f"case {name} {prim} {n}")
        out += [f"p {bits(p[0])} {bits(p[1])} {bits(p[2])}" for p in pts]
        out += [f"r {bits(r)}" for r in res]
        out.append("planted" + "".join(f" {i}" for i in planted))
        clean = n - len(planted)
        for est in ("mad", "qn"):
            for side in SIDES:
                m, scale, limit, status, mask = robust(res, est, side)
                if status != "decided":
                    fail(f"{name}: {est} zero scale")
                if side == "both":
                    if not pset <= set(mask):
                        fail(f"{name}: {est} misses a planted outlier")
                    false_ex = len(set(mask) - pset)
                    if false_ex * 100 > clean:
                        fail(f"{name}: {est} excludes {false_ex} of {clean} clean points")
                idx = "".join(f" {i}" for i in mask)
                out.append(f"robust {est} {side} {bits(m)} {bits(scale)} {bits(limit)} {status}{idx}")
        for side in SIDES:
            sd, status, mask = nsigma(res, side)
            if status != "decided":
                fail(f"{name}: nsigma zero scale")
            out.append(f"nsigma {side} {bits(sd)} {status}" + "".join(f" {i}" for i in mask))
        out.append("end")
    return "\n".join(out) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        old = OUT.read_text(encoding="utf-8").replace("\r\n", "\n") if OUT.exists() else ""
        if old != text:
            print(f"stale: {OUT}", file=sys.stderr)
            return 1
        print("ok")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
