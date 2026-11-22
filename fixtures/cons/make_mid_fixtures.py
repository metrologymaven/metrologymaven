#!/usr/bin/env python3
"""Independent closed-form reference generator for KRN.CONS.OPS.MID (AC-1..AC-4, CC-BY-4.0).

Writes `fixtures/cons/mid-v1/`:
  - `midpoint_points.tsv`: 1,000 seeded point/centre pairs and exact midpoints
  - `symmetry_planes.tsv`: 1,000 seeded parallel plane pairs (random sign flips) and mid-planes
  - `symmetry_lines.tsv`:  1,000 seeded parallel line pairs (random sign flips) and mid-lines
  - `symmetry_circles.tsv`: 1,000 seeded coaxial circle pairs (random sign flips) and mid-circles
  - `bisect_lines.tsv`:    1,000 seeded intersecting line pairs (acute & obtuse) and bisectors
  - `bisect_planes.tsv`:   1,000 seeded intersecting plane pairs (acute & obtuse) and bisectors

Run: python3 fixtures/cons/make_mid_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

from __future__ import annotations

import math
import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent / "mid-v1"
SEED = 20261122
N_CASES = 1000


def dot(a: tuple[float, float, float], b: tuple[float, float, float]) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def add(a: tuple[float, float, float], b: tuple[float, float, float]) -> tuple[float, float, float]:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def sub(a: tuple[float, float, float], b: tuple[float, float, float]) -> tuple[float, float, float]:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def scale(a: tuple[float, float, float], s: float) -> tuple[float, float, float]:
    return (a[0] * s, a[1] * s, a[2] * s)


def norm(a: tuple[float, float, float]) -> float:
    return math.sqrt(dot(a, a))


def canon_dir(a: tuple[float, float, float]) -> tuple[float, float, float]:
    n = norm(a)
    u = (a[0] / n, a[1] / n, a[2] / n)
    for c in u:
        if c != 0.0:
            if c < 0.0:
                return (-u[0], -u[1], -u[2])
            break
    return u


def rand_unit(rng: random.Random) -> tuple[float, float, float]:
    while True:
        v = (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0))
        n = norm(v)
        if n > 0.1:
            return (v[0] / n, v[1] / n, v[2] / n)


def rand_pt(rng: random.Random, lo: float = -10.0, hi: float = 10.0) -> tuple[float, float, float]:
    return (rng.uniform(lo, hi), rng.uniform(lo, hi), rng.uniform(lo, hi))


def fmt(vals: list[float]) -> str:
    return "\t".join(f"{x:.17g}" for x in vals)


def gen_all() -> dict[str, bytes]:
    rng = random.Random(SEED)

    # 1. midpoint_points: p1(3) p2(3) -> mid(3)
    mp_lines = ["# p1x p1y p1z p2x p2y p2z mx my mz"]
    for _ in range(N_CASES):
        p1 = rand_pt(rng)
        p2 = rand_pt(rng)
        m = scale(add(p1, p2), 0.5)
        mp_lines.append(fmt([*p1, *p2, *m]))

    # 2. symmetry_planes: p1(3) n1(3) p2(3) n2(3) -> pm(3) nm(3)
    sp_lines = ["# p1(3) n1(3) p2(3) n2(3) pm(3) nm(3)"]
    for _ in range(N_CASES):
        u = canon_dir(rand_unit(rng))
        s1 = 1.0 if rng.random() < 0.5 else -1.0
        s2 = 1.0 if rng.random() < 0.5 else -1.0
        n1 = scale(u, s1)
        n2 = scale(u, s2)
        p1 = rand_pt(rng)
        offset = rng.uniform(-2.0, 2.0)
        p2 = add(p1, scale(u, offset))
        pm = add(p1, scale(u, 0.5 * offset))
        sp_lines.append(fmt([*p1, *n1, *p2, *n2, *pm, *u]))

    # 3. symmetry_lines: p1(3) d1(3) p2(3) d2(3) -> pm(3) dm(3)
    sl_lines = ["# p1(3) d1(3) p2(3) d2(3) pm(3) dm(3)"]
    for _ in range(N_CASES):
        u = canon_dir(rand_unit(rng))
        s1 = 1.0 if rng.random() < 0.5 else -1.0
        s2 = 1.0 if rng.random() < 0.5 else -1.0
        d1 = scale(u, s1)
        d2 = scale(u, s2)
        p1 = rand_pt(rng)
        shift = rand_pt(rng, -2.0, 2.0)
        p2 = add(p1, shift)
        q2 = add(p2, scale(u, dot(sub(p1, p2), u)))
        pm = scale(add(p1, q2), 0.5)
        sl_lines.append(fmt([*p1, *d1, *p2, *d2, *pm, *u]))

    # 4. symmetry_circles: c1(3) n1(3) r1 c2(3) n2(3) r2 -> cm(3) nm(3) rm
    sc_lines = ["# c1(3) n1(3) r1 c2(3) n2(3) r2 cm(3) nm(3) rm"]
    for _ in range(N_CASES):
        u = canon_dir(rand_unit(rng))
        s1 = 1.0 if rng.random() < 0.5 else -1.0
        s2 = 1.0 if rng.random() < 0.5 else -1.0
        n1 = scale(u, s1)
        n2 = scale(u, s2)
        c1 = rand_pt(rng)
        h = rng.uniform(-1.0, 1.0)
        c2 = add(c1, scale(u, h))
        r1 = rng.uniform(0.005, 1.0)
        r2 = rng.uniform(0.005, 1.0)
        cm = scale(add(c1, c2), 0.5)
        rm = 0.5 * (r1 + r2)
        sc_lines.append(fmt([*c1, *n1, r1, *c2, *n2, r2, *cm, *u, rm]))

    # 5. bisect_lines: branch(0|1) p1(3) d1(3) p2(3) d2(3) -> pb(3) db(3)
    bl_lines = ["# branch p1(3) d1(3) p2(3) d2(3) pb(3) db(3)"]
    for idx in range(N_CASES):
        branch = idx % 2
        while True:
            u1 = canon_dir(rand_unit(rng))
            u2 = canon_dir(rand_unit(rng))
            if dot(u1, u2) < 0.0:
                u2 = scale(u2, -1.0)
            dp = dot(u1, u2)
            if 0.05 < dp < 0.99:
                break
        inter = rand_pt(rng)
        p1 = add(inter, scale(u1, rng.uniform(-2.0, 2.0)))
        p2 = add(inter, scale(u2, rng.uniform(-2.0, 2.0)))
        d1 = scale(u1, 1.0 if rng.random() < 0.5 else -1.0)
        d2 = scale(u2, 1.0 if rng.random() < 0.5 else -1.0)
        raw = add(u1, u2) if branch == 0 else sub(u1, u2)
        db = canon_dir(raw)
        bl_lines.append(f"{branch}\t" + fmt([*p1, *d1, *p2, *d2, *inter, *db]))

    # 6. bisect_planes: branch(0|1) p1(3) n1(3) p2(3) n2(3) -> pb(3) nb(3)
    bp_lines = ["# branch p1(3) n1(3) p2(3) n2(3) pb(3) nb(3)"]
    for idx in range(N_CASES):
        branch = idx % 2
        while True:
            u1 = canon_dir(rand_unit(rng))
            u2 = canon_dir(rand_unit(rng))
            if dot(u1, u2) < 0.0:
                u2 = scale(u2, -1.0)
            dp = dot(u1, u2)
            if 0.05 < dp < 0.99:
                break
        # Intersection line passes through `p0` along `u1 x u2`
        p0 = rand_pt(rng)
        # Pick points on planes 1 and 2
        l = (u1[1] * u2[2] - u1[2] * u2[1], u1[2] * u2[0] - u1[0] * u2[2], u1[0] * u2[1] - u1[1] * u2[0])
        lu = canon_dir(l)
        t1 = canon_dir((u1[1] * lu[2] - u1[2] * lu[1], u1[2] * lu[0] - u1[0] * lu[2], u1[0] * lu[1] - u1[1] * lu[0]))
        t2 = canon_dir((u2[1] * lu[2] - u2[2] * lu[1], u2[2] * lu[0] - u2[0] * lu[2], u2[0] * lu[1] - u2[1] * lu[0]))
        p1 = add(p0, add(scale(lu, rng.uniform(-1.0, 1.0)), scale(t1, rng.uniform(-1.0, 1.0))))
        p2 = add(p0, add(scale(lu, rng.uniform(-1.0, 1.0)), scale(t2, rng.uniform(-1.0, 1.0))))
        n1 = scale(u1, 1.0 if rng.random() < 0.5 else -1.0)
        n2 = scale(u2, 1.0 if rng.random() < 0.5 else -1.0)
        raw = add(u1, u2) if branch == 0 else sub(u1, u2)
        nb = canon_dir(raw)
        bp_lines.append(f"{branch}\t" + fmt([*p1, *n1, *p2, *n2, *p0, *nb]))

    return {
        "midpoint_points.tsv": ("\n".join(mp_lines) + "\n").encode("utf-8"),
        "symmetry_planes.tsv": ("\n".join(sp_lines) + "\n").encode("utf-8"),
        "symmetry_lines.tsv": ("\n".join(sl_lines) + "\n").encode("utf-8"),
        "symmetry_circles.tsv": ("\n".join(sc_lines) + "\n").encode("utf-8"),
        "bisect_lines.tsv": ("\n".join(bl_lines) + "\n").encode("utf-8"),
        "bisect_planes.tsv": ("\n".join(bp_lines) + "\n").encode("utf-8"),
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    files = gen_all()
    if check:
        drift = []
        for name, expected in files.items():
            p = HERE / name
            if not p.exists() or p.read_bytes() != expected:
                drift.append(name)
        if drift:
            print(f"cons/mid-v1 fixtures out of date: {', '.join(drift)}", file=sys.stderr)
            return 1
        print(f"cons/mid-v1 fixtures: ok ({len(files)} files, {N_CASES} cases each)")
        return 0

    HERE.mkdir(parents=True, exist_ok=True)
    for name, content in files.items():
        (HERE / name).write_bytes(content)
    print(f"wrote {len(files)} cons/mid-v1 fixtures ({N_CASES} cases each)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
