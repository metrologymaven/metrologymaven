#!/usr/bin/env python3
"""Independent reference for KRN.ENV.MIC.TR (trust region of the maximum inscribed fits).

Writes fixtures/fit/mic-tr-v1/cases.txt (CC-BY-4.0 project data). With --check, compares
instead of writing and exits 1 on any difference. Stdlib only; nothing here runs the Rust code.

What it computes, from the rules in spec/krn/env/mic/tr/NODE.md and the pinned rules in the
module documentation of crates/fid-fit/src/mic_tr.rs (written from the same spec):

1. Constants, to 80 significant digits in decimal.Decimal and then rounded once to the nearest
   binary64 (float(Decimal) goes through the decimal string, which CPython rounds correctly):
   - C_j = cos(j pi / 16), j = 0..8 (pi by Machin's formula, cos by its Taylor series);
   - the 2D facet normals u_k = (cos(k pi / 16), sin(k pi / 16)), k = 0..31, from C_j by the
     quadrant symmetries (sign flips and swaps only, so they are exact copies of C_j);
   - K2 = cos(pi / 32), the apothem of the regular 32-gon of circumradius 1;
   - the 3D facet normals: the 6 axes, the 12 (+-1, +-1, 0)/sqrt(2) and the 8
     (+-1, +-1, +-1)/sqrt(3) directions, in the order listed in `normals3`;
   - K3 = 1 / sqrt(9 - 2 sqrt(2) - 2 sqrt(6)), the inradius of the polyhedron cut by those 26
     planes when its farthest vertex is at distance 1 (that vertex is (1, sqrt2 - 1,
     sqrt3 - sqrt2) up to symmetry; `check_polyhedron` confirms it by enumerating every vertex
     in exact-enough decimal arithmetic).
2. The binary64 operations of the pinned rules, each a single IEEE-754 operation (CPython
   floats are binary64, no FMA): the radius fraction * r; the inradius radius * K;
   red::dot as the Neumaier sum of the plain products; red::norm2 with the power-of-two
   scaling; the direction rules of fid-fit's contract (unit length, sign rule); the cylinder
   basis; b_k = dot(a_k, centre) + inradius; slack_k = b_k - dot(a_k, x); the total-order
   minimum of the slacks; the tolerance floor (16 * EPSILON) * (max |centre_i| + radius),
   below which a tolerance is refused (a point on the boundary of a far-from-origin region
   has a computed slack of a few ulps of its coordinates, which a smaller tolerance would let
   pass).

The least-squares results are hand-built (circles, spheres and cylinders with chosen centres,
axes and radii), as the leaf's Test fixtures section says; no least-squares fit is run. The
checked positions are built here from the expected rows: on a facet, 1 percent inside along a
facet normal, at a vertex, outside, at the centre and (cylinders) shifted along the axis; and
the on-facet and centre positions again at a tolerance equal to the floor, and one ulp below it
(refused).
"""
import itertools
import math
import struct
import sys
from decimal import Decimal, getcontext
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "mic-tr-v1" / "cases.txt"

getcontext().prec = 80


def hx(x):
    return struct.pack(">d", x).hex()


# ---------------------------------------------------------------- decimal constants


def d_atan_inv(n):
    """atan(1/n) by its series."""
    n = Decimal(n)
    x = 1 / n
    x2 = x * x
    term = x
    s = Decimal(0)
    k = 0
    while True:
        t = term / (2 * k + 1)
        if t == 0 or abs(t) < Decimal(10) ** -95:
            break
        s += t if k % 2 == 0 else -t
        term *= x2
        k += 1
    return s


PI = 16 * d_atan_inv(5) - 4 * d_atan_inv(239)


def d_cos(x):
    s = Decimal(0)
    term = Decimal(1)
    k = 0
    while abs(term) > Decimal(10) ** -95:
        s += term
        term = -term * x * x / ((2 * k + 1) * (2 * k + 2))
        k += 1
    return s


def d_sqrt(x):
    return Decimal(x).sqrt()


