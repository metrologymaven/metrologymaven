#!/usr/bin/env python3
"""Independent fixture generator and oracles for KRN.ENV.XCHK (AC-1, AC-2). CC-BY-4.0 project data.

Stdlib only, no shared code with `crates/fid-fit`, no third-party data. Writes:

  fixtures/fit/xchk-mcc-v1/cases.txt  1 000 seeded point sets (550 planar, 450 spatial) of 3 to
      5 000 points: uniform discs and balls (near the origin and up to 8 m away), exact
      cocircular and cospherical lattice points with and without interior points, and small
      integer grids full of tied supports. Every coordinate is an integer multiple of
      2^-24 m, so the oracle works in exact integer arithmetic:
        * n <= 30: brute force over every 2-, 3- (and for spheres 4-) point support set, keeping
          the smallest enclosing ball; cross-checked against the exact incremental algorithm.
        * n > 30: the exact incremental (nested-loop) algorithm in rational arithmetic.
      Expected: r^2 as an exact fraction, the radius and centre as correctly rounded binary64
      values, and the set of every point exactly on the minimum enclosing ball.
  fixtures/fit/xchk-mic-v1/cases.txt  24 seeded hole-like planar point sets (lobed form, 360,
      180 and 90 degrees of coverage, 60 to 300 points) with the convex hull as the region
      (and 3 full-coverage sets clipped by a square). Expected: the maximum inscribed radius
      and centre from a grid-refinement oracle (branch and bound on a 1-Lipschitz function,
      refined to cells below 1e-13 m) and whether the optimum lies on the region's boundary.

Generation uses only IEEE-754 `+ - * /` and `math.sqrt` (correctly rounded), plus a Taylor
cosine written here, so the bytes are the same on every platform.

Run:
  python fixtures/fit/make_xchk_fixtures.py
  python fixtures/fit/make_xchk_fixtures.py --check
"""

import argparse
import math
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
MCC_DIR = HERE / "xchk-mcc-v1"
MIC_DIR = HERE / "xchk-mic-v1"
SEED_MCC = 0x5EED_0E4C_C0C0_0001
SEED_MIC = 0x5EED_0E4C_0A1C_0002
UNIT_EXP = -24  # one coordinate unit is 2^-24 m
MASK = 0xFFFFFFFFFFFFFFFF

getcontext().prec = 60


class SplitMix64:
    """SplitMix64 (Steele, Lea and Flood 2014). The `gen` lines of xchk-mcc-v1 use it, so a
    reader regenerates those points with integer arithmetic only."""

    def __init__(self, seed: int) -> None:
        self.state = seed & MASK

    def next_u64(self) -> int:
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return (z ^ (z >> 31)) & MASK

    def uniform(self, lo: float = 0.0, hi: float = 1.0) -> float:
        u = (self.next_u64() >> 11) * (1.0 / (1 << 53))
        return lo + (hi - lo) * u

    def randint(self, lo: int, hi: int) -> int:
        return lo + (self.next_u64() % (hi - lo + 1))

    def shuffle(self, items: list) -> None:
        for i in range(len(items) - 1, 0, -1):
            j = self.next_u64() % (i + 1)
            items[i], items[j] = items[j], items[i]


def f64_hex(x: float) -> str:
    if x == 0.0:
        x = 0.0
    return struct.pack(">d", x).hex()


# ------------------------------------------------------------------ MCC: exact integer balls
#
# A ball is (cnum, den, r2num): centre cnum / den (den > 0), r^2 = r2num / den^2.


def sq(v) -> int:
    return sum(x * x for x in v)


def inside(ball, p) -> bool:
    cnum, den, r2num = ball
    return sq([den * pi - ci for pi, ci in zip(p, cnum)]) <= r2num


def det2(m) -> int:
    return m[0][0] * m[1][1] - m[0][1] * m[1][0]


def det3(m) -> int:
    return (
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    )


