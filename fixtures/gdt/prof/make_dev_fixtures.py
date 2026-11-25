#!/usr/bin/env python3
"""Independent oracle for GDT.PROF.DEV (fid-gps `prof::dev`).

Writes `dev_sphere_plane.json` (AC-1, AC-3, AC-4, pose and restriction cases) and
`dev_nurbs_approx.json` (AC-2) next to this script. All geometry is invented. Every
expected value is computed here from the binary64 input coordinates with 60-digit
`decimal` arithmetic and the closed-form geometry of the faces (sphere radial foot,
plane orthogonal foot, circle-edge foot); nothing is computed by the Rust code.

The pose digest is SHA-256 over the RFC 8785 canonical JSON
`{"r": [[r00, r01, r02], [r10, ...], [r20, ...]], "t": [t0, t1, t2]}` with every number
written as `"0x"` + 16 lowercase hex digits of its IEEE-754 bits.

Usage: `python3 make_dev_fixtures.py` (write) or `python3 make_dev_fixtures.py --check`
(regenerate in memory and compare with the committed files byte for byte). Stdlib only.
"""

import hashlib
import json
import math
import struct
import sys
from decimal import Decimal, getcontext
from pathlib import Path

getcontext().prec = 60
HERE = Path(__file__).resolve().parent

R = 0.02  # dome radius, metres (20 mm)
OFF = 5e-5  # +-0.05 mm
BOUNDARY_TOL = Decimal("1e-9")


def hx(x: float) -> str:
    return "0x" + struct.pack(">d", x).hex()


def hx3(p):
    return [hx(float(c)) for c in p]


def dstr(x: Decimal) -> str:
    # 30 significant digits: far below the 1e-12 m comparison tolerance.
    if x == 0:
        return "0"
    return format(x, ".29e")


def d3(p):
    return [dstr(c) for c in p]


def D(x: float) -> Decimal:
    return Decimal(x)  # exact binary64 value


def dsqrt(x: Decimal) -> Decimal:
    return x.sqrt()


def norm(v):
    return dsqrt(sum(c * c for c in v))


class SplitMix:
    """SplitMix64: reproducible invented points."""

    def __init__(self, seed):
        self.s = seed & 0xFFFFFFFFFFFFFFFF

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
        return z ^ (z >> 31)

    def uniform(self, lo, hi):
        u = (self.next() >> 40) / 16777216.0
        return lo + u * (hi - lo)


# ------------------------------------------------------------------ bodies (topology JSON)


def dome_body():
    """Invented dome: upper hemisphere r = 20 mm (face F2-dome, index 0) on a planar
    disk base z = 0 (face F1-base, index 1, outward normal -z), sharing the equator."""
    return {
        "body_id": "444556444f4d4500000000000000a001",
        "vertices": [{"point": [R, 0.0, 0.0], "label": "V-eq"}],
        "edges": [
            {
                "start": 0,
                "end": 0,
                "range": [0.0, 2.0 * math.pi],
                "curve": {"type": "circle", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0],
                          "ref_dir": [1.0, 0.0, 0.0], "radius": R},
                "label": "E-eq",
            }
        ],
        "coedges": [
            {"edge": 0, "sense": "same", "pcurve": None, "label": "C-dome"},
            {"edge": 0, "sense": "reversed", "pcurve": None, "label": "C-base"},
        ],
        "loops": [
            {"coedges": [0], "outer": True, "label": "L-dome"},
            {"coedges": [1], "outer": True, "label": "L-base"},
        ],
        "faces": [
            {"surface": {"type": "sphere", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0],
                         "ref_dir": [1.0, 0.0, 0.0], "radius": R},
             "sense": "same", "loops": [0], "label": "F2-dome"},
            {"surface": {"type": "plane", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, -1.0],
                         "ref_dir": [1.0, 0.0, 0.0]},
             "sense": "same", "loops": [1], "label": "F1-base"},
        ],
        "shells": [{"faces": [0, 1], "closed": True}],
    }


