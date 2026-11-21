#!/usr/bin/env python3
"""Independent reference for KRN.ENV.MCC.CYL (minimum circumscribed cylinder) fixtures
(CC-BY-4.0 project data).

Writes fixtures/fit/mcc-cyl-v1/cases.txt. Nothing here runs the Rust code: stdlib only, every
value is computed in Decimal at 60 significant digits (sin and cos by Taylor series, pi by
Machin's formula, square roots by the decimal module), never with the platform libm.

Planted envelopes (`env-*`). A cylinder (axis d, mid-height point p0, radius R, length L) with
five contact points on its surface and every other point strictly inside it (form error: the
radius of a non-contact point is R - delta, delta uniform in [0.5, 20] micrometres). The five
contacts (angle theta_k, height t_k) are drawn until the KKT system of the minimum circumscribed
cylinder at the planted cylinder,
    sum lambda_k = 1,  sum lambda_k (cos, sin) theta_k = 0,  sum lambda_k t_k (cos, sin) theta_k = 0,
has a solution with every lambda_k >= 0.05 (five independent active rows, positive multipliers:
a strict local minimum). After rounding every point to binary64, the reference is recomputed
exactly: Newton's method in Decimal on the five equations dist(q_k, axis) = R (unknowns: two
tangent-plane axis parameters, two axis offsets, R) from the planted values, to a step below
1e-40; then the multipliers are recomputed at the reference and must still be >= 0.05, and every
other point must be inside by at least 0.4 micrometres. So the reference is the exact strict local
minimum of the rounded data, and the expected contact set is the five planted indices.

Other cases:
- `tie`: a planted envelope with a sixth point placed on the surface at a zero-multiplier position
  (between two contacts), so after rounding it is tied with the active set to about 1e-16 m.
- `twoaxes`: points in a slab 1.00 x 1.02 x 0.25 (scaled by 20 mm), whose enclosing cylinders
  along the two long sides are two near-equal envelope axes (AC-2 as specified). Hand-labelled
  `flag two_axes`: the fit must report disagreement or fail closed, never return one axis
  unflagged.
- `half-1066-<k>` and `shortarc-<k>`: half cylinders 1.066 radii long and 30-degree arcs 0.2 radii
  long (150 points; axes 0.01 m and 1 km from the origin for the half cylinders), hand-labelled
  `flag never_confidently_wrong` (the enclosing cylinder of such data is not
  the measured surface's, and there is no reference: a fit must be refused or flagged).
- `pilot` (review of #374): a short pilot diameter, radius 50 mm, length 2.5 mm, 20 um two-lobe
  ovality, +-0.3 um noise, 600 points. Its near-axis local minimum (radius about r + 20 um) is
  not the global one: `better_axis` (the ovality's major axis) and `better_r`, the radius of a
  feasible cylinder along it (exact in Decimal), which is below r + 10 um. Hand-labelled `flag
  pilot_not_global`: a fit must be refused or flagged unless its radius is at most `better_r`.
- `pilot-5mm` (second review of #374): the same pilot 5 mm long with theta and height uniform.
  Four fixed directions all pass from their pair bounds here, yet a cylinder tilted towards the
  major axis is smaller: `better_axis` and `better_r` from a search over tilts (exact feasible
  radius). Same flag and rule as `pilot`.
- `oval-free` and `oval-fixed`: an oval long cylinder with four exact contacts at the major-axis
  ends of both end faces, free direction and fixed at the planted axis. Four active rows for
  five parameters, two for three: a degenerate active set. Hand-labelled `flag
  degenerate_active_set`: refused with code `solver` (fail closed; the certifier does not
  support a degenerate active set).
- `disc`: a ring of points 0.1 mm thick and 20 mm in radius (AC-4): `expect ill_conditioned`.
- `few-4`: four points: `expect too_few_points n=4 need=5`.

Output lines (hex = 16 lowercase hex digits of an IEEE-754 double):
  case <name> coverage <degrees>
  points <x y z per point>
  ref_axis <3 hex> ref_point <3 hex> ref_r <hex>   reference cylinder (planted cases)
  contacts <indices>                               expected active set, ascending
  tie_point <index>                                (tie case)
  fixed_dir <3 hex>                                (fixed-direction cases)
  better_axis <3 hex> better_r <hex>               (pilot: a feasible cylinder that beats near-axis fits)
  flag <...>                                       (hand-labelled cases)
  expect start | <error code fields>
  end
"""

