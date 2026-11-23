#!/usr/bin/env python3
"""Independent closed-form reference generator for KRN.CONS.OPS.PROJ (AC-1..AC-4, CC-BY-4.0).

Writes `fixtures/cons/proj-v1/`:
  - `point_plane.tsv`:  1,000 seeded point-onto-plane cases
  - `point_line.tsv`:   1,000 seeded point-onto-line cases
  - `line_plane.tsv`:   1,000 seeded line-onto-plane cases
  - `circle_plane.tsv`: 1,000 seeded circle-onto-plane cases (tilted -> ellipse)

Run: python3 fixtures/cons/make_proj_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

from __future__ import annotations

from decimal import Decimal, getcontext
import math
import random
import sys
from pathlib import Path

getcontext().prec = 60

HERE = Path(__file__).resolve().parent / "proj-v1"
SEED = 2026112302
N_CASES = 1000


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def norm(a):
    return math.sqrt(dot(a, a))


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


def dec_cross(a, b):
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def dec_norm(a):
    return dec_dot(a, a).sqrt()


def dec_canon(a):
    n = dec_norm(a)
    u = (a[0] / n, a[1] / n, a[2] / n)
    for c in u:
        if c != 0:
            if c < 0:
                return (-u[0], -u[1], -u[2])
            break
    return u


def fmt(vals) -> str:
    return "\t".join(f"{v:.17g}" for v in vals)


def gen_point_plane(rng: random.Random) -> str:
    lines = ["# c(3) pp(3) np(3) exp_c(3) exp_signed_dist(1)"]
    for _ in range(N_CASES):
        c = rand_pt(rng)
        pp = rand_pt(rng)
        np_ = rand_unit(rng)
        dc, dpp, dnp = to_dec3(c), to_dec3(pp), dec_canon(to_dec3(np_))
        s = dec_dot(tuple(dc[k] - dpp[k] for k in range(3)), dnp)
        rc = tuple(float(dc[k] - s * dnp[k]) for k in range(3))
        lines.append(fmt([*c, *pp, *np_, *rc, float(s)]))
    return "\n".join(lines) + "\n"


def gen_point_line(rng: random.Random) -> str:
    lines = ["# c(3) pl(3) dl(3) exp_c(3) exp_dist(1)"]
    for _ in range(N_CASES):
        c = rand_pt(rng)
        pl = rand_pt(rng)
        dl = rand_unit(rng)
        dc, dpl, ddl = to_dec3(c), to_dec3(pl), dec_canon(to_dec3(dl))
        t = dec_dot(tuple(dc[k] - dpl[k] for k in range(3)), ddl)
        rc = tuple(dpl[k] + t * ddl[k] for k in range(3))
        dist = dec_norm(tuple(dc[k] - rc[k] for k in range(3)))
        lines.append(fmt([*c, *pl, *dl, *(float(x) for x in rc), float(dist)]))
    return "\n".join(lines) + "\n"


def gen_line_plane(rng: random.Random) -> str:
    lines = ["# pl(3) dl(3) pp(3) np(3) exp_p(3) exp_d(3) exp_signed_dist(1)"]
    for _ in range(N_CASES):
        pl = rand_pt(rng)
        pp = rand_pt(rng)
        np_ = rand_unit(rng)
        while True:
            dl = rand_unit(rng)
            if norm(cross(dl, np_)) > 0.15:
                break
        dpl, ddl = to_dec3(pl), dec_canon(to_dec3(dl))
        dpp, dnp = to_dec3(pp), dec_canon(to_dec3(np_))
        proj_raw = tuple(ddl[k] - dec_dot(ddl, dnp) * dnp[k] for k in range(3))
        dd = dec_canon(proj_raw)
        s = dec_dot(tuple(dpl[k] - dpp[k] for k in range(3)), dnp)
        rp = tuple(dpl[k] - s * dnp[k] for k in range(3))
        # Canonical Line3 foot closest to origin: rp - dot(rp, dd) * dd.
        rp_min = tuple(rp[k] - dec_dot(rp, dd) * dd[k] for k in range(3))
        lines.append(fmt([*pl, *dl, *pp, *np_, *(float(x) for x in rp_min), *(float(x) for x in dd), float(s)]))
    return "\n".join(lines) + "\n"


def gen_circle_plane(rng: random.Random) -> str:
    lines = ["# c(3) nc(3) r(1) pp(3) np(3) exp_c(3) exp_d(3) exp_a(1) exp_b(1) exp_signed_dist(1)"]
    for _ in range(N_CASES):
        c = rand_pt(rng)
        pp = rand_pt(rng)
        r = rng.uniform(0.005, 2.5)
        np_ = rand_unit(rng)
        while True:
            nc = rand_unit(rng)
            ct = abs(dot(nc, np_))
            if 0.15 < ct < 0.98:
                break
        dc, dnc, dr = to_dec3(c), dec_canon(to_dec3(nc)), Decimal(r)
        dpp, dnp = to_dec3(pp), dec_canon(to_dec3(np_))
        s = dec_dot(tuple(dc[k] - dpp[k] for k in range(3)), dnp)
        rc = tuple(dc[k] - s * dnp[k] for k in range(3))
        dd = dec_canon(dec_cross(dnc, dnp))
        ct_dec = abs(dec_dot(dnc, dnp))
        ra = dr
        rb = dr * ct_dec
        lines.append(
            fmt([*c, *nc, r, *pp, *np_, *(float(x) for x in rc), *(float(x) for x in dd), float(ra), float(rb), float(s)])
        )
    return "\n".join(lines) + "\n"


def main() -> int:
    check = "--check" in sys.argv[1:]
    rng = random.Random(SEED)
    files = {
        "point_plane.tsv": gen_point_plane(rng),
        "point_line.tsv": gen_point_line(rng),
        "line_plane.tsv": gen_line_plane(rng),
        "circle_plane.tsv": gen_circle_plane(rng),
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
