#!/usr/bin/env python3
"""Seeded reference generator for fixtures/cons/shape-defs-v1 (KRN.CONS.ELEM.SHAPE, AC-1..AC-5).

License: CC-BY-4.0.
Generates 300 independent closed-form test cases for each of the 9 definitional shape
variants (cylinder: 3, cone: 2, slot: 2, rectangle: 2).
"""

from __future__ import annotations

import math
from pathlib import Path

OUT = Path(__file__).resolve().parent
N = 300


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

    def v3(self, lo: float = -4.0, hi: float = 4.0) -> list[float]:
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
    for x in u:
        if x != 0.0:
            if x < 0.0:
                u = [-c for c in u]
            break
    return [0.0 if x == 0.0 else x for x in u]


def unit_only(v: list[float]) -> list[float]:
    n = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
    return [0.0 if x == 0.0 else x / n for x in v]


def add3(a: list[float], b: list[float]) -> list[float]:
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def sub3(a: list[float], b: list[float]) -> list[float]:
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def scale3(a: list[float], s: float) -> list[float]:
    return [a[0] * s, a[1] * s, a[2] * s]


def cross3(a: list[float], b: list[float]) -> list[float]:
    return [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]


def basis_from_normal(n: list[float]) -> tuple[list[float], list[float]]:
    ref = [0.37, 0.61, 0.70] if abs(n[0] * 0.37 + n[1] * 0.61 + n[2] * 0.70) < 0.85 else [0.73, -0.59, 0.34]
    u = norm_dir(cross3(n, ref))
    v = norm_dir(cross3(n, u))
    return u, v


