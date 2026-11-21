#!/usr/bin/env python3
"""Independent reference for KRN.LSQ.RND (least-squares circle and sphere) fixtures
(CC-BY-4.0 project data).

Writes fixtures/fit/rnd-partial-v1/cases.txt. Nothing here runs the Rust code: stdlib only,
every value is computed in Decimal at 60 significant digits (sin and cos by Taylor series, pi
by Machin's formula, square roots by the decimal module), never with the platform libm.

1. Each point set is generated from planted geometry and rounded once to binary64 (the input).
2. The reference fit is the least-squares element of those *rounded* points (each binary64
   value taken exactly as a Fraction), computed by Gauss-Newton in Decimal from the planted
   truth, with the normal equations solved by Gaussian elimination:
   - circle2: the points are projected orthogonally onto the projection plane (any orthonormal
     basis of it; the result is basis-free), residuals rho_i - r over (centre, r);
   - circle3: two residuals per point, the height h_i = (q_i - c) . n over the circle's plane and
     rho_i - r with rho_i the in-plane distance from the centre (their squares sum to the squared
     3D distance to the circle); parameters: two tangent offsets of n, the centre (3) and r; after
     each step n <- unit(n + a e1 + b e2), c <- c + x e1 + y e2 + z n;
   - sphere: residuals |q_i - c| - r over (c, r).
   A fixed radius drops the r column. It stops when the step norm is below 1e-40 (at most 400
   iterations, else the script fails).
3. The expected values are the reference centre (3D, also for circle2: on the projection plane),
   normal (circle3), radius and residual RMS (circle3: of the 3D distances), rounded to binary64
   once. The Rust test compares its result with them within stated tolerances; they are
   different computations (binary64 Levenberg-Marquardt on centred, scaled data) of the same
   minimum.

Cases (seeded random.Random on the case name; SI metres; 200 points; noise uniform in
[-sqrt(3), sqrt(3)] micrometres, radial for all and also along the normal for circle3):

- `c2-<cov>`, `c3-<cov>-<off>`, `sp-<cov>-<off>`: coverage 360, 90 and 45 degrees (for the
  sphere: a spherical cap whose apex angle is the coverage, 360 a full sphere); centre about
  0.01 m, 0.3 m and 1000 m from the origin (c2: in the plane z = 0, the default projection plane,
  with z coordinates spread over +-5 mm that the projection removes); radius 0.005 to 0.05 m.
- `c2-tilt`: a circle in a tilted plane through a point 0.3 m out, fitted with that projection
  plane.
- `*-fixed-r`: the fixed-radius constraint (the planted radius) and its constrained reference.
- `*-20`: 20-degree arcs and caps, hand-labelled `insufficient_coverage` (below the leaf's
  30-degree default, `ls_rnd::default_params`).
- Error cases: `sp-few-2` (2 points: too_few_points n=2 need=4) and `c3-line` (10 collinear
  points: collinear n=10 rank=1).
- Review of #417, hand-labelled:
  - `sp-ring-<deg>`: 200 points on one circle of a sphere (polar angle 30, 45 and 60 degrees
    about a random axis, full azimuth, radial noise as above). A ring does not determine a
    sphere (every sphere through the circle fits it), so each must be refused: `expect refused`,
    any of insufficient_coverage, coplanar or ill_conditioned, never a fit.
  - `sp-coplanar`: 50 points in the plane z = 0.125 (coplanar n=50 rank=2).
  - `c2-collinear`: 20 points on a line of the default projection plane (collinear n=20 rank=1).
  - `c2-cond`: a noise-free arc of 0.02 degrees (radius 0.02 m) fitted with `min_coverage 0`.
    The script computes the exact 2-norm condition number of the column-scaled Jacobian at the
    planted circle in Decimal and requires it above 2e8, twice the 1e8 fit-level limit:
    ill_conditioned.
  - `c3-maxiter`: a 90-degree arc fitted with `max_iter 1` (not_converged).
  - `sp-fixed-r-zero`: fixed radius 0 (invalid_params constraints.fixed_radius).
  - `sp-fixed-r-tiny`: a 90-degree cap fitted with a fixed radius of 1/100 of the planted one:
    no sphere of that radius is near the data, `expect refused`.

Output lines (hex = 16 lowercase hex digits of an IEEE-754 double):
  case <name> kind <circle2 | circle3 | sphere> coverage <degrees>
  points <x y z per point>
  plane <p 3 hex> <n 3 hex>                 circle2 projection plane (optional: default z = 0)
  fixed_r <hex>                             constraint (optional)
  min_coverage <hex> / max_iter <decimal>   parameter overrides (optional)
  truth_c <3 hex> / truth_n <3 hex> / truth_r <hex>     planted geometry (truth_n: circles)
  ref_c <3 hex> ref_n <3 hex> ref_r <hex> ref_rms <hex>  reference (ref_n: circle3)
  expect start | <error code fields>
  end
"""