import random
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

OUT = Path(__file__).with_name("mcc-cyl-v1") / "cases.txt"
getcontext().prec = 60
EPS = Decimal(10) ** -40
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
        if Decimal("0.01") < dot(v, v) <= 1:
            break
    d = unit(v)
    e1, e2 = basis(d)
    return d, e1, e2


def offset(rng, dist):
    v = unit([2 * u01(rng) - 1 for _ in range(3)])
    return [x * dist * (Decimal("0.9") + Decimal("0.2") * u01(rng)) for x in v]


def at(p, d, e1, e2, t, a, b):
    return [p[k] + t * d[k] + a * e1[k] + b * e2[k] for k in range(3)]


def rounded(p):
    return [dec(fl(x)) for x in p]


def solve(a, b):
    n = len(b)
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for col in range(n):
        piv = max(range(col, n), key=lambda i: abs(m[i][col]))
        if m[piv][col] == 0:
            return None
        m[col], m[piv] = m[piv], m[col]
        for i in range(col + 1, n):
            f = m[i][col] / m[col][col]
            for j in range(col, n + 1):
                m[i][j] -= f * m[col][j]
    x = [Decimal(0)] * n
    for i in reversed(range(n)):
        x[i] = (m[i][n] - sum(m[i][j] * x[j] for j in range(i + 1, n))) / m[i][i]
    return x


def kkt(angles, heights):
    rows = [[Decimal(1)] * 5]
    cs = [sin_cos(a) for a in angles]
    rows.append([c for s, c in cs])
    rows.append([s for s, c in cs])
    rows.append([t * c for (s, c), t in zip(cs, heights)])
    rows.append([t * s for (s, c), t in zip(cs, heights)])
    return solve(rows, [Decimal(1), Decimal(0), Decimal(0), Decimal(0), Decimal(0)])


def geometry(d0, e1, e2, p0, x):
    """Axis direction (unit) and point of the tangent-plane parameters x = (a, b, u, v)."""
    d = unit([d0[k] + x[0] * e1[k] + x[1] * e2[k] for k in range(3)])
    p = [p0[k] + x[2] * e1[k] + x[3] * e2[k] for k in range(3)]
    return d, p


def dist(q, p, d):
    w = [q[k] - p[k] for k in range(3)]
    t = dot(w, d)
    perp = [w[k] - t * d[k] for k in range(3)]
    return norm(perp), t, perp


def reference(pts, contacts, d0, p0, r0):
    """Newton on dist(q_k) = R over the five contacts; returns (d, p, R, lambdas)."""
    e1, e2 = basis(d0)
    x = [Decimal(0)] * 4 + [r0]
    for _ in range(100):
        d, p = geometry(d0, e1, e2, p0, x)
        # Jacobian by central differences would need libm-free derivatives: use the analytic
        # gradient in the frame (e1, e2) of d0 (exact derivative of the parameterisation).
        rows, res = [], []
        n_d = norm([d0[k] + x[0] * e1[k] + x[1] * e2[k] for k in range(3)])
        for k in contacts:
            rho, t, perp = dist(pts[k], p, d)
            g1, g2 = dot(perp, e1) / rho, dot(perp, e2) / rho
            rows.append([-t * g1 / n_d, -t * g2 / n_d, -g1, -g2, Decimal(-1)])
            res.append(rho - x[4])
        step = solve(rows, [-r for r in res])
        x = [x[i] + step[i] for i in range(5)]
        if sum(s * s for s in step).sqrt() < EPS:
            break
    else:
        raise SystemExit("reference Newton did not converge")
    d, p = geometry(d0, e1, e2, p0, x)
    # Multipliers: grad R = sum lambda_k grad rho_k in (a, b, u, v, R): solve J^T lambda = e_R
    # with J the rows of (rho - R) (R column -1), i.e. -J^T lambda = e_R.
    jt = [[-rows[j][i] for j in range(5)] for i in range(5)]
    lam = solve(jt, [Decimal(0)] * 4 + [Decimal(1)])
    return d, p, x[4], lam


