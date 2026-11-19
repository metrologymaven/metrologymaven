#!/usr/bin/env python3
"""Independent reference for GEO.FRAME.APPLY; writes fixtures/geo/frame/apply_expected.bin.

Run:  python tools/fixtures/geo_apply_cases.py [--check]
With --check nothing is written; the script exits non-zero if the file differs.

Stdlib only (fractions, json, struct); shares no code with fid-geo-frame or fid-geo-types.

Inputs (all existing fixtures, read as IEEE 754 bit patterns, so nothing is re-rounded):

* transform 17 of fixtures/geo/frame/xform_cases.json ("random_11", a similarity);
* fixtures/geo/types/cloud_grid_2scans.txt: 6 144 points and the two scan origins (AC-1);
* fixtures/geo/types/pointset_small.txt with the normals of pointset_small_attrs.txt
  (the cloud fixture has no normals, so AC-1's normal check runs on this set);
* fixtures/geo/types/mesh_cube.txt: the 8 vertices of the unit cube.

Expected values come from a bit-exact model of the expressions pinned in the leaf's Design
(and in GEO.FRAME.XFORM): with dot3(a, b) = (a0*b0 + a1*b1) + a2*b2,

* point and scan origin:  y[i] = s * dot3(R[i], x) + t[i];
* normal:                  m[i] = dot3(R[i], n)   (rotation only, no scale, no renormalisation).

CPython floats are IEEE binary64 with round-to-nearest-even and never fuse a multiply-add, so
the model's bits are the Rust results on every target. Every model value is also checked
against exact rational arithmetic (fractions.Fraction): each coordinate within 1e-13 m of
s R x + t, each normal component within 1e-15 of R n, and every output normal's length
within 1e-12 of 1 (checked as | |m|^2 - 1 | <= 2e-12 - 1e-24, exactly). The script also maps
every point back through the model of the pinned inverse (R^T, 1/s, -(s' (R^T t))) and asserts
the largest coordinate error is at most 1e-12 m (AC-2).

apply_expected.bin layout (little-endian; every f64 is its IEEE 754 bit pattern):

    8 bytes   magic "FGFAPPL1"
    u64       transform index (17)
    u64 n     cloud point count, then n f64 x, n f64 y, n f64 z
    u64 m     scan-origin count, then per origin: u64 scanner ID, f64 x, y, z
    u64 n     point-set count, then n f64 x, n y, n z, then n f64 nx, n ny, n nz
    u64 n     mesh vertex count, then n f64 x, n y, n z
"""

import json
import struct
import sys
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TYPES = ROOT / "fixtures" / "geo" / "types"
OUT = ROOT / "fixtures" / "geo" / "frame" / "apply_expected.bin"
XFORM_INDEX = 17


def f64(h):
    return struct.unpack("<d", struct.pack("<Q", int(h, 16)))[0]


def lines(path):
    for raw in path.read_text(encoding="utf-8").splitlines():
        s = raw.strip()
        if s and not s.startswith("#"):
            yield s.split()


# ------------------------------------------------------------------ inputs


def load_transform():
    cases = json.loads((ROOT / "fixtures" / "geo" / "frame" / "xform_cases.json").read_text(encoding="utf-8"))
    c = cases["cases"][XFORM_INDEX]
    assert c["name"] == "random_11" and c["kind"] == "similarity", c["name"]
    r = [f64(h) for h in c["r"].split()]
    t = [f64(h) for h in c["t"].split()]
    s = f64(c["s"])
    inv = (
        [f64(h) for h in c["i_r"].split()],
        [f64(h) for h in c["i_t"].split()],
        f64(c["i_s"]),
    )
    return [r[0:3], r[3:6], r[6:9]], t, s, inv


def load_cloud():
    pts, origins = [], []
    for f in lines(TYPES / "cloud_grid_2scans.txt"):
        if f[0] == "p":
            assert int(f[1]) == len(pts)
            pts.append([f64(f[3]), f64(f[4]), f64(f[5])])
        elif f[0] == "origin":
            origins.append((int(f[1]), [f64(f[2]), f64(f[3]), f64(f[4])]))
    assert len(pts) == 6144 and len(origins) == 2
    return pts, origins


def load_pointset():
    pts, normals = [], []
    for f in lines(TYPES / "pointset_small.txt"):
        if f[0] == "p":
            assert int(f[1]) == len(pts)
            pts.append([f64(f[2]), f64(f[3]), f64(f[4])])
    for f in lines(TYPES / "pointset_small_attrs.txt"):
        if f[0] == "a":
            assert int(f[1]) == len(normals)
            normals.append([f64(f[2]), f64(f[3]), f64(f[4])])
    assert len(pts) == 1000 and len(normals) == 1000
    return pts, normals


def load_mesh():
    verts = [[float(v) for v in f[1:4]] for f in lines(TYPES / "mesh_cube.txt") if f[0] == "v"]
    assert len(verts) == 8
    return verts


# ------------------------------------------------------------------ pinned float model


