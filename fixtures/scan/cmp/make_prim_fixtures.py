#!/usr/bin/env python3
"""Independent analytic oracle for SCAN.CMP.DEV.PRIM fixtures (CC0).

Generates:
  - fixtures/scan/cmp/dev_sphere.json (AC-1: r = 50 mm CAD/mesh sphere, points at r = 49.9..50.1 mm)
  - fixtures/scan/cmp/dev_cube_edges.json (AC-2, REQ-006: 20 mm cube, face/edge/corner points)
  - fixtures/scan/cmp/dev_open_plate.json (AC-3: open 50 mm plate, interior/diagonal/beyond-border)

Stdlib only (json, math, os, struct, sys). Shares no code with Rust `fid-scan-cmp`.

Run:  python fixtures/scan/cmp/make_prim_fixtures.py [--check]
With --check nothing is written; the script exits non-zero if any file differs.
"""

import json
import math
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def f64_hex(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def make_sphere():
    r = 0.05
    # Deterministic unit directions (axes, face diagonals, body diagonals, skewed angles)
    raw_dirs = [
        (1.0, 0.0, 0.0),
        (-1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
        (0.0, -1.0, 0.0),
        (0.0, 0.0, 1.0),
        (0.0, 0.0, -1.0),
        (1.0, 1.0, 0.0),
        (1.0, -1.0, 1.0),
        (-2.0, 3.0, 6.0),
        (3.0, -4.0, 12.0),
        (-5.0, -12.0, 0.0),
        (8.0, 15.0, 0.0),
    ]
    # Radii spanning 49.9 mm .. 50.1 mm, including exact 50.0 mm
    radii = [
        0.0499,
        0.04995,
        0.05,
        0.050025,
        0.0501,
        0.04992,
        0.05008,
        0.04999,
        0.05001,
        0.05,
        0.0499,
        0.0501,
    ]
    cases = []
    for idx, (d, rq) in enumerate(zip(raw_dirs, radii)):
        norm = math.sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
        u = [d[0] / norm, d[1] / norm, d[2] / norm]
        p = [rq * u[0], rq * u[1], rq * u[2]]
        dev = rq - r
        if dev == 0.0:
            dev = 0.0
        cases.append({
            "name": f"sphere_{idx:02d}",
            "point": p,
            "normal": u,
            "expected_m": dev,
            "expected_hex": f64_hex(dev),
        })
    return {
        "about": "SCAN.CMP.DEV.PRIM AC-1 sphere fixture (r = 50 mm); written by make_prim_fixtures.py (CC0)",
        "radius_m": r,
        "max_distance_m": 0.005,
        "max_angle_rad": math.pi / 6.0,
        "cases": cases,
    }


def make_cube_edges():
    h = 0.01  # 20 mm cube -> [-0.01, +0.01]^3
    # 8 vertices
    vertices = [
        [-h, -h, -h],  # 0
        [ h, -h, -h],  # 1
        [ h,  h, -h],  # 2
        [-h,  h, -h],  # 3
        [-h, -h,  h],  # 4
        [ h, -h,  h],  # 5
        [ h,  h,  h],  # 6
        [-h,  h,  h],  # 7
    ]
    # 12 CCW outward triangles
    triangles = [
        # -Z face (normal 0, 0, -1)
        [0, 2, 1], [0, 3, 2],
        # +Z face (normal 0, 0, +1)
        [4, 5, 6], [4, 6, 7],
        # -Y face (normal 0, -1, 0)
        [0, 1, 5], [0, 5, 4],
        # +Y face (normal 0, +1, 0)
        [3, 7, 6], [3, 6, 2],
        # -X face (normal -1, 0, 0)
        [0, 4, 7], [0, 7, 3],
        # +X face (normal +1, 0, 0)
        [1, 2, 6], [1, 6, 5],
    ]

    def oracle_cube(p):
        clamped = [max(-h, min(h, p[k])) for k in range(3)]
        on_x = abs(p[0]) >= h
        on_y = abs(p[1]) >= h
        on_z = abs(p[2]) >= h
        outside = (abs(p[0]) > h) or (abs(p[1]) > h) or (abs(p[2]) > h)
        on_boundary = (not outside) and (abs(p[0]) == h or abs(p[1]) == h or abs(p[2]) == h)
        if outside:
            diff = [p[k] - clamped[k] for k in range(3)]
            dist = math.sqrt(sum(c * c for c in diff))
            shortest = dist
            # Closest feature normal on cube boundary:
            # faces active where |p_k| >= h (strictly > h determines the Voronoi region)
            n_dir = [
                (1.0 if p[k] > h else (-1.0 if p[k] < -h else 0.0))
                for k in range(3)
            ]
            n_len = math.sqrt(sum(c * c for c in n_dir))
            nf = [c / n_len for c in n_dir]
            normal_dev = sum((p[k] - clamped[k]) * nf[k] for k in range(3))
        elif on_boundary:
            shortest = 0.0
            normal_dev = 0.0
        else:
            # Strictly inside: shortest distance is negative of min gap to faces
            gaps = [h - abs(p[k]) for k in range(3)]
            min_gap = min(gaps)
            shortest = -min_gap
            # Find unique closest face (if unambiguous)
            best_k = gaps.index(min_gap)
            nf = [0.0, 0.0, 0.0]
            nf[best_k] = 1.0 if p[best_k] >= 0.0 else -1.0
            foot = list(p)
            foot[best_k] = h if p[best_k] >= 0.0 else -h
            normal_dev = sum((p[k] - foot[k]) * nf[k] for k in range(3))
        if shortest == 0.0:
            shortest = 0.0
        if normal_dev == 0.0:
            normal_dev = 0.0
        return shortest, normal_dev

    specs = [
        ("face_pos_x_out", [0.013, 0.002, -0.003], "face"),
        ("face_neg_z_out", [0.001, -0.004, -0.0125], "face"),
        ("face_pos_y_in", [0.002, 0.007, -0.001], "face"),
        ("face_neg_x_in", [-0.0085, 0.001, 0.002], "face"),
        ("edge_xy_out", [0.013, 0.014, 0.002], "edge"),
        ("edge_xz_out", [-0.012, 0.003, 0.011], "edge"),
        ("edge_yz_out", [0.0, -0.013, -0.014], "edge"),
        ("corner_ppp_out", [0.013, 0.014, 0.015], "corner"),
        ("corner_nnn_out", [-0.012, -0.011, -0.012], "corner"),
        ("corner_pmp_out", [0.0115, -0.012, 0.011], "corner"),
        ("on_face", [0.01, 0.002, -0.003], "surface"),
        ("on_edge", [0.01, 0.01, 0.002], "surface"),
        ("on_corner", [0.01, 0.01, 0.01], "surface"),
    ]

    cases = []
    for name, p, region in specs:
        shortest, normal_dev = oracle_cube(p)
        cases.append({
            "name": name,
            "region": region,
            "point": p,
            "shortest_m": shortest,
            "shortest_hex": f64_hex(shortest),
            "normal_m": normal_dev,
            "normal_hex": f64_hex(normal_dev),
        })

    return {
        "about": "SCAN.CMP.DEV.PRIM AC-2 / REQ-006 20 mm cube fixture; written by make_prim_fixtures.py (CC0)",
        "half_extent_m": h,
        "max_distance_m": 0.02,
        "max_angle_rad": math.pi,
        "vertices": vertices,
        "triangles": triangles,
        "cases": cases,
    }


def make_open_plate():
    s = 0.05  # 50 mm square in z = 0
    vertices = [
        [0.0, 0.0, 0.0],
        [s,   0.0, 0.0],
        [s,   s,   0.0],
        [0.0, s,   0.0],
    ]
    triangles = [
        [0, 1, 2],
        [0, 2, 3],
    ]

    def oracle_plate(p):
        cx = max(0.0, min(s, p[0]))
        cy = max(0.0, min(s, p[1]))
        off_edge = (p[0] < 0.0) or (p[0] > s) or (p[1] < 0.0) or (p[1] > s)
        dx = p[0] - cx
        dy = p[1] - cy
        dz = p[2]
        d = math.sqrt(dx * dx + dy * dy + dz * dz)
        if off_edge:
            shortest = d if d != 0.0 else 0.0
            normal_status = "off_edge"
            normal_val = None
        else:
            # Strictly inside (or on edge/interior of) [0, s]^2; note diagonal is interior!
            # On exact boundary border of plate: off_edge would only trigger if foot is on border
            on_border = (p[0] == 0.0) or (p[0] == s) or (p[1] == 0.0) or (p[1] == s)
            if on_border:
                shortest = abs(dz) if dz != 0.0 else 0.0
                normal_status = "off_edge"
                normal_val = None
            else:
                shortest = dz if dz != 0.0 else 0.0
                normal_status = "valid"
                normal_val = dz if dz != 0.0 else 0.0
        return shortest, normal_status, normal_val

    specs = [
        ("interior_above", [0.02, 0.03, 0.002]),
        ("interior_below", [0.02, 0.03, -0.002]),
        ("interior_diagonal_above", [0.025, 0.025, 0.003]),
        ("beyond_pos_x_above", [0.055, 0.025, 0.001]),
        ("beyond_neg_x_below", [-0.003, 0.025, -0.004]),
        ("beyond_corner_in_plane", [-0.003, -0.004, 0.0]),
        ("on_border_edge", [0.05, 0.025, 0.002]),
    ]

    cases = []
    for name, p in specs:
        shortest, normal_status, normal_val = oracle_plate(p)
        cases.append({
            "name": name,
            "point": p,
            "shortest_m": shortest,
            "shortest_hex": f64_hex(shortest),
            "normal_status": normal_status,
            "normal_m": normal_val,
            "normal_hex": f64_hex(normal_val) if normal_val is not None else "",
        })

    return {
        "about": "SCAN.CMP.DEV.PRIM AC-3 open 50 mm plate fixture; written by make_prim_fixtures.py (CC0)",
        "size_m": s,
        "max_distance_m": 0.02,
        "max_angle_rad": math.pi / 6.0,
        "vertices": vertices,
        "triangles": triangles,
        "cases": cases,
    }


def render():
    return {
        os.path.join(HERE, "dev_sphere.json"): json.dumps(make_sphere(), indent=1, sort_keys=True) + "\n",
        os.path.join(HERE, "dev_cube_edges.json"): json.dumps(make_cube_edges(), indent=1, sort_keys=True) + "\n",
        os.path.join(HERE, "dev_open_plate.json"): json.dumps(make_open_plate(), indent=1, sort_keys=True) + "\n",
    }


def main():
    files = render()
    if "--check" in sys.argv[1:]:
        bad = False
        for path, text in files.items():
            try:
                with open(path, encoding="utf-8", newline="") as f:
                    have = f.read()
            except FileNotFoundError:
                have = None
            if have != text:
                print("stale: " + os.path.relpath(path), file=sys.stderr)
                bad = True
        return 1 if bad else 0
    for path, text in files.items():
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