import random
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

OUT = Path(__file__).with_name("rnd-partial-v1") / "cases.txt"
getcontext().prec = 60
N = 200
EPS = Decimal(10) ** -40
MAX_GN = 400
UM = Decimal("1e-6")


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


def noise(rng):
    return (2 * u01(rng) - 1) * Decimal(3).sqrt() * UM


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


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


def rand_dir(rng):
    while True:
        v = [2 * u01(rng) - 1 for _ in range(3)]
        n2 = dot(v, v)
        if Decimal("0.01") < n2 <= 1:
            return unit(v)


def offset(rng, dist):
    v = rand_dir(rng)
    return [x * dist * (Decimal("0.9") + Decimal("0.2") * u01(rng)) for x in v]


def rounded(p):
    return [dec(fl(x)) for x in p]


def sign_norm(n):
    k = next(k for k in range(3) if n[k] != 0)
    return [-x for x in n] if n[k] < 0 else n


def radius(rng):
    return Decimal("0.005") + Decimal("0.045") * u01(rng)


def circle_points(rng, c, n, r, cov, out_of_plane):
    e1, e2 = basis(n)
    arc = dec(cov) * PI / 180
    phase = 2 * PI * u01(rng)
    pts = []
    for _ in range(N):
        s, co = sin_cos(phase + arc * u01(rng))
        rr = r + noise(rng)
        h = out_of_plane(rng)
        pts.append(rounded([c[k] + rr * (co * e1[k] + s * e2[k]) + h * n[k] for k in range(3)]))
    return pts


def sphere_points(rng, c, r, cov):
    axis = rand_dir(rng)
    e1, e2 = basis(axis)
    # Cap of apex angle cov: cos(theta) uniform in [cos(cov / 2), 1]; 360 is the full sphere.
    _, ca = sin_cos(dec(cov) * PI / 360)
    pts = []
    for _ in range(N):
        z = ca + (1 - ca) * u01(rng)
        sxy = (1 - z * z).sqrt()
        s, co = sin_cos(2 * PI * u01(rng))
        rr = r + noise(rng)
        d = [z * axis[k] + sxy * (co * e1[k] + s * e2[k]) for k in range(3)]
        pts.append(rounded([c[k] + rr * d[k] for k in range(3)]))
    return pts


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


def gn_step(rows, res):
    n = len(rows[0])
    jtj = [[sum(rw[i] * rw[j] for rw in rows) for j in range(n)] for i in range(n)]
    jtr = [-sum(rw[i] * e for rw, e in zip(rows, res)) for i in range(n)]
    return solve(jtj, jtr)


def step_norm(x):
    return sum(xi * xi for xi in x).sqrt()


def fail(x):
    raise SystemExit(f"reference Gauss-Newton did not converge (last step {float(step_norm(x)):e})")


def gn_round(pts, c, r, free_r=True):
    """Circle (2D points as [u, v]) or sphere (3D points): residuals |q - c| - r."""
    dim = len(c)
    for _ in range(MAX_GN):
        rows, res = [], []
        for q in pts:
            w = [q[k] - c[k] for k in range(dim)]
            rho = norm(w)
            rows.append([-x / rho for x in w] + ([Decimal(-1)] if free_r else []))
            res.append(rho - r)
        x = gn_step(rows, res)
        c = [c[k] + x[k] for k in range(dim)]
        if free_r:
            r = r + x[dim]
        if step_norm(x) < EPS:
            d = [norm([q[k] - c[k] for k in range(dim)]) - r for q in pts]
            return c, r, (sum(e * e for e in d) / len(pts)).sqrt()
    fail(x)


