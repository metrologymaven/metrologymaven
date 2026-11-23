#!/usr/bin/env python3
"""Seeded reference generator for fixtures/cons/prim-defs-v1 (KRN.CONS.ELEM.PRIM, AC-1..AC-5).

License: CC-BY-4.0.
Generates 500 independent closed-form test cases for each of the 16 definitional primitive
variants (point: 4, line: 4, plane: 4, circle: 2, sphere: 2), using deterministic SplitMix64
streams and exact high-precision Decimal arithmetic for circumcircle and circumsphere solves.
"""

from __future__ import annotations

import decimal
import math
from decimal import Decimal
from pathlib import Path

decimal.getcontext().prec = 60

OUT = Path(__file__).resolve().parent
N = 500


class Rng:
    def __init__(self, seed: int) -> None:
        self.s = seed & ((1 << 64) - 1)

    def u64(self) -> int:
        self.s = (self.s + 0x9E3779B97F4A7C15) & ((1 << 64) - 1)
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & ((1 << 64) - 1)
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & ((1 << 64) - 1)
        return z ^ (z >> 31)

    def uni(self) -> float:
        return (self.u64() >> 11) * (2.0**-53)

    def range(self, lo: float, hi: float) -> float:
        return lo + (hi - lo) * self.uni()

    def v3(self, lo: float = -5.0, hi: float = 5.0) -> list[float]:
        return [self.range(lo, hi), self.range(lo, hi), self.range(lo, hi)]

    def unit3(self) -> list[float]:
        while True:
            v = self.v3(-1.0, 1.0)
            n = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
            if n > 0.2:
                return norm_dir([v[0] / n, v[1] / n, v[2] / n])


def norm_dir(v: list[float]) -> list[float]:
    n = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
    u = [v[0] / n, v[1] / n, v[2] / n]
    # Sign-normalisation matching fid_fit::contract::normalize_direction (REQ-KRN.LSQ.TRAIT-007):
    # first non-zero component is positive.
    for x in u:
        if x != 0.0:
            if x < 0.0:
                u = [-c for c in u]
            break
    return [0.0 if x == 0.0 else x for x in u]


def add3(a: list[float], b: list[float]) -> list[float]:
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def sub3(a: list[float], b: list[float]) -> list[float]:
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def scale3(a: list[float], s: float) -> list[float]:
    return [a[0] * s, a[1] * s, a[2] * s]


def dot3(a: list[float], b: list[float]) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross3(a: list[float], b: list[float]) -> list[float]:
    return [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]


def norm3(a: list[float]) -> float:
    return math.sqrt(dot3(a, a))


def solve3_dec(rows: list[list[Decimal]], rhs: list[Decimal]) -> list[Decimal]:
    # Exact Gaussian elimination with partial pivoting in 60-digit Decimal.
    a = [r[:] + [rhs[i]] for i, r in enumerate(rows)]
    for col in range(3):
        piv = max(range(col, 3), key=lambda r: abs(a[r][col]))
        a[col], a[piv] = a[piv], a[col]
        pv = a[col][col]
        for r in range(col + 1, 3):
            f = a[r][col] / pv
            for c in range(col, 4):
                a[r][c] -= f * a[col][c]
    x = [Decimal(0)] * 3
    for i in range(2, -1, -1):
        s = a[i][3]
        for j in range(i + 1, 3):
            s -= a[i][j] * x[j]
        x[i] = s / a[i][i]
    return x