def f(dv):
    """Round a Decimal once to the nearest binary64; an exact zero becomes +0.0."""
    v = float(dv)
    return v + 0.0


C = [f(d_cos(PI * j / 16)) if j < 8 else 0.0 for j in range(9)]
assert C[0] == 1.0
K2 = f(d_cos(PI / 32))
R2 = f(1 / d_sqrt(2))
R3 = f(1 / d_sqrt(3))
K3 = f(1 / d_sqrt(9 - 2 * d_sqrt(2) - 2 * d_sqrt(6)))


def normals2():
    out = []
    for k in range(32):
        q, j = divmod(k, 8)
        c, s = C[j], C[8 - j]
        u = [(c, s), (-s, c), (-c, -s), (s, -c)][q]
        out.append((u[0] + 0.0, u[1] + 0.0))
    return out


def normals3():
    out = []
    for i in range(3):
        for s in (1.0, -1.0):
            v = [0.0, 0.0, 0.0]
            v[i] = s
            out.append(tuple(v))
    for i, j in ((0, 1), (0, 2), (1, 2)):
        for si in (1.0, -1.0):
            for sj in (1.0, -1.0):
                v = [0.0, 0.0, 0.0]
                v[i] = si * R2
                v[j] = sj * R2
                out.append(tuple(v))
    for s in itertools.product((1.0, -1.0), repeat=3):
        out.append(tuple(x * R3 for x in s))
    return out


def check_polyhedron():
    """Every vertex of {x : n_k . x <= 1} lies within 1 / K3 of the origin, and one is at it."""
    getcontext().prec = 50
    r2 = 1 / d_sqrt(2)
    r3 = 1 / d_sqrt(3)
    ns = []
    for i in range(3):
        for s in (1, -1):
            v = [Decimal(0)] * 3
            v[i] = Decimal(s)
            ns.append(v)
    for i, j in ((0, 1), (0, 2), (1, 2)):
        for si in (1, -1):
            for sj in (1, -1):
                v = [Decimal(0)] * 3
                v[i] = si * r2
                v[j] = sj * r2
                ns.append(v)
    for s in itertools.product((1, -1), repeat=3):
        ns.append([x * r3 for x in s])

    def det(m):
        return (
            m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
        )

    far = Decimal(0)
    count = 0
    eps = Decimal(10) ** -30
    for a, b, c in itertools.combinations(range(26), 3):
        m = [ns[a], ns[b], ns[c]]
        dt = det(m)
        if abs(dt) < eps:
            continue
        x = []
        for k in range(3):
            mk = [row[:] for row in m]
            for r in range(3):
                mk[r][k] = Decimal(1)
            x.append(det(mk) / dt)
        if all(sum(n[i] * x[i] for i in range(3)) <= 1 + eps for n in ns):
            count += 1
            d = d_sqrt(sum(t * t for t in x))
            if d > far:
                far = d
    exact = d_sqrt(9 - 2 * d_sqrt(2) - 2 * d_sqrt(6))
    assert abs(far - exact) < Decimal(10) ** -25, (far, exact)
    assert count > 0
    getcontext().prec = 80


# ---------------------------------------------------------------- binary64 rules


def neumaier(terms):
    s = -0.0
    c = -0.0
    for x in terms:
        t = s + x
        if abs(s) >= abs(x):
            c += (s - t) + x
        else:
            c += (x - t) + s
        s = t
    return s if c == 0.0 else s + c


def dot(a, b):
    assert len(a) == len(b)
    return neumaier([a[i] * b[i] for i in range(len(a))])


def norm2(xs):
    m = 0.0
    for x in xs:
        if abs(x) > m:
            m = abs(x)
    if m == 0.0:
        return 0.0
    e = math.frexp(m)[1] - 1
    assert -1022 <= e <= 1022
    d1 = math.ldexp(1.0, -e)
    r = math.sqrt(neumaier([(x * d1 * 1.0) * (x * d1 * 1.0) for x in xs]))
    return r * math.ldexp(1.0, e)