def c3_parts(q, c, n):
    w = [q[k] - c[k] for k in range(3)]
    h = dot(w, n)
    perp = [w[k] - h * n[k] for k in range(3)]
    return w, h, perp, norm(perp)


def gn_circle3(pts, c, n, r, free_r=True):
    n = unit(n)
    for _ in range(MAX_GN):
        e1, e2 = basis(n)
        rows, res = [], []
        for q in pts:
            w, h, perp, rho = c3_parts(q, c, n)
            g = [x / rho for x in perp]
            tail_h = [Decimal(0)] if free_r else []
            tail_r = [Decimal(-1)] if free_r else []
            rows.append([dot(w, e1), dot(w, e2), -dot(n, e1), -dot(n, e2), Decimal(-1)] + tail_h)
            rows.append([-h * dot(g, e1), -h * dot(g, e2), -dot(g, e1), -dot(g, e2), -dot(g, n)] + tail_r)
            res.extend([h, rho - r])
        x = gn_step(rows, res)
        a, b, cx, cy, cz = x[:5]
        c = [c[k] + cx * e1[k] + cy * e2[k] + cz * n[k] for k in range(3)]
        n = unit([n[k] + a * e1[k] + b * e2[k] for k in range(3)])
        if free_r:
            r = r + x[5]
        if step_norm(x) < EPS:
            d2 = []
            for q in pts:
                _, h, _, rho = c3_parts(q, c, n)
                d2.append(h * h + (rho - r) ** 2)
            return c, sign_norm(n), r, (sum(d2) / len(pts)).sqrt()
    fail(x)


def emit(out, name, kind, cov, pts, truth, ref=None, extra=(), expect="start"):
    tc, tn, tr = truth
    out.append(f"case {name} kind {kind} coverage {cov}")
    out.append("points " + " ".join(hx(fl(x)) for p in pts for x in p))
    out.extend(extra)
    out.append("truth_c " + hxs(tc))
    if tn is not None:
        out.append("truth_n " + hxs(tn))
    out.append("truth_r " + hx(fl(tr)))
    if ref is not None:
        rc, rn, rr, rms = ref
        out.append("ref_c " + hxs(rc))
        if rn is not None:
            out.append("ref_n " + hxs(rn))
        out.append("ref_r " + hx(fl(rr)))
        out.append("ref_rms " + hx(fl(rms)))
    out.append(f"expect {expect}")
    out.append("end")


Z = [Decimal(0), Decimal(0), Decimal(1)]
COVER_ERR = f"insufficient_coverage n={N}"
DISTS = (("0", Decimal("0.01")), ("1", Decimal("0.3")), ("km", Decimal(1000)))


def circle2_case(out, name, cov, fixed=False, tilt=False, expect="start"):
    rng = random.Random(f"rnd-partial-v1/{name}")
    r = radius(rng)
    extra = []
    if tilt:
        n = rand_dir(rng)
        c = offset(rng, Decimal("0.3"))
        pts = circle_points(rng, c, n, r, cov, lambda g: Decimal(0))
        # The projection plane passes through a rounded point near the centre.
        p0 = rounded([c[k] + Decimal("0.001") * n[k] for k in range(3)])
        nr = rounded(n)
        extra.append("plane " + hxs(p0) + " " + hxs(nr))
        nu = unit(nr)
    else:
        c = offset(rng, Decimal("0.3"))
        c[2] = Decimal(0)
        n = Z
        pts = circle_points(rng, c, n, r, cov, lambda g: (2 * u01(g) - 1) * Decimal("0.005"))
        p0, nu = [Decimal(0)] * 3, Z
    if fixed:
        extra.append("fixed_r " + hx(fl(r)))
    ref = None
    if expect == "start":
        e1, e2 = basis(nu)
        uv = [[dot([q[k] - p0[k] for k in range(3)], e) for e in (e1, e2)] for q in pts]
        cuv = [dot([c[k] - p0[k] for k in range(3)], e) for e in (e1, e2)]
        (cu, cv), rr, rms = gn_round(uv, cuv, r, free_r=not fixed)
        rc = [p0[k] + cu * e1[k] + cv * e2[k] for k in range(3)]
        ref = (rc, None, rr, rms)
    emit(out, name, "circle2", cov, pts, (c, n, r), ref, extra, expect)


