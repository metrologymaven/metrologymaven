#!/usr/bin/env python3
"""Independent reference for KRN.LSQ.AXIS.CYL (least-squares cylinder) fixtures
(CC-BY-4.0 project data).

Writes fixtures/fit/axis-cyl-v1/cases.txt. Nothing here runs the Rust code: stdlib only, every
value is computed in Decimal at 60 significant digits (sin and cos by Taylor series, pi by
Machin's formula, square roots by the decimal module), never with the platform libm.

1. Each point set is generated from planted geometry and rounded once to binary64 (the input).
2. The reference fit is the least-squares cylinder of those *rounded* points (each binary64
   value taken exactly as a Fraction), computed by Gauss-Newton in Decimal from the planted
   truth: at each iterate (P, d, r), with P the foot of the perpendicular from the exact
   centroid, an orthonormal (e1, e2) of d, residuals rho_i - r, the analytic Jacobian
   [-t g1, -t g2, -g1, -g2, -1] (g = perp . e / rho) and the normal equations solved by Gaussian
   elimination in Decimal; then d <- unit(d + a e1 + b e2), P <- P + u e1 + v e2, r <- r + dr.
   It stops when the step norm is below 1e-40 (with non-zero residuals Gauss-Newton converges
   linearly, here by a factor of about 1e-5 per iteration on the well-posed cases; at most
   400 iterations, else the script fails). Constrained cases drop the fixed columns (fixed
   radius: dr; fixed direction: a, b).
3. The expected values are the reference cylinder (axis direction, the foot point, radius), its
   residual RMS and the RMS of the least-squares plane (the exact smallest principal moment of
   the centred points, by Jacobi in Decimal), all rounded to binary64 once. The Rust test
   compares its result with the reference within stated tolerances; they are different
   computations (binary64 Levenberg-Marquardt on centred, scaled data) of the same minimum.

Cases (seeded random.Random on the case name; SI metres):

- `arc-<deg>-<off>`: coverage 360, 180, 90, 60 and 30 degrees, axis about 0.01 m, 0.3 m and
  1000 m from the origin; radius 0.01 to 0.05 m, length 0.5 to 3 radii; 200 points, noise along
  the normal uniform in [-sqrt(3), sqrt(3)] micrometres.
- `half-1066-<seed>` and `half-sweep-<L>`: half cylinders (180 degrees) of length 1.066 radii
  (where L^2/12 equals the transverse variance of a half circle and the principal axes mix;
  review of #358), seeds at 0.01 m, 0.3 m and 1000 m, and a sweep 1.03 to 1.11 radii.
- `fixed-r-<k>` / `fixed-dir-<k>`: the fixed-radius constraint (the planted radius) and the
  fixed-axis-direction constraint (the planted axis, rounded) with the constrained reference.
- `shortarc-<k>`: 30-degree arcs 0.2 radii long with 40 to 100 micrometres of noise (the
  sagitta is about 0.6 mm), where the starts can land far from the true axis and on
  different minima. Hand-labelled `flag never_confidently_wrong`: a returned fit either matches
  the reference or carries `local_optimum_only` or `suspect_axis`, or the fit is refused with
  `implausible_fit`; AC-3's "two plausible axes" cases are among them.
- `wrong-start`: a half cylinder of length 1.066 radii fitted from one start whose axis is the
  half circle's bisector (perpendicular to the true axis); the reference is the Gauss-Newton
  local minimum from that start, hand-labelled `flag suspect_axis` (rule 8 of `ls_cyl`).
- Error cases: `few-4` (4 points: TooFewPoints n 4 need 5) and `coplanar` (a circle in
  z = 0.125: Coplanar rank 2).

Output lines (hex = 16 lowercase hex digits of an IEEE-754 double):
  case <name> coverage <degrees>
  points <x y z per point>
  truth_axis <3 hex> / truth_point <3 hex> / truth_r <hex>     planted geometry
  fixed_r <hex> | fixed_dir <3 hex>                            constraint (optional)
  bound_rms <hex>              fixed_dir cases: RMS of the best plane containing that direction
  start_axis <3 hex> start_point <3 hex> start_r <hex>          one-start override (optional)
  ref_axis <3 hex> ref_point <3 hex> ref_r <hex>               reference cylinder
  ref_rms <hex> plane_rms <hex>
  flag <local_optimum_only | suspect_axis>                     (optional)
  expect start | <error code fields>
  end
"""

import random
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

OUT = Path(__file__).with_name("axis-cyl-v1") / "cases.txt"
getcontext().prec = 60
N = 200
EPS = Decimal(10) ** -40
MAX_GN = 400