def write_tsv(name: str, header: str, rows: list[list[float]]) -> None:
    lines = [f"# CC-BY-4.0 | {header}"]
    for r in rows:
        lines.append("\t".join(repr(float(v)) for v in r))
    (OUT / name).write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    rng = Rng(2026112301)

    # 1. point.coords
    rows = []
    for _ in range(N):
        c = rng.v3()
        rows.append(c + c)
    write_tsv("point_coords.tsv", "x y z exp_x exp_y exp_z", rows)

    # 2. point.centre_of (sphere/circle centres)
    rows = []
    for _ in range(N):
        c = rng.v3()
        r = rng.range(0.05, 3.0)
        rows.append(c + [r] + c)
    write_tsv("point_centre_of.tsv", "cx cy cz r exp_x exp_y exp_z", rows)

    # 3. point.apex_of (cone)
    rows = []
    for _ in range(N):
        apex = rng.v3()
        d = rng.unit3()
        ha = rng.range(0.1, 1.2)
        rows.append(apex + d + [ha] + apex)
    write_tsv("point_apex_of.tsv", "ax ay az dx dy dz half_angle exp_x exp_y exp_z", rows)

    # 4. point.along_axis
    rows = []
    for _ in range(N):
        p = rng.v3()
        d = rng.unit3()
        dist = rng.range(-4.0, 4.0)
        exp = add3(p, scale3(d, dist))
        rows.append(p + d + [dist] + exp)
    write_tsv("point_along_axis.tsv", "px py pz dx dy dz dist exp_x exp_y exp_z", rows)

    # 5. line.two_points
    rows = []
    for _ in range(N):
        while True:
            p0, p1 = rng.v3(), rng.v3()
            if norm3(sub3(p1, p0)) > 0.1:
                break
        d = norm_dir(sub3(p1, p0))
        rows.append(p0 + p1 + p0 + d)
    write_tsv("line_two_points.tsv", "p0x p0y p0z p1x p1y p1z exp_px exp_py exp_pz exp_dx exp_dy exp_dz", rows)

    # 6. line.axis_of (cylinder)
    rows = []
    for _ in range(N):
        p = rng.v3()
        d = rng.unit3()
        r = rng.range(0.05, 2.0)
        rows.append(p + d + [r] + p + d)
    write_tsv("line_axis_of.tsv", "px py pz dx dy dz r exp_px exp_py exp_pz exp_dx exp_dy exp_dz", rows)

    # 7. line.point_direction (point + line)
    rows = []
    for _ in range(N):
        pt = rng.v3()
        lp = rng.v3()
        d = rng.unit3()
        rows.append(pt + lp + d + pt + d)
    write_tsv("line_point_direction.tsv", "ptx pty ptz lpx lpy lpz dx dy dz exp_px exp_py exp_pz exp_dx exp_dy exp_dz", rows)

    # 8. line.normal_through (plane + point)
    rows = []
    for _ in range(N):
        pp = rng.v3()
        n = rng.unit3()
        pt = rng.v3()
        rows.append(pp + n + pt + pt + n)
    write_tsv("line_normal_through.tsv", "ppx ppy ppz nx ny nz ptx pty ptz exp_px exp_py exp_pz exp_dx exp_dy exp_dz", rows)

    # 9. plane.three_points
    rows = []
    for _ in range(N):
        while True:
            p0, p1, p2 = rng.v3(), rng.v3(), rng.v3()
            cr = cross3(sub3(p1, p0), sub3(p2, p0))
            if norm3(cr) > 0.2:
                break
        n = norm_dir(cr)
        c = [(p0[i] + p1[i] + p2[i]) / 3.0 for i in range(3)]
        rows.append(p0 + p1 + p2 + c + n)
    write_tsv("plane_three_points.tsv", "p0(3) p1(3) p2(3) exp_p(3) exp_n(3)", rows)

    # 10. plane.point_normal
    rows = []
    for _ in range(N):
        pt = rng.v3()
        lp = rng.v3()
        d = rng.unit3()
        rows.append(pt + lp + d + pt + d)
    write_tsv("plane_point_normal.tsv", "pt(3) lp(3) d(3) exp_p(3) exp_n(3)", rows)

    # 11. plane.line_point
    rows = []
    for _ in range(N):
        while True:
            pl = rng.v3()
            d = rng.unit3()
            pt = rng.v3()
            cr = cross3(d, sub3(pt, pl))
            if norm3(cr) > 0.2:
                break
        n = norm_dir(cr)
        rows.append(pl + d + pt + pl + n)
    write_tsv("plane_line_point.tsv", "pl(3) d(3) pt(3) exp_p(3) exp_n(3)", rows)

    # 12. plane.two_lines (250 intersecting + 250 parallel)
    rows = []
    for i in range(N):
        if i < N // 2:
            # Intersecting at q
            while True:
                q = rng.v3()
                d1 = rng.unit3()
                d2 = rng.unit3()
                cr = cross3(d1, d2)
                if norm3(cr) > 0.2:
                    break
            p1 = add3(q, scale3(d1, rng.range(-2.0, 2.0)))
            p2 = add3(q, scale3(d2, rng.range(-2.0, 2.0)))
            n = norm_dir(cr)
            rows.append(p1 + d1 + p2 + d2 + q + n)
        else:
            # Parallel separated
            d = rng.unit3()
            while True:
                p1 = rng.v3()
                off = rng.v3(-3.0, 3.0)
                perp = cross3(d, off)
                if norm3(perp) > 0.2:
                    break
            p2 = add3(p1, off)
            n = norm_dir(perp)
            mid = scale3(add3(p1, p2), 0.5)
            rows.append(p1 + d + p2 + d + mid + n)
    write_tsv("plane_two_lines.tsv", "p1(3) d1(3) p2(3) d2(3) exp_p(3) exp_n(3)", rows)

    # 13. circle.three_points (generate from known circle so exact truth is independent!)
    rows = []
    for _ in range(N):
        c = rng.v3()
        n = rng.unit3()
        r = rng.range(0.1, 4.0)
        # Orthonormal basis u, v in plane normal to n
        ref = [1.0, 0.0, 0.0] if abs(n[0]) < 0.8 else [0.0, 1.0, 0.0]
        u = norm_dir(cross3(n, ref))
        v = norm_dir(cross3(n, u))
        a0 = rng.range(0.0, 2.0 * math.pi)
        a1 = a0 + rng.range(0.6, 2.0)
        a2 = a1 + rng.range(0.6, 2.0)
        pts = [
            add3(c, add3(scale3(u, r * math.cos(a)), scale3(v, r * math.sin(a))))
            for a in (a0, a1, a2)
        ]
        # Independent high-precision Decimal circumcircle check from pts:
        dp = [[Decimal.from_float(x) for x in p] for p in pts]
        e12 = [dp[1][k] - dp[0][k] for k in range(3)]
        e13 = [dp[2][k] - dp[0][k] for k in range(3)]
        cr = [
            e12[1] * e13[2] - e12[2] * e13[1],
            e12[2] * e13[0] - e12[0] * e13[2],
            e12[0] * e13[1] - e12[1] * e13[0],
        ]
        cn = (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt()
        dn = [x / cn for x in cr]
        rhs = [
            Decimal("0.5") * sum(x * x for x in e12),
            Decimal("0.5") * sum(x * x for x in e13),
            Decimal(0),
        ]
        dx = solve3_dec([e12, e13, dn], rhs)
        dc = [float(dp[0][k] + dx[k]) for k in range(3)]
        dr = float((dx[0] * dx[0] + dx[1] * dx[1] + dx[2] * dx[2]).sqrt())
        en = norm_dir([float(x) for x in dn])
        rows.append(pts[0] + pts[1] + pts[2] + dc + en + [dr])
    write_tsv("circle_three_points.tsv", "p0(3) p1(3) p2(3) exp_c(3) exp_n(3) exp_r", rows)

    # 14. circle.centre_axis_radius
    rows = []
    for _ in range(N):
        c = rng.v3()
        lp = rng.v3()
        d = rng.unit3()
        r = rng.range(0.05, 5.0)
        rows.append(c + lp + d + [r] + c + d + [r])
    write_tsv("circle_centre_axis_radius.tsv", "c(3) lp(3) d(3) r exp_c(3) exp_n(3) exp_r", rows)

    # 15. sphere.four_points
    rows = []
    for _ in range(N):
        c = rng.v3()
        r = rng.range(0.2, 4.0)
        while True:
            dirs = [rng.unit3() for _ in range(4)]
            pts = [add3(c, scale3(d, r)) for d in dirs]
            a, b, cv = sub3(pts[1], pts[0]), sub3(pts[2], pts[0]), sub3(pts[3], pts[0])
            vol = abs(dot3(a, cross3(b, cv)))
            if vol > 0.1 * (r**3):
                break
        dp = [[Decimal.from_float(x) for x in p] for p in pts]
        da = [dp[1][k] - dp[0][k] for k in range(3)]
        db = [dp[2][k] - dp[0][k] for k in range(3)]
        dc_v = [dp[3][k] - dp[0][k] for k in range(3)]
        rhs = [
            Decimal("0.5") * sum(x * x for x in da),
            Decimal("0.5") * sum(x * x for x in db),
            Decimal("0.5") * sum(x * x for x in dc_v),
        ]
        dx = solve3_dec([da, db, dc_v], rhs)
        ec = [float(dp[0][k] + dx[k]) for k in range(3)]
        er = float((dx[0] * dx[0] + dx[1] * dx[1] + dx[2] * dx[2]).sqrt())
        rows.append(pts[0] + pts[1] + pts[2] + pts[3] + ec + [er])
    write_tsv("sphere_four_points.tsv", "p0(3) p1(3) p2(3) p3(3) exp_c(3) exp_r", rows)

    # 16. sphere.centre_radius
    rows = []
    for _ in range(N):
        c = rng.v3()
        r = rng.range(0.05, 5.0)
        rows.append(c + [r] + c + [r])
    write_tsv("sphere_centre_radius.tsv", "c(3) r exp_c(3) exp_r", rows)


if __name__ == "__main__":
    main()
