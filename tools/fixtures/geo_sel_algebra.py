#!/usr/bin/env python3
"""Independent reference for GEO.SEL.ALGEBRA; writes fixtures/geo/sel/algebra_cases.json.

Stdlib only (random, hashlib, struct, json, math). Shares no code with crates/fid-geo-sel:
- Set algebra (`union`, `intersection`, `difference`, `sym_difference`, `invert`) is computed
  using Python's built-in `set` operators and canonicalised to half-open ranges.
- Triangle `grow` and `shrink` (`edge` and `vertex` adjacency, n rings) are computed with
  Python dict/set BFS directly over the mesh triangles (`mesh_cube`, `mesh_open_flipped`,
  and `mesh_sphere_1e5` from `fixtures/geo/index/make_bvh_fixtures.py`).
- Point `grow` and `shrink` (radius r) are computed by brute-force pairwise squared distance
  `(dx*dx + dy*dy) + dz*dz <= r*r` (the exact pinned rule of `GEO.INDEX.KDTREE`) over
  `fixtures/geo/types/pointset_small.txt`.

Seed: 73 (spec/geo/sel/algebra/NODE.md Test fixtures).

Run:  python tools/fixtures/geo_sel_algebra.py [--check]
With --check nothing is written; the script exits non-zero if the file differs.
"""

import hashlib
import json
import math
import os
import random
import struct
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
OUT = os.path.join(ROOT, "fixtures", "geo", "sel", "algebra_cases.json")
DIGEST_VECTORS = os.path.join(ROOT, "fixtures", "geo", "types", "digest_vectors.json")
POINTSET_SMALL = os.path.join(ROOT, "fixtures", "geo", "types", "pointset_small.txt")
MESH_CUBE = os.path.join(ROOT, "fixtures", "geo", "types", "mesh_cube.txt")
MESH_OPEN = os.path.join(ROOT, "fixtures", "geo", "types", "mesh_open_flipped.txt")

sys.path.insert(0, os.path.join(ROOT, "tools", "fixtures"))
sys.path.insert(0, os.path.join(ROOT, "fixtures", "geo", "index"))
import geo_sel_sets as sel_ref  # noqa: E402
import make_bvh_fixtures as bvh_ref  # noqa: E402


def ranges_str(members):
    return " ".join(f"{s}:{e}" for s, e in sel_ref.ranges_of(members))


def digest_hex(domain, target, members):
    enc, _ = sel_ref.encode(domain, target, members)
    return hashlib.sha256(enc).hexdigest()


def load_vector_digest(name):
    with open(DIGEST_VECTORS, encoding="utf-8") as f:
        doc = json.load(f)
    for v in doc["vectors"]:
        if v["name"] == name:
            return bytes.fromhex(v["sha256"])
    raise SystemExit(f"missing digest vector: {name}")


def hexf(s):
    return struct.unpack("<d", struct.pack("<Q", int(s, 16)))[0]


def load_points(path):
    pts = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line.startswith("p "):
                continue
            parts = line.split()
            pts.append((hexf(parts[2]), hexf(parts[3]), hexf(parts[4])))
    return pts


def load_mesh(path):
    verts, tris = [], []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or line.startswith("frame "):
                continue
            parts = line.split()
            if parts[0] == "v":
                verts.append((float(parts[1]), float(parts[2]), float(parts[3])))
            elif parts[0] == "t":
                tris.append((int(parts[1]), int(parts[2]), int(parts[3])))
    return verts, tris


def mesh_topology(tris):
    edge_tris = {}
    vert_tris = {}
    for t, (a, b, c) in enumerate(tris):
        for v in {a, b, c}:
            vert_tris.setdefault(v, set()).add(t)
        if a == b or b == c or c == a:
            continue
        for u, v in ((a, b), (b, c), (c, a)):
            key = (u, v) if u < v else (v, u)
            edge_tris.setdefault(key, set()).add(t)
    bnd_edges = {k for k, ts in edge_tris.items() if len(ts) == 1}
    bnd_verts = {v for (u, w) in bnd_edges for v in (u, w)}
    return edge_tris, vert_tris, bnd_edges, bnd_verts


