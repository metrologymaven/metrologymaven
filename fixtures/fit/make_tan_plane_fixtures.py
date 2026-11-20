#!/usr/bin/env python3
"""Independent reference for KRN.ENV.TAN.PLANE (tangential plane) fixtures (CC-BY-4.0 project data).

Writes fixtures/fit/tan-plane-v1/cases.txt. Nothing here runs the Rust code (stdlib only, exact
rational arithmetic with fractions.Fraction; the only roundings are the final conversions to
binary64, done exactly: Fraction -> float is correctly rounded in CPython, and square roots
are rounded with integer isqrt, never with the platform libm).

1. Point sets. Each plate is built from a few *support* points (raised pins on the outward
   side, feet on the material side, or a crowned apex and corners) and *bulk* points that are
   convex combinations of the supports with strictly positive integer weights, so they lie
   strictly inside the hull. The plate is then rotated by an exact rational rotation (Cayley
   transform of a rational skew matrix) and translated, and every coordinate is rounded to
   binary64. From here on everything is computed exactly from those binary64 values. The
   script re-checks that every bulk point is strictly inside every supporting plane of the
   supports (so the hull vertices are exactly the supports).
2. Constrained L1 (sum of perpendicular depths below a supporting plane from the outward
   side). For every triple of supports spanning a plane, oriented so that its normal has a
   positive dot product with the outward direction, the plane is kept when every point is on
   or below it; its value is sum_i (n.p_k - n.p_i) / |n| over all points. The optimum is the
   smallest value (compared exactly via squares). Its contact set is every point with exact
   depth <= side_tol. All optimal planes are listed (the symmetric crown has four).
3. External minimax (smallest largest depth). The width of the point set along a direction n
   is (max n.p - min n.p) / |n|; its minimum over all directions is attained at a facet normal
   (three supports) or at the cross product of two support edges, so the minimum over those
   candidate directions is the exact global minimum. The tangent plane touches the max side;
   its contacts are the points with depth <= side_tol.
4. Fixed normal (any variant). With the fixed direction f (a binary64 vector), the depths are
   (max_j f.p_j - f.p_i) / |f|; minimax is the largest depth, L1 their sum, L2 the sum of their
   squares (m^2). Contacts: depth <= side_tol.
5. Expected values are written as the correctly rounded binary64 of the exact value; the unit
   normal component by component (sign carried). The Rust test compares with tolerances,
   because the fit evaluates the same exact problem in binary64 (centring, rotation).

Run:  python fixtures/fit/make_tan_plane_fixtures.py          (writes the fixtures)
      python fixtures/fit/make_tan_plane_fixtures.py --check  (fails if a fixture is stale)
"""

import itertools
import math
import random
import struct
import sys
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "tan-plane-v1" / "cases.txt"
SIDE_TOL = 1e-12  # metres, the fixtures' side_tol
ROCK_TOL = 1e-9


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return f"{bits(float(x)):016x}"


def sqrt_round(q):
    """The binary64 value nearest to sqrt(q) for a rational q >= 0."""
    q = F(q)
    if q == 0:
        return 0.0
    num, den = q.numerator, q.denominator
    k = max(0, 160 - (num.bit_length() - den.bit_length())) // 2 + 2
    while True:
        scaled = (num << (2 * k)) // den
        r = math.isqrt(scaled)
        if r * r * den == num << (2 * k):
            return float(F(r, 1 << k))
        lo, hi = float(F(r, 1 << k)), float(F(r + 1, 1 << k))
        if lo == hi:
            return lo
        k += 32


def ratio_sqrt(n, s):
    """n / sqrt(s), correctly rounded, for rational n and s > 0."""
    v = sqrt_round(F(n) * F(n) / F(s))
    return -v if n < 0 else v


def sub(u, v):
    return tuple(F(a) - F(b) for a, b in zip(u, v))


def cross(u, v):
    return (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])


def dot(u, v):
    return sum(F(a) * F(b) for a, b in zip(u, v))


def norm2(u):
    return dot(u, u)


# --- construction ----------------------------------------------------------------------------