def dot3(a0, a1, a2, b0, b1, b2):
    return (a0 * b0 + a1 * b1) + a2 * b2


def apply_point(r, t, s, x):
    return [s * dot3(r[i][0], r[i][1], r[i][2], x[0], x[1], x[2]) + t[i] for i in range(3)]


def apply_dir(r, x):
    return [dot3(r[i][0], r[i][1], r[i][2], x[0], x[1], x[2]) for i in range(3)]


def inverse(r, t, s):
    si = 1.0 / s
    u = [dot3(r[0][i], r[1][i], r[2][i], t[0], t[1], t[2]) for i in range(3)]
    return [[r[0][i], r[1][i], r[2][i]] for i in range(3)], [-(si * u[i]) for i in range(3)], si


# ------------------------------------------------------------------ exact checks


def exact_point(r, t, s, x):
    return [Fraction(s) * sum(Fraction(r[i][k]) * Fraction(x[k]) for k in range(3)) + Fraction(t[i]) for i in range(3)]


def exact_dir(r, x):
    return [sum(Fraction(r[i][k]) * Fraction(x[k]) for k in range(3)) for i in range(3)]


def check_points(r, t, s, xs, ys, what):
    for x, y in zip(xs, ys):
        e = exact_point(r, t, s, x)
        for k in range(3):
            assert abs(Fraction(y[k]) - e[k]) <= Fraction(1, 10**13), (what, x, k)


def check_normals(r, ns, ms):
    lim = Fraction(2, 10**12) - Fraction(1, 10**24)
    for n, m in zip(ns, ms):
        e = exact_dir(r, n)
        for k in range(3):
            assert abs(Fraction(m[k]) - e[k]) <= Fraction(1, 10**15), (n, k)
        sq = sum(Fraction(v) * Fraction(v) for v in m)
        # |m|^2 within 2e-12 - 1e-24 of 1 implies | |m| - 1 | <= 1e-12.
        assert abs(sq - 1) <= lim, n


def round_trip_error(fwd, inv, xs):
    r, t, s = fwd
    ri, ti, si = inv
    worst = 0.0
    for x in xs:
        y = apply_point(r, t, s, x)
        z = apply_point(ri, ti, si, y)
        for k in range(3):
            d = abs(Fraction(z[k]) - Fraction(x[k]))
            if d > worst:
                worst = d
    return worst


# ------------------------------------------------------------------ build


def u64(v):
    return struct.pack("<Q", v)


def col(vals):
    return b"".join(struct.pack("<d", v) for v in vals)


def columns(pts):
    return col([p[0] for p in pts]) + col([p[1] for p in pts]) + col([p[2] for p in pts])


def build():
    r, t, s, (ir, it, i_s) = load_transform()
    # The fixture's inverse is the pinned inverse of the model (xform_cases.json AC-1).
    mr, mt, ms = inverse(r, t, s)
    assert [v for row in mr for v in row] == ir and mt == it and ms == i_s

    cloud, origins = load_cloud()
    cloud_out = [apply_point(r, t, s, p) for p in cloud]
    origins_out = [(sid, apply_point(r, t, s, o)) for sid, o in origins]
    check_points(r, t, s, cloud, cloud_out, "cloud")
    check_points(r, t, s, [o for _, o in origins], [o for _, o in origins_out], "origin")

    pset, normals = load_pointset()
    pset_out = [apply_point(r, t, s, p) for p in pset]
    normals_out = [apply_dir(r, n) for n in normals]
    check_points(r, t, s, pset, pset_out, "pointset")
    check_normals(r, normals, normals_out)

    cube = load_mesh()
    cube_out = [apply_point(r, t, s, p) for p in cube]
    check_points(r, t, s, cube, cube_out, "mesh")

    # AC-2: forward then the pinned inverse returns within 1e-12 m.
    worst = round_trip_error((r, t, s), (mr, mt, ms), cloud + [o for _, o in origins] + pset + cube)
    assert worst <= Fraction(1, 10**12), float(worst)

    out = b"FGFAPPL1" + u64(XFORM_INDEX)
    out += u64(len(cloud_out)) + columns(cloud_out)
    out += u64(len(origins_out)) + b"".join(u64(sid) + col(o) for sid, o in origins_out)
    out += u64(len(pset_out)) + columns(pset_out) + columns(normals_out)
    out += u64(len(cube_out)) + columns(cube_out)
    return out, float(worst)


def main():
    check = "--check" in sys.argv[1:]
    data, worst = build()
    rel = OUT.relative_to(ROOT)
    if check:
        if not OUT.exists() or OUT.read_bytes() != data:
            print(f"{rel} is stale; run python tools/fixtures/geo_apply_cases.py", file=sys.stderr)
            return 1
        print(f"geo_apply_cases: up to date (round trip max error {worst:.3e} m)")
        return 0
    OUT.write_bytes(data)
    print(f"wrote {rel} ({len(data)} bytes; round trip max error {worst:.3e} m)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