def tri_grow(tris, topo, members, rings, adj):
    edge_tris, vert_tris, _, _ = topo
    cur = set(members)
    frontier = set(members)
    for _ in range(rings):
        nxt = set()
        for t in frontier:
            a, b, c = tris[t]
            if adj == "edge":
                if a == b or b == c or c == a:
                    continue
                for u, v in ((a, b), (b, c), (c, a)):
                    key = (u, v) if u < v else (v, u)
                    for nb in edge_tris[key]:
                        if nb not in cur:
                            nxt.add(nb)
            else:
                for v in (a, b, c):
                    for nb in vert_tris[v]:
                        if nb not in cur:
                            nxt.add(nb)
        cur |= nxt
        frontier = nxt
        if not frontier:
            break
    return cur


def tri_shrink(tris, topo, members, rings, adj):
    edge_tris, vert_tris, bnd_edges, bnd_verts = topo
    n = len(tris)
    if rings == 0 or not members:
        return set(members)
    eroded = set(range(n)) - set(members)
    frontier = set(eroded)
    for step in range(rings):
        nxt = set()
        if step == 0:
            if adj == "edge":
                for key in bnd_edges:
                    for t in edge_tris[key]:
                        if t not in eroded:
                            nxt.add(t)
            else:
                for v in bnd_verts:
                    for t in vert_tris[v]:
                        if t not in eroded:
                            nxt.add(t)
        for t in frontier:
            a, b, c = tris[t]
            if adj == "edge":
                if a == b or b == c or c == a:
                    continue
                for u, v in ((a, b), (b, c), (c, a)):
                    key = (u, v) if u < v else (v, u)
                    for nb in edge_tris[key]:
                        if nb not in eroded:
                            nxt.add(nb)
            else:
                for v in (a, b, c):
                    for nb in vert_tris[v]:
                        if nb not in eroded:
                            nxt.add(nb)
        if not nxt:
            break
        eroded |= nxt
        frontier = nxt
    return set(range(n)) - eroded


def dist2(p, q):
    dx = p[0] - q[0]
    dy = p[1] - q[1]
    dz = p[2] - q[2]
    return (dx * dx + dy * dy) + dz * dz


def pt_grow(pts, members, radius):
    r2 = radius * radius
    out = set(members)
    for i in members:
        pi = pts[i]
        for j, pj in enumerate(pts):
            if j not in out and dist2(pj, pi) <= r2:
                out.add(j)
    return out


def pt_shrink(pts, members, radius):
    r2 = radius * radius
    n = len(pts)
    unselected = [i for i in range(n) if i not in members]
    out = set(members)
    for j in unselected:
        pj = pts[j]
        for i in list(out):
            if dist2(pts[i], pj) <= r2:
                out.discard(i)
    return out


def rand_set(rng, bound, runs_lo=1, runs_hi=8, len_hi=40):
    out = set()
    for _ in range(rng.randrange(runs_lo, runs_hi + 1)):
        start = rng.randrange(bound)
        length = rng.randrange(1, len_hi + 1)
        for x in range(start, min(start + length, bound)):
            out.add(x)
    return out