def solve_int(m, rhs):
    """Cramer's rule over the integers: (numerators, den) with den > 0, or None if singular."""
    k = len(m)
    d = det2(m) if k == 2 else det3(m)
    if d == 0:
        return None
    nums = []
    for j in range(k):
        mj = [[rhs[i] if c == j else m[i][c] for c in range(k)] for i in range(k)]
        nums.append(det2(mj) if k == 2 else det3(mj))
    if d < 0:
        d = -d
        nums = [-x for x in nums]
    return nums, d


def ball_from(pts):
    """The smallest ball with every point of `pts` (1 to dim + 1 points) on its boundary,
    centre in their affine hull. None when they are affinely dependent."""
    a = pts[0]
    dim = len(a)
    if len(pts) == 1:
        return (list(a), 1, 0)
    if len(pts) == 2:
        b = pts[1]
        cnum = [x + y for x, y in zip(a, b)]
        return (cnum, 2, sq([x - y for x, y in zip(a, b)]))
    rows = []
    rhs = []
    for q in pts[1:]:
        rows.append([2 * (qi - ai) for qi, ai in zip(q, a)])
        rhs.append(sq(q) - sq(a))
    if len(rows) < dim:
        # Three points in 3D: add the plane equation n . x = n . a.
        u = [x - y for x, y in zip(pts[1], a)]
        v = [x - y for x, y in zip(pts[2], a)]
        n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        rows.append(n)
        rhs.append(sum(x * y for x, y in zip(n, a)))
    sol = solve_int(rows, rhs)
    if sol is None:
        return None
    cnum, den = sol
    return (cnum, den, sq([den * ai - ci for ai, ci in zip(a, cnum)]))


def smaller(b1, b2) -> bool:
    return b1[2] * b2[1] * b2[1] < b2[2] * b1[1] * b1[1]


def same_ball(b1, b2) -> bool:
    c1, d1, r1 = b1
    c2, d2, r2 = b2
    return all(x * d2 == y * d1 for x, y in zip(c1, c2)) and r1 * d2 * d2 == r2 * d1 * d1


def brute_force_mcc(pts):
    """AC-1 oracle for n <= 30: every 2-, 3- and (3D) 4-point support set."""
    from itertools import combinations

    n = len(pts)
    dim = len(pts[0])
    best = None
    for k in range(2, dim + 2):
        for idx in combinations(range(n), k):
            b = ball_from([pts[i] for i in idx])
            if b is None:
                continue
            if best is not None and not smaller(b, best):
                continue
            if all(inside(b, p) for p in pts):
                best = b
    return best


def incremental_mcc(pts, rng):
    """Exact incremental (nested-loop) minimum enclosing ball, Welzl's algorithm unrolled for
    dimension 2 or 3 over a SplitMix64 shuffle."""
    dim = len(pts[0])
    order = list(range(len(pts)))
    rng.shuffle(order)
    p = [pts[i] for i in order]

    def need(b):
        if b is None:
            raise RuntimeError("affinely dependent support in exact arithmetic")
        return b

    ball = (list(p[0]), 1, 0)
    for i in range(1, len(p)):
        if inside(ball, p[i]):
            continue
        ball = (list(p[i]), 1, 0)
        for j in range(i):
            if inside(ball, p[j]):
                continue
            ball = need(ball_from([p[i], p[j]]))
            for k in range(j):
                if inside(ball, p[k]):
                    continue
                ball = need(ball_from([p[i], p[j], p[k]]))
                if dim == 3:
                    for m in range(k):
                        if inside(ball, p[m]):
                            continue
                        ball = need(ball_from([p[i], p[j], p[k], p[m]]))
    return ball


def gen_ball_points(dim, count, seed, rad, centre):
    """The `gen` rule: rejection sampling of integer vectors in [-rad, rad]^dim with
    |v|^2 <= rad^2, plus `centre`, drawing coordinates in order from SplitMix64(seed)."""
    rng = SplitMix64(seed)
    out = []
    while len(out) < count:
        v = [rng.next_u64() % (2 * rad + 1) - rad for _ in range(dim)]
        if sq(v) <= rad * rad:
            out.append(tuple(x + c for x, c in zip(v, centre)))
    return out