def cayley(k):
    """Exact rational rotation (I - K)^-1 (I + K) for the skew matrix of k."""
    a, b, c = (F(x) for x in k)
    K = [[F(0), -c, b], [c, F(0), -a], [-b, a, F(0)]]
    I = [[F(int(i == j)) for j in range(3)] for i in range(3)]
    M = [[I[i][j] - K[i][j] for j in range(3)] for i in range(3)]
    P = [[I[i][j] + K[i][j] for j in range(3)] for i in range(3)]
    # Inverse of M by the adjugate.
    det = (M[0][0] * (M[1][1] * M[2][2] - M[1][2] * M[2][1])
           - M[0][1] * (M[1][0] * M[2][2] - M[1][2] * M[2][0])
           + M[0][2] * (M[1][0] * M[2][1] - M[1][1] * M[2][0]))
    adj = [[F(0)] * 3 for _ in range(3)]
    for i in range(3):
        for j in range(3):
            r = [x for x in range(3) if x != j]
            c_ = [x for x in range(3) if x != i]
            minor = M[r[0]][c_[0]] * M[r[1]][c_[1]] - M[r[0]][c_[1]] * M[r[1]][c_[0]]
            adj[i][j] = (-1) ** (i + j) * minor
    inv = [[adj[i][j] / det for j in range(3)] for i in range(3)]
    R = [[sum(inv[i][t] * P[t][j] for t in range(3)) for j in range(3)] for i in range(3)]
    for i in range(3):
        for j in range(3):
            assert sum(R[t][i] * R[t][j] for t in range(3)) == int(i == j)
    return R


def apply(R, t, p):
    return tuple(sum(R[i][j] * F(p[j]) for j in range(3)) + F(t[i]) for i in range(3))


def build(supports, n_bulk, seed, rot, shift):
    rng = random.Random(seed)
    sup = [tuple(F(c) for c in s) for s in supports]
    pts = list(sup)
    for _ in range(n_bulk):
        w = [F(rng.randint(1, 1000)) for _ in sup]
        tw = sum(w)
        pts.append(tuple(sum(wk * s[c] for wk, s in zip(w, sup)) / tw for c in range(3)))
    R = cayley(rot) if rot else None
    out = []
    for p in pts:
        q = apply(R, shift, p) if R else tuple(F(c) + F(t) for c, t in zip(p, shift))
        out.append(tuple(float(c) for c in q))
    up = (0, 0, 1)
    outward = tuple(float(c) for c in (apply(R, (0, 0, 0), up) if R else up))
    return out, outward


def frac_pts(pts):
    return [tuple(F(c) for c in p) for p in pts]


# --- oracle ----------------------------------------------------------------------------------


def oriented(n, out):
    d = dot(n, out)
    if d == 0:
        return None
    return n if d > 0 else tuple(-c for c in n)


def supporting_planes(P, idx, out):
    """(n, offset) for every triple of idx whose plane has every point of P on or below it."""
    res = []
    for i, j, k in itertools.combinations(idx, 3):
        n = cross(sub(P[j], P[i]), sub(P[k], P[i]))
        if n == (0, 0, 0):
            continue
        n = oriented(n, out)
        if n is None:
            continue
        off = dot(n, P[i])
        if all(dot(n, p) <= off for p in P):
            res.append((n, off))
    return res


def contacts(P, n, off):
    tol2 = F(SIDE_TOL) ** 2 * norm2(n)
    return [i for i, p in enumerate(P) if (off - dot(n, p)) ** 2 <= tol2 and off - dot(n, p) >= 0]


def check_interior(P, n_sup, out):
    """Every bulk point lies strictly below every supporting plane of the supports, in both
    orientations (so it is strictly inside their hull)."""
    sup = list(range(n_sup))
    planes = []
    for i, j, k in itertools.combinations(sup, 3):
        n = cross(sub(P[j], P[i]), sub(P[k], P[i]))
        if n == (0, 0, 0):
            continue
        off = dot(n, P[i])
        vals = [dot(n, P[s]) for s in sup]
        if all(v <= off for v in vals):
            planes.append((n, off))
        if all(v >= off for v in vals):
            planes.append((tuple(-c for c in n), -off))
    for p in P[n_sup:]:
        assert all(dot(n, p) < off for n, off in planes), "bulk point not strictly interior"


def unit(n):
    s = norm2(n)
    return tuple(ratio_sqrt(c, s) for c in n)


def l1_optima(P, n_sup, out):
    best = None
    rows = []
    for n, off in supporting_planes(P, range(n_sup), out):
        num = sum(off - dot(n, p) for p in P)
        rows.append((num, n, off))
    for num, n, off in rows:
        key = num * num / norm2(n)
        if best is None or key < best:
            best = key
    opt = [(num, n, off) for num, n, off in rows if num * num / norm2(n) == best]
    # Distinct planes only (a facet with four coplanar supports appears once per triple).
    seen, res = set(), []
    for num, n, off in opt:
        c = tuple(contacts(P, n, off))
        if c not in seen:
            seen.add(c)
            res.append((ratio_sqrt(num, norm2(n)), c, unit(n)))
    return sorted(res, key=lambda r: r[1])