def build():
    rng = random.Random(73)
    pts_small = load_points(POINTSET_SMALL)
    assert len(pts_small) == 1000
    d_pt_small = load_vector_digest("pointset_small")

    _, cube_tris = load_mesh(MESH_CUBE)
    assert len(cube_tris) == 12
    cube_topo = mesh_topology(cube_tris)
    d_cube = load_vector_digest("mesh_cube")

    _, open_tris = load_mesh(MESH_OPEN)
    open_topo = mesh_topology(open_tris)
    d_open = hashlib.sha256(b"fid sel algebra mesh_open_flipped").digest()

    _, sphere_tris = bvh_ref.sphere_mesh()
    assert len(sphere_tris) == 100_820
    sphere_topo = mesh_topology(sphere_tris)
    d_sphere = hashlib.sha256(b"fid sel algebra mesh_sphere_1e5").digest()

    set_ops = []
    # Seeded set algebra cases over mesh_sphere_1e5 (triangles, 100_820) and pointset_small (points, 1000).
    for idx, (elem, domain, bound, target) in enumerate([
        ("mesh_sphere_1e5", "triangles", len(sphere_tris), d_sphere),
        ("mesh_sphere_1e5", "triangles", len(sphere_tris), d_sphere),
        ("mesh_sphere_1e5", "triangles", len(sphere_tris), d_sphere),
        ("pointset_small", "points", len(pts_small), d_pt_small),
        ("pointset_small", "points", len(pts_small), d_pt_small),
        ("pointset_small", "points", len(pts_small), d_pt_small),
    ]):
        a = rand_set(rng, bound, 2, 10, 60 if bound > 2000 else 30)
        b = rand_set(rng, bound, 2, 10, 60 if bound > 2000 else 30)
        u = a | b
        inter = a & b
        diff = a - b
        sdiff = a ^ b
        inv_a = set(range(bound)) - a
        set_ops.append({
            "name": f"seeded_{elem}_{idx}",
            "element": elem,
            "domain": domain,
            "bound": bound,
            "target_digest": target.hex(),
            "a": ranges_str(a),
            "b": ranges_str(b),
            "union": ranges_str(u),
            "union_sha256": digest_hex(domain, target, u),
            "intersection": ranges_str(inter),
            "intersection_sha256": digest_hex(domain, target, inter),
            "difference": ranges_str(diff),
            "difference_sha256": digest_hex(domain, target, diff),
            "sym_difference": ranges_str(sdiff),
            "sym_difference_sha256": digest_hex(domain, target, sdiff),
            "invert_a": ranges_str(inv_a),
            "invert_a_sha256": digest_hex(domain, target, inv_a),
        })

    # Hand-crafted boundary / touching-range set algebra cases.
    for name, bound, a, b in [
        ("touching_ranges", 20, {0, 1, 2, 3, 4}, {5, 6, 7, 8, 9}),
        ("disjoint_and_empty", 15, set(), {2, 3, 4, 10, 11}),
        ("full_and_subset", 12, set(range(12)), {0, 5, 11}),
        ("identical", 30, {1, 2, 3, 10, 11, 29}, {1, 2, 3, 10, 11, 29}),
    ]:
        u, inter, diff, sdiff = a | b, a & b, a - b, a ^ b
        inv_a = set(range(bound)) - a
        set_ops.append({
            "name": name,
            "element": "synthetic",
            "domain": "points",
            "bound": bound,
            "target_digest": d_pt_small.hex(),
            "a": ranges_str(a),
            "b": ranges_str(b),
            "union": ranges_str(u),
            "union_sha256": digest_hex("points", d_pt_small, u),
            "intersection": ranges_str(inter),
            "intersection_sha256": digest_hex("points", d_pt_small, inter),
            "difference": ranges_str(diff),
            "difference_sha256": digest_hex("points", d_pt_small, diff),
            "sym_difference": ranges_str(sdiff),
            "sym_difference_sha256": digest_hex("points", d_pt_small, sdiff),
            "invert_a": ranges_str(inv_a),
            "invert_a_sha256": digest_hex("points", d_pt_small, inv_a),
        })

    # Mesh triangle grow / shrink cases (AC-1, AC-2).
    mesh_ops = []

    def add_mesh(name, elem, tris, topo, target, op, adj, rings, members):
        res = tri_grow(tris, topo, members, rings, adj) if op == "grow" else tri_shrink(tris, topo, members, rings, adj)
        mesh_ops.append({
            "name": name,
            "element": elem,
            "op": op,
            "adjacency": adj,
            "rings": rings,
            "target_digest": target.hex(),
            "input": ranges_str(members),
            "result": ranges_str(res),
            "result_count": len(res),
            "result_sha256": digest_hex("triangles", target, res),
        })

    # AC-2: single selected triangle on mesh_cube grown by 1 ring (edge and vertex).
    add_mesh("cube_ac2_edge_grow_1", "mesh_cube", cube_tris, cube_topo, d_cube, "grow", "edge", 1, {0})
    add_mesh("cube_ac2_vertex_grow_1", "mesh_cube", cube_tris, cube_topo, d_cube, "grow", "vertex", 1, {0})
    add_mesh("cube_edge_grow_2", "mesh_cube", cube_tris, cube_topo, d_cube, "grow", "edge", 2, {0})
    add_mesh("cube_edge_shrink_after_grow", "mesh_cube", cube_tris, cube_topo, d_cube, "shrink", "edge", 1,
             tri_grow(cube_tris, cube_topo, {0}, 1, "edge"))
    add_mesh("cube_closed_full_shrink_edge", "mesh_cube", cube_tris, cube_topo, d_cube, "shrink", "edge", 2,
             set(range(12)))
    add_mesh("cube_closed_full_shrink_vertex", "mesh_cube", cube_tris, cube_topo, d_cube, "shrink", "vertex", 2,
             set(range(12)))

    # Open mesh boundary shrink.
    add_mesh("open_full_shrink_edge_1", "mesh_open_flipped", open_tris, open_topo, d_open, "shrink", "edge", 1,
             set(range(len(open_tris))))
    add_mesh("open_full_shrink_vertex_1", "mesh_open_flipped", open_tris, open_topo, d_open, "shrink", "vertex", 1,
             set(range(len(open_tris))))
    add_mesh("open_grow_edge_1", "mesh_open_flipped", open_tris, open_topo, d_open, "grow", "edge", 1, {0})

    # Seeded mesh_sphere_1e5 cases (AC-1).
    sphere_seed_a = {rng.randrange(len(sphere_tris)) for _ in range(5)}
    sphere_seed_b = tri_grow(sphere_tris, sphere_topo, sphere_seed_a, 3, "edge")
    add_mesh("sphere_grow_edge_0", "mesh_sphere_1e5", sphere_tris, sphere_topo, d_sphere, "grow", "edge", 0,
             sphere_seed_a)
    add_mesh("sphere_grow_edge_1", "mesh_sphere_1e5", sphere_tris, sphere_topo, d_sphere, "grow", "edge", 1,
             sphere_seed_a)
    add_mesh("sphere_grow_edge_3", "mesh_sphere_1e5", sphere_tris, sphere_topo, d_sphere, "grow", "edge", 3,
             sphere_seed_a)
    add_mesh("sphere_grow_vertex_2", "mesh_sphere_1e5", sphere_tris, sphere_topo, d_sphere, "grow", "vertex", 2,
             sphere_seed_a)
    add_mesh("sphere_shrink_edge_1", "mesh_sphere_1e5", sphere_tris, sphere_topo, d_sphere, "shrink", "edge", 1,
             sphere_seed_b)
    add_mesh("sphere_shrink_vertex_1", "mesh_sphere_1e5", sphere_tris, sphere_topo, d_sphere, "shrink", "vertex", 1,
             sphere_seed_b)

    # Point grow / shrink cases on pointset_small (AC-1).
    point_ops = []
    pt_seed_a = rand_set(rng, len(pts_small), 2, 5, 15)
    pt_seed_b = pt_grow(pts_small, pt_seed_a, 0.025)
    for name, op, radius, members in [
        ("pt_grow_r0", "grow", 0.0, pt_seed_a),
        ("pt_grow_r008", "grow", 0.008, pt_seed_a),
        ("pt_grow_r025", "grow", 0.025, pt_seed_a),
        ("pt_shrink_r008", "shrink", 0.008, pt_seed_b),
        ("pt_shrink_r020", "shrink", 0.020, pt_seed_b),
        ("pt_shrink_full", "shrink", 0.020, set(range(len(pts_small)))),
        ("pt_grow_empty", "grow", 0.020, set()),
    ]:
        res = pt_grow(pts_small, members, radius) if op == "grow" else pt_shrink(pts_small, members, radius)
        point_ops.append({
            "name": name,
            "element": "pointset_small",
            "op": op,
            "radius_hex": f"{struct.unpack('<Q', struct.pack('<d', radius))[0]:016x}",
            "target_digest": d_pt_small.hex(),
            "input": ranges_str(members),
            "result": ranges_str(res),
            "result_count": len(res),
            "result_sha256": digest_hex("points", d_pt_small, res),
        })

    doc = {
        "generator": "tools/fixtures/geo_sel_algebra.py (stdlib set/BFS/brute-force, independent of fid-geo-sel)",
        "license": "CC-BY-4.0",
        "seed": 73,
        "spec": "GEO.SEL.ALGEBRA",
        "version": 1,
        "set_ops": set_ops,
        "mesh_ops": mesh_ops,
        "point_ops": point_ops,
    }
    return json.dumps(doc, indent=2, sort_keys=True) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        with open(OUT, encoding="utf-8", newline="") as f:
            if f.read() != text:
                print(f"{OUT} is stale; rerun tools/fixtures/geo_sel_algebra.py", file=sys.stderr)
                sys.exit(1)
        print("algebra_cases.json up to date")
        return
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"wrote {OUT} ({len(text)} bytes)")


if __name__ == "__main__":
    main()