def dec(x):
    if isinstance(x, Decimal):
        return x
    x = Fraction(x)
    return Decimal(x.numerator) / Decimal(x.denominator)


def fl(x):
    return float(Fraction(x))


def hx(f):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", f))[0]


def hxs(v):
    return " ".join(hx(fl(x)) for x in v)


def machin_pi():
    def atan_inv(k):
        x = Decimal(1) / k
        x2 = x * x
        term, total, n = x, x, 1
        while True:
            term = -term * x2
            n += 2
            t = term / n
            if abs(t) < Decimal(10) ** -65:
                return total
            total += t

    return 16 * atan_inv(5) - 4 * atan_inv(239)


PI = machin_pi()


def sin_cos(t):
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
        if abs(term) < Decimal(10) ** -65:
            return s, c


def u01(rng):
    return Decimal(rng.getrandbits(52)) / Decimal(2**52)


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def norm(v):
    return dot(v, v).sqrt()


def unit(v):
    n = norm(v)
    return [x / n for x in v]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def basis(d):
    k = min(range(3), key=lambda j: abs(d[j]))
    ek = [Decimal(int(j == k)) for j in range(3)]
    e1 = unit(cross(d, ek))
    return e1, cross(d, e1)


def frame(rng):
    while True:
        v = [2 * u01(rng) - 1 for _ in range(3)]
        n2 = dot(v, v)
        if Decimal("0.01") < n2 <= 1:
            break
    d = unit(v)
    e1, e2 = basis(d)
    return d, e1, e2


def offset(rng, dist):
    v = unit([2 * u01(rng) - 1 for _ in range(3)])
    return [x * dist * (Decimal("0.9") + Decimal("0.2") * u01(rng)) for x in v]


def at(p, d, e1, e2, t, a, b):
    return [p[k] + t * d[k] + a * e1[k] + b * e2[k] for k in range(3)]


