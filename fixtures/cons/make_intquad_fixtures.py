#!/usr/bin/env python3
"""Independent closed-form reference generator for KRN.CONS.OPS.INTQUAD (AC-1..AC-4, CC-BY-4.0).

Writes `fixtures/cons/intquad-v1/` (500 seeded cases per variant = 4,000 cases):
  - `line_sphere.tsv`, `line_cylinder.tsv`, `line_cone.tsv`
  - `plane_sphere.tsv`, `plane_cylinder.tsv`, `plane_cone.tsv`
  - `circle_circle.tsv`, `sphere_sphere.tsv`

Run: python3 fixtures/cons/make_intquad_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

from __future__ import annotations

from decimal import Decimal, getcontext
import math
import random
import sys
from pathlib import Path

getcontext().prec = 60

HERE = Path(__file__).resolve().parent / "intquad-v1"
SEED = 2026112304
N_CASES = 500


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


def unit(a):
    n = norm(a)
    return (a[0] / n, a[1] / n, a[2] / n)


def canon(a):
    u = unit(a)
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
            if abs(u[0]) > 1e-3:
                return u


def rand_pt(rng: random.Random):
    return (rng.uniform(-20.0, 20.0), rng.uniform(-20.0, 20.0), rng.uniform(-20.0, 20.0))


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


def dec_unit(a):
    n = dec_norm(a)
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


def ortho_basis(u):
    ref = (0.31, -0.57, 0.76)
    if abs(dot(u, ref)) > 0.85:
        ref = (0.71, 0.43, -0.55)
    e1 = unit(cross(u, ref))
    e2 = unit(cross(u, e1))
    return e1, e2


def gen_line_sphere(rng: random.Random) -> str:
    lines = ["# pl(3) dl(3) c(3) r(1) exp1(3) exp2(3)"]
    for _ in range(N_CASES):
        c = rand_pt(rng)
        r = rng.uniform(0.05, 3.0)
        u = canon(rand_unit(rng))
        e1, e2 = ortho_basis(u)
        h = rng.uniform(0.0, 0.7 * r)
        ang = rng.uniform(0.0, 2.0 * math.pi)
        offset = add(scale(e1, h * math.cos(ang)), scale(e2, h * math.sin(ang)))
        pl = add(add(c, offset), scale(u, rng.uniform(-4.0, 4.0)))

        dpl, du, dc, dr = to_dec3(pl), dec_unit(to_dec3(u)), to_dec3(c), Decimal(r)
        w = tuple(dpl[k] - dc[k] for k in range(3))
        tm = -dec_dot(w, du)
        f = tuple(w[k] + tm * du[k] for k in range(3))
        dh = dec_norm(f)
        delta = ((dr - dh) * (dr + dh)).sqrt()
        p1 = tuple(float(dpl[k] + (tm - delta) * du[k]) for k in range(3))
        p2 = tuple(float(dpl[k] + (tm + delta) * du[k]) for k in range(3))
        lines.append(fmt([*pl, *u, *c, r, *p1, *p2]))
    return "\n".join(lines) + "\n"


def gen_line_cylinder(rng: random.Random) -> str:
    lines = ["# pl(3) dl(3) pc(3) dc(3) r(1) exp1(3) exp2(3)"]
    for _ in range(N_CASES):
        pc = rand_pt(rng)
        dc = canon(rand_unit(rng))
        r = rng.uniform(0.05, 3.0)
        while True:
            dl = canon(rand_unit(rng))
            if norm(cross(dl, dc)) > 0.25:
                break
        e1, e2 = ortho_basis(dc)
        h = rng.uniform(0.0, 0.7 * r)
        ang = rng.uniform(0.0, 2.0 * math.pi)
        pt_on = add(add(pc, scale(dc, rng.uniform(-3.0, 3.0))), add(scale(e1, h * math.cos(ang)), scale(e2, h * math.sin(ang))))
        pl = add(pt_on, scale(dl, rng.uniform(-4.0, 4.0)))

        dpl, dul = to_dec3(pl), dec_unit(to_dec3(dl))
        dpc, duc, dr = to_dec3(pc), dec_canon(to_dec3(dc)), Decimal(r)
        up = tuple(dul[k] - dec_dot(dul, duc) * duc[k] for k in range(3))
        a = dec_dot(up, up)
        w = tuple(dpl[k] - dpc[k] for k in range(3))
        wp = tuple(w[k] - dec_dot(w, duc) * duc[k] for k in range(3))
        tm = -dec_dot(wp, up) / a
        fp = tuple(wp[k] + tm * up[k] for k in range(3))
        dh = dec_norm(fp)
        delta = (((dr - dh) * (dr + dh)) / a).sqrt()
        p1 = tuple(float(dpl[k] + (tm - delta) * dul[k]) for k in range(3))
        p2 = tuple(float(dpl[k] + (tm + delta) * dul[k]) for k in range(3))
        lines.append(fmt([*pl, *dl, *pc, *dc, r, *p1, *p2]))
    return "\n".join(lines) + "\n"


def gen_line_cone(rng: random.Random) -> str:
    lines = ["# pl(3) dl(3) apex(3) dc(3) alpha(1) exp1(3) exp2(3)"]
    for _ in range(N_CASES):
        apex = rand_pt(rng)
        dc = rand_unit(rng)
        alpha = rng.uniform(0.25, 1.0)
        e1, e2 = ortho_basis(dc)
        z = rng.uniform(0.5, 4.0)
        rad = z * math.tan(alpha)
        ang1 = rng.uniform(0.0, 2.0 * math.pi)
        ang2 = ang1 + rng.uniform(0.8, 2.3)
        z2 = z * rng.uniform(0.7, 1.3)
        rad2 = z2 * math.tan(alpha)
        q1 = add(add(apex, scale(dc, z)), add(scale(e1, rad * math.cos(ang1)), scale(e2, rad * math.sin(ang1))))
        q2 = add(add(apex, scale(dc, z2)), add(scale(e1, rad2 * math.cos(ang2)), scale(e2, rad2 * math.sin(ang2))))
        dl = canon(sub(q2, q1))
        pl = add(q1, scale(dl, rng.uniform(-2.0, 2.0)))

        dpl, dul = to_dec3(pl), dec_unit(to_dec3(dl))
        da, duc = to_dec3(apex), dec_unit(to_dec3(dc))
        # High-precision cos(alpha) using math.cos on Decimal via Taylor series
        ax = Decimal(alpha)
        # Compute cos(ax) to 50 digits via Taylor series
        cos_a = Decimal(1)
        term = Decimal(1)
        for k in range(1, 30):
            term = -term * ax * ax / Decimal((2 * k - 1) * (2 * k))
            cos_a += term
        c2 = cos_a * cos_a
        du = dec_dot(dul, duc)
        w = tuple(dpl[k] - da[k] for k in range(3))
        wu = dec_dot(w, duc)
        qa = du * du - c2
        qb = Decimal(2) * (du * wu - c2 * dec_dot(w, dul))
        qc = wu * wu - c2 * dec_dot(w, w)
        disc = (qb * qb - Decimal(4) * qa * qc).sqrt()
        r1 = (-qb - disc) / (Decimal(2) * qa)
        r2 = (-qb + disc) / (Decimal(2) * qa)
        t_lo, t_hi = (r1, r2) if r1 <= r2 else (r2, r1)
        p1 = tuple(float(dpl[k] + t_lo * dul[k]) for k in range(3))
        p2 = tuple(float(dpl[k] + t_hi * dul[k]) for k in range(3))
        lines.append(fmt([*pl, *dl, *apex, *dc, alpha, *p1, *p2]))
    return "\n".join(lines) + "\n"


def gen_plane_sphere(rng: random.Random) -> str:
    lines = ["# pp(3) np(3) c(3) r(1) exp_c(3) exp_n(3) exp_r(1)"]
    for _ in range(N_CASES):
        c = rand_pt(rng)
        r = rng.uniform(0.05, 3.0)
        np_ = canon(rand_unit(rng))
        s = rng.uniform(-0.7 * r, 0.7 * r)
        e1, e2 = ortho_basis(np_)
        pp = add(sub(c, scale(np_, s)), add(scale(e1, rng.uniform(-2.0, 2.0)), scale(e2, rng.uniform(-2.0, 2.0))))
        dpp, dnp, dc, dr = to_dec3(pp), dec_canon(to_dec3(np_)), to_dec3(c), Decimal(r)
        ds = dec_dot(tuple(dc[k] - dpp[k] for k in range(3)), dnp)
        dh = abs(ds)
        rc = tuple(float(dc[k] - ds * dnp[k]) for k in range(3))
        rr = float(((dr - dh) * (dr + dh)).sqrt())
        lines.append(fmt([*pp, *np_, *c, r, *rc, *(float(x) for x in dnp), rr]))
    return "\n".join(lines) + "\n"


def gen_plane_cylinder(rng: random.Random) -> str:
    lines = ["# pp(3) np(3) pc(3) dc(3) r(1) exp_c(3) exp_d(3) exp_a(1) exp_b(1)"]
    for _ in range(N_CASES):
        pc = rand_pt(rng)
        dc = canon(rand_unit(rng))
        r = rng.uniform(0.05, 3.0)
        while True:
            np_ = canon(rand_unit(rng))
            ct = abs(dot(np_, dc))
            if 0.2 < ct < 0.95:
                break
        e1, e2 = ortho_basis(np_)
        base = add(pc, scale(dc, rng.uniform(-3.0, 3.0)))
        pp = add(base, add(scale(e1, rng.uniform(-2.0, 2.0)), scale(e2, rng.uniform(-2.0, 2.0))))
        dpp, dnp = to_dec3(pp), dec_canon(to_dec3(np_))
        dpc, duc, dr = to_dec3(pc), dec_canon(to_dec3(dc)), Decimal(r)
        dp = dec_dot(dnp, duc)
        t = dec_dot(tuple(dpp[k] - dpc[k] for k in range(3)), dnp) / dp
        rc = tuple(float(dpc[k] + t * duc[k]) for k in range(3))
        cr = dec_cross(dnp, duc)
        dd = dec_canon(dec_cross(dnp, cr))
        ra = float(dr / abs(dp))
        lines.append(fmt([*pp, *np_, *pc, *dc, r, *rc, *(float(x) for x in dd), ra, r]))
    return "\n".join(lines) + "\n"


def gen_plane_cone(rng: random.Random) -> str:
    lines = ["# pp(3) np(3) apex(3) dc(3) alpha(1) exp_c(3) exp_d(3) exp_a(1) exp_b(1)"]
    for _ in range(N_CASES):
        apex = rand_pt(rng)
        dc = rand_unit(rng)
        alpha = rng.uniform(0.15, 0.55)
        # Tilt plane normal by phi in (0.15, pi/2 - alpha - 0.15)
        phi = rng.uniform(0.15, math.pi * 0.5 - alpha - 0.15)
        e1, e2 = ortho_basis(dc)
        az = rng.uniform(0.0, 2.0 * math.pi)
        tang = add(scale(e1, math.cos(az)), scale(e2, math.sin(az)))
        up_pos = unit(add(scale(dc, math.cos(phi)), scale(tang, math.sin(phi))))
        np_ = canon(up_pos)
        h = rng.uniform(0.5, 4.0)
        pe1, pe2 = ortho_basis(up_pos)
        pp = add(add(apex, scale(up_pos, h)), add(scale(pe1, rng.uniform(-1.0, 1.0)), scale(pe2, rng.uniform(-1.0, 1.0))))

        dpp, dnp = to_dec3(pp), dec_canon(to_dec3(np_))
        da, duc = to_dec3(apex), dec_unit(to_dec3(dc))
        dp = dec_dot(dnp, duc)
        dup = dnp if dp >= 0 else (-dnp[0], -dnp[1], -dnp[2])
        cphi = abs(dp)
        sphi = dec_norm(dec_cross(dup, duc))
        dh = dec_dot(tuple(dpp[k] - da[k] for k in range(3)), dup)
        ax = Decimal(alpha)
        sin_a = ax
        term = ax
        for k in range(1, 30):
            term = -term * ax * ax / Decimal((2 * k) * (2 * k + 1))
            sin_a += term
        cos_a = (Decimal(1) - sin_a * sin_a).sqrt()
        kk = cphi * cphi - sin_a * sin_a
        umaj_raw = tuple(duc[k] - cphi * dup[k] for k in range(3))
        umaj_pos = tuple(umaj_raw[k] / sphi for k in range(3))
        xc = (dh * sphi * cphi) / kk
        rc = tuple(float(da[k] + dh * dup[k] + xc * umaj_pos[k]) for k in range(3))
        ra = float((dh * cos_a * sin_a) / kk)
        rb = float((dh * sin_a) / kk.sqrt())
        dd = dec_canon(umaj_pos)
        lines.append(fmt([*pp, *np_, *apex, *dc, alpha, *rc, *(float(x) for x in dd), ra, rb]))
    return "\n".join(lines) + "\n"


def gen_circle_circle(rng: random.Random) -> str:
    lines = ["# c1(3) n(3) r1(1) c2(3) r2(1) exp1(3) exp2(3)"]
    for _ in range(N_CASES):
        c1 = rand_pt(rng)
        n = canon(rand_unit(rng))
        e1, e2 = ortho_basis(n)
        r1 = rng.uniform(0.2, 3.0)
        r2 = rng.uniform(0.2, 3.0)
        d = rng.uniform(abs(r1 - r2) + 0.05, r1 + r2 - 0.05)
        ang = rng.uniform(0.0, 2.0 * math.pi)
        dir_vec = add(scale(e1, math.cos(ang)), scale(e2, math.sin(ang)))
        c2 = add(c1, scale(dir_vec, d))

        dc1, dn, dr1, dc2, dr2 = to_dec3(c1), dec_canon(to_dec3(n)), Decimal(r1), to_dec3(c2), Decimal(r2)
        delta = tuple(dc2[k] - dc1[k] for k in range(3))
        ip = tuple(delta[k] - dec_dot(delta, dn) * dn[k] for k in range(3))
        dd = dec_norm(ip)
        de1 = tuple(ip[k] / dd for k in range(3))
        de2 = dec_canon(dec_cross(dn, de1))
        x = (dd * dd + dr1 * dr1 - dr2 * dr2) / (Decimal(2) * dd)
        h = ((dr1 - x) * (dr1 + x)).sqrt()
        mid = tuple(dc1[k] + x * de1[k] for k in range(3))
        p1 = tuple(float(mid[k] - h * de2[k]) for k in range(3))
        p2 = tuple(float(mid[k] + h * de2[k]) for k in range(3))
        lines.append(fmt([*c1, *n, r1, *c2, r2, *p1, *p2]))
    return "\n".join(lines) + "\n"


def gen_sphere_sphere(rng: random.Random) -> str:
    lines = ["# c1(3) r1(1) c2(3) r2(1) exp_c(3) exp_n(3) exp_r(1)"]
    for _ in range(N_CASES):
        c1 = rand_pt(rng)
        u = rand_unit(rng)
        r1 = rng.uniform(0.2, 3.0)
        r2 = rng.uniform(0.2, 3.0)
        d = rng.uniform(abs(r1 - r2) + 0.05, r1 + r2 - 0.05)
        c2 = add(c1, scale(u, d))
        dc1, dr1, dc2, dr2 = to_dec3(c1), Decimal(r1), to_dec3(c2), Decimal(r2)
        delta = tuple(dc2[k] - dc1[k] for k in range(3))
        dd = dec_norm(delta)
        du = tuple(delta[k] / dd for k in range(3))
        x = (dd * dd + dr1 * dr1 - dr2 * dr2) / (Decimal(2) * dd)
        rc = tuple(float(dc1[k] + x * du[k]) for k in range(3))
        rr = float(((dr1 - x) * (dr1 + x)).sqrt())
        dn = dec_canon(du)
        lines.append(fmt([*c1, r1, *c2, r2, *rc, *(float(x) for x in dn), rr]))
    return "\n".join(lines) + "\n"


def main() -> int:
    check = "--check" in sys.argv[1:]
    rng = random.Random(SEED)
    files = {
        "line_sphere.tsv": gen_line_sphere(rng),
        "line_cylinder.tsv": gen_line_cylinder(rng),
        "line_cone.tsv": gen_line_cone(rng),
        "plane_sphere.tsv": gen_plane_sphere(rng),
        "plane_cylinder.tsv": gen_plane_cylinder(rng),
        "plane_cone.tsv": gen_plane_cone(rng),
        "circle_circle.tsv": gen_circle_circle(rng),
        "sphere_sphere.tsv": gen_sphere_sphere(rng),
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
