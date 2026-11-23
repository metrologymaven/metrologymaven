#!/usr/bin/env python3
"""Independent closed-form reference generator for KRN.CONS.OPS.INTLIN (AC-1..AC-4, CC-BY-4.0).

Writes `fixtures/cons/intlin-v1/`:
  - `line_line.tsv`:   1,000 seeded 3D line pairs (including inputs ~1 km from origin & skew pairs)
  - `line_plane.tsv`:  1,000 seeded line-plane pairs (including inputs ~1 km from origin)
  - `plane_plane.tsv`: 1,000 seeded plane-plane pairs (including inputs ~1 km from origin)
  - `plane3.tsv`:      1,000 seeded plane-plane-plane triples (including inputs ~1 km from origin)

Run: python3 fixtures/cons/make_intlin_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

from __future__ import annotations

from decimal import Decimal, getcontext
import math
import random
import sys
from pathlib import Path

getcontext().prec = 60

HERE = Path(__file__).resolve().parent / "intlin-v1"
SEED = 2026112301
N_CASES = 1000


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def add(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def scale(a, s):
    return (a[0] * s, a[1] * s, a[2] * s)


def norm(a):
    return math.sqrt(dot(a, a))


def canon_dir(a):
    n = norm(a)
    u = (a[0] / n, a[1] / n, a[2] / n)
    for c in u:
        if c != 0.0:
            if c < 0.0:
                return (-u[0], -u[1], -u[2])
            break
    return u


def rand_unit(rng: random.Random):
    while True:
        v = (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0))
        n = norm(v)
        if n > 0.25:
            u = (v[0] / n, v[1] / n, v[2] / n)
            if abs(u[0]) > 1e-4:
                return u


def rand_origin(rng: random.Random, idx: int):
    # Mix near-origin and ~1 km from origin (AC-1).
    mag = 1000.0 if (idx % 3 == 0) else 10.0
    return (
        rng.uniform(-mag, mag),
        rng.uniform(-mag, mag),
        rng.uniform(-mag, mag),
    )


def fmt(vals) -> str:
    return "\t".join(f"{v:.17g}" for v in vals)


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


def dec_canon_dir(a):
    n = dec_norm(a)
    u = (a[0] / n, a[1] / n, a[2] / n)
    for c in u:
        if c != 0:
            if c < 0:
                return (-u[0], -u[1], -u[2])
            break
    return u


def solve_dec_3x3(rows, rhs):
    a = [[rows[i][j] for j in range(3)] + [rhs[i]] for i in range(3)]
    for col in range(3):
        piv = max(range(col, 3), key=lambda r: abs(a[r][col]))
        a[col], a[piv] = a[piv], a[col]
        pv = a[col][col]
        for j in range(col, 4):
            a[col][j] /= pv
        for r in range(3):
            if r != col:
                f = a[r][col]
                for j in range(col, 4):
                    a[r][j] -= f * a[col][j]
    return (a[0][3], a[1][3], a[2][3])


def gen_line_line(rng: random.Random) -> str:
    lines = ["# p1(3) d1(3) p2(3) d2(3) exp_c(3) exp_skew(1)"]
    for i in range(N_CASES):
        base = rand_origin(rng, i)
        u1 = rand_unit(rng)
        while True:
            u2 = rand_unit(rng)
            if norm(cross(u1, u2)) > 0.15:
                break
        n_perp = canon_dir(cross(u1, u2))
        skew = 0.0 if (i % 2 == 0) else rng.uniform(1e-5, 0.05)
        q1 = add(base, scale(n_perp, 0.5 * skew))
        q2 = add(base, scale(n_perp, -0.5 * skew))
        p1 = add(q1, scale(u1, rng.uniform(-5.0, 5.0)))
        p2 = add(q2, scale(u2, rng.uniform(-5.0, 5.0)))

        # Independent 60-digit Decimal closed-form reference from actual stored f64 inputs.
        dp1, du1 = to_dec3(p1), dec_canon_dir(to_dec3(u1))
        dp2, du2 = to_dec3(p2), dec_canon_dir(to_dec3(u2))
        b = dec_dot(du1, du2)
        w = (dp1[0] - dp2[0], dp1[1] - dp2[1], dp1[2] - dp2[2])
        d1 = dec_dot(w, du1)
        d2 = dec_dot(w, du2)
        det = Decimal(1) - b * b
        s = (-d1 + b * d2) / det
        t = (d2 - b * d1) / det
        rq1 = tuple(dp1[k] + s * du1[k] for k in range(3))
        rq2 = tuple(dp2[k] + t * du2[k] for k in range(3))
        rmid = tuple(float((rq1[k] + rq2[k]) / Decimal(2)) for k in range(3))
        rskew = float(dec_norm(tuple(rq1[k] - rq2[k] for k in range(3))))
        lines.append(fmt([*p1, *u1, *p2, *u2, *rmid, rskew]))
    return "\n".join(lines) + "\n"


def gen_line_plane(rng: random.Random) -> str:
    lines = ["# pl(3) dl(3) pp(3) np(3) exp_c(3)"]
    for i in range(N_CASES):
        base = rand_origin(rng, i)
        np_ = rand_unit(rng)
        while True:
            dl = rand_unit(rng)
            if abs(dot(dl, np_)) > 0.15:
                break
        t1, t2 = canon_dir(cross(np_, (0.123, -0.456, 0.789))), None
        t2 = canon_dir(cross(np_, t1))
        pl = add(base, scale(dl, rng.uniform(-5.0, 5.0)))
        pp = add(base, add(scale(t1, rng.uniform(-3.0, 3.0)), scale(t2, rng.uniform(-3.0, 3.0))))

        dpl, ddl = to_dec3(pl), dec_canon_dir(to_dec3(dl))
        dpp, dnp = to_dec3(pp), dec_canon_dir(to_dec3(np_))
        diff = tuple(dpp[k] - dpl[k] for k in range(3))
        t = dec_dot(diff, dnp) / dec_dot(ddl, dnp)
        rc = tuple(float(dpl[k] + t * ddl[k]) for k in range(3))
        lines.append(fmt([*pl, *dl, *pp, *np_, *rc]))
    return "\n".join(lines) + "\n"


def gen_plane_plane(rng: random.Random) -> str:
    lines = ["# p1(3) n1(3) p2(3) n2(3) exp_p(3) exp_d(3)"]
    for i in range(N_CASES):
        base = rand_origin(rng, i)
        n1 = rand_unit(rng)
        while True:
            n2 = rand_unit(rng)
            if norm(cross(n1, n2)) > 0.15:
                break
        d_approx = canon_dir(cross(n1, n2))
        b1 = canon_dir(cross(n1, d_approx))
        b2 = canon_dir(cross(n2, d_approx))
        p1 = add(base, add(scale(d_approx, rng.uniform(-4.0, 4.0)), scale(b1, rng.uniform(-4.0, 4.0))))
        p2 = add(base, add(scale(d_approx, rng.uniform(-4.0, 4.0)), scale(b2, rng.uniform(-4.0, 4.0))))

        dp1, dn1 = to_dec3(p1), dec_canon_dir(to_dec3(n1))
        dp2, dn2 = to_dec3(p2), dec_canon_dir(to_dec3(n2))
        dd = dec_canon_dir(dec_cross(dn1, dn2))
        rp = solve_dec_3x3([dn1, dn2, dd], [dec_dot(dn1, dp1), dec_dot(dn2, dp2), Decimal(0)])
        lines.append(fmt([*p1, *n1, *p2, *n2, *(float(x) for x in rp), *(float(x) for x in dd)]))
    return "\n".join(lines) + "\n"


def gen_plane3(rng: random.Random) -> str:
    lines = ["# p1(3) n1(3) p2(3) n2(3) p3(3) n3(3) exp_c(3)"]
    for i in range(N_CASES):
        base = rand_origin(rng, i)
        n1 = rand_unit(rng)
        while True:
            n2 = rand_unit(rng)
            if norm(cross(n1, n2)) > 0.2:
                break
        c12 = canon_dir(cross(n1, n2))
        while True:
            n3 = rand_unit(rng)
            if norm(cross(n2, n3)) > 0.2 and norm(cross(n3, n1)) > 0.2 and abs(dot(c12, n3)) > 0.2:
                break
        pts = []
        for n in (n1, n2, n3):
            t1 = canon_dir(cross(n, (0.31, -0.57, 0.76)))
            t2 = canon_dir(cross(n, t1))
            pts.append(add(base, add(scale(t1, rng.uniform(-3.0, 3.0)), scale(t2, rng.uniform(-3.0, 3.0)))))
        p1, p2, p3 = pts

        dp1, dn1 = to_dec3(p1), dec_canon_dir(to_dec3(n1))
        dp2, dn2 = to_dec3(p2), dec_canon_dir(to_dec3(n2))
        dp3, dn3 = to_dec3(p3), dec_canon_dir(to_dec3(n3))
        rc = solve_dec_3x3([dn1, dn2, dn3], [dec_dot(dn1, dp1), dec_dot(dn2, dp2), dec_dot(dn3, dp3)])
        lines.append(fmt([*p1, *n1, *p2, *n2, *p3, *n3, *(float(x) for x in rc)]))
    return "\n".join(lines) + "\n"


def main() -> int:
    check = "--check" in sys.argv[1:]
    rng = random.Random(SEED)
    files = {
        "line_line.tsv": gen_line_line(rng),
        "line_plane.tsv": gen_line_plane(rng),
        "plane_plane.tsv": gen_plane_plane(rng),
        "plane3.tsv": gen_plane3(rng),
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
