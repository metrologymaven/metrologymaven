#!/usr/bin/env python3
"""Independent closed-form reference generator for KRN.CONS.OPS.OFFS (AC-1..AC-4, CC-BY-4.0).

Writes `fixtures/cons/offs-v1/`:
  - `plane_normal.tsv`:  1,000 seeded plane-normal offset cases
  - `point_dir.tsv`:     1,000 seeded point-direction offset cases
  - `line_dir.tsv`:      1,000 seeded line-direction offset cases
  - `circle_axial.tsv`:  1,000 seeded circle-axial offset cases
  - `circle_radial.tsv`: 1,000 seeded circle-radial offset cases

Run: python3 fixtures/cons/make_offs_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

from __future__ import annotations

from decimal import Decimal, getcontext
import math
import random
import sys
from pathlib import Path

getcontext().prec = 60

HERE = Path(__file__).resolve().parent / "offs-v1"
SEED = 2026112303
N_CASES = 1000


def norm(a):
    return math.sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2])


def rand_unit(rng: random.Random):
    while True:
        v = (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0))
        n = norm(v)
        if n > 0.25:
            u = (v[0] / n, v[1] / n, v[2] / n)
            if abs(u[0]) > 1e-4:
                return u


def rand_pt(rng: random.Random):
    return (rng.uniform(-50.0, 50.0), rng.uniform(-50.0, 50.0), rng.uniform(-50.0, 50.0))


def to_dec3(v):
    return (Decimal(v[0]), Decimal(v[1]), Decimal(v[2]))


def dec_dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def dec_unit(a):
    n = dec_dot(a, a).sqrt()
    return (a[0] / n, a[1] / n, a[2] / n)


def dec_canon(a):
    u = dec_unit(a)
    for c in u:
        if c != 0:
            if c < 0:
                return (-u[0], -u[1], -u[2])
            break
    return u


def fmt(vals) -> str:
    return "\t".join(f"{v:.17g}" for v in vals)


def gen_plane_normal(rng: random.Random) -> str:
    lines = ["# p(3) n(3) dist(1) exp_p(3) exp_n(3)"]
    for _ in range(N_CASES):
        p = rand_pt(rng)
        n = rand_unit(rng)
        d = rng.uniform(-2.0, 2.0)
        dp, du, dn_can, dd = to_dec3(p), dec_unit(to_dec3(n)), dec_canon(to_dec3(n)), Decimal(d)
        rp_raw = tuple(dp[k] + dd * du[k] for k in range(3))
        proj = dec_dot(rp_raw, dn_can)
        rp = tuple(float(proj * dn_can[k]) for k in range(3))
        lines.append(fmt([*p, *n, d, *rp, *(float(x) for x in dn_can)]))
    return "\n".join(lines) + "\n"


def gen_point_dir(rng: random.Random) -> str:
    lines = ["# c(3) dir(3) dist(1) exp_c(3)"]
    for _ in range(N_CASES):
        c = rand_pt(rng)
        dv = rand_unit(rng)
        d = rng.uniform(-5.0, 5.0)
        dc, ddir, dd = to_dec3(c), dec_unit(to_dec3(dv)), Decimal(d)
        rc = tuple(float(dc[k] + dd * ddir[k]) for k in range(3))
        lines.append(fmt([*c, *dv, d, *rc]))
    return "\n".join(lines) + "\n"


def gen_line_dir(rng: random.Random) -> str:
    lines = ["# p(3) d(3) dir(3) dist(1) exp_p(3) exp_d(3)"]
    for _ in range(N_CASES):
        p = rand_pt(rng)
        ld = rand_unit(rng)
        dv = rand_unit(rng)
        dist = rng.uniform(-5.0, 5.0)
        dp, dld, ddir, ddist = to_dec3(p), dec_canon(to_dec3(ld)), dec_unit(to_dec3(dv)), Decimal(dist)
        rp_raw = tuple(dp[k] + ddist * ddir[k] for k in range(3))
        t = dec_dot(rp_raw, dld)
        rp = tuple(float(rp_raw[k] - t * dld[k]) for k in range(3))
        lines.append(fmt([*p, *ld, *dv, dist, *rp, *(float(x) for x in dld)]))
    return "\n".join(lines) + "\n"


def gen_circle_axial(rng: random.Random) -> str:
    lines = ["# c(3) n(3) r(1) dist(1) exp_c(3) exp_n(3) exp_r(1)"]
    for _ in range(N_CASES):
        c = rand_pt(rng)
        n = rand_unit(rng)
        r = rng.uniform(0.005, 5.0)
        dist = rng.uniform(-3.0, 3.0)
        dc, du, dn_can, ddist = to_dec3(c), dec_unit(to_dec3(n)), dec_canon(to_dec3(n)), Decimal(dist)
        rc = tuple(float(dc[k] + ddist * du[k]) for k in range(3))
        lines.append(fmt([*c, *n, r, dist, *rc, *(float(x) for x in dn_can), r]))
    return "\n".join(lines) + "\n"


def gen_circle_radial(rng: random.Random) -> str:
    lines = ["# c(3) n(3) r(1) dist(1) exp_r(1)"]
    for _ in range(N_CASES):
        c = rand_pt(rng)
        n = rand_unit(rng)
        r = rng.uniform(0.02, 5.0)
        dist = rng.uniform(-0.015, 2.0)
        er = float(Decimal(r) + Decimal(dist))
        lines.append(fmt([*c, *n, r, dist, er]))
    return "\n".join(lines) + "\n"


def main() -> int:
    check = "--check" in sys.argv[1:]
    rng = random.Random(SEED)
    files = {
        "plane_normal.tsv": gen_plane_normal(rng),
        "point_dir.tsv": gen_point_dir(rng),
        "line_dir.tsv": gen_line_dir(rng),
        "circle_axial.tsv": gen_circle_axial(rng),
        "circle_radial.tsv": gen_circle_radial(rng),
    }
    if check:
        for name, expected in files.items():
            path = HERE / name
            if not path.exists() or path.read_text(encoding="utf-8") != expected:
                print(f"mismatch: {path}", file=sys.stderr)
                return 1
        print(f"ok: {len(files)} fixture files verified in {HERE}")
        return 0
    HERE.mkdir(parents=True, exist_ok=True)
    for name, content in files.items():
        (HERE / name).write_text(content, encoding="utf-8")
    print(f"wrote {len(files)} fixture files to {HERE}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