def unit(d):
    n = norm2(d)
    assert n >= 2.0 ** -1022
    return [x / n for x in d]


def sign_normalize(u):
    first = next((x for x in u if x != 0.0), 0.0)
    if first < 0.0:
        u = [-x for x in u]
    return [x + 0.0 for x in u]


def total_lt(a, b):
    if a == b:
        return math.copysign(1.0, a) < 0 and math.copysign(1.0, b) > 0
    return a < b


def basis(d):
    """The cylinder's facet-plane basis (e1, e2) for a unit axis d."""
    j = 0
    for i in (1, 2):
        if total_lt(abs(d[i]), abs(d[j])):
            j = i
    t = [(1.0 if i == j else 0.0) - d[j] * d[i] for i in range(3)]
    e1 = [x + 0.0 for x in unit(t)]
    e2 = [
        d[1] * e1[2] - d[2] * e1[1],
        d[2] * e1[0] - d[0] * e1[2],
        d[0] * e1[1] - d[1] * e1[0],
    ]
    return e1, e2


def region(kind, vals, fraction):
    """centre, axis, radius, inradius, rows for a hand-built least-squares feature."""
    if kind == "circle2":
        cx, cy, r = vals
        centre = [cx, cy, 0.0]
        axis = None
    elif kind == "sphere":
        cx, cy, cz, r = vals
        centre = [cx, cy, cz]
        axis = None
    else:
        px, py, pz, dx, dy, dz, r = vals
        centre = [px, py, pz]
        axis = sign_normalize(unit([dx, dy, dz]))
    radius = fraction * r
    if kind == "sphere":
        inr = radius * K3
    else:
        inr = radius * K2
    rows = []
    if kind == "circle2":
        for c, s in normals2():
            rows.append([c, s, 0.0])
    elif kind == "sphere":
        for n in normals3():
            rows.append(list(n))
    else:
        e1, e2 = basis(axis)
        for c, s in normals2():
            rows.append([c * e1[i] + s * e2[i] for i in range(3)])
    out = []
    for a in rows:
        out.append((a, dot(a, centre) + inr))
    return centre, axis, radius, inr, out


def slack(rows, x):
    m = None
    for a, b in rows:
        s = b - dot(a, x)
        if m is None or total_lt(s, m):
            m = s
    return m


def along(p, a, t):
    return [p[i] + t * a[i] for i in range(3)]


LS = [
    # name, kind, values (circle2: cx cy r; sphere: cx cy cz r; cylinder: p d r), fraction
    ("circle_origin", "circle2", [0.0, 0.0, 0.01], 0.2),
    ("circle_offset", "circle2", [1.25, -0.75, 0.0125], 0.2),
    ("circle_decimal", "circle2", [0.1, 0.2, 0.0033], 0.2),
    ("circle_far", "circle2", [-812.37, 1530.5, 0.0451], 0.2),
    ("sphere_origin", "sphere", [0.0, 0.0, 0.0, 0.02], 0.2),
    ("sphere_offset", "sphere", [0.5, -0.3, 1.7, 0.0251], 0.2),
    ("cyl_z", "cylinder", [0.1, 0.2, 0.3, 0.0, 0.0, 1.0, 0.015], 0.2),
    ("cyl_skew", "cylinder", [-0.02, 0.4, 0.05, 1.0, 2.0, -2.0, 0.008], 0.2),
    ("cyl_flip", "cylinder", [0.3, -0.1, 0.0, -0.3, 0.4, 0.866, 0.0125], 0.2),
    ("circle_fraction", "circle2", [0.05, 0.05, 0.02], 0.35),
]

TOL = 1e-9
EPS = 2.0 ** -52


def tol_floor(centre, radius):
    m = 0.0
    for c in centre:
        if abs(c) > m:
            m = abs(c)
    return (16.0 * EPS) * (m + radius)


