#!/usr/bin/env python3
"""Independent reference for KRN.LSQ.AXIS.CONE (least-squares cone) fixtures
(CC-BY-4.0 project data).

Writes fixtures/fit/axis-cone-v1/cases.txt. Nothing here runs the Rust code: stdlib only, every
value is computed in Decimal at 60 significant digits (sin and cos by Taylor series, pi by
Machin's formula, square roots by the decimal module), never with the platform libm.

1. Each point set is generated from a planted cone and rounded once to binary64 (the input).
   A cone is a unit axis d (from the apex into the cone), a reference point p0 on the axis, the
   radius r0 at p0 and the half-angle alpha; a point at axial coordinate t (from p0) and
   azimuth phi with normal offset nu is
   p0 + (t - nu sin(alpha)) d + (r0 + t tan(alpha) + nu cos(alpha)) (cos(phi) e1 + sin(phi) e2).
2. The reference fit is the least-squares cone of those *rounded* points (each binary64 value
   taken exactly as a Fraction), computed by Gauss-Newton in Decimal from the planted truth.
   The residual of a point is its signed orthogonal distance to the generator line in its
   meridian plane, e = (rho - r) cos(alpha) - t sin(alpha), with t = (q - P) . d and rho the
   distance of q from the axis. At each iterate (P, d, r, alpha), with P the foot of the
   perpendicular from the exact centroid and (e1, e2) an orthonormal pair of d, the analytic
   Jacobian of (a, b, u, v, dr, dalpha) (axis tilt by a e1 + b e2, axis shift by u e1 + v e2)
   is [-g1 (t c + rho s), -g2 (t c + rho s), -g1 c, -g2 c, -c, -(rho - r) s - t c] with
   g = perp . e / rho, (s, c) = (sin, cos)(alpha); the normal equations are solved by Gaussian
   elimination in Decimal; then d <- unit(d + a e1 + b e2), P <- P + u e1 + v e2, r <- r + dr,
   alpha <- alpha + dalpha, and P is moved along the new axis to the centroid's foot with the
   radius there (r + shift tan(alpha)). It stops when the step norm is below 1e-40 (at most
   400 iterations, else the script fails). A fixed half-angle drops the dalpha column.
3. The standard uncertainties at the reference cone, sigma * sqrt(diag((J^T J)^-1)) with the
   Jacobian of step 2 about it and sigma^2 = sum e^2 / (m - n), in Decimal, as the frame-free
   combinations sqrt(u_a^2 + u_b^2) (axis, radians), sqrt(u_u^2 + u_v^2) (point across the
   axis, metres), u_r and u_alpha.
4. The expected values are the reference cone (axis direction, the foot point, the radius
   there, the half-angle, the apex P - (r / tan(alpha)) d), its residual RMS and the RMS of the
   least-squares plane (the exact smallest principal moment of the centred points, by Jacobi in
   Decimal), all rounded to binary64 once. The Rust test compares its result with the reference
   within stated tolerances; they are different computations (binary64 Levenberg-Marquardt on
   centred, scaled data) of the same minimum.

Cases (seeded random.Random on the case name; SI metres; 200 points; noise along the surface
normal uniform in [-sqrt(3), sqrt(3)] micrometres; r0 0.01 to 0.05 m; the axial extent 0.5 to
1.5 radii, capped so that the radius stays above 0.3 r0):

- `cone-<alpha>-<cov>-<off>`: half-angles 5, 20, 45 and 70 degrees, angular coverage 360 and
  120 degrees, axis about 0.01 m, 0.3 m and 1000 m from the origin (AC-4's far cones); and
  `cone-30-90-<off>`: 30 degrees at 90 degrees of coverage.
- `fixed-ha-<k>`: the fixed half-angle (the planted one, rounded) with the constrained
  reference.
- Hand-labelled refusals (`expect ill_posed <why>`, from the leaf's rules with a 0.1 degree
  threshold, AC-3): `ha-0.01` (a 0.01 degree cone: near_zero), `ha-89.99` (an 89.99 degree
  cone, the radial band 0.5 to 1.5 r0: near_ninety or cond), `short-band` (a 30 degree cone
  whose axial extent is 3e-4 radii: cond, the tilt and shift of the axis are indistinguishable).
- Hand-labelled `expect hard` cases, data on which the starts can disagree, the axis is weak
  or wrong, or no cone beats the plane: `hard-shortarc-<k>` (30-degree arcs of a 20-degree
  cone 0.2 radii long, 20 to 150 micrometres of noise), `hard-noisy-<k>` (full 30-degree cones
  0.5 radii long with 1 to 16 mm of noise), `hard-blob-<k>` (points uniform in a 40 x 40 mm
  box 0.04 mm to 40 mm thick: no cone at all) and `hard-steep-<k>` (75 to 89.5-degree cones on
  a radial band 0.5 to 1.5 r0, 5 micrometres of noise). The label is the rule "never
  confidently wrong": a fit is either refused (`ill_conditioned`, `implausible_fit`,
  `multi_start`) or returned with `local_optimum_only`, `suspect_axis` or a weak axis, which
  the `Fit` trait refuses, or it is unflagged and its axis is within 10 of its own standard
  uncertainties and 0.01 rad of the planted one. There is no reference for these cases.
- Error cases: `few-5` (5 points: too_few_points n=5 need=6) and `coplanar` (a circle in
  z = 0.125: coplanar n=24 rank=2).

Output lines (hex = 16 lowercase hex digits of an IEEE-754 double):
  case <name> coverage <degrees>
  points <x y z per point>
  truth_axis <3 hex> / truth_point <3 hex> / truth_r <hex> / truth_alpha <hex>   planted cone
  fixed_alpha <hex>                                              constraint (optional)
  ref_axis <3 hex> ref_point <3 hex> ref_r <hex> ref_alpha <hex> ref_apex <3 hex>
  ref_rms <hex> plane_rms <hex>
  ref_u_axis <hex> ref_u_point <hex> ref_u_r <hex> ref_u_alpha <hex>  (no ref_u_alpha when fixed)
  expect start | hard | ill_posed <near_zero | near_ninety | cond | near_ninety_or_cond> | <error>
  end
"""