def write_tsv(name: str, header: str, rows: list[list[float]]) -> None:
    lines = [f"# CC-BY-4.0 | {header}"]
    for r in rows:
        lines.append("\t".join(repr(float(v)) for v in r))
    (OUT / name).write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    rng = Rng(2026112302)

    # 1. cylinder.axis_radius
    rows = []
    for _ in range(N):
        p = rng.v3()
        d = rng.unit3()
        r = rng.range(0.05, 3.0)
        rows.append(p + d + [r] + p + d + [r])
    write_tsv("cylinder_axis_radius.tsv", "p(3) d(3) r exp_p(3) exp_d(3) exp_r", rows)

    # 2. cylinder.circle_length
    rows = []
    for _ in range(N):
        c = rng.v3()
        n = rng.unit3()
        r = rng.range(0.05, 3.0)
        length = rng.range(0.1, 4.0) * (1.0 if rng.uni() > 0.5 else -1.0)
        exp_p = add3(c, scale3(n, 0.5 * length))
        rows.append(c + n + [r, length] + exp_p + n + [r])
    write_tsv("cylinder_circle_length.tsv", "c(3) n(3) r length exp_p(3) exp_d(3) exp_r", rows)

    # 3. cylinder.two_circles (coaxial, nearly equal radii within 1e-8 m)
    rows = []
    for _ in range(N):
        c1 = rng.v3()
        d = rng.unit3()
        h = rng.range(0.1, 4.0)
        c2 = add3(c1, scale3(d, h))
        r1 = rng.range(0.1, 3.0)
        r2 = r1 + rng.range(-1e-8, 1e-8)
        mid = scale3(add3(c1, c2), 0.5)
        rm = 0.5 * (r1 + r2)
        rows.append(c1 + d + [r1] + c2 + d + [r2] + mid + d + [rm])
    write_tsv("cylinder_two_circles.tsv", "c1(3) n1(3) r1 c2(3) n2(3) r2 exp_p(3) exp_d(3) exp_r", rows)

    # 4. cone.two_circles (both r2 > r1 and r1 > r2)
    rows = []
    for i in range(N):
        apex = rng.v3()
        d_cone = unit_only(rng.v3(-1.0, 1.0))
        ha = rng.range(0.08, 1.1)
        s1 = rng.range(0.2, 2.0)
        s2 = s1 + rng.range(0.2, 2.5)
        if i % 2 == 1:
            s1, s2 = s2, s1
        c1 = add3(apex, scale3(d_cone, s1))
        c2 = add3(apex, scale3(d_cone, s2))
        r1 = s1 * math.tan(ha)
        r2 = s2 * math.tan(ha)
        n_circ = norm_dir(d_cone)
        rows.append(c1 + n_circ + [r1] + c2 + n_circ + [r2] + apex + d_cone + [ha])
    write_tsv("cone_two_circles.tsv", "c1(3) n1(3) r1 c2(3) n2(3) r2 exp_apex(3) exp_d(3) exp_ha", rows)

    # 5. cone.axis_apex_angle
    rows = []
    for i in range(N):
        apex = rng.v3()
        d = rng.unit3()
        p = add3(apex, scale3(d, rng.range(-2.0, 2.0)))
        ha = rng.range(0.08, 1.2)
        flip = float(i % 2)
        exp_d = [-x for x in d] if flip == 1.0 else d
        rows.append(p + d + apex + [ha, flip] + apex + exp_d + [ha])
    write_tsv("cone_axis_apex_angle.tsv", "p(3) d(3) apex(3) ha flip exp_apex(3) exp_d(3) exp_ha", rows)

    # 6. slot.two_circles
    rows = []
    for _ in range(N):
        c1 = rng.v3()
        n = rng.unit3()
        u, _ = basis_from_normal(n)
        length = rng.range(0.1, 4.0)
        c2 = add3(c1, scale3(u, length))
        r1 = rng.range(0.05, 1.5)
        r2 = r1 + rng.range(-1e-8, 1e-8)
        mid = scale3(add3(c1, c2), 0.5)
        width = r1 + r2
        rows.append(c1 + n + [r1] + c2 + n + [r2] + mid + n + u + [length, width])
    write_tsv("slot_two_circles.tsv", "c1(3) n1(3) r1 c2(3) n2(3) r2 exp_c(3) exp_n(3) exp_d(3) exp_l exp_w", rows)

    # 7. slot.centre_line_sizes
    rows = []
    for _ in range(N):
        pp = rng.v3()
        n = rng.unit3()
        u, v = basis_from_normal(n)
        c = add3(pp, add3(scale3(u, rng.range(-2.0, 2.0)), scale3(v, rng.range(-2.0, 2.0))))
        length = rng.range(0.0, 4.0)
        width = rng.range(0.05, 2.0)
        rows.append(pp + n + c + u + [length, width] + c + n + u + [length, width])
    write_tsv("slot_centre_line_sizes.tsv", "pp(3) n(3) pl(3) d(3) length width exp_c(3) exp_n(3) exp_d(3) exp_l exp_w", rows)

    # 8. rectangle.two_line_pairs
    rows = []
    for _ in range(N):
        c = rng.v3()
        n = rng.unit3()
        u, v = basis_from_normal(n)
        length = rng.range(0.5, 4.0)
        width = rng.range(0.1, length)
        # Pair A parallel to u, separated by width along v; Pair B parallel to v, separated by length along u
        pa1 = add3(c, add3(scale3(u, rng.range(-1.0, 1.0)), scale3(v, -0.5 * width)))
        pa2 = add3(c, add3(scale3(u, rng.range(-1.0, 1.0)), scale3(v, 0.5 * width)))
        pb1 = add3(c, add3(scale3(u, -0.5 * length), scale3(v, rng.range(-1.0, 1.0))))
        pb2 = add3(c, add3(scale3(u, 0.5 * length), scale3(v, rng.range(-1.0, 1.0))))
        rows.append(pa1 + u + pa2 + u + pb1 + v + pb2 + v + c + n + u + [length, width])
    write_tsv("rectangle_two_line_pairs.tsv", "pa1(3) da1(3) pa2(3) da2(3) pb1(3) db1(3) pb2(3) db2(3) exp_c(3) exp_n(3) exp_d(3) exp_l exp_w", rows)

    # 9. rectangle.centre_dir_sizes
    rows = []
    for _ in range(N):
        pp = rng.v3()
        n = rng.unit3()
        u, v = basis_from_normal(n)
        c = add3(pp, add3(scale3(u, rng.range(-2.0, 2.0)), scale3(v, rng.range(-2.0, 2.0))))
        lp = add3(c, scale3(v, rng.range(-1.0, 1.0)))
        length = rng.range(0.5, 4.0)
        width = rng.range(0.1, length)
        rows.append(c + pp + n + lp + u + [length, width] + c + n + u + [length, width])
    write_tsv("rectangle_centre_dir_sizes.tsv", "c(3) pp(3) n(3) lp(3) d(3) length width exp_c(3) exp_n(3) exp_d(3) exp_l exp_w", rows)


if __name__ == "__main__":
    main()