def circle3_case(out, name, cov, dist, fixed=False, expect="start"):
    rng = random.Random(f"rnd-partial-v1/{name}")
    r = radius(rng)
    n = rand_dir(rng)
    c = offset(rng, dist)
    pts = circle_points(rng, c, n, r, cov, noise)
    extra = ["fixed_r " + hx(fl(r))] if fixed else []
    ref = gn_circle3(pts, c, n, r, free_r=not fixed) if expect == "start" else None
    emit(out, name, "circle3", cov, pts, (c, n, r), ref, extra, expect)


def sphere_case(out, name, cov, dist, fixed=False, expect="start"):
    rng = random.Random(f"rnd-partial-v1/{name}")
    r = radius(rng)
    c = offset(rng, dist)
    pts = sphere_points(rng, c, r, cov)
    extra = ["fixed_r " + hx(fl(r))] if fixed else []
    ref = None
    if expect == "start":
        rc, rr, rms = gn_round(pts, c, r, free_r=not fixed)
        ref = (rc, None, rr, rms)
    emit(out, name, "sphere", cov, pts, (c, None, r), ref, extra, expect)


def sym_eig_min_max(a):
    """Extreme eigenvalues of a small symmetric Decimal matrix by cyclic Jacobi."""
    n = len(a)
    a = [row[:] for row in a]
    for _ in range(200):
        off = sum(a[i][j] ** 2 for i in range(n) for j in range(n) if i != j)
        if off < Decimal(10) ** -110:
            break
        for p_ in range(n):
            for q_ in range(p_ + 1, n):
                if a[p_][q_] == 0:
                    continue
                theta = (a[q_][q_] - a[p_][p_]) / (2 * a[p_][q_])
                t = (1 if theta >= 0 else -1) / (abs(theta) + (theta * theta + 1).sqrt())
                cs = 1 / (t * t + 1).sqrt()
                sn = t * cs
                for k in range(n):
                    akp, akq = a[k][p_], a[k][q_]
                    a[k][p_], a[k][q_] = cs * akp - sn * akq, sn * akp + cs * akq
                for k in range(n):
                    apk, aqk = a[p_][k], a[q_][k]
                    a[p_][k], a[q_][k] = cs * apk - sn * aqk, sn * apk + cs * aqk
    d = [a[i][i] for i in range(n)]
    return min(d), max(d)


def circle_cond(uv, c, r):
    """2-norm condition number of the column-scaled Jacobian [-g_u, -g_v, -1] at (c, r)."""
    rows = []
    for q in uv:
        w = [q[0] - c[0], q[1] - c[1]]
        rho = norm(w)
        rows.append([-w[0] / rho, -w[1] / rho, Decimal(-1)])
    scale = [sum(rw[j] ** 2 for rw in rows).sqrt() for j in range(3)]
    rows = [[rw[j] / scale[j] for j in range(3)] for rw in rows]
    jtj = [[sum(rw[i] * rw[j] for rw in rows) for j in range(3)] for i in range(3)]
    lo, hi = sym_eig_min_max(jtj)
    return (hi / lo).sqrt()