def lattice_circle(n_sq):
    pts = []
    r = math.isqrt(n_sq)
    for x in range(-r, r + 1):
        y2 = n_sq - x * x
        y = math.isqrt(y2)
        if y * y == y2:
            pts.append((x, y))
            if y != 0:
                pts.append((x, -y))
    return sorted(set(pts))


def lattice_sphere(n_sq):
    pts = []
    r = math.isqrt(n_sq)
    for x in range(-r, r + 1):
        for y in range(-r, r + 1):
            z2 = n_sq - x * x - y * y
            if z2 < 0:
                continue
            z = math.isqrt(z2)
            if z * z == z2:
                pts.append((x, y, z))
                if z != 0:
                    pts.append((x, y, -z))
    return sorted(set(pts))


CIRCLE_N = 5 * 13 * 17 * 29  # 64 lattice points
SPHERE_N = 3 * 5 * 7 * 11  # 1155: many lattice points on the sphere


def affinely_full(pts) -> bool:
    dim = len(pts[0])
    a = pts[0]
    vecs = [[x - y for x, y in zip(q, a)] for q in pts[1:]]
    if dim == 2:
        return any(det2([u, v]) != 0 for i, u in enumerate(vecs) for v in vecs[i + 1 :])
    for i, u in enumerate(vecs):
        for j in range(i + 1, len(vecs)):
            v = vecs[j]
            n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
            if n == [0, 0, 0]:
                continue
            if any(sum(a1 * b1 for a1, b1 in zip(n, w)) != 0 for w in vecs):
                return True
    return False


def random_offset(rng, dim, far):
    lim = 1 << (27 if far else 22)
    return tuple(rng.randint(-lim, lim) for _ in range(dim))