import random
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

OUT = Path(__file__).with_name("axis-cone-v1") / "cases.txt"
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


def tan(t):
    s, c = sin_cos(t)
    return s / c


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


def deg(x):
    return dec(x) * PI / 180


def cone(name, alpha_deg, cov, dist, extent=None, band=None, noise_um=Decimal(1)):
    """Planted cone points (rounded to binary64, back as exact Decimals) and the truth.
    `extent` fixes the axial extent in radii; `band` = (lo, hi) samples the radius in
    [lo r0, hi r0] instead of the axial coordinate (for steep cones)."""
    rng = random.Random(f"axis-cone-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, dist)
    alpha = deg(alpha_deg)
    s, c = sin_cos(alpha)
    ta = s / c
    arc = deg(cov)
    phase = 2 * PI * u01(rng)
    r0 = Decimal("0.01") + Decimal("0.04") * u01(rng)
    ratio = Decimal("0.5") + Decimal(1) * u01(rng)
    if extent is not None:
        ratio = extent
    cap = Decimal("1.4") / ta
    if ratio > cap:
        ratio = cap
    length = r0 * ratio
    pts = []
    for _ in range(N):
        sp, cp = sin_cos(phase + arc * u01(rng))
        if band is None:
            t = length * (u01(rng) - Decimal("0.5"))
        else:
            rho = r0 * (band[0] + (band[1] - band[0]) * u01(rng))
            t = (rho - r0) / ta
        nu = (2 * u01(rng) - 1) * Decimal(3).sqrt() * noise_um * Decimal("1e-6")
        ax = t - nu * s
        rad = r0 + t * ta + nu * c
        q = [p0[k] + ax * d[k] + rad * (cp * e1[k] + sp * e2[k]) for k in range(3)]
        pts.append([dec(fl(x)) for x in q])
    return pts, (d, p0, r0, alpha)


def centroid(pts):
    n = len(pts)
    return [sum(p[k] for p in pts) / n for k in range(3)]


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


def residuals(pts, p, d, r, alpha):
    s, c = sin_cos(alpha)
    out = []
    for q in pts:
        w = [q[k] - p[k] for k in range(3)]
        t = dot(w, d)
        perp = [w[k] - t * d[k] for k in range(3)]
        rho = norm(perp)
        out.append(((rho - r) * c - t * s, t, perp, rho))
    return out, s, c


def to_foot(cen, p, d, r, alpha):
    """Moves P along the axis to the centroid's foot, with the radius there."""
    shift = dot([cen[k] - p[k] for k in range(3)], d)
    return [p[k] + shift * d[k] for k in range(3)], r + shift * tan(alpha)


def gauss_newton(pts, p, d, r, alpha, free_alpha=True):
    cen = centroid(pts)
    d = unit(d)
    p, r = to_foot(cen, p, d, r, alpha)
    for _ in range(MAX_GN):
        e1, e2 = basis(d)
        rows, res = [], []
        out, s, c = residuals(pts, p, d, r, alpha)
        for ei, t, perp, rho in out:
            g1, g2 = dot(perp, e1) / rho, dot(perp, e2) / rho
            tilt = t * c + rho * s
            row = [-g1 * tilt, -g2 * tilt, -g1 * c, -g2 * c, -c]
            if free_alpha:
                row.append(-(rho - r) * s - t * c)
            rows.append(row)
            res.append(ei)
        n = len(rows[0])
        jtj = [[sum(rw[i] * rw[j] for rw in rows) for j in range(n)] for i in range(n)]
        jtr = [-sum(rw[i] * e for rw, e in zip(rows, res)) for i in range(n)]
        x = solve(jtj, jtr)
        a, b, u, v, dr = x[:5]
        da = x[5] if free_alpha else Decimal(0)
        d = unit([d[k] + a * e1[k] + b * e2[k] for k in range(3)])
        p = [p[k] + u * e1[k] + v * e2[k] for k in range(3)]
        r = r + dr
        alpha = alpha + da
        p, r = to_foot(cen, p, d, r, alpha)
        if sum(xi * xi for xi in x).sqrt() < EPS:
            out, _, _ = residuals(pts, p, d, r, alpha)
            rms = (sum(e[0] * e[0] for e in out) / len(pts)).sqrt()
            if not (0 < alpha < PI / 2 and r > 0):
                raise SystemExit(f"reference cone not canonical (alpha {float(alpha)}, r {float(r)})")
            back = r / tan(alpha)
            apex = [p[k] - back * d[k] for k in range(3)]
            return p, d, r, alpha, apex, rms
    raise SystemExit(f"reference Gauss-Newton did not converge (last step {float(sum(xi * xi for xi in x).sqrt()):e})")


def uncertainty(pts, ref, free_alpha=True):
    """Standard uncertainties sigma * sqrt(diag((J^T J)^-1)) at the reference cone, with the
    Jacobian of the Gauss-Newton step above about it (axis tilt, axis shift, radius, half-angle)
    and sigma^2 = sum e^2 / (m - n); the inverse by Gaussian elimination against unit vectors.
    Returned as the frame-free combinations: axis sqrt(u_a^2 + u_b^2) (radians), point
    sqrt(u_u^2 + u_v^2) (metres), radius (metres), half-angle (radians, None when fixed)."""
    p, d, r, alpha = ref[0], ref[1], ref[2], ref[3]
    e1, e2 = basis(d)
    out, s, c = residuals(pts, p, d, r, alpha)
    rows, res = [], []
    for ei, t, perp, rho in out:
        g1, g2 = dot(perp, e1) / rho, dot(perp, e2) / rho
        tilt = t * c + rho * s
        row = [-g1 * tilt, -g2 * tilt, -g1 * c, -g2 * c, -c]
        if free_alpha:
            row.append(-(rho - r) * s - t * c)
        rows.append(row)
        res.append(ei)
    n = len(rows[0])
    jtj = [[sum(rw[i] * rw[j] for rw in rows) for j in range(n)] for i in range(n)]
    var = [solve(jtj, [Decimal(int(i == k)) for i in range(n)])[k] for k in range(n)]
    s2 = sum(e * e for e in res) / (len(pts) - n)
    u = [(s2 * v).sqrt() for v in var]
    return ((u[0] ** 2 + u[1] ** 2).sqrt(), (u[2] ** 2 + u[3] ** 2).sqrt(), u[4], u[5] if free_alpha else None)


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


def emit(out, name, cov, pts, truth, ref, extra=(), expect="start", free_alpha=True):
    d, p0, r0, alpha = truth
    out.append(f"case {name} coverage {cov}")
    out.append("points " + " ".join(hx(fl(x)) for p in pts for x in p))
    out.append("truth_axis " + hxs(d))
    out.append("truth_point " + hxs(p0))
    out.append("truth_r " + hx(fl(r0)))
    out.append("truth_alpha " + hx(fl(alpha)))
    for line in extra:
        out.append(line)
    if ref is not None:
        rp, rd, rr, ra, rapex, rms = ref
        out.append("ref_axis " + hxs(rd))
        out.append("ref_point " + hxs(rp))
        out.append("ref_r " + hx(fl(rr)))
        out.append("ref_alpha " + hx(fl(ra)))
        out.append("ref_apex " + hxs(rapex))
        out.append("ref_rms " + hx(fl(rms)))
        out.append("plane_rms " + hx(fl(plane_rms(pts))))
        ua, up, ur, uh = uncertainty(pts, ref, free_alpha)
        out.append("ref_u_axis " + hx(fl(ua)))
        out.append("ref_u_point " + hx(fl(up)))
        out.append("ref_u_r " + hx(fl(ur)))
        if uh is not None:
            out.append("ref_u_alpha " + hx(fl(uh)))
    out.append(f"expect {expect}")
    out.append("end")


def render():
    out = [
        "# KRN.LSQ.AXIS.CONE fixture axis-cone-v1 (CC-BY-4.0). Generated by fixtures/fit/make_axis_cone_fixtures.py; do not edit.",
        "# Seeded partial cones with Decimal least-squares references, a fixed half-angle, ill-posed and error cases.",
    ]
    dists = (("0", Decimal("0.01")), ("1", Decimal("0.3")), ("km", Decimal(1000)))
    plan = [(a, cov) for a in (5, 20, 45, 70) for cov in (360, 120)] + [(30, 90)]
    for alpha, cov in plan:
        for tag, dist in dists:
            name = f"cone-{alpha}-{cov}-{tag}"
            pts, truth = cone(name, alpha, cov, dist)
            d, p0, r0, a = truth
            emit(out, name, cov, pts, truth, gauss_newton(pts, p0, d, r0, a))
    for k, (alpha, cov) in enumerate(((30, 360), (60, 180))):
        name = f"fixed-ha-{k}"
        pts, truth = cone(name, alpha, cov, Decimal("0.3"))
        d, p0, r0, a = truth
        afix = dec(fl(a))
        ref = gauss_newton(pts, p0, d, r0, afix, free_alpha=False)
        emit(out, name, cov, pts, truth, ref, extra=[f"fixed_alpha {hx(fl(afix))}"], free_alpha=False)
    name = "ha-0.01"
    pts, truth = cone(name, Decimal("0.01"), 360, Decimal("0.3"), extent=Decimal(2))
    emit(out, name, 360, pts, truth, None, expect="ill_posed near_zero")
    name = "ha-89.99"
    pts, truth = cone(name, Decimal("89.99"), 360, Decimal("0.3"), band=(Decimal("0.5"), Decimal("1.5")))
    emit(out, name, 360, pts, truth, None, expect="ill_posed near_ninety_or_cond")
    name = "short-band"
    pts, truth = cone(name, 30, 360, Decimal("0.3"), extent=Decimal("0.0003"))
    emit(out, name, 360, pts, truth, None, expect="ill_posed cond")
    for k in range(SHORT_ARCS):
        name = f"hard-shortarc-{k}"
        noise = SHORT_ARC_NOISE_UM[k % len(SHORT_ARC_NOISE_UM)]
        pts, truth = cone(name, 20, 30, Decimal("0.3"), extent=Decimal("0.2"), noise_um=noise)
        emit(out, name, 30, pts, truth, None, expect="hard")
    for k, noise in enumerate(NOISY_UM):
        name = f"hard-noisy-{k}"
        pts, truth = cone(name, 30, 360, Decimal("0.3"), extent=Decimal("0.5"), noise_um=noise)
        emit(out, name, 360, pts, truth, None, expect="hard")
    for k, thick in BLOBS:
        name = f"hard-blob-{k}"
        pts, truth = blob(name, Decimal(thick))
        emit(out, name, 360, pts, truth, None, expect="hard")
    for k, alpha in enumerate(STEEP_DEG):
        name = f"hard-steep-{k}"
        pts, truth = cone(name, alpha, 360, Decimal("0.3"), band=(Decimal("0.5"), Decimal("1.5")), noise_um=Decimal(5))
        emit(out, name, 360, pts, truth, None, expect="hard")
    z = [Decimal(0), Decimal(0), Decimal(1)]
    zero3 = [Decimal(0)] * 3
    rng = random.Random("axis-cone-v1/few-5")
    few = [[Decimal(i) / 8, u01(rng), u01(rng)] for i in range(5)]
    emit(out, "few-5", 360, few, (z, zero3, Decimal(1), deg(30)), None, expect="too_few_points n=5 need=6")
    cop = []
    for i in range(24):
        s, cc = sin_cos(2 * PI * i / 24)
        cop.append([Decimal("0.02") * cc, Decimal("0.02") * s, Decimal("0.125")])
    emit(out, "coplanar", 360, cop, (z, zero3, Decimal("0.02"), deg(30)), None, expect="coplanar n=24 rank=2")
    return "\n".join(out) + "\n"


SHORT_ARCS = 12
SHORT_ARC_NOISE_UM = [Decimal(x) for x in (20, 40, 60, 80, 100, 150)]
NOISY_UM = [Decimal(x) for x in (1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000, 8000, 10000, 12000, 16000)]
BLOBS = [(0, "0.04"), (2, "0.01"), (3, "0.004"), (5, "0.00004"), (72, "0.012")]
STEEP_DEG = [Decimal(x) for x in ("75", "80", "85", "88", "89.5")]


def blob(name, thick):
    """Points uniform in a 40 x 40 mm box of thickness `thick` 0.3 m out (no cone): the truth
    lines are placeholders (the box's frame, a 30 degree half-angle)."""
    rng = random.Random(f"axis-cone-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, Decimal("0.3"))
    pts = []
    for _ in range(N):
        a, b, h = (u01(rng) - Decimal("0.5")) * Decimal("0.04"), (u01(rng) - Decimal("0.5")) * Decimal("0.04"), (u01(rng) - Decimal("0.5")) * thick
        q = [p0[k] + a * e1[k] + b * e2[k] + h * d[k] for k in range(3)]
        pts.append([dec(fl(x)) for x in q])
    return pts, (d, p0, Decimal("0.02"), deg(30))


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