def square(prefix, x0, y0, side, vi, ei, ci, li):
    """Vertices, line edges, coedges and the loop of an axis-aligned square at z = 0 (CCW)."""
    pts = [(x0, y0), (x0 + side, y0), (x0 + side, y0 + side), (x0, y0 + side)]
    dirs = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]
    verts = [{"point": [p[0], p[1], 0.0], "label": f"{prefix}-V{k + 1}"} for k, p in enumerate(pts)]
    edges = []
    for k in range(4):
        o = pts[k]
        d = dirs[k]
        edges.append({
            "start": vi + k,
            "end": vi + (k + 1) % 4,
            "range": [0.0, side],
            "curve": {"type": "line", "origin": [o[0], o[1], 0.0], "direction": [d[0], d[1], 0.0],
                      "normal": [0.0, 0.0, 1.0]},
            "label": f"{prefix}-E{k + 1}",
        })
    coedges = [{"edge": ei + k, "sense": "same", "pcurve": None, "label": f"{prefix}-C{k + 1}"} for k in range(4)]
    loop = {"coedges": [ci + k for k in range(4)], "outer": True, "label": f"{prefix}-L"}
    return verts, edges, coedges, loop


def approx_body():
    """Invented: face N-face, a biquadratic B-spline patch that is the square [0, 20 mm]^2
    of the plane z = 0 (control points on a uniform grid, so S(u, v) = (0.02 u, 0.02 v, 0)),
    and face P-face, the analytic plane square [30, 50] x [0, 20] mm at z = 0; each in its
    own open shell."""
    side = 0.02
    nv, ne, nc, nl = square("N", 0.0, 0.0, side, 0, 0, 0, 0)
    pv, pe, pc, pl = square("P", 0.03, 0.0, side, 4, 4, 4, 1)
    ctrl = [[0.01 * i, 0.01 * j, 0.0] for i in range(3) for j in range(3)]
    return {
        "body_id": "415050524f5800000000000000000a02",
        "vertices": nv + pv,
        "edges": ne + pe,
        "coedges": nc + pc,
        "loops": [nl, pl],
        "faces": [
            {"surface": {"type": "nurbs", "degree_u": 2, "degree_v": 2,
                         "knots_u": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], "knots_v": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                         "n_u": 3, "n_v": 3, "ctrl": ctrl},
             "sense": "same", "loops": [0], "label": "N-face"},
            {"surface": {"type": "plane", "origin": [0.03, 0.0, 0.0], "axis": [0.0, 0.0, 1.0],
                         "ref_dir": [1.0, 0.0, 0.0]},
             "sense": "same", "loops": [1], "label": "P-face"},
        ],
        "shells": [{"faces": [0], "closed": False}, {"faces": [1], "closed": False}],
    }


# ------------------------------------------------------------------ poses