def scaled_lattice(rng, base, dim, target_rad):
    s = max(1, target_rad // math.isqrt(CIRCLE_N if dim == 2 else SPHERE_N))
    return s, [tuple(s * x for x in p) for p in base]


def interior_points(rng, dim, count, rad_sq_limit, centre, rad):
    out = []
    while len(out) < count:
        v = [rng.randint(-rad, rad) for _ in range(dim)]
        if sq(v) < rad_sq_limit:
            out.append(tuple(x + c for x, c in zip(v, centre)))
    return out


def dedupe(pts):
    seen = set()
    out = []
    for p in pts:
        if p not in seen:
            seen.add(p)
            out.append(p)
    return out


def make_mcc_cases():
    rng = SplitMix64(SEED_MCC)
    circ = lattice_circle(CIRCLE_N)
    sph = lattice_sphere(SPHERE_N)
    assert len(circ) == 64, len(circ)
    assert len(sph) >= 100, len(sph)
    plan = [
        ("circle", "uniform", 280),
        ("circle", "cocircular", 90),
        ("circle", "grid", 60),
        ("circle", "large", 100),
        ("circle", "large_cocircular", 20),
        ("sphere", "uniform", 230),
        ("sphere", "cospherical", 70),
        ("sphere", "grid", 40),
        ("sphere", "large", 90),
        ("sphere", "large_cospherical", 20),
    ]
    cases = []
    serial = 0
    for kind, family, count in plan:
        dim = 2 if kind == "circle" else 3
        min_n = dim + 1
        made = 0
        while made < count:
            far = rng.randint(0, 9) == 0
            centre = random_offset(rng, dim, far)
            explicit = []
            gen = None
            if family == "uniform":
                # Every size up to 30; spatial sizes skewed small (the 4-point brute force is
                # the generator's cost).
                n = rng.randint(min_n, 30) if dim == 2 else rng.randint(min_n, rng.randint(min_n, 30))
                rad = rng.randint(1 << 12, 1 << 21)
                explicit = dedupe(interior_points(rng, dim, n, rad * rad + 1, centre, rad))
            elif family in ("cocircular", "cospherical"):
                base = circ if dim == 2 else sph
                s, lat = scaled_lattice(rng, base, dim, rng.randint(1 << 14, 1 << 20))
                k = rng.randint(min_n, 24)
                picks = list(range(len(lat)))
                rng.shuffle(picks)
                on = [tuple(x + c for x, c in zip(lat[i], centre)) for i in sorted(picks[:k])]
                rad = s * math.isqrt(CIRCLE_N if dim == 2 else SPHERE_N)
                extra = rng.randint(0, 30 - k)
                inner = interior_points(rng, dim, extra, (rad - 2) * (rad - 2), centre, rad - 2)
                explicit = dedupe(on + inner)
            elif family == "grid":
                g = rng.randint(1, 5)
                s = rng.randint(1 << 10, 1 << 18)
                cells = [
                    tuple(s * c for c in v)
                    for v in (
                        [(x, y) for x in range(g + 1) for y in range(g + 1)]
                        if dim == 2
                        else [(x, y, z) for x in range(g + 1) for y in range(g + 1) for z in range(g + 1)]
                    )
                ]
                rng.shuffle(cells)
                k = rng.randint(min_n, min(30, len(cells)))
                explicit = [tuple(x + c for x, c in zip(v, centre)) for v in cells[:k]]
            elif family == "large":
                # Log-uniform size in [31, 5000].
                n = int(31.0 * (5000.0 / 31.0) ** rng.uniform())
                n = min(max(n, 31), 5000)
                rad = rng.randint(1 << 12, 1 << 21)
                gen = (n, rng.next_u64(), rad, centre)
            else:  # large_cocircular / large_cospherical
                base = circ if dim == 2 else sph
                s, lat = scaled_lattice(rng, base, dim, rng.randint(1 << 14, 1 << 20))
                rad = s * math.isqrt(CIRCLE_N if dim == 2 else SPHERE_N)
                explicit = [tuple(x + c for x, c in zip(p, centre)) for p in lat]
                inner_rad = rad // 2
                n_in = rng.randint(0, 4000)
                gen = (n_in, rng.next_u64(), inner_rad, centre) if n_in > 0 else None
            pts = list(explicit)
            if gen is not None:
                pts += gen_ball_points(dim, gen[0], gen[1], gen[2], gen[3])
            if len(pts) < min_n or not affinely_full(pts[: min(len(pts), 64)] if len(pts) > 64 else pts):
                continue
            serial += 1
            made += 1
            cid = f"{kind[0]}{serial:04d}"
            inc = incremental_mcc(pts, SplitMix64(serial))
            if len(pts) <= 30:
                bf = brute_force_mcc(pts)
                if bf is None or not same_ball(bf, inc):
                    raise RuntimeError(f"oracle disagreement on {cid}")
                oracle = "brute"
                ball = bf
            else:
                oracle = "incremental"
                ball = inc
            cases.append((cid, kind, family, oracle, explicit, gen, pts, ball))
    return cases


def ball_expect(ball, pts):
    cnum, den, r2num = ball
    unit = Fraction(2) ** UNIT_EXP
    r = (Decimal(r2num).sqrt() / Decimal(den)) * (Decimal(2) ** UNIT_EXP)
    r_f = float(r)
    centre = [float(Fraction(c, den) * unit) for c in cnum]
    on = [i for i, p in enumerate(pts) if sq([den * pi - ci for pi, ci in zip(p, cnum)]) == r2num]
    return r_f, centre, on


def mcc_text(cases) -> bytes:
    out = [
        "# KRN.ENV.XCHK AC-1: minimum enclosing circle and sphere cases.",
        "# Generated by fixtures/fit/make_xchk_fixtures.py (CC-BY-4.0). Do not edit.",
        f"# Coordinates are integers in units of 2^{UNIT_EXP} m (exact in binary64).",
        "# case <id> <circle|sphere> <family> <oracle brute|incremental>",
        "# pts <count> <coords...>            explicit points (2 or 3 integers each)",
        "# gen <count> <seed> <rad> <centre...> then SplitMix64(seed) rejection sampling of",
        "#     v in [-rad, rad]^dim (each coordinate next_u64() mod (2 rad + 1) - rad, in order),",
        "#     kept when |v|^2 <= rad^2, point v + centre; appended after the explicit points",
        "# expect <r2 numerator> <r2 denominator> <r hex> <centre hex...> on <k> <indices...>",
        "#     r^2 exact in units^2 (fraction), r and centre in metres (correctly rounded),",
        "#     `on`: every point exactly on the minimum enclosing ball",
    ]
    for cid, kind, family, oracle, explicit, gen, pts, ball in cases:
        out.append(f"case {cid} {kind} {family} {oracle}")
        flat = " ".join(str(x) for p in explicit for x in p)
        out.append(f"pts {len(explicit)}" + (f" {flat}" if flat else ""))
        if gen is not None:
            out.append(f"gen {gen[0]} {gen[1]} {gen[2]} " + " ".join(str(c) for c in gen[3]))
        r_f, centre, on = ball_expect(ball, pts)
        cnum, den, r2num = ball
        out.append(
            f"expect {r2num} {den * den} {f64_hex(r_f)} "
            + " ".join(f64_hex(c) for c in centre)
            + f" on {len(on)} "
            + " ".join(str(i) for i in on)
        )
        out.append("end")
    return ("\n".join(out) + "\n").encode("ascii")


# ------------------------------------------------------------------ MIC: grid-refinement oracle

TWO_PI = 6.283185307179586
PI = 3.141592653589793


def cos_t(x: float) -> float:
    """Cosine from + - * / only (range reduction by 2 pi, Taylor series to x^40)."""
    k = math.floor(x / TWO_PI + 0.5)
    y = x - k * TWO_PI
    y2 = y * y
    term = 1.0
    s = 1.0
    for i in range(1, 21):
        term = -term * y2 / ((2 * i - 1) * (2 * i))
        s += term
    return s


def sin_t(x: float) -> float:
    return cos_t(x - PI / 2.0)


def orient(a, b, c) -> int:
    ax, ay = Fraction(a[0]), Fraction(a[1])
    d = (Fraction(b[0]) - ax) * (Fraction(c[1]) - ay) - (Fraction(b[1]) - ay) * (Fraction(c[0]) - ax)
    return (d > 0) - (d < 0)


def hull(pts):
    """Strict CCW convex hull (Andrew's monotone chain, exact orientation)."""
    p = sorted(set(pts))
    lower = []
    for q in p:
        while len(lower) >= 2 and orient(lower[-2], lower[-1], q) <= 0:
            lower.pop()
        lower.append(q)
    upper = []
    for q in reversed(p):
        while len(upper) >= 2 and orient(upper[-2], upper[-1], q) <= 0:
            upper.pop()
        upper.append(q)
    return lower[:-1] + upper[:-1]


def clip(poly, clipper):
    """Sutherland-Hodgman: convex `poly` intersected with convex CCW `clipper` (floats)."""
    out = list(poly)
    m = len(clipper)
    for i in range(m):
        a = clipper[i]
        b = clipper[(i + 1) % m]
        inp = out
        out = []
        if not inp:
            break

        def side(p):
            return (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])

        for j in range(len(inp)):
            cur = inp[j]
            prev = inp[j - 1]
            sc = side(cur)
            sp = side(prev)
            if sc >= 0.0:
                if sp < 0.0:
                    t = sp / (sp - sc)
                    out.append((prev[0] + t * (cur[0] - prev[0]), prev[1] + t * (cur[1] - prev[1])))
                out.append(cur)
            elif sp >= 0.0:
                t = sp / (sp - sc)
                out.append((prev[0] + t * (cur[0] - prev[0]), prev[1] + t * (cur[1] - prev[1])))
    return out