def review_cases(out):
    # Rings on a sphere: refused.
    for deg in (30, 45, 60):
        name = f"sp-ring-{deg}"
        rng = random.Random(f"rnd-partial-v1/{name}")
        r = radius(rng)
        c = offset(rng, Decimal("0.3"))
        axis = rand_dir(rng)
        e1, e2 = basis(axis)
        st, ct = sin_cos(dec(deg) * PI / 180)
        pts = []
        for _ in range(N):
            s, co = sin_cos(2 * PI * u01(rng))
            rr = r + noise(rng)
            d = [ct * axis[k] + st * (co * e1[k] + s * e2[k]) for k in range(3)]
            pts.append(rounded([c[k] + rr * d[k] for k in range(3)]))
        emit(out, name, "sphere", 0, pts, (c, None, r), expect="refused")
    rng = random.Random("rnd-partial-v1/sp-coplanar")
    cop = [rounded([Decimal("0.02") * u01(rng), Decimal("0.02") * u01(rng), Decimal("0.125")]) for _ in range(50)]
    emit(out, "sp-coplanar", "sphere", 0, cop, ([Decimal(0)] * 3, None, Decimal(1)), expect="coplanar n=50 rank=2")
    col = [[Decimal(i) / 16, Decimal(i) / 8, Decimal(i % 3) / 4] for i in range(20)]
    emit(out, "c2-collinear", "circle2", 0, col, ([Decimal(0)] * 3, Z, Decimal(1)), expect="collinear n=20 rank=1")
    # A noise-free arc of 0.02 degrees: the condition number exceeds the 1e8 fit-level limit.
    r = Decimal("0.02")
    c = [Decimal("0.3"), Decimal("0.1"), Decimal(0)]
    arc = Decimal("0.02") * PI / 180
    pts = []
    for i in range(50):
        s, co = sin_cos(arc * i / 49)
        pts.append(rounded([c[0] + r * co, c[1] + r * s, Decimal(0)]))
    cond = circle_cond([[q[0], q[1]] for q in pts], c[:2], r)
    if cond <= Decimal("2e8"):
        raise SystemExit(f"c2-cond: condition {float(cond):e} is not above 2e8")
    emit(out, "c2-cond", "circle2", 0, pts, (c, Z, r), extra=["min_coverage " + hx(0.0)], expect="ill_conditioned")
    name = "c3-maxiter"
    rng = random.Random(f"rnd-partial-v1/{name}")
    r = radius(rng)
    n = rand_dir(rng)
    c = offset(rng, Decimal("0.3"))
    pts = circle_points(rng, c, n, r, 90, noise)
    emit(out, name, "circle3", 90, pts, (c, n, r), extra=["max_iter 1"], expect="not_converged")
    for name, factor, expect in (
        ("sp-fixed-r-zero", Decimal(0), "invalid_params constraints.fixed_radius"),
        ("sp-fixed-r-tiny", Decimal("0.01"), "refused"),
    ):
        rng = random.Random(f"rnd-partial-v1/{name}")
        r = radius(rng)
        c = offset(rng, Decimal("0.3"))
        pts = sphere_points(rng, c, r, 90)
        emit(out, name, "sphere", 90, pts, (c, None, r), extra=["fixed_r " + hx(fl(r * factor))], expect=expect)


def render():
    out = [
        "# KRN.LSQ.RND fixture rnd-partial-v1 (CC-BY-4.0). Generated by fixtures/fit/make_rnd_fixtures.py; do not edit.",
        "# Seeded partial circles and spheres with Decimal least-squares references, constraints and error cases.",
    ]
    for cov in (360, 90, 45):
        circle2_case(out, f"c2-{cov}", cov)
    circle2_case(out, "c2-tilt", 90, tilt=True)
    circle2_case(out, "c2-fixed-r", 90, fixed=True)
    circle2_case(out, "c2-20", 20, expect=COVER_ERR)
    for cov in (360, 90, 45):
        for tag, dist in DISTS:
            circle3_case(out, f"c3-{cov}-{tag}", cov, dist)
    circle3_case(out, "c3-fixed-r", 90, Decimal("0.3"), fixed=True)
    circle3_case(out, "c3-20", 20, Decimal("0.3"), expect=COVER_ERR)
    for cov in (360, 90, 45):
        for tag, dist in DISTS:
            sphere_case(out, f"sp-{cov}-{tag}", cov, dist)
    sphere_case(out, "sp-fixed-r", 90, Decimal("0.3"), fixed=True)
    sphere_case(out, "sp-20", 20, Decimal("0.3"), expect=COVER_ERR)
    rng = random.Random("rnd-partial-v1/sp-few-2")
    few = [[u01(rng), u01(rng), u01(rng)] for _ in range(2)]
    emit(out, "sp-few-2", "sphere", 0, few, ([Decimal(0)] * 3, None, Decimal(1)), expect="too_few_points n=2 need=4")
    line = [[Decimal(i) / 16, Decimal(i) / 8, Decimal("0.25")] for i in range(10)]
    emit(out, "c3-line", "circle3", 0, line, ([Decimal(0)] * 3, Z, Decimal(1)), expect="collinear n=10 rank=1")
    review_cases(out)
    return "\n".join(out) + "\n"


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