def minimax_optimum(P, n_sup, out):
    sup = [P[i] for i in range(n_sup)]
    dirs = []
    for i, j, k in itertools.combinations(range(n_sup), 3):
        dirs.append(cross(sub(sup[j], sup[i]), sub(sup[k], sup[i])))
    edges = list(itertools.combinations(range(n_sup), 2))
    for (i, j), (k, l) in itertools.combinations(edges, 2):
        dirs.append(cross(sub(sup[j], sup[i]), sub(sup[l], sup[k])))
    best = None
    for n in dirs:
        if n == (0, 0, 0):
            continue
        n = oriented(n, out)
        if n is None:
            continue
        vals = [dot(n, p) for p in sup]
        wd = max(vals) - min(vals)
        key = wd * wd / norm2(n)
        if best is None or key < best[0]:
            best = (key, n, wd, max(vals))
    _, n, wd, off = best
    # Uniqueness of the optimal direction (up to scale) among the candidates.
    for m in dirs:
        if m == (0, 0, 0) or oriented(m, out) is None:
            continue
        m = oriented(m, out)
        vals = [dot(m, p) for p in sup]
        w2 = max(vals) - min(vals)
        if w2 * w2 / norm2(m) == best[0]:
            assert cross(m, n) == (0, 0, 0), "minimax optimum is not unique"
    return ratio_sqrt(wd, norm2(n)), tuple(contacts(P, n, off)), unit(n)


def fixed_values(P, f):
    vals = [dot(f, p) for p in P]
    off = max(vals)
    s = norm2(f)
    depths = [off - v for v in vals]
    c = tuple(contacts(P, f, off))
    mm = ratio_sqrt(max(depths), s)
    l1 = ratio_sqrt(sum(depths), s)
    l2 = float(sum(d * d for d in depths) / s)
    return mm, l1, l2, c


# --- cases -----------------------------------------------------------------------------------


def pins(scale_xy, top, bottom):
    """Three raised pins and three feet of a plate (metres, plate frame)."""
    a = F(scale_xy)
    return [
        (a, F(0), F(top[0])),
        (-a / 2, a * F(4, 5), F(top[1])),
        (-a / 2, -a * F(4, 5), F(top[2])),
        (-a, a / 10, F(bottom[0])),
        (a / 2, -a * F(9, 10), F(bottom[1])),
        (a * F(3, 5), a * F(17, 20), F(bottom[2])),
    ]


def crown():
    """Symmetric crowned plate: apex, four high corners, a jittered 5x5 grid (corners and
    centre removed) with a jitter that is invariant under the 90-degree rotation
    (x, y) -> (-y, x), so the four facets through the apex tie exactly in constrained L1."""
    A = F(1, 16)
    h = A / 8
    pts = [(F(0), F(0), h), (A, A, F(0)), (-A, A, F(0)), (-A, -A, F(0)), (A, -A, F(0))]
    reps = [(A, F(0)), (A, A / 2), (A / 2, A), (A / 2, F(0)), (A / 2, A / 2)]
    jit = [F(1, 64), F(-3, 128), F(1, 32), F(-1, 64), F(5, 128)]
    for (x, y), j in zip(reps, jit):
        for _ in range(4):
            pts.append((x, y, -h / 2 + j * h))
            x, y = -y, x
    # Supports: every point that is not strictly inside (the oracle enumerates all of them).
    return [tuple(float(c) for c in p) for p in pts], (0.0, 0.0, 1.0)


CASES = []


def add_case(name, pts, outward, n_sup, fixed, variants, note):
    CASES.append((name, pts, outward, n_sup, fixed, variants, note))