def dist(a, b) -> float:
    dx = a[0] - b[0]
    dy = a[1] - b[1]
    return math.sqrt(dx * dx + dy * dy)


def seg_closest(p, a, b):
    dx = b[0] - a[0]
    dy = b[1] - a[1]
    ll = dx * dx + dy * dy
    t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / ll if ll > 0.0 else 0.0
    t = 0.0 if t < 0.0 else (1.0 if t > 1.0 else t)
    return (a[0] + t * dx, a[1] + t * dy)


def poly_inside(p, poly) -> bool:
    m = len(poly)
    for i in range(m):
        a = poly[i]
        b = poly[(i + 1) % m]
        if (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) < 0.0:
            return False
    return True


def boundary_dist(p, poly) -> float:
    m = len(poly)
    best = None
    for i in range(m):
        q = seg_closest(p, poly[i], poly[(i + 1) % m])
        d = dist(p, q)
        if best is None or d < best:
            best = d
    return best


def project(p, poly):
    """`p` if inside the polygon, else its nearest boundary point."""
    if poly_inside(p, poly):
        return p, 0.0
    m = len(poly)
    best = None
    for i in range(m):
        q = seg_closest(p, poly[i], poly[(i + 1) % m])
        d = dist(p, q)
        if best is None or d < best[1]:
            best = (q, d)
    return best