def centroid(pts):
    n = len(pts)
    return [sum(p[k] for p in pts) / n for k in range(3)]


def foot(c, p, d):
    w = [c[k] - p[k] for k in range(3)]
    t = dot(w, d)
    return [p[k] + t * d[k] for k in range(3)]


def canonical(d):
    k = next(i for i in range(3) if d[i] != 0)
    return [-x for x in d] if d[k] < 0 else d


def planted(name, n, cov, dist_off, tie=False):
    rng = random.Random(f"mcc-cyl-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, dist_off)
    r = Decimal("0.01") + Decimal("0.04") * u01(rng)
    length = r * (1 + 2 * u01(rng))
    arc = dec(cov) * PI / 180
    phase = 2 * PI * u01(rng)
    while True:
        angles = [phase + arc * u01(rng) for _ in range(5)]
        heights = [length * (u01(rng) - Decimal("0.5")) for _ in range(5)]
        lam = kkt(angles, heights)
        if lam is not None and min(lam) >= Decimal("0.05"):
            break
    contact_pts = []
    for a, t in zip(angles, heights):
        s, c = sin_cos(a)
        contact_pts.append(at(p0, d, e1, e2, t, r * c, r * s))
    others = []
    for _ in range(n - 5 - (1 if tie else 0)):
        s, c = sin_cos(phase + arc * u01(rng))
        t = length * (u01(rng) - Decimal("0.5"))
        rr = r - (Decimal("0.5") + Decimal("19.5") * u01(rng)) * UM
        others.append(at(p0, d, e1, e2, t, rr * c, rr * s))
    tie_pt = None
    if tie:
        # On the surface, between contacts 0 and 1 in angle and at contact 0's height.
        s, c = sin_cos((angles[0] + angles[1]) / 2)
        tie_pt = at(p0, d, e1, e2, heights[0], r * c, r * s)
    order = list(range(n))
    rng.shuffle(order)
    pts = [None] * n
    for k in range(5):
        pts[order[k]] = contact_pts[k]
    off = 5
    if tie:
        pts[order[5]] = tie_pt
        off = 6
    for k, q in enumerate(others):
        pts[order[off + k]] = q
    pts = [rounded(q) for q in pts]
    contacts = sorted(order[:5])
    rd, rp, rr, lam = reference(pts, [order[k] for k in range(5)], d, p0, r)
    if min(lam) < Decimal("0.05"):
        raise SystemExit(f"{name}: multiplier {min(lam)} after rounding")
    for i, q in enumerate(pts):
        if i in contacts or (tie and i == order[5]):
            continue
        if dist(q, rp, rd)[0] > rr - Decimal("0.4") * UM:
            raise SystemExit(f"{name}: point {i} not inside")
    c = centroid(pts)
    ref = (canonical(rd), foot(c, rp, rd), rr)
    return pts, ref, contacts, (order[5] if tie else None)


def surface_points(name, n, cov, ratio, dist_off, noise_um=Decimal(1)):
    rng = random.Random(f"mcc-cyl-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, dist_off)
    r = Decimal("0.01") + Decimal("0.04") * u01(rng)
    length = r * ratio
    arc = dec(cov) * PI / 180
    phase = 2 * PI * u01(rng)
    pts = []
    for _ in range(n):
        s, c = sin_cos(phase + arc * u01(rng))
        t = length * (u01(rng) - Decimal("0.5"))
        rr = r + (2 * u01(rng) - 1) * Decimal(3).sqrt() * noise_um * UM
        pts.append(rounded(at(p0, d, e1, e2, t, rr * c, rr * s)))
    return pts, d


def pilot(name):
    """A short pilot diameter (review of #374): radius 50 mm, length 2.5 mm, two-lobe ovality
    r(theta) = r + 20 um cos(2 theta), radial noise uniform in +-0.3 um, 600 points.

    Returns the points, the major-axis direction e1 and the radius of a feasible cylinder along
    e1 (centre: the middle of the bounding box of the points projected on the plane normal to
    e1; radius: the largest distance from it, exact in Decimal). That radius bounds the minimum
    circumscribed cylinder from above, and it is below r + 10 um, while a cylinder whose axis is
    near the pilot's own must reach the major-axis ends, about r + 20 um from that axis; so a
    near-axis result is not the global minimum.
    """
    rng = random.Random(f"mcc-cyl-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, Decimal("0.3"))
    r, length, amp, noise = Decimal("0.05"), Decimal("0.0025"), 20 * UM, Decimal("0.3") * UM
    pts = []
    for _ in range(600):
        th = 2 * PI * u01(rng)
        s, c = sin_cos(th)
        _, c2 = sin_cos(2 * th)
        t = length * (u01(rng) - Decimal("0.5"))
        rr = r + amp * c2 + (2 * u01(rng) - 1) * noise
        pts.append(rounded(at(p0, d, e1, e2, t, rr * c, rr * s)))
    proj = []
    for q in pts:
        w = [q[k] - p0[k] for k in range(3)]
        proj.append((dot(w, e2), dot(w, d)))
    cy = (min(y for y, _ in proj) + max(y for y, _ in proj)) / 2
    cz = (min(z for _, z in proj) + max(z for _, z in proj)) / 2
    better = max(((y - cy) ** 2 + (z - cz) ** 2).sqrt() for y, z in proj)
    if not better < r + 10 * UM:
        raise SystemExit(f"{name}: the sideways cylinder ({better}) does not beat the near-axis radius")
    return pts, e1, better


def welzl_centre(pts2, rng):
    """Centre of the smallest circle enclosing 2D float points (iterative Welzl, shuffled by
    `rng`). Only a centre is taken from it: the radius claimed is recomputed exactly."""
    p = pts2[:]
    rng.shuffle(p)

    def d2(a, b):
        return (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2

    def c2(a, b):
        c = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
        return c, d2(a, c)

    def c3(a, b, c):
        bx, by, cx, cy = b[0] - a[0], b[1] - a[1], c[0] - a[0], c[1] - a[1]
        d = 2 * (bx * cy - by * cx)
        if d == 0:
            return max((c2(a, b), c2(b, c), c2(a, c)), key=lambda t: t[1])
        b2, cc2 = bx * bx + by * by, cx * cx + cy * cy
        o = (a[0] + (cy * b2 - by * cc2) / d, a[1] + (bx * cc2 - cx * b2) / d)
        return o, d2(a, o)

    out = lambda x, c, r2: d2(x, c) > r2 * (1 + 1e-12)
    c, r2 = p[0], 0.0
    for i in range(1, len(p)):
        if out(p[i], c, r2):
            c, r2 = p[i], 0.0
            for j in range(i):
                if out(p[j], c, r2):
                    c, r2 = c2(p[i], p[j])
                    for k in range(j):
                        if out(p[k], c, r2):
                            c, r2 = c3(p[i], p[j], p[k])
    return c


def pilot_tilted(name, length):
    """The construction of the second review of #374: radius 50 mm, two-lobe ovality r + 20 um
    cos(2 theta), noise uniform in +-0.3 um, length `length`, 600 points, theta and height
    uniform (this script's own generator). The near-axis local minimum is about r + 20 um, and
    a cylinder tilted towards the major axis is smaller (the review found 49.99765 mm at 75
    degrees for its own sample). Tilts phi from 60 to 89.5 degrees in steps of 0.5 degrees: the
    direction cos(phi) d + sin(phi) e1, a centre from a float Welzl circle of the projections,
    and the radius from that centre recomputed exactly in Decimal (a feasible cylinder). Returns
    the points, the best direction and its radius, which must be below r + 10 um."""
    rng = random.Random(f"mcc-cyl-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, Decimal("0.3"))
    r, amp, noise = Decimal("0.05"), 20 * UM, Decimal("0.3") * UM
    pts = []
    for _ in range(600):
        th = 2 * PI * u01(rng)
        s, c = sin_cos(th)
        _, c2 = sin_cos(2 * th)
        t = length * (u01(rng) - Decimal("0.5"))
        rr = r + amp * c2 + (2 * u01(rng) - 1) * noise
        pts.append(rounded(at(p0, d, e1, e2, t, rr * c, rr * s)))
    best = None
    for k in range(60):
        phi = (Decimal(60) + Decimal(k) / 2) * PI / 180
        sp, cp = sin_cos(phi)
        u1 = [-sp * d[i] + cp * e1[i] for i in range(3)]
        axis = [cp * d[i] + sp * e1[i] for i in range(3)]
        proj = []
        for q in pts:
            w = [q[i] - p0[i] for i in range(3)]
            proj.append((dot(w, u1), dot(w, e2)))
        cy, cz = welzl_centre([(float(y), float(z)) for y, z in proj], random.Random(f"{name}/{k}"))
        cy, cz = dec(cy), dec(cz)
        rad = max(((y - cy) ** 2 + (z - cz) ** 2).sqrt() for y, z in proj)
        if best is None or rad < best[1]:
            best = (axis, rad)
    if not best[1] < r + 10 * UM:
        raise SystemExit(f"{name}: no tilted cylinder beats the near-axis radius ({best[1]})")
    return pts, best[0], best[1]


def oval(name):
    """An oval long cylinder (radius 20 mm, length 40 mm, r(theta) = r + 20 um cos(2 theta)) with
    four exact contacts at the major-axis ends of both end faces; the other 596 points are inside
    by 0.5 to 2 um. Its near-axis envelope has four active rows (free direction, five
    parameters) and two contacts in projection (fixed direction, three parameters): a
    degenerate active set, which the certificate does not support (fail closed, `solver`)."""
    rng = random.Random(f"mcc-cyl-v1/{name}")
    d, e1, e2 = frame(rng)
    p0 = offset(rng, Decimal("0.3"))
    r, amp = Decimal("0.02"), 20 * UM
    length = 2 * r
    pts = [at(p0, d, e1, e2, t, sg * (r + amp), Decimal(0)) for t in (-length / 2, length / 2) for sg in (1, -1)]
    for _ in range(596):
        th = 2 * PI * u01(rng)
        s, c = sin_cos(th)
        _, c2 = sin_cos(2 * th)
        t = length * (u01(rng) - Decimal("0.5"))
        rr = r + amp * c2 - (Decimal("0.5") + Decimal("1.5") * u01(rng)) * UM
        pts.append(at(p0, d, e1, e2, t, rr * c, rr * s))
    order = list(range(600))
    rng.shuffle(order)
    out = [None] * 600
    for k, q in enumerate(pts):
        out[order[k]] = rounded(q)
    return out, d


def emit(out, name, cov, pts, ref=None, contacts=None, extra=(), flag=None, expect="start"):
    out.append(f"case {name} coverage {cov}")
    out.append("points " + " ".join(hx(fl(x)) for p in pts for x in p))
    if ref is not None:
        rd, rp, rr = ref
        out.append("ref_axis " + hxs(rd))
        out.append("ref_point " + hxs(rp))
        out.append("ref_r " + hx(fl(rr)))
    if contacts is not None:
        out.append("contacts " + " ".join(str(k) for k in contacts))
    out.extend(extra)
    if flag:
        out.append(f"flag {flag}")
    out.append(f"expect {expect}")
    out.append("end")


def render():
    out = [
        "# KRN.ENV.MCC.CYL fixture mcc-cyl-v1 (CC-BY-4.0). Generated by fixtures/fit/make_mcc_cyl_fixtures.py; do not edit.",
        "# Planted minimum circumscribed cylinders with exact Decimal references, a tie, two axes, flagged and error cases.",
    ]
    for name, n, cov, off in (
        ("env-360-0", 1000, 360, Decimal("0.01")),
        ("env-360-1", 1000, 360, Decimal("0.3")),
        ("env-360-km", 1000, 360, Decimal(1000)),
        ("env-200-1", 1000, 200, Decimal("0.3")),
        ("env-360-big", 2000, 360, Decimal("0.3")),
    ):
        pts, ref, contacts, _ = planted(name, n, cov, off)
        emit(out, name, cov, pts, ref, contacts)
    # Fixed direction: the planted envelope with its (rounded) reference axis as the constraint.
    pts, ref, contacts, _ = planted("fixed-dir", 1000, 360, Decimal("0.3"))
    # With the direction fixed at the optimum's, the convex 2D problem has the same radius and
    # axis (the 3D minimum is the smallest enclosing circle at its own direction).
    emit(out, "fixed-dir", 360, pts, ref, None, extra=["fixed_dir " + hxs(ref[0])])
    pts, ref, contacts, tie = planted("tie", 1000, 360, Decimal("0.3"), tie=True)
    emit(out, "tie", 360, pts, ref, contacts, extra=[f"tie_point {tie}"])
    rng = random.Random("mcc-cyl-v1/twoaxes")
    s = Decimal("0.02")
    slab = [rounded([s * (u01(rng) - Decimal("0.5")), s * Decimal("1.02") * (u01(rng) - Decimal("0.5")),
                     s * Decimal("0.25") * (u01(rng) - Decimal("0.5"))]) for _ in range(200)]
    emit(out, "twoaxes", 360, slab, flag="two_axes")
    for k, off in ((0, Decimal("0.01")), (2, Decimal(1000))):
        pts, _ = surface_points(f"half-1066-{k}", 150, 180, Decimal("1.066"), off)
        emit(out, f"half-1066-{k}", 180, pts, flag="never_confidently_wrong")
    for k in range(2):
        pts, _ = surface_points(f"shortarc-{k}", 150, 30, Decimal("0.2"), Decimal("0.3"), noise_um=Decimal(20))
        emit(out, f"shortarc-{k}", 30, pts, flag="never_confidently_wrong")
    pts, axis, better = pilot("pilot")
    emit(out, "pilot", 360, pts, extra=["better_axis " + hxs(axis), "better_r " + hx(fl(better))],
         flag="pilot_not_global")
    pts, axis, better = pilot_tilted("pilot-5mm", Decimal("0.005"))
    emit(out, "pilot-5mm", 360, pts, extra=["better_axis " + hxs(axis), "better_r " + hx(fl(better))],
         flag="pilot_not_global")
    pts, axis = oval("oval-free")
    emit(out, "oval-free", 360, pts, flag="degenerate_active_set")
    pts, axis = oval("oval-fixed")
    emit(out, "oval-fixed", 360, pts, extra=["fixed_dir " + hxs(axis)], flag="degenerate_active_set")
    rng = random.Random("mcc-cyl-v1/disc")
    disc = []
    for _ in range(400):
        sn, c = sin_cos(2 * PI * u01(rng))
        disc.append(rounded([Decimal("0.02") * c, Decimal("0.02") * sn, Decimal("0.0001") * u01(rng)]))
    emit(out, "disc", 360, disc, expect="ill_conditioned")
    rng = random.Random("mcc-cyl-v1/few-4")
    few = [[Decimal(i) / 8, u01(rng), u01(rng)] for i in range(4)]
    emit(out, "few-4", 360, few, expect="too_few_points n=4 need=5")
    return "\n".join(out) + "\n"


MEC3_OUT = OUT.with_name("mec3.txt")


def render_mec3():
    """Thin, nearly right triangles for `mec3` (review 3 of #374), exact in rationals.

    On a circle of radius 0.05 about (0.01, -0.007) (so coordinate differences round, as for
    centred data): vertices at angles 0.7 and 0.7 + eps (eps apart) and a third at
    0.7 + pi + phi, nearly antipodal: phi = eps / 2 and eps / 3 (acute: the centre is inside,
    every arc below pi) and -eps / 2 (obtuse). Coordinates rounded to binary64; then, exactly in
    Fractions on the rounded coordinates, the squared sides, the smallest enclosing circle's
    squared radius (a quarter of the longest squared side for a right or obtuse triangle, else
    s0 s1 s2 / (4 (2 area)^2)) and the ratio 2 area / longest squared side; its square root to
    60 digits, rounded to binary64. `thin` is that ratio below 1e-6 (none is within a factor
    1.5 of it), where `mec3` falls back to half the longest side.

    Lines: `tri <6 hex: ax ay bx by cx cy> <hex R> thin|full`.
    """
    out = ["# mec3 thin-triangle cases (KRN.ENV.MCC.CYL, CC-BY-4.0). Generated by fixtures/fit/make_mcc_cyl_fixtures.py; do not edit."]
    ctr = (Decimal("0.01"), Decimal("-0.007"))
    rad = Decimal("0.05")
    for eps_s in ("1e-4", "3e-5", "1e-5", "5e-6", "1e-6", "3e-7", "1e-7"):
        eps = Decimal(eps_s)
        base = Decimal("0.7")
        for phi in (eps / 2, eps / 3, -eps / 2):
            pts = []
            for ang in (base, base + eps, base + PI + phi):
                sn, cs = sin_cos(ang)
                pts.append((Fraction(fl(ctr[0] + rad * cs)), Fraction(fl(ctr[1] + rad * sn))))
            (ax, ay), (bx, by), (cx, cy) = pts

            def sq(p, q):
                return (p[0] - q[0]) ** 2 + (p[1] - q[1]) ** 2

            sides = [sq(pts[0], pts[1]), sq(pts[1], pts[2]), sq(pts[2], pts[0])]
            k = max(range(3), key=lambda i: sides[i])
            rest = sum(sides) - sides[k]
            area2 = abs((bx - ax) * (cy - ay) - (by - ay) * (cx - ax))
            ratio = area2 / sides[k]
            if Fraction(2, 3) * Fraction(1, 10**6) < ratio < Fraction(3, 2) * Fraction(1, 10**6):
                raise SystemExit(f"mec3 case eps {eps_s}: ratio {float(ratio)} too near the threshold")
            if sides[k] >= rest:
                r2 = sides[k] / 4
            else:
                r2 = sides[0] * sides[1] * sides[2] / (4 * area2 * area2)
            r = (Decimal(r2.numerator) / Decimal(r2.denominator)).sqrt()
            coords = " ".join(hx(float(x)) for p in pts for x in p)
            thin = "thin" if ratio < Fraction(1, 10**6) else "full"
            out.append(f"tri {coords} {hx(fl(r))} {thin}")
    return "\n".join(out) + "\n"


def main():
    outputs = ((OUT, render()), (MEC3_OUT, render_mec3()))
    if "--check" in sys.argv[1:]:
        stale = [p for p, text in outputs if not (p.exists() and p.read_text(encoding="utf-8") == text)]
        for p in stale:
            print(f"{p} is stale; rerun without --check", file=sys.stderr)
        if stale:
            return 1
        print("mcc-cyl-v1 fixtures are up to date")
        return 0
    OUT.parent.mkdir(exist_ok=True)
    for p, text in outputs:
        p.write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