IDENTITY = ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0])
# 90 degrees about +z, then a small translation (exact binary64 entries).
ROT_Z = ([[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], [0.00048828125, -0.000244140625, 0.0001220703125])


def pose_json(pose):
    r, t = pose
    return {"r": [hx3(row) for row in r], "t": hx3(t)}


def pose_digest(pose):
    canon = json.dumps(pose_json(pose), sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return "sha256:" + hashlib.sha256(canon.encode("ascii")).hexdigest()


def apply_pose(pose, p):
    """Exact real R p + t of binary64 inputs."""
    r, t = pose
    return [sum(D(r[i][k]) * D(p[k]) for k in range(3)) + D(t[i]) for i in range(3)]


def invert_pose_float(pose, q):
    """A binary64 measured point whose pose image is (near) q: R^T (q - t), rounded."""
    r, t = pose
    d = [q[i] - t[i] for i in range(3)]
    return [r[0][k] * d[0] + r[1][k] * d[1] + r[2][k] * d[2] for k in range(3)]


# ------------------------------------------------------------------ expected answers on the dome

SQRT_HALF = Decimal(1) / Decimal(2).sqrt()


def expect_sphere(q):
    """Radial foot on the dome (interior of the upper hemisphere)."""
    n = norm(q)
    u = [c / n for c in q]
    foot = [D(R) * c for c in u]
    # Distance from the foot to the equator circle, the only boundary of F2-dome.
    rho = dsqrt(foot[0] * foot[0] + foot[1] * foot[1])
    eq = [foot[0] / rho * D(R), foot[1] / rho * D(R), Decimal(0)]
    db = norm([foot[k] - eq[k] for k in range(3)])
    assert foot[2] > 0
    return {"face": "F2-dome", "foot": d3(foot), "normal": d3(u), "signed_dev": dstr(n - D(R)),
            "on_boundary": db <= BOUNDARY_TOL, "no_projection": False, "_db": db}


def expect_plane(q):
    foot = [q[0], q[1], Decimal(0)]
    assert dsqrt(q[0] * q[0] + q[1] * q[1]) < D(R) - Decimal("0.001")
    return {"face": "F1-base", "foot": d3(foot), "normal": ["0", "0", "-1"], "signed_dev": dstr(-q[2]),
            "on_boundary": False, "no_projection": False}


def expect_edge(q):
    """Foot on the equator: both faces tie, the lower ID F1-base is reported; sign and normal
    from the two incident faces (sphere radial normal and -z, angle pi each)."""
    rho = dsqrt(q[0] * q[0] + q[1] * q[1])
    c = [q[0] / rho, q[1] / rho]
    foot = [D(R) * c[0], D(R) * c[1], Decimal(0)]
    d = norm([q[k] - foot[k] for k in range(3)])
    n = [c[0] * SQRT_HALF, c[1] * SQRT_HALF, -SQRT_HALF]
    side = sum((q[k] - foot[k]) * n[k] for k in range(3))
    assert side > 0
    return {"face": "F1-base", "foot": d3(foot), "normal": d3(n), "signed_dev": dstr(d),
            "on_boundary": True, "no_projection": False}


def no_projection():
    return {"no_projection": True}


def strip(e):
    return {k: v for k, v in e.items() if not k.startswith("_")}


def case(cid, pose, faces, radius, cad_points, expect, note):
    """`cad_points` are where the points should land in the CAD frame; the fixture stores
    measured points (pose inverse, rounded) and the expected values for their exact image."""
    pts, exp = [], []
    for q in cad_points:
        p = invert_pose_float(pose, q) if pose is not IDENTITY else list(q)
        pts.append(hx3(p))
        exp.append(strip(expect(apply_pose(pose, p))))
    return {"id": cid, "note": note, "toleranced_faces": faces, "search_radius_m": radius,
            "pose": pose_json(pose), "pose_digest": pose_digest(pose), "points": pts, "expected": exp}


def unit_dir(rng, zlo, zhi):
    """A unit vector with zlo <= z <= zhi by rejection from the cube; only + - * / and the
    correctly rounded sqrt, so the points are the same on every platform (no libm)."""
    while True:
        v = [rng.uniform(-1.0, 1.0) for _ in range(3)]
        n2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
        if not 0.0625 <= n2 <= 1.0:
            continue
        n = math.sqrt(n2)
        u = [c / n for c in v]
        if zlo <= u[2] <= zhi:
            return u


def sphere_points(rng, n):
    out = []
    for k in range(n):
        u = unit_dir(rng, 0.36, 0.99)  # polar angle from +z about 8 to 69 degrees
        s = R + (OFF if k % 2 == 0 else -OFF)
        out.append([s * u[0], s * u[1], s * u[2]])
    return out


def plane_points(rng, n):
    out = []
    while len(out) < n:
        x, y = rng.uniform(-0.015, 0.015), rng.uniform(-0.015, 0.015)
        if x * x + y * y > 0.015 * 0.015:
            continue
        z = -OFF if len(out) % 2 == 0 else OFF
        out.append([x, y, z])
    return out


# Unit directions in the xy plane from Pythagorean triples (no libm).
XY_DIRS = [(1.0, 0.0), (3 / 5, 4 / 5), (-5 / 13, 12 / 13), (-8 / 17, -15 / 17), (7 / 25, -24 / 25),
           (-1.0, 0.0), (20 / 29, 21 / 29), (-12 / 37, -35 / 37)]


def dome_fixture():
    rng = SplitMix(20261124)
    both = [{"face": "F2-dome", "approx_error_m": 0.0}, {"face": "F1-base", "approx_error_m": 0.0}]
    sp = sphere_points(rng, 40)
    pp = plane_points(rng, 40)

    def mixed(q):
        return expect_sphere(q) if q[2] > Decimal("0.001") else expect_plane(q)

    cases = [
        case("ac1_identity", IDENTITY, both, 0.001, sp + pp, mixed,
             "AC-1: sphere and plane points at +-0.05 mm, identity pose"),
        case("pose_rot_z", ROT_Z, both, 0.001, sphere_points(rng, 12) + plane_points(rng, 12), mixed,
             "the same rules after a 90 degree rotation about z and a translation"),
    ]
    # AC-3: feet on the shared equator edge.
    a = 1e-4
    edge_pts = []
    for cx, cy in XY_DIRS[:6]:
        edge_pts.append([(R + a) * cx, (R + a) * cy, -a])
    for cx, cy in XY_DIRS[6:]:
        edge_pts.append([(R + a) * cx, (R + a) * cy, 0.0])
    cases.append(case("ac3_shared_edge", IDENTITY, both, 0.001, edge_pts, expect_edge,
                      "AC-3: points whose foot is on the equator shared by both faces"))
    # REQ-003 for an interior foot: a radial foot 8e-10 m above the equator is on the
    # boundary within 1e-9 m; one 5e-9 m above it is not.
    a2 = 1e-5
    near = []
    for h, (cx, cy) in [(8e-10, XY_DIRS[1]), (8e-10, XY_DIRS[3]), (5e-9, XY_DIRS[1]), (5e-9, XY_DIRS[3])]:
        sv = h / R  # sine of the latitude
        cv = math.sqrt(1.0 - sv * sv)
        near.append([(R + a2) * cv * cx, (R + a2) * cv * cy, (R + a2) * sv])

    def near_expect(q):
        e = expect_sphere(q)
        assert abs(e["_db"] - BOUNDARY_TOL) > Decimal("1e-10")
        # The equator (edge) foot must be farther by more than the 1e-15 m tie window.
        rho = dsqrt(q[0] * q[0] + q[1] * q[1])
        eq = [q[0] / rho * D(R), q[1] / rho * D(R), Decimal(0)]
        d_edge = norm([q[k] - eq[k] for k in range(3)])
        assert d_edge - (norm(q) - D(R)) > Decimal("1e-14")
        return e

    cases.append(case("boundary_band_interior", IDENTITY, both, 0.001, near, near_expect,
                      "REQ-003: interior feet within and beyond 1e-9 m of the equator"))
    # AC-4: 10 mm away with a 1 mm search radius.
    far = [[0.0, 0.0, R + 0.01], [0.0, 0.0, -0.01], [R + 0.01, 0.0, 0.0]]
    cases.append(case("ac4_far", IDENTITY, both, 0.001, far, lambda q: no_projection(),
                      "AC-4: points 10 mm from every toleranced face, search radius 1 mm"))
    # Restriction: only the dome is toleranced; base points are 5 mm or more from it.
    dome_only = [{"face": "F2-dome", "approx_error_m": 0.0}]
    rp = sphere_points(rng, 4) + plane_points(rng, 4)
    cases.append(case("restrict_dome", IDENTITY, dome_only, 0.001, rp,
                      lambda q: expect_sphere(q) if q[2] > Decimal("0.001") else no_projection(),
                      "only F2-dome toleranced: base points have no projection within 1 mm"))
    return {
        "description": "GDT.PROF.DEV invented dome: exact sphere r 20 mm on a planar base, points at "
                       "+-0.05 mm, shared-edge, boundary-band, far and restriction cases",
        "license": "CC-BY-4.0",
        "generator": "fixtures/gdt/prof/make_dev_fixtures.py",
        "tolerance_m": 1e-12,
        "body": dome_body(),
        "cases": cases,
    }


def approx_fixture():
    rng = SplitMix(7)
    faces = [{"face": "N-face", "approx_error_m": 2e-7}, {"face": "P-face", "approx_error_m": 0.0}]
    pts = []
    for k in range(20):
        x0 = 0.0 if k < 10 else 0.03
        z = 3e-5 if k % 2 == 0 else -3e-5
        pts.append([x0 + rng.uniform(0.003, 0.017), rng.uniform(0.003, 0.017), z])

    def expect(q):
        on_n = q[0] < Decimal("0.025")
        return {"face": "N-face" if on_n else "P-face", "foot": d3([q[0], q[1], Decimal(0)]),
                "normal": ["0", "0", "1"], "signed_dev": dstr(q[2]), "on_boundary": False,
                "no_projection": False, "approx_error_m": "2e-7" if on_n else "0"}

    return {
        "description": "GDT.PROF.DEV AC-2 invented: a B-spline face declared with nominal_approximation_error "
                       "2e-7 m and an exact plane face, points at +-0.03 mm",
        "license": "CC-BY-4.0",
        "generator": "fixtures/gdt/prof/make_dev_fixtures.py",
        "tolerance_m": 1e-12,
        "body": approx_body(),
        "max_approx_error_m": "2e-7",
        "cases": [case("ac2_approx", IDENTITY, faces, 0.001, pts, expect,
                       "AC-2: every deviation on N-face carries 2e-7 m, P-face 0; maximum 2e-7 m")],
    }


def render(obj) -> str:
    return json.dumps(obj, indent=1, sort_keys=False) + "\n"


def main():
    outs = {"dev_sphere_plane.json": render(dome_fixture()), "dev_nurbs_approx.json": render(approx_fixture())}
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, text in outs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"{name}: differs from the generator output", file=sys.stderr)
                bad += 1
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
            print(f"wrote {name}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