def f_min(c, pts):
    best = None
    for p in pts:
        d = dist(c, p)
        if best is None or d < best:
            best = d
    return best


def mic_oracle(pts, poly):
    """Branch and bound for max over the polygon of f(c) = min_i |p_i - c|. f is 1-Lipschitz,
    so a square cell of half-size s with feasible sample x (the cell centre, or its projection
    onto the polygon) holds no value above f(x) + |x - centre| + s sqrt(2). Cells are split in
    four until s < 1e-13 m; every cell whose bound reaches the best sample is kept."""
    xs = [p[0] for p in poly]
    ys = [p[1] for p in poly]
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    span = max(x1 - x0, y1 - y0)
    g = 24
    s = span / (2 * g)
    rt2 = math.sqrt(2.0)
    cells = []
    for i in range(g):
        for j in range(g):
            cells.append(((x0 + (2 * i + 1) * s, y0 + (2 * j + 1) * s), pts))
    best_val = -1.0
    best_pt = None
    while True:
        evals = []
        for centre, cand in cells:
            x, off = project(centre, poly)
            if off > s * rt2:
                continue  # the cell misses the polygon
            v = f_min(x, cand)
            ub = v + off + s * rt2
            evals.append((centre, x, v, ub, cand))
            if v > best_val or (v == best_val and (x[0], x[1]) < (best_pt[0], best_pt[1])):
                best_val = v
                best_pt = x
        if s < 1e-13:
            break
        nxt = []
        h = s / 2.0
        for centre, x, v, ub, cand in evals:
            if ub < best_val:
                continue
            # Points that can be nearest anywhere in this cell.
            reach = v + 2.0 * (math.sqrt((x[0] - centre[0]) ** 2 + (x[1] - centre[1]) ** 2) + s * rt2) + 1e-12
            sub = [p for p in cand if dist(x, p) <= reach]
            for dx in (-h, h):
                for dy in (-h, h):
                    nxt.append(((centre[0] + dx, centre[1] + dy), sub))
        cells = nxt
        s = h
        if len(cells) > 400000:
            raise RuntimeError("branch and bound did not narrow")
    return best_val, best_pt


def make_mic_cases():
    rng = SplitMix64(SEED_MIC)
    cases = []
    serial = 0
    for cov_deg in (360, 180, 90):
        for _ in range(7):
            serial += 1
            cases.append(mic_case(rng, serial, cov_deg, False))
    for _ in range(3):
        serial += 1
        cases.append(mic_case(rng, serial, 360, True))
    return cases