def cylinder(name, cov, dist, length_ratio=None, noise_um=Decimal(1)):
    """Planted cylinder points (rounded to binary64, back as exact Decimals) and the truth."""
    rng = random.Random(f"axis-cyl-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, dist)
    arc = dec(cov) * PI / 180
    phase = 2 * PI * u01(rng)
    r = Decimal("0.01") + Decimal("0.04") * u01(rng)
    ratio = Decimal("0.5") + Decimal("2.5") * u01(rng)
    if length_ratio is not None:
        ratio = length_ratio
    length = r * ratio
    pts = []
    for _ in range(N):
        s, c = sin_cos(phase + arc * u01(rng))
        t = length * u01(rng)
        rr = r + (2 * u01(rng) - 1) * Decimal(3).sqrt() * noise_um * Decimal("1e-6")
        pts.append([dec(fl(x)) for x in at(p0, d, e1, e2, t, rr * c, rr * s)])
    s, c = sin_cos(phase + arc / 2)
    bisector = [c * e1[k] + s * e2[k] for k in range(3)]
    return pts, d, p0, r, bisector


def centroid(pts):
    n = len(pts)
    return [sum(p[k] for p in pts) / n for k in range(3)]


def foot(c, p, d):
    w = [c[k] - p[k] for k in range(3)]
    t = dot(w, d)
    return [p[k] + t * d[k] for k in range(3)]


def solve(a, b):
    """Gaussian elimination with partial pivoting (Decimal)."""
    n = len(b)
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for col in range(n):
        piv = max(range(col, n), key=lambda i: abs(m[i][col]))
        m[col], m[piv] = m[piv], m[col]
        for i in range(col + 1, n):
            f = m[i][col] / m[col][col]
            for j in range(col, n + 1):
                m[i][j] -= f * m[col][j]
    x = [Decimal(0)] * n
    for i in reversed(range(n)):
        x[i] = (m[i][n] - sum(m[i][j] * x[j] for j in range(i + 1, n))) / m[i][i]
    return x


def residuals(pts, p, d, r):
    out = []
    for q in pts:
        w = [q[k] - p[k] for k in range(3)]
        t = dot(w, d)
        perp = [w[k] - t * d[k] for k in range(3)]
        rho = norm(perp)
        out.append((rho - r, t, perp, rho))
    return out


def gauss_newton(pts, p, d, r, free_dir=True, free_r=True):
    c = centroid(pts)
    p = foot(c, p, unit(d))
    d = unit(d)
    for _ in range(MAX_GN):
        e1, e2 = basis(d)
        rows, res = [], []
        for ri, t, perp, rho in residuals(pts, p, d, r):
            g1, g2 = dot(perp, e1) / rho, dot(perp, e2) / rho
            row = ([-t * g1, -t * g2] if free_dir else []) + [-g1, -g2] + ([Decimal(-1)] if free_r else [])
            rows.append(row)
            res.append(ri)
        n = len(rows[0])
        jtj = [[sum(rw[i] * rw[j] for rw in rows) for j in range(n)] for i in range(n)]
        jtr = [-sum(rw[i] * e for rw, e in zip(rows, res)) for i in range(n)]
        x = solve(jtj, jtr)
        it = iter(x)
        a, b = (next(it), next(it)) if free_dir else (Decimal(0), Decimal(0))
        u, v = next(it), next(it)
        dr = next(it) if free_r else Decimal(0)
        d = unit([d[k] + a * e1[k] + b * e2[k] for k in range(3)])
        p = foot(c, [p[k] + u * e1[k] + v * e2[k] for k in range(3)], d)
        r = r + dr
        if sum(xi * xi for xi in x).sqrt() < EPS:
            rms = (sum(e[0] * e[0] for e in residuals(pts, p, d, r)) / len(pts)).sqrt()
            if d[next(k for k in range(3) if d[k] != 0)] < 0:
                d = [-x for x in d]
            return p, d, r, rms
    raise SystemExit(f"reference Gauss-Newton did not converge (last step {float(sum(xi * xi for xi in x).sqrt()):e})")


def plane_rms(pts):
    """RMS distance to the least-squares plane: sqrt(smallest eigenvalue of the scatter / n),
    by cyclic Jacobi in Decimal."""
    c = centroid(pts)
    a = [[sum((q[i] - c[i]) * (q[j] - c[j]) for q in pts) for j in range(3)] for i in range(3)]
    for _ in range(100):
        off = sum(a[i][j] ** 2 for i in range(3) for j in range(3) if i != j)
        if off < Decimal(10) ** -100:
            break
        for p_, q_ in ((0, 1), (0, 2), (1, 2)):
            if a[p_][q_] == 0:
                continue
            theta = (a[q_][q_] - a[p_][p_]) / (2 * a[p_][q_])
            t = (1 if theta >= 0 else -1) / (abs(theta) + (theta * theta + 1).sqrt())
            cs = 1 / (t * t + 1).sqrt()
            sn = t * cs
            for k in range(3):
                akp, akq = a[k][p_], a[k][q_]
                a[k][p_], a[k][q_] = cs * akp - sn * akq, sn * akp + cs * akq
            for k in range(3):
                apk, aqk = a[p_][k], a[q_][k]
                a[p_][k], a[q_][k] = cs * apk - sn * aqk, sn * apk + cs * aqk
    lam = min(a[i][i] for i in range(3))
    return (lam / len(pts)).sqrt()


def plane_rms_containing(pts, d):
    """RMS distance to the best plane that contains direction d: the least-squares line of the
    centred points projected onto an orthonormal (e1, e2) of d, i.e. the square root of the
    smaller eigenvalue of the 2 x 2 scatter over n (closed form in Decimal)."""
    c = centroid(pts)
    e1, e2 = basis(unit(d))
    u = [dot([q[k] - c[k] for k in range(3)], e1) for q in pts]
    v = [dot([q[k] - c[k] for k in range(3)], e2) for q in pts]
    suu = sum(x * x for x in u)
    svv = sum(x * x for x in v)
    suv = sum(x * y for x, y in zip(u, v))
    lam = (suu + svv) / 2 - (((suu - svv) / 2) ** 2 + suv * suv).sqrt()
    return (lam / len(pts)).sqrt()


def emit(out, name, cov, pts, truth, ref, extra=(), flag=None, expect="start"):
    d, p0, r = truth
    out.append(f"case {name} coverage {cov}")
    out.append("points " + " ".join(hx(fl(x)) for p in pts for x in p))
    out.append("truth_axis " + hxs(d))
    out.append("truth_point " + hxs(p0))
    out.append("truth_r " + hx(fl(r)))
    for line in extra:
        out.append(line)
    if ref is not None:
        rp, rd, rr, rms = ref
        out.append("ref_axis " + hxs(rd))
        out.append("ref_point " + hxs(rp))
        out.append("ref_r " + hx(fl(rr)))
        out.append("ref_rms " + hx(fl(rms)))
        out.append("plane_rms " + hx(fl(plane_rms(pts))))
    if flag:
        out.append(f"flag {flag}")
    out.append(f"expect {expect}")
    out.append("end")


def fitted(name, cov, dist, **kw):
    pts, d, p0, r, bis = cylinder(name, cov, dist, **kw)
    return pts, (d, p0, r), bis


def render():
    out = [
        "# KRN.LSQ.AXIS.CYL fixture axis-cyl-v1 (CC-BY-4.0). Generated by fixtures/fit/make_axis_cyl_fixtures.py; do not edit.",
        "# Seeded partial cylinders with Decimal least-squares references, constraints, flagged and error cases.",
    ]
    dists = (("0", Decimal("0.01")), ("1", Decimal("0.3")), ("km", Decimal(1000)))
    for cov in (360, 180, 90, 60, 30):
        for tag, dist in dists:
            name = f"arc-{cov}-{tag}"
            pts, truth, _ = fitted(name, cov, dist)
            emit(out, name, cov, pts, truth, gauss_newton(pts, truth[1], truth[0], truth[2]))
    for k, (tag, dist) in enumerate(dists):
        name = f"half-1066-{tag}"
        pts, truth, _ = fitted(name, 180, dist, length_ratio=Decimal("1.066"))
        emit(out, name, 180, pts, truth, gauss_newton(pts, truth[1], truth[0], truth[2]))
    for ratio in ("1.03", "1.05", "1.07", "1.09", "1.11"):
        name = f"half-sweep-{ratio}"
        pts, truth, _ = fitted(name, 180, Decimal("0.3"), length_ratio=Decimal(ratio))
        emit(out, name, 180, pts, truth, gauss_newton(pts, truth[1], truth[0], truth[2]))
    for k, cov in enumerate((360, 90)):
        name = f"fixed-r-{k}"
        pts, truth, _ = fitted(name, cov, Decimal("0.3"))
        ref = gauss_newton(pts, truth[1], truth[0], truth[2], free_r=False)
        emit(out, name, cov, pts, truth, ref, extra=[f"fixed_r {hx(fl(truth[2]))}"])
    for k, cov in enumerate((360, 60)):
        name = f"fixed-dir-{k}"
        pts, truth, _ = fitted(name, cov, Decimal("0.3"))
        dfix = [dec(fl(x)) for x in truth[0]]
        ref = gauss_newton(pts, truth[1], dfix, truth[2], free_dir=False)
        extra = ["fixed_dir " + hxs(dfix), "bound_rms " + hx(fl(plane_rms_containing(pts, dfix)))]
        emit(out, name, cov, pts, truth, ref, extra=extra)
    for k in range(SHORT_ARCS):
        name = f"shortarc-{k}"
        noise = SHORT_ARC_NOISE_UM[k % len(SHORT_ARC_NOISE_UM)]
        pts, truth, _ = fitted(name, 30, Decimal("0.3"), length_ratio=Decimal("0.2"), noise_um=noise)
        emit(out, name, 30, pts, truth, gauss_newton(pts, truth[1], truth[0], truth[2]), flag="never_confidently_wrong")
    name = "wrong-start"
    pts, truth, bis = fitted(name, 180, Decimal("0.3"), length_ratio=Decimal("1.066"))
    c = centroid(pts)
    start_r = truth[2] * Decimal("0.43")
    ref = gauss_newton(pts, c, bis, start_r)
    extra = ["start_axis " + hxs(bis), "start_point " + hxs(c), "start_r " + hx(fl(start_r))]
    emit(out, name, 180, pts, truth, ref, extra=extra, flag="suspect_axis")
    z = [Decimal(0), Decimal(0), Decimal(1)]
    zero3 = [Decimal(0)] * 3
    rng = random.Random("axis-cyl-v1/few-4")
    few = [[Decimal(i) / 8, u01(rng), u01(rng)] for i in range(4)]
    emit(out, "few-4", 360, few, (z, zero3, Decimal(1)), None, expect="too_few_points n=4 need=5")
    cop = []
    for i in range(24):
        s, cc = sin_cos(2 * PI * i / 24)
        cop.append([Decimal("0.02") * cc, Decimal("0.02") * s, Decimal("0.125")])
    emit(out, "coplanar", 360, cop, (z, zero3, Decimal("0.02")), None, expect="coplanar n=24 rank=2")
    return "\n".join(out) + "\n"


SHORT_ARCS = 12
SHORT_ARC_NOISE_UM = [Decimal(x) for x in (40, 50, 60, 70, 80, 100)]


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        if not (OUT.exists() and OUT.read_text(encoding="utf-8") == text):
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT} is up to date")
        return 0
    OUT.parent.mkdir(exist_ok=True)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