def make():
    pts, out = build(pins(F(1, 10), [F(1, 10000), F(12, 100000), F(9, 100000)],
                          [F(-1, 1000), F(-11, 10000), F(-9, 10000)]), 34, 11, None, (0, 0, 0))
    add_case("pins-small", pts, out, 6, (1.0 / 64, -1.0 / 32, 1.0), ["minimax", "l1"],
             "40 points near the origin, outward +z")
    pts, out = build(pins(F(3, 20), [F(2, 10000), F(15, 100000), F(25, 100000)],
                          [F(-2, 1000), F(-23, 10000), F(-19, 10000)]), 194, 12,
                     (F(1, 7), F(-2, 9), F(1, 11)), (F(25, 2), F(-13, 4), F(7)))
    add_case("slant-far", pts, out, 6, (out[0] + 1.0 / 128, out[1] - 1.0 / 256, out[2]), ["minimax", "l1"],
             "200 points on a slant, 14 m from the origin")
    pts, out = build([(F(2, 5), F(1, 1000), F(3, 1000000)), (F(-2, 5), F(3, 2000), F(5, 1000000)),
                      (F(0), F(-9, 5000), F(4, 1000000)), (F(19, 50), F(-19, 10000), F(-2, 1000000)),
                      (F(-39, 100), F(-3, 2500), F(-1, 1000000)), (F(1, 50), F(19, 10000), F(-3, 1000000))],
                     54, 13, (F(-1, 5), F(1, 13), F(2, 3)), (F(-1, 2), F(3, 4), F(1, 8)))
    add_case("thin-strip", pts, out, 6, (out[0], out[1] + 1.0 / 1024, out[2]), ["minimax", "l1"],
             "nearly degenerate: 0.8 m x 4 mm strip, 5 um form, on a slant")
    pts, out = build(pins(F(1, 5), [F(3, 10000), F(1, 10000), F(2, 10000)],
                          [F(-3, 1000), F(-27, 10000), F(-31, 10000)]), 994, 14,
                     (F(2, 9), F(1, 6), F(-1, 10)), (F(-3, 2), F(5, 8), F(21, 8)))
    add_case("bulk-1000", pts, out, 6, (out[0] - 1.0 / 512, out[1], out[2] + 1.0 / 64), ["minimax", "l1"],
             "1000 points on a slant")
    pts, out = crown()
    add_case("crown-sym", pts, out, len(pts), (1.0 / 16, 1.0 / 32, 1.0), ["l1"],
             "symmetric crowned plate, 25 points: constrained L1 rocks on four facets")


def render():
    make()
    lines = [
        "# KRN.ENV.TAN.PLANE fixtures (CC-BY-4.0). Generated by fixtures/fit/make_tan_plane_fixtures.py;",
        "# do not edit. Values are binary64 bit patterns (16 hex digits).",
        f"# side_tol {hx(SIDE_TOL)} rock_tol {hx(ROCK_TOL)}; seed 1, 8 starts.",
        "# case <name>; note; outward <xyz>; fixed <xyz>; p <xyz> per point;",
        "# expect <minimax|l1> <objective> <contacts,...>[;<contacts,...>...] <unit normal xyz>",
        "#   (the free-normal optimum; several contact sets when it rocks);",
        "# fixedv <minimax> <l1> <l2> <contacts,...> (the fixed-normal values of the three variants);",
        "# end",
    ]
    for name, pts, out, n_sup, fixed, variants, note in CASES:
        P = frac_pts(pts)
        O = tuple(F(c) for c in out)
        if n_sup < len(P):
            check_interior(P, n_sup, O)
        lines.append(f"case {name}")
        lines.append(f"note {note}")
        lines.append("outward " + " ".join(hx(c) for c in out))
        lines.append("fixed " + " ".join(hx(c) for c in fixed))
        for p in pts:
            lines.append("p " + " ".join(hx(c) for c in p))
        if "minimax" in variants:
            v, c, n = minimax_optimum(P, n_sup, O)
            lines.append(f"expect minimax {hx(v)} {','.join(map(str, c))} " + " ".join(hx(x) for x in n))
        if "l1" in variants:
            opt = l1_optima(P, n_sup, O)
            v = opt[0][0]
            assert all(o[0] == v for o in opt)
            sets = ";".join(",".join(map(str, o[1])) for o in opt)
            lines.append(f"expect l1 {hx(v)} {sets} " + " ".join(hx(x) for x in opt[0][2]))
        mm, l1, l2, c = fixed_values(P, tuple(F(x) for x in fixed))
        lines.append(f"fixedv {hx(mm)} {hx(l1)} {hx(l2)} {','.join(map(str, c))}")
        lines.append("end")
    return "\n".join(lines) + "\n"


def main():
    text = render()
    if "--check" in sys.argv:
        old = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if old.replace("\r\n", "\n") != text:
            print(f"stale: {OUT}")
            sys.exit(1)
        print("ok")
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