def checks(kind, centre, axis, inr, radius, rows):
    """(label, x, expected verdict) points for AC-2."""
    out = [("centre", list(centre))]
    for k in (0, 5):
        out.append((f"on_facet_{k}", along(centre, rows[k][0], inr)))
    out.append(("inside_1pct_facet_3", along(centre, rows[3][0], 0.99 * inr)))
    out.append(("outside_facet_2", along(centre, rows[2][0], 1.5 * inr)))
    if kind == "sphere":
        # The farthest vertex (1, sqrt2 - 1, sqrt3 - sqrt2) * radius, up to rounding.
        u = unit([1.0, f(d_sqrt(2) - 1), f(d_sqrt(3) - d_sqrt(2))])
        out.append(("vertex", along(centre, u, inr / K3)))
    else:
        # The vertex between facets 0 and 1, at distance radius along the bisector.
        a0, a1 = rows[0][0], rows[1][0]
        bis = unit([a0[i] + a1[i] for i in range(3)])
        out.append(("vertex", along(centre, bis, inr / K2)))
    if axis is not None:
        out.append(("inside_1pct_facet_3_shifted", along(along(centre, rows[3][0], 0.99 * inr), axis, 0.75)))
    floor = tol_floor(centre, radius)
    assert floor <= TOL
    tols = [(label, x, TOL) for label, x in out]
    for label, x in out[:3]:
        tols.append((label + "_at_floor", x, floor))
    tols.append((out[2][0] + "_below_floor", out[2][1], math.nextafter(floor, 0.0)))
    res = []
    for label, x, tol in tols:
        s = slack(rows, x)
        if tol < floor:
            verdict = "refused"
        elif s <= tol:
            verdict = "unbounded"
        else:
            verdict = "ok"
        res.append((label, x, tol, s, verdict))
    return res


def text():
    check_polyhedron()
    lines = [
        "# KRN.ENV.MIC.TR cases: hand-built least-squares results, trust regions and boundary checks.",
        "# Generated by fixtures/fit/make_mic_tr_fixtures.py (CC-BY-4.0). Do not edit.",
        "# feature <kind> <bits...>; fraction <bits>; then the expected record:",
        "# centre <bits x3>; axis none|<bits x3>; radius <bits>; inradius <bits>; row <a bits x3> <b bits>",
        "# floor <bits> (the smallest accepted boundary_tol)",
        "# check <label> <x bits x3> <boundary_tol bits> <slack bits> ok|unbounded|refused",
    ]
    for name, kind, vals, fraction in LS:
        centre, axis, radius, inr, rows = region(kind, vals, fraction)
        lines.append(f"case {name}")
        lines.append(f"feature {kind} " + " ".join(hx(v) for v in vals))
        lines.append(f"fraction {hx(fraction)}")
        lines.append("centre " + " ".join(hx(v) for v in centre))
        lines.append("axis " + ("none" if axis is None else " ".join(hx(v) for v in axis)))
        lines.append(f"radius {hx(radius)}")
        lines.append(f"inradius {hx(inr)}")
        lines.append(f"floor {hx(tol_floor(centre, radius))}")
        for a, b in rows:
            lines.append("row " + " ".join(hx(v) for v in a) + " " + hx(b))
        for label, x, tol, s, verdict in checks(kind, centre, axis, inr, radius, rows):
            lines.append(f"check {label} " + " ".join(hx(v) for v in x) + f" {hx(tol)} {hx(s)} {verdict}")
        lines.append("end")
    return "\n".join(lines) + "\n"


def main():
    data = text().encode("utf-8")
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_bytes() != data:
            print("stale: fixtures/fit/mic-tr-v1/cases.txt")
            sys.exit(1)
        print("ok: fixtures/fit/mic-tr-v1/cases.txt")
        return
    if "--consts" in sys.argv[1:]:
        print("C", [repr(c) for c in C])
        print("K2", repr(K2), "K3", repr(K3), "R2", repr(R2), "R3", repr(R3))
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(data)
    print(f"wrote fixtures/fit/mic-tr-v1/cases.txt ({len(data)} bytes)")


if __name__ == "__main__":
    main()