def mic_case(rng, serial, cov_deg, clipped):
    n = rng.randint(60, 300)
    cx = rng.uniform(-0.5, 0.5)
    cy = rng.uniform(-0.5, 0.5)
    rad = rng.uniform(0.004, 0.04)
    lobes = rng.randint(2, 5)
    amp = rad * rng.uniform(0.001, 0.01)
    phase = rng.uniform(0.0, TWO_PI)
    noise = rad * rng.uniform(1e-5, 1e-4)
    theta0 = rng.uniform(0.0, TWO_PI)
    cov = TWO_PI * (cov_deg / 360.0)
    pts = []
    for _ in range(n):
        t = theta0 + cov * rng.uniform()
        r = rad + amp * cos_t(lobes * t + phase) + noise * rng.uniform(-1.0, 1.0)
        pts.append((cx + r * cos_t(t), cy + r * sin_t(t)))
    region = hull(pts)
    clip_poly = None
    if clipped:
        # A square of half-width 0.4 rad about a point 0.5 rad from the centre: it cuts the
        # region so the optimum is pushed to (or near) the square's edge.
        a = rng.uniform(0.0, TWO_PI)
        qx = cx + 0.5 * rad * cos_t(a)
        qy = cy + 0.5 * rad * sin_t(a)
        hw = 0.4 * rad
        clip_poly = [(qx - hw, qy - hw), (qx + hw, qy - hw), (qx + hw, qy + hw), (qx - hw, qy + hw)]
        region = clip(region, clip_poly)
    val, at = mic_oracle(pts, region)
    bd = boundary_dist(at, region)
    if bd <= 1e-11:
        on_boundary = 1
    elif bd >= 1e-6:
        on_boundary = 0
    else:
        raise RuntimeError(f"ambiguous boundary distance {bd} in case {serial}")
    return (f"m{serial:03d}", cov_deg, "hull_clip" if clipped else "hull", pts, clip_poly, val, at, on_boundary)


def mic_text(cases) -> bytes:
    out = [
        "# KRN.ENV.XCHK AC-2: maximum inscribed circle in a convex region (Voronoi path).",
        "# Generated by fixtures/fit/make_xchk_fixtures.py (CC-BY-4.0). Do not edit.",
        "# Coordinates are binary64 bit patterns (hex), metres.",
        "# case <id> <coverage degrees> <region hull|hull_clip>",
        "# pts <n> <x y ...>      the points; the region is their convex hull",
        "# clip <m> <x y ...>     (hull_clip only) CCW square the hull is intersected with",
        "# expect <r> <cx> <cy> <on_region_boundary 0|1>   grid-refinement oracle",
    ]
    for cid, cov, region, pts, clip_poly, val, at, onb in cases:
        out.append(f"case {cid} {cov} {region}")
        out.append(f"pts {len(pts)} " + " ".join(f64_hex(v) for p in pts for v in p))
        if clip_poly is not None:
            out.append(f"clip {len(clip_poly)} " + " ".join(f64_hex(v) for p in clip_poly for v in p))
        out.append(f"expect {f64_hex(val)} {f64_hex(at[0])} {f64_hex(at[1])} {onb}")
        out.append("end")
    return ("\n".join(out) + "\n").encode("ascii")


def expected_outputs():
    return {
        MCC_DIR / "cases.txt": mcc_text(make_mcc_cases()),
        MIC_DIR / "cases.txt": mic_text(make_mic_cases()),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="verify the fixtures match regenerated bytes")
    args = ap.parse_args()
    outputs = expected_outputs()
    if args.check:
        ok = True
        for p, data in sorted(outputs.items()):
            if not p.exists():
                print(f"MISSING {p}", file=sys.stderr)
                ok = False
            elif p.read_bytes() != data:
                print(f"DRIFT {p}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    for p, data in sorted(outputs.items()):
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
